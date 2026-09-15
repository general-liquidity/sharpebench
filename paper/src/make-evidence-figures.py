#!/usr/bin/env python3
"""Figures computed from the committed evidence records.

Reads paper/evidence/final/*.jsonl and writes vector PDFs into paper/figures/.
Every plotted result and data-dependent crossing is reduced from records written
by the sweep, risk-managed evaluation, pass witness, or thousand-agent floor.
The horizontal 0.95 line is the protocol's declared eligibility bar, not an
estimated result; the records span a configuration grid, and the thousand-agent
diagnostic is scored at its observable field size of 1,000.

Uncertainty is drawn wherever the records carry a spread. Only the
luck-deflation figure has one: each trial count holds five luck-floor agents,
and their range is drawn beside the best of them. The other three plot
quantities with no spread in the committed records: a worst-window drawdown is
one maximum over all runs of a panel, the witness is one common-random-number
draw, and the thousand-agent ECDF is itself the whole field's distribution.

Colours are Okabe-Ito, and every contrast a figure's claim rests on also differs
by marker, line style or hatching, so it survives greyscale. Fonts are embedded
as TrueType and the PDF dates are omitted, so a regeneration is byte-identical.
Run from the repository root:

    python paper/src/make-evidence-figures.py                 # all four figures
    python paper/src/make-evidence-figures.py pass-witness    # one figure
    python paper/src/make-evidence-figures.py luck-floor-1000

Figure names: drawdowns, luck-deflation, pass-witness, luck-floor-1000, all.
"""
import json
import os
import sys

import matplotlib

matplotlib.use("pdf")
matplotlib.rcParams["pdf.fonttype"] = 42
matplotlib.rcParams["ps.fonttype"] = 42
matplotlib.rcParams["hatch.linewidth"] = 0.8
import matplotlib.pyplot as plt
from matplotlib.lines import Line2D

HERE = os.path.dirname(os.path.abspath(__file__))
EV = os.path.join(HERE, "..", "evidence", "final")
OUT = os.path.join(HERE, "..", "figures")

# Okabe-Ito.
INK = "#000000"
BLUE = "#0072B2"
ORANGE = "#E69F00"
GREEN = "#009E73"
VERMILLION = "#D55E00"
RULE = "#bdbdbd"
GRID = "#e6e6e6"

PDF_METADATA = {"CreationDate": None, "ModDate": None}

DSR_BAR = 0.95
DASH = (0, (5, 4))

DATASETS = [
    ("us-indices-1w", "US eq 1w"),
    ("us-indices-1d", "US eq 1d"),
    ("crypto-majors-1w", "crypto 1w"),
    ("crypto-majors-1d", "crypto 1d"),
    ("crypto-majors-4h", "crypto 4h"),
    ("crypto-majors-1h", "crypto 1h"),
    ("fx-majors-1d", "FX 1d"),
    ("commodities-1d", "cmdty 1d"),
    ("rates-1d", "rates 1d"),
]


class EvidenceSupportError(ValueError):
    """The committed records do not support the requested figure."""


# The two eligibility paths of the thousand-agent floor, as the per-agent record
# field and the summary block that stores the same count independently.
LUCK_FLOOR_PATHS = (
    ("shipped_floor", "rank_eligible_shipped_floor"),
    ("field_measured", "rank_eligible_field"),
)
LUCK_FLOOR_DATASETS = ("us-indices-1d", "crypto-majors-1d")


def load(name):
    path = os.path.join(EV, f"{name}.jsonl")
    if not os.path.exists(path):
        raise EvidenceSupportError(f"missing evidence file: {path}")
    out = []
    with open(path, encoding="utf-8") as h:
        for number, line in enumerate(h, start=1):
            line = line.strip()
            if not line:
                continue
            # Every nonempty line is a record. Skipping the ones that do not
            # end in "}" silently discarded truncated records, so a damaged
            # file rendered as a smaller but normal-looking figure.
            try:
                record = json.loads(line)
            except json.JSONDecodeError as exc:
                raise EvidenceSupportError(
                    f"{path}:{number}: not a JSON record: {exc}"
                ) from exc
            if not isinstance(record, dict):
                raise EvidenceSupportError(
                    f"{path}:{number}: record is {type(record).__name__}, not an object"
                )
            out.append(record)
    if not out:
        raise EvidenceSupportError(f"no records in {path}")
    return out


def require(rows, what):
    """Refuse an empty selection instead of rendering it as a complete result."""
    if not rows:
        raise EvidenceSupportError(f"no records select {what}")
    return rows


def only(rows, what):
    """One record per identity, rather than a first match or a blank placeholder."""
    require(rows, what)
    if len(rows) != 1:
        raise EvidenceSupportError(f"{len(rows)} records select {what}; expected one")
    return rows[0]


def eligible_union(agents, summaries):
    """Distinct (dataset, agent_id) cells eligible on either path.

    The paths overlap, so adding their counts double counts every cell eligible
    on both. Each marginal is checked against the summary record the producer
    stored independently, and identities must be unique for the union to mean
    anything.
    """
    union = set()
    for dataset, summary in summaries.items():
        rows = [r for r in agents if r["dataset"] == dataset]
        identities = {(r["dataset"], r["agent_id"]) for r in rows}
        if len(identities) != len(rows):
            raise EvidenceSupportError(
                f"luck-floor-1000: repeated agent identity in {dataset}"
            )
        for path, field in LUCK_FLOOR_PATHS:
            cells = {(r["dataset"], r["agent_id"]) for r in rows if r[field]}
            stored = summary[path]["n_rank_eligible"]
            if len(cells) != stored:
                raise EvidenceSupportError(
                    f"luck-floor-1000: {dataset} {path} eligibility is {len(cells)} "
                    f"in the records and {stored} in the summary"
                )
            union |= cells
    return len(union)


def luck_floor_field_size(agents, summaries):
    """The random-agent field size, established from the records themselves.

    The axis label used to assert 1,000 agents whatever the file held, so a
    file missing rows was renormalized onto a label it no longer supported.
    Each dataset's agent rows are counted and checked against the size the
    producer stored independently, and the datasets must agree, because one
    label describes both curves.
    """
    sizes = {}
    for dataset, summary in summaries.items():
        rows = [r for r in agents if r["dataset"] == dataset]
        declared = summary["n_agents"]
        if len(rows) != declared:
            raise EvidenceSupportError(
                f"luck-floor-1000: {dataset} has {len(rows)} agent records and its "
                f"summary declares {declared}"
            )
        sizes[dataset] = declared
    distinct = set(sizes.values())
    if len(distinct) != 1:
        raise EvidenceSupportError(
            "luck-floor-1000: datasets disagree on field size: "
            + ", ".join(f"{d}={n}" for d, n in sorted(sizes.items()))
        )
    return distinct.pop()


def default_cell(recs, dataset=None):
    """The sweep's default configuration. The risk-managed evaluation runs only
    that configuration and omits the grid fields, so records without them are
    already the default cell."""
    return [
        r for r in recs
        if r.get("kind", "gate") == "gate"
        and r.get("dsr_bar", DSR_BAR) == DSR_BAR and r.get("n_trials", 50) == 50
        and r.get("sr_std_pinned") is None
        and (dataset is None or r["dataset"] == dataset)
    ]


def style(ax):
    for sp in ("top", "right"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color(RULE)
    ax.spines["bottom"].set_color(RULE)
    ax.set_axisbelow(True)
    ax.grid(axis="y", color=GRID, linewidth=0.9)
    ax.tick_params(colors=INK, length=0)


def save(fig, name):
    fig.savefig(os.path.join(OUT, name), bbox_inches="tight", metadata=PDF_METADATA)
    plt.close(fig)
    print(f"wrote {name}")


# ---- Figure A: worst-run drawdown per dataset, three agents --------------------
# No spread is drawn: each bar is one maximum over every window and seed of a
# panel, and the records store only that maximum, not the per-run drawdowns.
def fig_drawdowns():
    rm = load("risk-managed")
    fig, ax = plt.subplots(figsize=(7.8, 3.9))
    labels, bh, mo, rmv = [], [], [], []
    for ds, short in DATASETS:
        sweep = default_cell(load(ds))
        labels.append(short)
        for agent, values, recs in (
            ("buy-and-hold", bh, sweep),
            ("momentum", mo, sweep),
            ("risk-managed", rmv, default_cell(rm, ds)),
        ):
            row = only(
                [r for r in recs if r["agent_id"] == agent],
                f"the {ds} {agent} default cell",
            )
            values.append(row["worst_run_drawdown"])
    x = range(len(labels))
    w = 0.27
    for offset, values, color, hatch, label in (
        (-w, bh, BLUE, "", "buy-and-hold"),
        (0.0, mo, ORANGE, "////", "momentum"),
        (w, rmv, GREEN, "....", "risk-managed"),
    ):
        ax.bar([i + offset for i in x], values, w, color=color, hatch=hatch,
               edgecolor=INK, linewidth=0.6, label=label, zorder=3)
    # The bound is named in the legend rather than beside its line, where every
    # label position overprinted a bar.
    ax.axhline(0.20, color=INK, linewidth=1.1, linestyle=DASH, zorder=4,
               label="never-catastrophic bound (0.20)")
    ax.set_xticks(list(x))
    ax.set_xticklabels(labels, fontsize=9.5, color=INK)
    ax.set_ylabel("worst single-window drawdown", fontsize=11, color=INK)
    ax.set_ylim(0, 1.05)
    ax.legend(frameon=False, fontsize=9, ncol=4, loc="lower left",
              bbox_to_anchor=(0.0, 1.0), handlelength=2.4, columnspacing=1.0,
              borderaxespad=0.2)
    style(ax)
    fig.tight_layout()
    save(fig, "evidence-drawdowns.pdf")


# ---- Figure B: best luck-floor DSR vs N on real data ---------------------------
# The line is the best of the field's luck-floor agents at each effective trial
# count; the vertical bar beneath each point runs down to the worst of them, so
# the spread across the zero-skill agents is visible. One agent's DSR is a
# pooled-track statistic: the records carry no per-seed or per-window spread
# and no interval for it.
LUCK_DEFLATION_SERIES = (
    ("crypto-majors-1w", "crypto 1w", VERMILLION, "s", "-", 1.0 / 1.06),
    ("rates-1d", "rates 1d", BLUE, "o", DASH, 1.0),
    ("us-indices-1w", "US eq 1w", INK, "^", (0, (1, 1.6)), 1.06),
)


def luck_floor_range(recs, n, ds):
    """(best, worst, count) of deflated Sharpe over the luck-floor agents at `n`
    trials. One record per agent: a repeated identity would let one agent stand
    for two and move the range the legend attributes to the field."""
    rows = require(
        [r for r in recs
         if r.get("effective_n_trials", r["n_trials"]) == n
         and r["agent_id"].startswith("luck")],
        f"{ds} luck-floor agents at {n} effective trials",
    )
    if len({r["agent_id"] for r in rows}) != len(rows):
        raise EvidenceSupportError(
            f"{ds}: repeated luck-floor agent identity at {n} effective trials"
        )
    values = [r["deflated_sharpe"] for r in rows]
    return max(values), min(values), len(values)


def fig_luck_deflation():
    fig, ax = plt.subplots(figsize=(7.8, 3.9))
    counts = set()
    for ds, short, color, marker, ls, nudge in LUCK_DEFLATION_SERIES:
        recs = require(
            [r for r in load(ds) if r["sr_std_pinned"] is None and r["dsr_bar"] == DSR_BAR],
            f"{ds} at the {DSR_BAR} bar with no pinned dispersion",
        )
        ns = sorted({r.get("effective_n_trials", r["n_trials"]) for r in recs})
        best, worst, count = zip(*(luck_floor_range(recs, n, ds) for n in ns))
        counts.update(count)
        # Series sharing a trial count are nudged apart on the log axis so their
        # range bars do not overprint; the tick labels stay at the true counts.
        xs = [n * nudge for n in ns]
        ax.vlines(xs, worst, best, color=color, linewidth=1.3, zorder=2)
        ax.plot(xs, best, color=color, linewidth=2.0, linestyle=ls, marker=marker,
                markersize=5, label=f"best random agent, {short}", zorder=3)
        ax.scatter(xs, worst, color=color, marker="_", s=60, linewidths=1.3, zorder=3)
    ax.axhline(DSR_BAR, color=INK, linewidth=1.1, linestyle=DASH)
    ax.text(200, 0.905, "eligibility bar", ha="right", va="top", fontsize=9.5, color=INK)
    ax.set_xscale("log")
    ax.set_xticks([8, 10, 50, 200])
    ax.set_xticklabels(["8", "10", "50", "200"], fontsize=10)
    ax.set_xlabel("effective trials deflated for", fontsize=11, color=INK)
    ax.set_ylabel("deflated Sharpe of zero-skill agents", fontsize=10.5, color=INK)
    ax.set_ylim(-0.03, 1.05)
    if len(counts) != 1:
        raise EvidenceSupportError(
            f"luck-deflation: luck-floor field sizes differ across cells: {sorted(counts)}"
        )
    handles, labels = ax.get_legend_handles_labels()
    handles.append(Line2D([], [], color=INK, linewidth=1.3, marker="_", markersize=8))
    labels.append(f"range over the {counts.pop()} random agents")
    ax.legend(handles, labels, frameon=False, fontsize=9.5, loc="center right")
    style(ax)
    fig.tight_layout()
    save(fig, "evidence-luck-deflation.pdf")


# ---- Figure C: the pass-witness boundary --------------------------------------
# Top panel: the witness's deflated Sharpe against the injected per-period edge,
# one curve per window geometry. Bottom panel: the two gate outcomes per edge,
# filled where the gate passes. On both geometries DSR clears several grid
# steps before pass^k does, so the two crossings are separated. The records are
# one common-random-number draw, so there is no replicate spread to draw.
WITNESS_SHAPES = (
    ("weekly-shaped", BLUE, "o", "-"),
    ("daily-shaped", VERMILLION, "s", "-."),
)


def witness_geometry(rs, shape):
    """The window geometry one shape's records declare, stated once for all."""
    geometry = {(r["n_windows"], r["window_len"]) for r in rs}
    if len(geometry) != 1:
        raise EvidenceSupportError(
            f"witness records of shape {shape} declare {len(geometry)} geometries"
        )
    return geometry.pop()


def fig_pass_witness():
    recs = [r for r in load("pass-witness") if r["agent_id"] == "witness"]
    fig, (ax, ax2) = plt.subplots(
        2, 1, figsize=(7.8, 5.0), sharex=True,
        gridspec_kw={"height_ratios": [3.0, 1.35], "hspace": 0.08})

    rows = []  # (y position, label, color, marker, xs where the gate passes, ...)
    for i, (shape, color, marker, ls) in enumerate(WITNESS_SHAPES):
        rs = require(sorted((r for r in recs if r["shape"] == shape),
                            key=lambda r: r["injected_sharpe_per_period"]),
                     f"witness records of shape {shape}")
        n_windows, window_len = witness_geometry(rs, shape)
        label = f"{shape} ({n_windows} windows of {window_len} bars)"
        xs = [r["injected_sharpe_per_period"] for r in rs]
        ys = [r["deflated_sharpe"] for r in rs]
        ax.plot(xs, ys, color=color, linewidth=2.2, linestyle=ls, marker=marker,
                markersize=4.5, label=label, zorder=3)
        onset = min(require([r["injected_sharpe_per_period"] for r in rs
                             if r["rank_eligible"]],
                            f"rank-eligible {shape} witness records"))
        dsr_clear = min(require([r["injected_sharpe_per_period"] for r in rs
                                 if r["deflated_sharpe"] >= DSR_BAR],
                                f"{shape} witness records at or above the {DSR_BAR} bar"))
        for a in (ax, ax2):
            a.axvline(onset, color=color, linewidth=1.0, linestyle=(0, (2, 3)), zorder=1)
        ax.annotate(f"eligible from {onset:.2f}", (onset, 0.02), xytext=(4, 0),
                    textcoords="offset points", ha="left", va="bottom",
                    fontsize=9, color=INK)
        base = 2 * (len(WITNESS_SHAPES) - 1 - i)
        rows.append((base + 1, "DSR $\\geq$ 0.95", color, marker,
                     [r["injected_sharpe_per_period"] for r in rs
                      if r["deflated_sharpe"] >= DSR_BAR], xs, dsr_clear))
        rows.append((base, "pass$^k$ (rank-eligible)", color, marker,
                     [r["injected_sharpe_per_period"] for r in rs if r["passed_k"]],
                     xs, onset))

    ax.axhline(DSR_BAR, color=INK, linewidth=1.1, linestyle=DASH)
    ax.text(0.0, DSR_BAR - 0.02, "DSR bar (0.95)", ha="left", va="top",
            fontsize=9.5, color=INK)
    ax.set_ylabel("deflated Sharpe of the witness", fontsize=10.5, color=INK)
    ax.set_ylim(-0.03, 1.05)
    handles, labels = ax.get_legend_handles_labels()
    handles += [Line2D([], [], marker="o", color=INK, linestyle=""),
                Line2D([], [], marker="o", markerfacecolor="white",
                       markeredgecolor=INK, linestyle="")]
    labels += ["gate passes (lower panel)", "gate fails (lower panel)"]
    ax.legend(handles, labels, frameon=False, fontsize=9.5, loc="center right")
    style(ax)

    for y, label, color, marker, passing, xs, first in rows:
        failing = [x for x in xs if x not in passing]
        ax2.scatter(failing, [y] * len(failing), s=28, marker=marker,
                    facecolors="white", edgecolors=color, linewidths=1.2, zorder=3)
        ax2.scatter(passing, [y] * len(passing), s=32, marker=marker, color=color,
                    zorder=4)
        ax2.text(xs[-1] + 0.012, y, f"from {first:.2f}", ha="left", va="center",
                 fontsize=8.5, color=INK)
    ax2.set_yticks([r[0] for r in rows])
    ax2.set_yticklabels([r[1] for r in rows], fontsize=9)
    ax2.set_ylim(-0.7, len(rows) - 0.3)
    ax2.set_xlabel("injected per-period Sharpe of the witness", fontsize=11, color=INK)
    ax2.set_xlim(-0.02, 0.70)
    style(ax2)
    ax2.grid(False)
    ax2.tick_params(axis="y", colors=INK, length=0)
    save(fig, "evidence-pass-witness.pdf")


# ---- Figure D: the thousand-agent luck floor ---------------------------------
# ECDF of the deflated Sharpe over 1,000 random agents per dataset. The raw
# measured path is a deliberately unfloored diagnostic; the shipped path applies
# the precommitted annualized lower bound. Two stacked rows at column width:
# the full [0, 1] axis with the bar on top, and beneath it the same curves on
# the range the diagnostic occupies, so the two marked maxima are in frame. No
# band is drawn: the ECDF is the whole committed field's distribution, and the
# two maxima it marks are single order statistics of that field.
def fig_luck_floor_1000():
    recs = load("luck-floor-1000")
    agents = [r for r in recs if r["record"] == "agent"]
    summaries = {r["dataset"]: r for r in recs if r["record"] == "summary"}
    for ds in LUCK_FLOOR_DATASETS:
        if ds not in summaries:
            raise EvidenceSupportError(f"luck-floor-1000: no summary record for {ds}")
        require([r for r in agents if r["dataset"] == ds],
                f"luck-floor-1000 agent records for {ds}")
    n_random = luck_floor_field_size(agents, summaries)
    # Datasets differ by colour and by line style, the shipped path from the
    # diagnostic by a solid against a broken line, so both contrasts survive
    # greyscale.
    series = [
        ("us-indices-1d", "dsr_shipped_floor", "US eq 1d, shipped path", BLUE, "-"),
        ("us-indices-1d", "dsr_field", "US eq 1d, unfloored diagnostic", BLUE, DASH),
        ("crypto-majors-1d", "dsr_shipped_floor", "crypto 1d, shipped path",
         VERMILLION, "-."),
        ("crypto-majors-1d", "dsr_field", "crypto 1d, unfloored diagnostic",
         VERMILLION, (0, (1, 1.5))),
    ]
    fig, (ax, ax2) = plt.subplots(2, 1, figsize=(5.5, 6.4),
                                  gridspec_kw={"hspace": 0.34})
    for ds, field, label, color, ls in series:
        vals = require(sorted(r[field] for r in agents if r["dataset"] == ds),
                       f"luck-floor-1000 {field} values for {ds}")
        if len(vals) != n_random:
            raise EvidenceSupportError(
                f"luck-floor-1000: {ds} {field} has {len(vals)} values for a field "
                f"of {n_random}"
            )
        ecdf = [(i + 1) / len(vals) for i in range(len(vals))]
        # Coincident near-zero paths are drawn at different widths so the lines
        # beneath remain visible.
        lw = 1.4 if (ds, field) == ("crypto-majors-1d", "dsr_shipped_floor") else 2.4
        for a in (ax, ax2):
            a.step([0.0] + vals, [0.0] + ecdf, where="post", color=color, linestyle=ls,
                   linewidth=lw, label=label, zorder=3)

    crypto = summaries["crypto-majors-1d"]["field_measured"]
    five = crypto["max_first_5"]
    top = crypto["max"]
    eligible = eligible_union(agents, summaries)

    ax.axvline(DSR_BAR, color=INK, linewidth=1.1, linestyle=DASH)
    ax.text(DSR_BAR - 0.02, 1.0, "eligibility\nbar (0.95)", ha="right", va="top",
            fontsize=9, color=INK)
    ax.set_xlim(-0.02, 1.02)
    ax.set_ylim(0, 1.04)
    ax.set_xlabel("deflated Sharpe, full axis", fontsize=10, color=INK)
    ax.set_ylabel(f"fraction of the {n_random:,} random agents", fontsize=10, color=INK)
    ax.text(0.62, 0.06, f"{eligible} of {len(agents):,} agent-dataset cells\n"
            "eligible on either path", ha="center", va="bottom", fontsize=9, color=INK)
    ax.legend(frameon=False, fontsize=9, loc="center", bbox_to_anchor=(0.62, 0.55))
    ax.tick_params(labelsize=9)
    style(ax)

    zoom_hi = max(five, top) * 1.06
    ax2.axvline(five, color=VERMILLION, linewidth=1.0, linestyle=(0, (2, 3)))
    ax2.text(five + 0.003, 0.04, f"first-five streams\nmaximum ({five:.3f})", ha="left",
             va="bottom", fontsize=9, color=INK)
    ax2.annotate("Operational paths remain\nat or near zero",
                 (0.0, 0.55), xytext=(40, 0), textcoords="offset points", ha="left",
                 va="center", fontsize=9, color=INK,
                 arrowprops={"arrowstyle": "-", "color": INK, "linewidth": 0.8})
    ax2.annotate(f"1,000-agent maximum ({top:.3f})", (top, 1.0), xytext=(-88, -14),
                 textcoords="offset points", ha="right", va="top", fontsize=9,
                 color=INK, arrowprops={"arrowstyle": "-", "color": INK, "linewidth": 0.8})
    ax2.set_xlim(-0.004, zoom_hi)
    ax2.set_ylim(0, 1.04)
    ax2.set_xlabel("deflated Sharpe, diagnostic range", fontsize=10, color=INK)
    ax2.set_ylabel(f"fraction of the {n_random:,} random agents", fontsize=10, color=INK)
    ax2.tick_params(labelsize=9)
    style(ax2)
    fig.subplots_adjust(top=0.96, bottom=0.08, left=0.13, right=0.97)
    save(fig, "evidence-luck-floor-1000.pdf")


FIGURES = {
    "drawdowns": fig_drawdowns,
    "luck-deflation": fig_luck_deflation,
    "pass-witness": fig_pass_witness,
    "luck-floor-1000": fig_luck_floor_1000,
}

if __name__ == "__main__":
    wanted = sys.argv[1:] or ["all"]
    if "all" in wanted:
        wanted = list(FIGURES)
    unknown = [w for w in wanted if w not in FIGURES]
    if unknown:
        sys.exit(f"unknown figure(s) {unknown}; choose from {list(FIGURES)} or all")
    try:
        for name in wanted:
            FIGURES[name]()
    except EvidenceSupportError as exc:
        sys.exit(str(exc))
