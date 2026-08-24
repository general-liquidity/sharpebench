#!/usr/bin/env python3
"""Figures computed from the committed evidence records.

Reads paper/evidence/final/*.jsonl and writes vector PDFs into paper/figures/.
Every plotted result and data-dependent crossing is reduced from records written
by the sweep, risk-managed evaluation, pass witness, or thousand-agent floor.
The horizontal 0.95 line is the protocol's declared eligibility bar, not an
estimated result; the records span a configuration grid, and the thousand-agent
diagnostic is scored at its observable field size of 1,000. Run from the paper/
directory:

    python src/make-evidence-figures.py                 # all four figures
    python src/make-evidence-figures.py pass-witness    # one figure
    python src/make-evidence-figures.py luck-floor-1000

Figure names: drawdowns, luck-deflation, pass-witness, luck-floor-1000, all.
"""
import json
import os
import sys

import matplotlib

matplotlib.use("pdf")
matplotlib.rcParams["pdf.fonttype"] = 42
import matplotlib.pyplot as plt
from matplotlib.lines import Line2D

HERE = os.path.dirname(os.path.abspath(__file__))
EV = os.path.join(HERE, "..", "evidence", "final")
OUT = os.path.join(HERE, "..", "figures")

INK = "#0b1220"
GREEN = "#098551"
RED = "#dc2626"
BLUE = "#1d4ed8"
GRAY = "#6b7280"

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


def load(name):
    path = os.path.join(EV, f"{name}.jsonl")
    out = []
    with open(path, encoding="utf-8") as h:
        for line in h:
            line = line.strip()
            if line.endswith("}"):
                out.append(json.loads(line))
    return out


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
    ax.spines["left"].set_color("#cbd5e1")
    ax.spines["bottom"].set_color("#cbd5e1")
    ax.set_axisbelow(True)
    ax.grid(axis="y", color="#e8edf3", linewidth=0.9)
    ax.tick_params(colors=INK, length=0)


def save(fig, name):
    fig.savefig(os.path.join(OUT, name), bbox_inches="tight")
    plt.close(fig)
    print(f"wrote {name}")


# ---- Figure A: worst-run drawdown per dataset, three agents --------------------
def fig_drawdowns():
    rm = load("risk-managed")
    fig, ax = plt.subplots(figsize=(7.8, 3.9))
    labels, bh, mo, rmv = [], [], [], []
    for ds, short in DATASETS:
        sweep = default_cell(load(ds))
        cell_rm = [r for r in default_cell(rm, ds) if r["agent_id"] == "risk-managed"]
        labels.append(short)
        bh.append(next(r["worst_run_drawdown"] for r in sweep if r["agent_id"] == "buy-and-hold"))
        mo.append(next(r["worst_run_drawdown"] for r in sweep if r["agent_id"] == "momentum"))
        rmv.append(cell_rm[0]["worst_run_drawdown"] if cell_rm else float("nan"))
    x = range(len(labels))
    w = 0.27
    ax.bar([i - w for i in x], bh, w, color=GRAY, label="buy-and-hold", zorder=3)
    ax.bar(list(x), mo, w, color=RED, label="momentum", zorder=3)
    ax.bar([i + w for i in x], rmv, w, color=GREEN, label="risk-managed", zorder=3)
    ax.axhline(0.20, color=INK, linewidth=1.1, linestyle=DASH)
    ax.text(len(labels) - 0.5, 0.215, "never-catastrophic bound (0.20)", ha="right",
            va="bottom", fontsize=9.5, color=INK)
    ax.set_xticks(list(x))
    ax.set_xticklabels(labels, fontsize=9.5, color=INK)
    ax.set_ylabel("worst single-window drawdown", fontsize=11, color=INK)
    ax.set_ylim(0, 1.05)
    ax.legend(frameon=False, fontsize=9.5, ncol=3, loc="upper left")
    style(ax)
    fig.tight_layout()
    save(fig, "evidence-drawdowns.pdf")


# ---- Figure B: best luck-floor DSR vs N on real data ---------------------------
def fig_luck_deflation():
    fig, ax = plt.subplots(figsize=(7.8, 3.9))
    for ds, short, color in [
        ("crypto-majors-1w", "crypto 1w", RED),
        ("rates-1d", "rates 1d", BLUE),
        ("us-indices-1w", "US eq 1w", GRAY),
    ]:
        recs = [r for r in load(ds) if r["sr_std_pinned"] is None and r["dsr_bar"] == DSR_BAR]
        ns = sorted({r.get("effective_n_trials", r["n_trials"]) for r in recs})
        ys = [max(r["deflated_sharpe"] for r in recs
                  if r.get("effective_n_trials", r["n_trials"]) == n
                  and r["agent_id"].startswith("luck")) for n in ns]
        ax.plot(ns, ys, color=color, linewidth=2.2, marker="o", markersize=4.5,
                label=f"best random agent, {short}", zorder=3)
    ax.axhline(DSR_BAR, color=INK, linewidth=1.1, linestyle=DASH)
    ax.text(200, 0.905, "eligibility bar", ha="right", va="top", fontsize=9.5, color=INK)
    ax.set_xscale("log")
    ax.set_xticks([8, 10, 50, 200])
    ax.set_xticklabels(["8", "10", "50", "200"], fontsize=10)
    ax.set_xlabel("effective trials deflated for", fontsize=11, color=INK)
    ax.set_ylabel("deflated Sharpe of the best zero-skill agent", fontsize=10.5, color=INK)
    ax.set_ylim(-0.03, 1.05)
    ax.legend(frameon=False, fontsize=9.5, loc="upper right")
    style(ax)
    fig.tight_layout()
    save(fig, "evidence-luck-deflation.pdf")


# ---- Figure C: the pass-witness boundary --------------------------------------
# Top panel: the witness's deflated Sharpe against the injected per-period edge,
# one curve per window geometry. Bottom panel: the two gate outcomes per edge,
# filled where the gate passes. The daily geometry separates the two crossings;
# on the sampled weekly grid they coincide.
def fig_pass_witness():
    recs = [r for r in load("pass-witness") if r["agent_id"] == "witness"]
    shapes = [("weekly-shaped", "weekly-shaped (six 77-bar windows)", BLUE),
              ("daily-shaped", "daily-shaped (six 409-bar windows)", RED)]
    fig, (ax, ax2) = plt.subplots(
        2, 1, figsize=(7.8, 5.0), sharex=True,
        gridspec_kw={"height_ratios": [3.0, 1.35], "hspace": 0.08})

    rows = []  # (y position, label, color, xs where the gate passes)
    for i, (shape, label, color) in enumerate(shapes):
        rs = sorted((r for r in recs if r["shape"] == shape),
                    key=lambda r: r["injected_sharpe_per_period"])
        xs = [r["injected_sharpe_per_period"] for r in rs]
        ys = [r["deflated_sharpe"] for r in rs]
        ax.plot(xs, ys, color=color, linewidth=2.2, marker="o", markersize=4.5,
                label=label, zorder=3)
        onset = min(r["injected_sharpe_per_period"] for r in rs if r["rank_eligible"])
        dsr_clear = min(r["injected_sharpe_per_period"] for r in rs
                        if r["deflated_sharpe"] >= DSR_BAR)
        for a in (ax, ax2):
            a.axvline(onset, color=color, linewidth=1.0, linestyle=(0, (2, 3)), zorder=1)
        ax.annotate(f"eligible from {onset:.2f}", (onset, 0.02), xytext=(4, 0),
                    textcoords="offset points", ha="left", va="bottom",
                    fontsize=9, color=color)
        base = 2 * (len(shapes) - 1 - i)
        rows.append((base + 1, "DSR $\\geq$ 0.95", color,
                     [r["injected_sharpe_per_period"] for r in rs
                      if r["deflated_sharpe"] >= DSR_BAR], xs, dsr_clear))
        rows.append((base, "pass$^k$ (rank-eligible)", color,
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

    for y, label, color, passing, xs, first in rows:
        failing = [x for x in xs if x not in passing]
        ax2.scatter(failing, [y] * len(failing), s=28, facecolors="white",
                    edgecolors=color, linewidths=1.2, zorder=3)
        ax2.scatter(passing, [y] * len(passing), s=32, color=color, zorder=4)
        ax2.text(xs[-1] + 0.012, y, f"from {first:.2f}", ha="left", va="center",
                 fontsize=8.5, color=color)
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
# the precommitted annualized lower bound. Left: the full [0, 1] axis with the
# bar; right: the same curves on the range the floor occupies.
def fig_luck_floor_1000():
    recs = load("luck-floor-1000")
    agents = [r for r in recs if r["record"] == "agent"]
    summaries = {r["dataset"]: r for r in recs if r["record"] == "summary"}
    series = [
        ("us-indices-1d", "dsr_shipped_floor", "US eq 1d, shipped path", GRAY, "-"),
        ("us-indices-1d", "dsr_field", "US eq 1d, unfloored diagnostic", GRAY, DASH),
        ("crypto-majors-1d", "dsr_shipped_floor", "crypto 1d, shipped path", RED, "-"),
        ("crypto-majors-1d", "dsr_field", "crypto 1d, unfloored diagnostic", RED, DASH),
    ]
    fig, (ax, ax2) = plt.subplots(1, 2, figsize=(7.8, 3.6),
                                  gridspec_kw={"width_ratios": [1.0, 1.3], "wspace": 0.28})
    for ds, field, label, color, ls in series:
        vals = sorted(r[field] for r in agents if r["dataset"] == ds)
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
    eligible = sum(s["shipped_floor"]["n_rank_eligible"] + s["field_measured"]["n_rank_eligible"]
                   for s in summaries.values())

    ax.axvline(DSR_BAR, color=INK, linewidth=1.1, linestyle=DASH)
    ax.text(DSR_BAR - 0.03, 1.0, "eligibility\nbar (0.95)", ha="right", va="top",
            fontsize=9, color=INK)
    ax.set_xlim(-0.02, 1.02)
    ax.set_ylim(0, 1.04)
    ax.set_xlabel("deflated Sharpe, full axis", fontsize=10.5, color=INK)
    ax.set_ylabel("fraction of the 1,000 random agents", fontsize=10.5, color=INK)
    ax.text(0.5, 0.06, f"{eligible} of {len(agents):,} agent-dataset cells\n"
            "eligible on either path", ha="center", va="bottom", fontsize=9, color=INK)
    ax.legend(frameon=False, fontsize=8.5, loc="center", bbox_to_anchor=(0.47, 0.6))
    style(ax)

    ax2.axvline(five, color=RED, linewidth=1.0, linestyle=(0, (2, 3)))
    ax2.text(five + 0.0008, 0.06, f"first-five streams\nmaximum ({five:.3f})", ha="left",
             va="bottom", fontsize=8.5, color=RED)
    ax2.annotate("Operational paths remain\nat or near zero",
                 (0.0, 0.55), xytext=(58, 0), textcoords="offset points", ha="left",
                 va="center", fontsize=8.5, color=INK,
                 arrowprops={"arrowstyle": "-", "color": INK, "linewidth": 0.8})
    ax2.annotate(f"1,000-agent maximum ({top:.3f})", (top, 1.0), xytext=(-6, -52),
                 textcoords="offset points", ha="right", va="top", fontsize=8.5,
                 color=RED, arrowprops={"arrowstyle": "-", "color": RED, "linewidth": 0.8})
    ax2.set_xlim(-0.001, 0.05)
    ax2.set_ylim(0, 1.04)
    ax2.set_xlabel("deflated Sharpe, floor range", fontsize=10.5, color=INK)
    style(ax2)
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
    for name in wanted:
        FIGURES[name]()
