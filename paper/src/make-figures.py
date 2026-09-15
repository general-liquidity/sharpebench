#!/usr/bin/env python3
"""The two panels of the demonstration figure (fig:demotion, fig:deflation).

Panel (a), sharpebench-luck-demotion.pdf, is the shipped three-agent board. Every
bar height and every status label is read from
crates/sharpebench-core/golden/example_submissions.scores.json, the kernel's
score of suites/example_submissions.json, which the golden test
crates/sharpebench-core/tests/golden_scores.rs holds byte-identical to the
kernel's current output. No value is typed into this script.

Panel (b), sharpebench-deflation-curve.pdf, is one synthetic track (per-period
Sharpe 0.85 over 150 returns, normal moments) deflated against a growing trial
count at a cross-trial dispersion of 0.30 per period. It is computed through
kernel_stats, the kernel's PSR, expected-maximum-Sharpe and normal-quantile
functions transcribed in Python; test_kernel_stats.py pins that transcription to
the kernel's golden PSRs, to the Bailey and Lopez de Prado (2014) worked example
and to the committed tab:units bars.

Run from the repository root:

    python paper/src/make-figures.py
"""

import json
import os
import sys

import matplotlib

matplotlib.use("pdf")
matplotlib.rcParams["mathtext.fontset"] = "cm"
matplotlib.rcParams["pdf.fonttype"] = 42
matplotlib.rcParams["ps.fonttype"] = 42
matplotlib.rcParams["hatch.linewidth"] = 0.9
import matplotlib.pyplot as plt

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import kernel_stats  # noqa: E402

ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
OUT = os.path.join(ROOT, "paper", "figures")
GOLDEN = os.path.join(
    ROOT, "crates", "sharpebench-core", "golden", "example_submissions.scores.json"
)

# Okabe-Ito.
INK = "#000000"
BLUE = "#0072B2"
VERMILLION = "#D55E00"
PURPLE = "#CC79A7"
RULE = "#bdbdbd"
DASH = (0, (5, 4))

DEMO_AGENTS = ("skilled-momentum", "lucky-yolo", "ungated-bot")

SR, TRACK, SIGMA = 0.85, 150, 0.30
TRIALS = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048]
DSR_BAR = 0.95


class GoldenSupportError(ValueError):
    """The golden score record does not support the demonstration panel."""


def save(fig, name):
    fig.savefig(
        os.path.join(OUT, name),
        bbox_inches="tight",
        metadata={"CreationDate": None, "ModDate": None},
    )
    plt.close(fig)
    print(f"wrote {name}")


def style(ax):
    for sp in ("top", "right"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color(RULE)
    ax.spines["bottom"].set_color(RULE)
    ax.set_axisbelow(True)
    ax.tick_params(colors=INK)


def demo_status(score):
    """The one gate outcome that decides a demonstration agent's row.

    Each agent of the demonstration is meant to isolate one gate, so a record
    that fails more than one, or that is ineligible without failing either of
    the two named here, is refused rather than labelled with a guess.
    """
    failed = [
        name
        for name, ok in (("pass^k", score["passed_k"]), ("process", score["process_ok"]))
        if not ok
    ]
    agent = score["agent_id"]
    if score["rank_eligible"]:
        if failed:
            raise GoldenSupportError(f"{agent} is eligible yet fails {failed}")
        return "ranked", f"ranked #{score['rank_ordinal']}"
    if len(failed) != 1:
        raise GoldenSupportError(
            f"{agent} is ineligible and fails {failed or 'no named gate'}; "
            "expected exactly one of pass^k or the process gate"
        )
    if failed[0] == "pass^k":
        return "fails", "fails pass$^{k}$"
    return "zeroed", "zeroed by the process gate"


def load_demo():
    with open(GOLDEN, encoding="utf-8") as h:
        scores = json.load(h)
    rows = []
    for agent in DEMO_AGENTS:
        match = [s for s in scores if s["agent_id"] == agent]
        if len(match) != 1:
            raise GoldenSupportError(
                f"{len(match)} golden scores for {agent}; expected one"
            )
        rows.append(match[0])
    return rows


# ---- Panel (a): the shipped demonstration --------------------------------------
def fig_demotion():
    rows = load_demo()
    vals = [r["raw_mean_return"] for r in rows]
    kind, status = zip(*(demo_status(r) for r in rows))
    colors = {"ranked": BLUE, "fails": VERMILLION, "zeroed": PURPLE}
    hatches = {"ranked": "", "fails": "///", "zeroed": "xxx"}

    fig, ax = plt.subplots(figsize=(7.8, 4.7))
    for i, (v, k) in enumerate(zip(vals, kind)):
        ax.bar(i, v, width=0.58, color=colors[k], hatch=hatches[k],
               edgecolor=INK, linewidth=0.8, zorder=3)
    top = max(vals)
    ax.set_ylim(0, top * 1.41)
    for i, (v, s, k) in enumerate(zip(vals, status, kind)):
        ax.text(i, v + top * 0.10, s, ha="center", va="bottom", fontsize=12,
                fontweight="bold", color=INK)
        ax.text(i, v + top * 0.034, f"{v:.5f}", ha="center", va="bottom",
                fontsize=10.5, color=INK)
    ax.set_xticks(range(len(rows)))
    ax.set_xticklabels([r["agent_id"] for r in rows], fontsize=11, color=INK)
    ax.set_ylabel("raw mean return per period", fontsize=11.5, color=INK)
    style(ax)
    ax.tick_params(length=0)
    ax.grid(axis="y", color="#e6e6e6", linewidth=0.9)
    ax.margins(x=0.06)
    fig.tight_layout()
    save(fig, "sharpebench-luck-demotion.pdf")


# ---- Panel (b): one track against a growing trial count ------------------------
def deflation_curve():
    return [kernel_stats.deflated_sharpe_from_moments(SR, TRACK, n, SIGMA) for n in TRIALS]


def fig_deflation():
    dsrs = deflation_curve()
    fig, ax = plt.subplots(figsize=(7.8, 4.3))
    ax.plot(TRIALS, dsrs, color=BLUE, linewidth=2.4, marker="o", markersize=4.5,
            zorder=3, label="deflated Sharpe of the track")
    ax.axhline(DSR_BAR, color=INK, linewidth=1.3, linestyle=DASH, zorder=2)
    ax.text(TRIALS[-1], DSR_BAR + 0.01, "rank-eligibility bar (0.95)", ha="right",
            va="bottom", fontsize=10.5, color=INK)
    ax.set_xscale("log", base=2)
    ax.set_xticks(TRIALS)
    ax.set_xticklabels([str(n) for n in TRIALS], fontsize=9.5)
    ax.set_xlabel("strategies tried before this one was selected (N)", fontsize=11,
                  color=INK)
    ax.set_ylabel("deflated Sharpe", fontsize=11.5, color=INK)
    ax.set_ylim(0, 1.03)
    style(ax)
    ax.grid(color="#e6e6e6", linewidth=0.9)
    fig.tight_layout()
    save(fig, "sharpebench-deflation-curve.pdf")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    try:
        fig_demotion()
    except GoldenSupportError as exc:
        sys.exit(str(exc))
    fig_deflation()
