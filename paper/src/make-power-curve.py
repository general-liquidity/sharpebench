#!/usr/bin/env python3
"""Power of the default eligibility gates, as a property of their statistics.

No agent, simulator, market data or model is involved. Under stated assumptions
(serially independent normal per-period returns with a known true Sharpe ratio)
this computes how often the shipped gates would pass, so a reader can see which
true Sharpe levels the protocol can and cannot tell apart from no skill.

Two legs of the default predicate are modelled, each through the kernel's own
formula (kernel_stats, pinned by test_kernel_stats.py):

- the regime-robustness leg, pass^k in its default mode: every one of six
  windows must reach a per-run PSR of at least 0.90 against zero (eq:psr, the
  plug-in standard error at the observed Sharpe, sqrt(n - 1), population-
  normalized skewness and non-excess kurtosis, the kernel's normal CDF). On the
  real panels the eight execution seeds of a window vary slippage only, so one
  run per window is modelled.
- the deflation leg: the pooled six-window track must reach DSR 0.95, holding
  each default panel's committed per-period deflation bar fixed.

The bootstrap, process and mandate legs are not modelled.

The Monte Carlo does not step through a grid of Sharpe levels. Skewness and
kurtosis do not change when a constant is added to a series, and for fixed
moments the observed Sharpe ratios that pass eq:psr form a half-line above one
root of a quadratic (min_passing_sharpe), so each simulated window has one
threshold true Sharpe at and above which it passes, solved in closed form. A gate passes at
true Sharpe s exactly when s reaches the largest threshold its test involves.
The pass probability at every s is therefore the empirical CDF of those
per-replication maxima, the 5 and 95 percent crossings are its quantiles, and
one set of draws gives the whole curve without interpolation.

tab:dsr-mde's minimum admissible Sharpe is not simulated. It is the smallest
observed Sharpe whose DSR reaches 0.95 against a panel's committed bar over its
committed pooled length, under normal moments (skewness 0, kurtosis 3), solved
from the same closed form.

Run from the repository root:

    python paper/src/make-power-curve.py                # compute, then draw
    python paper/src/make-power-curve.py compute [--jobs N]
    python paper/src/make-power-curve.py figure         # draw from the committed JSONL

The output does not depend on --jobs: the draws are split into a fixed number
of seeded chunks, and chunk results are assembled in a fixed order.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from multiprocessing import Pool

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import kernel_stats  # noqa: E402

ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
EVIDENCE_DIR = os.path.join(ROOT, "paper", "evidence", "final")
EVIDENCE = os.path.join(EVIDENCE_DIR, "power-curve.jsonl")
FIGURE = os.path.join(ROOT, "paper", "figures", "power-curve.pdf")
COMMAND = "python paper/src/make-power-curve.py"

SEED = 20260915
REPLICATIONS = 200_000
CHUNKS = 64
BATCH = 125
N_WINDOWS = 6
PER_RUN_PSR_BAR = 0.90
DSR_BAR = 0.95
CROSSINGS = (5, 50, 95)
AT_SHARPE = (1.0, 2.0)
GRID_STEP = 0.01
GRID_MAX = 4.0
DIGITS = 6

# The figure's two geometries, as the contract names them: the daily
# us-indices-1d windows and the witness's weekly windows.
CURVES = (
    ("daily", 252.0, 408),
    ("weekly", 52.0, 77),
)

# The synthetic witness draws eight independent execution streams per window,
# so its pass^k leg asks all 48 runs to pass, not six. Summarized, not drawn.
WITNESS_CONSTRUCTION = (
    ("witness-daily", 252.0, 409, 8),
    ("witness-weekly", 52.0, 77, 8),
)

# The nine default panels of tab:eligibility and the default cell that selects
# each one's committed bar.
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
DEFAULT_CELL = {
    "dsr_bar": DSR_BAR,
    "n_trials": 50,
    "sr_std_pinned": None,
    "min_field_for_measured_sr_std": 5,
}


class PowerSupportError(ValueError):
    """The committed records do not support the requested computation."""


# ---- closed forms ---------------------------------------------------------------
def min_passing_sharpe(n, skew, kurt, z_bar, benchmark):
    """The smallest observed per-period Sharpe u with eq:psr at `benchmark`
    reaching the CDF level whose z threshold is `z_bar`.

    With a = (kurt - 1) / 4, the gate (u - b) sqrt(n - 1) >= z sqrt(1 - skew u +
    a u^2) with u > b squares to A u^2 + B u + C >= 0, where A = n - 1 - z^2 a,
    B = z^2 skew - 2 b (n - 1) and C = b^2 (n - 1) - z^2. The quadratic is
    -z^2 (1 - skew b + a b^2) < 0 at u = b, so b lies strictly between its roots
    and the passing set is [larger root, infinity) whenever A > 0. The larger
    root is taken in the form that avoids cancellation. Works elementwise on
    arrays.
    """
    n = float(n)
    a = (np.asarray(kurt, dtype=float) - 1.0) / 4.0
    z2 = z_bar * z_bar
    big_a = (n - 1.0) - z2 * a
    if np.any(big_a <= 0.0):
        raise PowerSupportError(
            f"kurtosis too large for a closed-form threshold over {int(n)} returns"
        )
    big_b = z2 * np.asarray(skew, dtype=float) - 2.0 * benchmark * (n - 1.0)
    big_c = benchmark * benchmark * (n - 1.0) - z2
    root = np.sqrt(big_b * big_b - 4.0 * big_a * big_c)
    q = -0.5 * (big_b + np.where(big_b >= 0.0, root, -root))
    return np.maximum(q / big_a, big_c / q)


def threshold_true_sharpe(z, z_bar, benchmark):
    """Per series (last axis) of standard-normal noise, the true per-period
    Sharpe s at and above which s + z passes the PSR gate.

    The kernel's Sharpe is the mean over the n - 1 standard deviation, and its
    skewness and kurtosis are population-normalized; all three are computed
    from z because adding s leaves the dispersion and shape unchanged.
    """
    n = z.shape[-1]
    center = z.mean(axis=-1)
    dev = z - center[..., None]
    dev2 = dev * dev
    m2 = dev2.mean(axis=-1)
    skew = (dev2 * dev).mean(axis=-1) / m2**1.5
    kurt = (dev2 * dev2).mean(axis=-1) / (m2 * m2)
    sd = np.sqrt(m2 * n / (n - 1.0))
    u = min_passing_sharpe(n, skew, kurt, z_bar, benchmark)
    return u * sd - center


# ---- Monte Carlo ------------------------------------------------------------------
def geometry_key(window_len, runs_per_window):
    return (N_WINDOWS, window_len, runs_per_window)


def simulate_chunk(task):
    """One seeded chunk: per replication, the pass^k threshold and one DSR
    threshold per bar. The seed depends on the geometry and the chunk index
    only, so a geometry shared by a curve and a panel is drawn once."""
    window_len, runs, bars, chunk, reps = task
    seq = np.random.SeedSequence(
        entropy=SEED, spawn_key=(*geometry_key(window_len, runs), chunk)
    )
    rng = np.random.Generator(np.random.PCG64(seq))
    z_k = kernel_stats.norm_cdf_inverse(PER_RUN_PSR_BAR)
    z_d = kernel_stats.norm_cdf_inverse(DSR_BAR)
    passk, dsr = [], [[] for _ in bars]
    done = 0
    while done < reps:
        size = min(BATCH, reps - done)
        z = rng.standard_normal((size, N_WINDOWS * runs, window_len))
        passk.append(threshold_true_sharpe(z, z_k, 0.0).max(axis=1))
        if bars:
            pooled = z.reshape(size, N_WINDOWS * runs * window_len)
            for slot, bar in enumerate(bars):
                dsr[slot].append(threshold_true_sharpe(pooled, z_d, bar))
        done += size
    return np.concatenate(passk), [np.concatenate(d) for d in dsr]


def simulate(geometries, replications, jobs):
    """{(window_len, runs): (passk thresholds, [dsr thresholds per bar])}."""
    if replications % CHUNKS:
        raise PowerSupportError(f"replications {replications} not a multiple of {CHUNKS}")
    reps = replications // CHUNKS
    tasks = [
        (window_len, runs, bars, chunk, reps)
        for (window_len, runs), bars in geometries.items()
        for chunk in range(CHUNKS)
    ]
    if jobs > 1:
        with Pool(jobs) as pool:
            results = pool.map(simulate_chunk, tasks, chunksize=1)
    else:
        results = [simulate_chunk(t) for t in tasks]
    out = {}
    for index, key in enumerate(geometries):
        parts = results[index * CHUNKS:(index + 1) * CHUNKS]
        n_bars = len(geometries[key])
        out[key] = (
            np.concatenate([p[0] for p in parts]),
            [np.concatenate([p[1][slot] for p in parts]) for slot in range(n_bars)],
        )
    return out


def crossing(sorted_ann, percent):
    """The smallest true Sharpe at which the pass probability reaches
    `percent`, with a normal-approximation binomial order-statistic 95% band."""
    r = len(sorted_ann)
    rank = -(-percent * r // 100)
    p = percent / 100.0
    half = 1.959963984540054 * math.sqrt(r * p * (1.0 - p))
    lo = max(1, math.floor(r * p - half))
    hi = min(r, math.ceil(r * p + half))
    return (
        round(float(sorted_ann[rank - 1]), DIGITS),
        round(float(sorted_ann[lo - 1]), DIGITS),
        round(float(sorted_ann[hi - 1]), DIGITS),
    )


def pass_probability(sorted_ann, sharpe):
    """P(pass) at a true annualized Sharpe: the share of thresholds at or below it."""
    r = len(sorted_ann)
    count = int(np.searchsorted(sorted_ann, sharpe, side="right"))
    p = count / r
    return p, round(math.sqrt(p * (1.0 - p) / r), DIGITS)


def leg_summary(thresholds_per_period, periods_per_year):
    ann = np.sort(thresholds_per_period * math.sqrt(periods_per_year))
    out = {}
    for percent in CROSSINGS:
        at, lo, hi = crossing(ann, percent)
        out[f"sharpe_at_{percent}pct"] = at
        out[f"sharpe_at_{percent}pct_band95"] = [lo, hi]
    for sharpe in AT_SHARPE:
        p, se = pass_probability(ann, sharpe)
        tag = f"{sharpe:.1f}".replace(".", "p")
        out[f"pass_probability_at_{tag}"] = p
        out[f"pass_probability_at_{tag}_se"] = se
    return out, ann


# ---- committed bars -------------------------------------------------------------
def load_jsonl(path):
    if not os.path.exists(path):
        raise PowerSupportError(f"missing evidence file: {path}")
    rows = []
    with open(path, encoding="utf-8") as h:
        for number, line in enumerate(h, start=1):
            if not line.strip():
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as exc:
                raise PowerSupportError(f"{path}:{number}: not a JSON record: {exc}") from exc
            if not isinstance(record, dict):
                raise PowerSupportError(f"{path}:{number}: not an object")
            rows.append(record)
    if not rows:
        raise PowerSupportError(f"no records in {path}")
    return rows


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


def panel_bar(sweep_dir, dataset):
    """The default cell's bar and geometry, required to be one value across the
    field: a bar is a field-level quantity, and two would mean two cells."""
    rows = [
        r for r in load_jsonl(os.path.join(sweep_dir, f"{dataset}.jsonl"))
        if all(r.get(k) == v for k, v in DEFAULT_CELL.items())
    ]
    if not rows:
        raise PowerSupportError(f"no default-cell records for {dataset}")
    values = {tuple(r[f] for f in PANEL_FIELDS) for r in rows}
    if len(values) != 1:
        raise PowerSupportError(
            f"{dataset}: the default cell carries {len(values)} distinct bars or geometries"
        )
    panel = dict(zip(PANEL_FIELDS, values.pop()))
    if panel["n_windows"] != N_WINDOWS:
        raise PowerSupportError(f"{dataset}: {panel['n_windows']} windows, not {N_WINDOWS}")
    if panel["pooled_observations"] != panel["n_windows"] * panel["window_len"]:
        raise PowerSupportError(f"{dataset}: pooled length is not windows times window length")
    if panel["deflation_null_mean_per_period"] != 0.0:
        raise PowerSupportError(f"{dataset}: a nonzero deflation null mean is not modelled")
    panel["agents"] = len(rows)
    return panel


def dsr_min_admissible(panel):
    """Smallest observed Sharpe (normal moments) with DSR at the committed bar >= 0.95."""
    u = float(
        min_passing_sharpe(
            panel["pooled_observations"],
            0.0,
            3.0,
            kernel_stats.norm_cdf_inverse(DSR_BAR),
            panel["deflation_bar_per_period"],
        )
    )
    return u, u * math.sqrt(panel["periods_per_year"])


# ---- compute --------------------------------------------------------------------
def compute(replications=REPLICATIONS, jobs=1, sweep_dir=EVIDENCE_DIR):
    panels = [(d, panel_bar(sweep_dir, d)) for d in PANELS]
    geometries = {}
    for _, p, n in CURVES:
        geometries.setdefault((n, 1), [])
    for _, p, n, runs in WITNESS_CONSTRUCTION:
        geometries.setdefault((n, runs), [])
    for _, panel in panels:
        bars = geometries.setdefault((panel["window_len"], 1), [])
        if panel["deflation_bar_per_period"] not in bars:
            bars.append(panel["deflation_bar_per_period"])
    draws = simulate(geometries, replications, jobs)

    records = [{
        "record": "meta",
        "command": COMMAND,
        "kind": "protocol_property",
        "assumptions": (
            "serially independent normal per-period returns with a known true Sharpe; "
            "pass^k in its default all-runs mode with one run per window; per-run PSR "
            "against zero at 0.90 and pooled DSR at 0.95 through eq:psr with sample "
            "moments; each panel's committed per-period deflation bar held fixed; the "
            "bootstrap, process and mandate legs not modelled"
        ),
        "seed": SEED,
        "replications": replications,
        "chunks": CHUNKS,
        "numpy": np.__version__,
        "n_windows": N_WINDOWS,
        "per_run_psr_bar": PER_RUN_PSR_BAR,
        "dsr_bar": DSR_BAR,
        "per_run_z_threshold": kernel_stats.norm_cdf_inverse(PER_RUN_PSR_BAR),
        "dsr_z_threshold": kernel_stats.norm_cdf_inverse(DSR_BAR),
        "grid_step_annualized": GRID_STEP,
        "grid_max_annualized": GRID_MAX,
    }]

    grid = [round(i * GRID_STEP, 2) for i in range(int(round(GRID_MAX / GRID_STEP)) + 1)]
    for name, ppy, n in CURVES:
        summary, ann = leg_summary(draws[(n, 1)][0], ppy)
        records.append({
            "record": "curve_summary", "geometry": name, "leg": "pass_k",
            "periods_per_year": ppy, "window_len": n, "n_windows": N_WINDOWS,
            "runs_per_window": 1, "replications": replications, **summary,
        })
        for sharpe in grid:
            p, se = pass_probability(ann, sharpe)
            records.append({
                "record": "curve_point", "geometry": name, "leg": "pass_k",
                "sharpe_annualized": sharpe,
                "sharpe_per_period": round(sharpe / math.sqrt(ppy), DIGITS),
                "pass_probability": p, "standard_error": se,
            })

    for name, ppy, n, runs in WITNESS_CONSTRUCTION:
        summary, _ = leg_summary(draws[(n, runs)][0], ppy)
        records.append({
            "record": "witness_construction_summary", "geometry": name, "leg": "pass_k",
            "periods_per_year": ppy, "window_len": n, "n_windows": N_WINDOWS,
            "runs_per_window": runs, "independent_runs": N_WINDOWS * runs,
            "replications": replications, **summary,
        })

    for dataset, panel in panels:
        key = (panel["window_len"], 1)
        passk, dsr_by_bar = draws[key]
        dsr = dsr_by_bar[geometries[key].index(panel["deflation_bar_per_period"])]
        ppy = panel["periods_per_year"]
        u, u_ann = dsr_min_admissible(panel)
        legs = {}
        for leg, thresholds in (
            ("pass_k", passk),
            ("dsr", dsr),
            ("both", np.maximum(passk, dsr)),
        ):
            legs[leg], _ = leg_summary(thresholds, ppy)
        records.append({
            "record": "panel", "dataset": dataset,
            **{k: panel[k] for k in PANEL_FIELDS},
            "default_cell_agents": panel["agents"],
            "dsr_min_admissible_sharpe_per_period": round(u, DIGITS + 3),
            "dsr_min_admissible_sharpe_annualized": round(u_ann, DIGITS),
            "replications": replications,
            "legs": legs,
        })
    return records


def write_jsonl(records, path):
    with open(path, "w", encoding="utf-8", newline="\n") as h:
        for record in records:
            h.write(json.dumps(record, sort_keys=False) + "\n")
    print(f"wrote {os.path.relpath(path, ROOT)}")


# ---- figure ---------------------------------------------------------------------
def fig_power(evidence=EVIDENCE, pdf=FIGURE):
    import matplotlib

    matplotlib.use("pdf")
    matplotlib.rcParams["pdf.fonttype"] = 42
    matplotlib.rcParams["ps.fonttype"] = 42
    import matplotlib.pyplot as plt

    records = load_jsonl(evidence)
    styles = {
        "daily": ("#0072B2", "-", "o"),
        "weekly": ("#D55E00", (0, (5, 3)), "s"),
    }
    fig, ax = plt.subplots(figsize=(7.8, 4.2))
    for name, ppy, n in CURVES:
        summary = [r for r in records if r["record"] == "curve_summary" and r["geometry"] == name]
        points = sorted(
            (r for r in records if r["record"] == "curve_point" and r["geometry"] == name),
            key=lambda r: r["sharpe_annualized"],
        )
        if len(summary) != 1 or not points:
            raise PowerSupportError(f"power-curve: no complete {name} curve in {evidence}")
        s = summary[0]
        if (s["periods_per_year"], s["window_len"]) != (ppy, n):
            raise PowerSupportError(f"power-curve: {name} curve is not the {n}-bar geometry")
        color, ls, marker = styles[name]
        xs = [r["sharpe_annualized"] for r in points]
        ys = [r["pass_probability"] for r in points]
        label = (
            f"{name}: {s['n_windows']} windows of {n} bars, {int(ppy)} per year\n"
            f"5% at {s['sharpe_at_5pct']:.2f}, 95% at {s['sharpe_at_95pct']:.2f}"
        )
        ax.plot(xs, ys, color=color, linestyle=ls, linewidth=2.0, marker=marker,
                markevery=25, markersize=5, label=label, zorder=3)
        for at in (s["sharpe_at_5pct"], s["sharpe_at_95pct"]):
            ax.plot([at, at], [0.0, 0.035], color=color, linewidth=1.4, zorder=3)
    for level, offset, va in ((0.05, 0.012, "bottom"), (0.95, -0.012, "top")):
        ax.axhline(level, color="#000000", linewidth=1.0, linestyle=(0, (1, 2)), zorder=2)
        ax.text(GRID_MAX, level + offset, f"{round(level * 100)}%", ha="right",
                va=va, fontsize=9.5, color="#000000")
    ax.set_xlim(0.0, GRID_MAX)
    ax.set_ylim(0.0, 1.02)
    ax.set_xlabel("true annualized Sharpe ratio (serially independent normal returns)",
                  fontsize=10.5)
    ax.set_ylabel("probability every window passes\nper-run PSR $\\geq$ 0.90 against zero",
                  fontsize=10.5)
    for sp in ("top", "right"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color("#bdbdbd")
    ax.spines["bottom"].set_color("#bdbdbd")
    ax.set_axisbelow(True)
    ax.grid(color="#e6e6e6", linewidth=0.9)
    ax.legend(frameon=False, fontsize=9, loc="upper left", labelspacing=0.9,
              bbox_to_anchor=(0.0, 0.91))
    fig.tight_layout()
    fig.savefig(pdf, bbox_inches="tight", metadata={"CreationDate": None, "ModDate": None})
    plt.close(fig)
    print(f"wrote {os.path.relpath(pdf, ROOT)}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("action", nargs="?", default="all", choices=("all", "compute", "figure"))
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--replications", type=int, default=REPLICATIONS)
    parser.add_argument("--sweep-dir", default=EVIDENCE_DIR)
    parser.add_argument("--evidence", default=EVIDENCE)
    parser.add_argument("--pdf", default=FIGURE)
    args = parser.parse_args(argv)
    try:
        if args.action in ("all", "compute"):
            write_jsonl(compute(args.replications, args.jobs, args.sweep_dir), args.evidence)
        if args.action in ("all", "figure"):
            fig_power(args.evidence, args.pdf)
    except PowerSupportError as exc:
        sys.exit(str(exc))


if __name__ == "__main__":
    main()
