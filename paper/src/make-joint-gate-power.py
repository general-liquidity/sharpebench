#!/usr/bin/env python3
"""Development-stage calibration of the full joint eligibility rule.

Ticket P12-A, section 10.A of the remaining-product-work plan. This is
**development-tier exploration, not frozen validation**: every number it writes
is exploratory, none of it may be reported as a validated claim, and nothing
here is tuned against a held-out set. It writes a new evidence namespace and
touches neither `paper/evidence/final/power-curve.jsonl` nor any other frozen
artifact, and it changes no scoring semantics or golden.

No agent, simulator, market data, model or provider is involved. The producer
reads the committed default-cell deflation bars and simulates returns.

Legs modelled, as `composite.rs` builds `rank_eligible`:

- the deflation leg, pooled DSR at least 0.95 against the panel's committed
  per-period bar in the default cell of tab:eligibility, held fixed;
- the per-window reliability leg, pass^k in its default all-runs mode: each of
  six windows reaches a per-run PSR of at least 0.90 against zero;
- the bootstrap leg, stationary-bootstrap p below 0.05 on the pooled track with
  the shipped 2000 resamples and 0.1 restart probability.

Legs NOT modelled: the process gate, the mandate gate, the risk gates, the
influential-vote and dispersion disclosure, costs, missingness, refusal and
unavailability accounting, and the measured-bar path's dependence on the field
that produced the bar (each bar is held at its committed value). Every
unmodelled leg is a further conjunct, so it can only refuse more: the true
Sharpe at which the shipped predicate reaches a given pass probability is at
least the value printed here, and its false-positive rate is at most the value
printed here.

Three legs, one set of draws. Each leg is a threshold on the true Sharpe:
adding a constant to a series leaves its dispersion and shape alone, so the
observed Sharpes that pass eq:psr form a half-line, and the bootstrap resamples
the centered track, whose null distribution of resampled means does not move
when the constant is added. The joint rule therefore passes at true Sharpe s
exactly when s reaches the largest of the three thresholds, and the pass
probability at every s is the empirical distribution function of those maxima.
The false-positive rate is that function at s = 0.

Dependence sensitivity repeats the calculation with stationary Gaussian AR(1)
returns at several first-order autocorrelations, normalized to unit marginal
standard deviation so a true annualized Sharpe means the same thing at every
rho.

The gate-design comparison is closed form and is reported only at matched
nominal false-positive rates. An earlier draft compared the every-window rule
at a nominal rate of 1e-6 with a pooled test at 0.05 and reported a fourteenfold
history cost; at a matched rate the factor is under two.

    python paper/src/make-joint-gate-power.py                 # compute
    python paper/src/make-joint-gate-power.py --jobs 32

The output does not depend on --jobs: the draws are split into a fixed number of
seeded chunks and chunk results are assembled in a fixed order. The runtime
estimate is printed and optionally written with --runtime-out; it is kept out of
the evidence file so the file's bytes stay reproducible.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
import time
from multiprocessing import Pool

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import joint_gate_power as jg  # noqa: E402
import kernel_stats  # noqa: E402

ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
SWEEP_DIR = os.path.join(ROOT, "paper", "evidence", "final")
EVIDENCE_DIR = os.path.join(ROOT, "paper", "evidence", "development")
EVIDENCE = os.path.join(EVIDENCE_DIR, "joint-gate-power.jsonl")
COMMAND = "python paper/src/make-joint-gate-power.py"
TIER = "development"

SEED = 20260923
REPLICATIONS = 64_000
CHUNKS = 64
BATCH = 125
JOINT_REPLICATIONS = 1_600
JOINT_CHUNKS = 64
JOINT_BATCH = 10
DIGITS = 6
INTERVAL_ALPHA = 0.05

# rho = 0 is the headline; the rest are the dependence sensitivity. The joint
# rule carries the bootstrap leg and costs about thirty times as much per
# replication, so it runs at fewer replications and fewer rho values.
RHOS_TWO_LEG = (0.0, -0.2, -0.1, 0.1, 0.2)
RHOS_JOINT = (0.0, -0.1, 0.1)

# The two runs draw different returns, so a rule measured in one cannot be
# compared with a rule measured in the other. Every rule the three-leg run can
# evaluate is therefore reported from its own draws as well, and each record
# names the run it came from.
TWO_LEG_RULES = ("per_window", "pass_k", "dsr", "two_leg")
JOINT_RULES = ("per_window", "pass_k", "dsr", "two_leg", "bootstrap", "joint")
TWO_LEG_DRAWS = "two_leg_run"
JOINT_DRAWS = "joint_run"

# The bootstrap leg costs one full resampling pass over the pooled track per
# replication: 2000 resamples of n returns each. Past this pooled length that is
# not affordable at the development tier, so the leg is left unmodelled on that
# geometry and said so in the output rather than quietly approximated.
BOOTSTRAP_MAX_POOLED = 8_000

# Where power is reported: multiples of each panel's own committed bar, so the
# region below and above the bar is covered whatever the bar is, plus four
# absolute anchors that are comparable across panels.
BAR_MULTIPLES = (0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0)
ABSOLUTE_SHARPES = (0.0, 0.5, 1.0, 2.0, 3.0)
CROSSINGS = (5, 50, 95)

PANEL_FIELDS = (
    "periods_per_year",
    "n_windows",
    "window_len",
    "pooled_observations",
    "trials_sr_std_source",
    "deflation_bar_per_period",
    "deflation_bar_annualized_equivalent",
    "deflation_null_mean_per_period",
    "effective_n_trials",
)
DEFAULT_CELL = {
    "dsr_bar": jg.DSR_BAR,
    "n_trials": 50,
    "sr_std_pinned": None,
    "min_field_for_measured_sr_std": 5,
}
PANELS = (
    "us-indices-1w",
    "us-indices-1d",
    "crypto-majors-1w",
    "crypto-majors-1d",
    "crypto-majors-4h",
    "crypto-majors-1h",
    "fx-majors-1d",
    "commodities-1d",
    "rates-1d",
)

LEGS_MODELLED = (
    "deflation: pooled DSR >= 0.95 against the panel's committed per-period bar",
    "reliability: pass^k in all-runs mode, six windows at per-run PSR >= 0.90 against zero",
    "bootstrap: stationary-bootstrap p < 0.05 on the pooled track, 2000 resamples, "
    "restart probability 0.1",
)
LEGS_NOT_MODELLED = (
    "process gate",
    "mandate gate",
    "risk gates",
    "influential-vote and dispersion disclosure",
    "costs, slippage and execution-seed variation",
    "missingness, refusal and unavailability accounting",
    "the measured bars' dependence on the field that produced them (bars held fixed)",
    "cross-agent dependence within a field (entries are drawn independently)",
    "the kernel's single fixed bootstrap seed (the leg is averaged over resampling noise)",
)


# ---- committed bars -------------------------------------------------------------
def load_jsonl(path):
    if not os.path.exists(path):
        raise jg.PowerSupportError(f"missing evidence file: {path}")
    rows = []
    with open(path, encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            if not line.strip():
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as exc:
                raise jg.PowerSupportError(f"{path}:{number}: not a JSON record: {exc}") from exc
            if not isinstance(record, dict):
                raise jg.PowerSupportError(f"{path}:{number}: not an object")
            rows.append(record)
    if not rows:
        raise jg.PowerSupportError(f"no records in {path}")
    return rows


def panel_bar(sweep_dir, dataset):
    """The default cell's bar, geometry and field size, required to be one value
    across the field: a bar is a field-level quantity, and two would mean two
    cells."""
    rows = [
        row
        for row in load_jsonl(os.path.join(sweep_dir, f"{dataset}.jsonl"))
        if all(row.get(key) == value for key, value in DEFAULT_CELL.items())
    ]
    if not rows:
        raise jg.PowerSupportError(f"no default-cell records for {dataset}")
    values = {tuple(row[field] for field in PANEL_FIELDS) for row in rows}
    if len(values) != 1:
        raise jg.PowerSupportError(
            f"{dataset}: the default cell carries {len(values)} distinct bars or geometries"
        )
    panel = dict(zip(PANEL_FIELDS, values.pop()))
    if panel["n_windows"] != jg.N_WINDOWS:
        raise jg.PowerSupportError(f"{dataset}: {panel['n_windows']} windows, not {jg.N_WINDOWS}")
    if panel["pooled_observations"] != panel["n_windows"] * panel["window_len"]:
        raise jg.PowerSupportError(
            f"{dataset}: pooled length is not windows times window length"
        )
    if panel["deflation_null_mean_per_period"] != 0.0:
        raise jg.PowerSupportError(f"{dataset}: a nonzero deflation null mean is not modelled")
    panel["field_size"] = len(rows)
    return panel


# ---- Monte Carlo ----------------------------------------------------------------
def rho_key(rho):
    """A non-negative integer spawn-key component for a rho in [-0.99, 0.99]."""
    scaled = round(rho * 1000.0)
    if abs(scaled - rho * 1000.0) > 1e-9:
        raise jg.PowerSupportError(f"rho {rho} is not a multiple of 0.001")
    return int(scaled) + 1000


def simulate_chunk(task):
    """One seeded chunk of one geometry: per replication, each leg's threshold
    true Sharpe. The seed depends on the mode, rho, geometry and chunk index
    only, so the result is independent of how the work is distributed."""
    with_bootstrap, rho, window_len, bars, chunk, replications = task
    pooled_len = jg.N_WINDOWS * window_len
    with_bootstrap = with_bootstrap and pooled_len <= BOOTSTRAP_MAX_POOLED
    sequence = np.random.SeedSequence(
        entropy=SEED,
        spawn_key=(int(with_bootstrap), rho_key(rho), jg.N_WINDOWS, window_len, chunk),
    )
    rng = np.random.Generator(np.random.PCG64(sequence))
    z_run = kernel_stats.norm_cdf_inverse(jg.PER_RUN_PSR_BAR)
    z_dsr = kernel_stats.norm_cdf_inverse(jg.DSR_BAR)
    batch = JOINT_BATCH if with_bootstrap else BATCH
    first, passk, boot = [], [], []
    dsr = [[] for _ in bars]
    done = 0
    while done < replications:
        size = min(batch, replications - done)
        track = jg.draw_track(rng, size, pooled_len, rho)
        windows = jg.window_thresholds(track, jg.N_WINDOWS, z_run)
        first.append(windows[:, 0])
        passk.append(windows.max(axis=1))
        for slot, bar in enumerate(bars):
            dsr[slot].append(jg.threshold_true_sharpe(track, z_dsr, bar))
        if with_bootstrap:
            boot.append(
                jg.bootstrap_threshold(
                    track, rng, jg.BOOTSTRAP_N_BOOT, jg.BOOTSTRAP_BLOCK_PROB, jg.BOOTSTRAP_ALPHA
                )
            )
        done += size
    return (
        np.concatenate(first),
        np.concatenate(passk),
        [np.concatenate(part) for part in dsr],
        np.concatenate(boot) if with_bootstrap else None,
    )


def simulate(with_bootstrap, rhos, geometries, replications, chunks, pool):
    """{(rho, window_len): (first_window, passk, [dsr per bar], bootstrap)}."""
    if replications % chunks:
        raise jg.PowerSupportError(f"replications {replications} is not a multiple of {chunks}")
    per_chunk = replications // chunks
    keys = [(rho, window_len) for rho in rhos for window_len in geometries]
    tasks = [
        (with_bootstrap, rho, window_len, geometries[window_len], chunk, per_chunk)
        for rho, window_len in keys
        for chunk in range(chunks)
    ]
    # One pool serves both phases: on Windows each worker re-imports NumPy, so
    # spawning a second set of workers costs more than the draws themselves.
    results = (
        pool.map(simulate_chunk, tasks, chunksize=1)
        if pool is not None
        else [simulate_chunk(task) for task in tasks]
    )
    out = {}
    for index, key in enumerate(keys):
        parts = results[index * chunks : (index + 1) * chunks]
        n_bars = len(geometries[key[1]])
        drew_bootstrap = parts[0][3] is not None
        out[key] = (
            np.sort(np.concatenate([part[0] for part in parts])),
            np.sort(np.concatenate([part[1] for part in parts])),
            [
                np.sort(np.concatenate([part[2][slot] for part in parts]))
                for slot in range(n_bars)
            ],
            np.sort(np.concatenate([part[3] for part in parts])) if drew_bootstrap else None,
        )
    return out


# ---- reductions -----------------------------------------------------------------
def passes_at(sorted_thresholds, sharpe):
    """How many replications the rule admits at a true annualized Sharpe."""
    return int(np.searchsorted(sorted_thresholds, sharpe, side="right"))


def crossing(sorted_ann, percent):
    """Smallest true Sharpe at which the pass probability reaches `percent`,
    with a normal-approximation binomial order-statistic 95 percent band."""
    total = len(sorted_ann)
    rank = -(-percent * total // 100)
    share = percent / 100.0
    half = 1.959963984540054 * math.sqrt(total * share * (1.0 - share))
    lo = max(1, math.floor(total * share - half))
    hi = min(total, math.ceil(total * share + half))
    return (
        round(float(sorted_ann[rank - 1]), DIGITS),
        round(float(sorted_ann[lo - 1]), DIGITS),
        round(float(sorted_ann[hi - 1]), DIGITS),
    )


def annualized(thresholds, periods_per_year):
    return np.sort(thresholds * math.sqrt(periods_per_year))


def rule_thresholds(draws, rho, panel, bar_slot, rule):
    """Per-replication threshold true Sharpe of one named rule, per period."""
    first, passk, dsr_by_bar, boot = draws[(rho, panel["window_len"])]
    dsr = dsr_by_bar[bar_slot]
    if rule == "per_window":
        return first
    if rule == "pass_k":
        return passk
    if rule == "dsr":
        return dsr
    if rule == "two_leg":
        return np.maximum(passk, dsr)
    if rule == "bootstrap":
        return boot
    if rule == "joint":
        return np.maximum(np.maximum(passk, dsr), boot)
    raise jg.PowerSupportError(f"unknown rule {rule}")


def false_positive_record(dataset, rho, rule, sorted_ann, replications, field_size, draws):
    admitted = passes_at(sorted_ann, 0.0)
    rate, se, lower, upper = jg.rate_with_interval(admitted, replications, INTERVAL_ALPHA)
    return {
        "record": "false_positive_rate",
        "tier": TIER,
        "dataset": dataset,
        "rho": rho,
        "rule": rule,
        "draws": draws,
        "replications": replications,
        "admitted": admitted,
        "per_entry_rate": rate,
        "per_entry_standard_error": round(se, DIGITS + 4),
        "per_entry_interval95": [round(lower, DIGITS + 4), round(upper, DIGITS + 4)],
        "field_size": field_size,
        "field_rate_independent_entries": jg.field_probability(rate, field_size),
        "field_rate_upper95_independent_entries": round(
            jg.field_probability(upper, field_size), DIGITS + 4
        ),
    }


def power_grid(panel):
    bar = panel["deflation_bar_annualized_equivalent"]
    values = {round(bar * multiple, 4) for multiple in BAR_MULTIPLES}
    values |= set(ABSOLUTE_SHARPES)
    return sorted(values)


def power_records(dataset, rho, rule, sorted_ann, replications, panel, draws):
    bar = panel["deflation_bar_annualized_equivalent"]
    out = []
    for sharpe in power_grid(panel):
        admitted = passes_at(sorted_ann, sharpe)
        rate, se, lower, upper = jg.rate_with_interval(
            admitted, replications, INTERVAL_ALPHA, exact=False
        )
        out.append(
            {
                "record": "power_point",
                "tier": TIER,
                "dataset": dataset,
                "rho": rho,
                "rule": rule,
                "draws": draws,
                "sharpe_annualized": sharpe,
                "sharpe_over_committed_bar": round(sharpe / bar, DIGITS),
                "replications": replications,
                "admitted": admitted,
                "pass_probability": rate,
                "standard_error": round(se, DIGITS + 4),
                "interval95_wilson": [round(lower, DIGITS + 4), round(upper, DIGITS + 4)],
            }
        )
    summary = {
        "record": "power_summary",
        "tier": TIER,
        "dataset": dataset,
        "rho": rho,
        "rule": rule,
        "draws": draws,
        "replications": replications,
    }
    for percent in CROSSINGS:
        at, lo, hi = crossing(sorted_ann, percent)
        summary[f"sharpe_at_{percent}pct"] = at
        summary[f"sharpe_at_{percent}pct_band95"] = [lo, hi]
    out.append(summary)
    return out


# ---- gate designs ---------------------------------------------------------------
SHIPPED_EVERY_WINDOW = ("every_window_6of6_psr_0.90", 6, 6, 1.0 - jg.PER_RUN_PSR_BAR)
CANDIDATE_SHAPES = (
    ("pooled_single_test", 1, 1),
    ("majority_4of6", 6, 4),
    ("every_window_6of6", 6, 6),
)
COMPARISON_EFFECTS = (1.0, 0.5)
COMPARISON_POWERS = (0.5, 0.8)
COMPARISON_PERIODS_PER_YEAR = 252.0


def gate_comparison_records():
    """Candidate designs at matched nominal rates, plus the one unmatched
    comparison kept as a labelled counterexample and never as a factor."""
    name, windows, required, alpha = SHIPPED_EVERY_WINDOW
    shipped = jg.GateDesign(name, windows, required, alpha)
    shipped_rate = shipped.nominal_false_positive_rate
    records = []
    for target_rate in (shipped_rate, 0.05):
        for shape_name, shape_windows, shape_required in CANDIDATE_SHAPES:
            candidate = jg.design_at_false_positive_rate(
                f"{shape_name}_at_rate_{target_rate:.6g}",
                shape_windows,
                shape_required,
                target_rate,
            )
            reference = (
                shipped
                if target_rate == shipped_rate
                else jg.design_at_false_positive_rate(
                    f"every_window_6of6_at_rate_{target_rate:.6g}", 6, 6, target_rate
                )
            )
            for effect in COMPARISON_EFFECTS:
                for power in COMPARISON_POWERS:
                    comparison = jg.require_matched(
                        jg.compare_designs(
                            reference,
                            candidate,
                            effect,
                            COMPARISON_PERIODS_PER_YEAR,
                            power,
                        )
                    )
                    records.append(
                        {
                            "record": "gate_comparison",
                            "tier": TIER,
                            "comparison": "matched_false_positive_rate",
                            "matched_rate": target_rate,
                            **comparison,
                        }
                    )
    pooled_005 = jg.GateDesign("pooled_single_test_psr_0.95", 1, 1, 0.05)
    for effect in COMPARISON_EFFECTS:
        for power in COMPARISON_POWERS:
            comparison = jg.compare_designs(
                shipped, pooled_005, effect, COMPARISON_PERIODS_PER_YEAR, power
            )
            if comparison["false_positive_rate_matched"]:
                raise jg.PowerSupportError(
                    "the superseded comparison is supposed to be unmatched"
                )
            records.append(
                {
                    "record": "gate_comparison",
                    "tier": TIER,
                    "comparison": "superseded_unmatched_false_positive_rate",
                    "superseded": (
                        "the fourteenfold history factor this row reproduces compares rules "
                        "at different nominal false-positive rates and is not a comparison "
                        "of gate designs; use the matched_false_positive_rate rows"
                    ),
                    **comparison,
                }
            )
    return records


# ---- compute --------------------------------------------------------------------
def compute(
    replications=REPLICATIONS,
    joint_replications=JOINT_REPLICATIONS,
    jobs=1,
    sweep_dir=SWEEP_DIR,
    rhos_two_leg=RHOS_TWO_LEG,
    rhos_joint=RHOS_JOINT,
):
    panels = [(dataset, panel_bar(sweep_dir, dataset)) for dataset in PANELS]
    geometries = {}
    for _, panel in panels:
        bars = geometries.setdefault(panel["window_len"], [])
        if panel["deflation_bar_per_period"] not in bars:
            bars.append(panel["deflation_bar_per_period"])
    for rho in rhos_joint:
        if rho not in rhos_two_leg:
            raise jg.PowerSupportError(f"rho {rho} is simulated jointly but not for two legs")

    timing = {}
    started = time.perf_counter()
    pool = Pool(jobs) if jobs > 1 else None
    timing["worker_startup_seconds"] = round(time.perf_counter() - started, 1)
    print(f"{jobs} workers up in {timing['worker_startup_seconds']}s; two-leg: "
          f"{replications} replications over {len(rhos_two_leg)} rho",
          file=sys.stderr, flush=True)
    try:
        started = time.perf_counter()
        two_leg = simulate(False, rhos_two_leg, geometries, replications, CHUNKS, pool)
        timing["two_leg_seconds"] = round(time.perf_counter() - started, 1)
        print(f"two-leg done in {timing['two_leg_seconds']}s; joint: {joint_replications} "
              f"replications over {len(rhos_joint)} rho", file=sys.stderr, flush=True)
        started = time.perf_counter()
        joint = simulate(True, rhos_joint, geometries, joint_replications, JOINT_CHUNKS, pool)
        timing["joint_seconds"] = round(time.perf_counter() - started, 1)
        print(f"joint done in {timing['joint_seconds']}s", file=sys.stderr, flush=True)
    finally:
        if pool is not None:
            pool.close()
            pool.join()

    records = [
        {
            "record": "meta",
            "tier": TIER,
            "claim_status": (
                "development-stage calibration, exploratory only; not frozen validation and "
                "not a validated claim"
            ),
            "command": COMMAND,
            "kind": "protocol_property",
            "ticket": "P12-A",
            "legs_modelled": list(LEGS_MODELLED),
            "legs_not_modelled": list(LEGS_NOT_MODELLED),
            "unmodelled_legs_direction": (
                "every unmodelled leg is a further conjunct, so it can only refuse more: the "
                "printed pass probabilities are upper bounds on the shipped predicate's and "
                "the printed false-positive rates are upper bounds on its rate"
            ),
            "null": "zero-skill: a true per-period Sharpe of zero",
            "alternative": (
                "a constant true per-period Sharpe added to every window, the same in each"
            ),
            "dependence": (
                "stationary Gaussian AR(1) at the listed first-order autocorrelations, "
                "normalized to unit marginal standard deviation so the effect definition "
                "does not change with rho; windows are contiguous slices of one track"
            ),
            "seed": SEED,
            "replications_two_leg": replications,
            "replications_joint": joint_replications,
            "chunks_two_leg": CHUNKS,
            "chunks_joint": JOINT_CHUNKS,
            "rhos_two_leg": list(rhos_two_leg),
            "rhos_joint": list(rhos_joint),
            "interval_method": (
                "false-positive rates: Clopper-Pearson, one-sided alpha 0.05 on each side; "
                "power points: Wilson score at the same level"
            ),
            "numpy": np.__version__,
            "per_run_psr_bar": jg.PER_RUN_PSR_BAR,
            "dsr_bar": jg.DSR_BAR,
            "bootstrap_alpha": jg.BOOTSTRAP_ALPHA,
            "bootstrap_n_boot": jg.BOOTSTRAP_N_BOOT,
            "bootstrap_block_prob": jg.BOOTSTRAP_BLOCK_PROB,
            "bootstrap_keep": jg.bootstrap_keep(jg.BOOTSTRAP_ALPHA, jg.BOOTSTRAP_N_BOOT),
            "bootstrap_max_pooled_observations": BOOTSTRAP_MAX_POOLED,
        }
    ]

    for dataset, panel in panels:
        records.append(
            {
                "record": "panel",
                "tier": TIER,
                "dataset": dataset,
                **{field: panel[field] for field in PANEL_FIELDS},
                "field_size": panel["field_size"],
                "power_grid_annualized": power_grid(panel),
            }
        )

    for dataset, panel in panels:
        slot = geometries[panel["window_len"]].index(panel["deflation_bar_per_period"])
        periods = panel["periods_per_year"]
        for rho in rhos_two_leg:
            for rule in TWO_LEG_RULES:
                ann = annualized(rule_thresholds(two_leg, rho, panel, slot, rule), periods)
                records.append(
                    false_positive_record(
                        dataset, rho, rule, ann, replications, panel["field_size"],
                        TWO_LEG_DRAWS,
                    )
                )
                records.extend(
                    power_records(dataset, rho, rule, ann, replications, panel, TWO_LEG_DRAWS)
                )
        if joint[(rhos_joint[0], panel["window_len"])][3] is None:
            records.append({
                "record": "leg_not_modelled",
                "tier": TIER,
                "dataset": dataset,
                "leg": "bootstrap",
                "pooled_observations": panel["pooled_observations"],
                "bootstrap_max_pooled": BOOTSTRAP_MAX_POOLED,
                "reason": (
                    "the pooled track is longer than the development tier affords at "
                    f"{jg.BOOTSTRAP_N_BOOT} resamples per replication; the joint rule is not "
                    "reported for this panel and its two-leg rows are an upper bound on the "
                    "shipped predicate's pass probability"
                ),
            })
            continue
        for rho in rhos_joint:
            for rule in JOINT_RULES:
                ann = annualized(rule_thresholds(joint, rho, panel, slot, rule), periods)
                records.append(
                    false_positive_record(
                        dataset, rho, rule, ann, joint_replications, panel["field_size"],
                        JOINT_DRAWS,
                    )
                )
                records.extend(
                    power_records(
                        dataset, rho, rule, ann, joint_replications, panel, JOINT_DRAWS
                    )
                )

    records.extend(gate_comparison_records())
    return records, timing


def write_jsonl(records, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        for record in records:
            handle.write(json.dumps(record, sort_keys=False) + "\n")
    print(f"wrote {os.path.relpath(path, ROOT)}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--replications", type=int, default=REPLICATIONS)
    parser.add_argument("--joint-replications", type=int, default=JOINT_REPLICATIONS)
    parser.add_argument("--sweep-dir", default=SWEEP_DIR)
    parser.add_argument("--evidence", default=EVIDENCE)
    parser.add_argument("--runtime-out", default=None)
    parser.add_argument(
        "--rhos-two-leg",
        default=",".join(str(rho) for rho in RHOS_TWO_LEG),
        help="comma-separated first-order autocorrelations for the two-leg run",
    )
    parser.add_argument(
        "--rhos-joint",
        default=",".join(str(rho) for rho in RHOS_JOINT),
        help="comma-separated autocorrelations for the three-leg run",
    )
    args = parser.parse_args(argv)
    try:
        records, timing = compute(
            args.replications,
            args.joint_replications,
            args.jobs,
            args.sweep_dir,
            tuple(float(value) for value in args.rhos_two_leg.split(",")),
            tuple(float(value) for value in args.rhos_joint.split(",")),
        )
        write_jsonl(records, args.evidence)
    except jg.PowerSupportError as exc:
        sys.exit(str(exc))
    timing["jobs"] = args.jobs
    timing["replications_two_leg"] = args.replications
    timing["replications_joint"] = args.joint_replications
    print("runtime " + json.dumps(timing, sort_keys=True))
    if args.runtime_out:
        with open(args.runtime_out, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(json.dumps(timing, sort_keys=True, indent=2) + "\n")


if __name__ == "__main__":
    main()
