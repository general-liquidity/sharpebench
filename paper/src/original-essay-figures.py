#!/usr/bin/env python3
"""Generate the SVG assets for the SharpeBench blog post — a luck-demotion bar
chart and the methodology equations (LaTeX via matplotlib mathtext).

Run from the repo root:  python scripts/blog/gen-sharpebench-assets.py
Outputs vector SVGs to public/blog/ (the site is light-only, so black math on a
transparent background renders correctly without any KaTeX dependency).
"""
import os

import matplotlib

matplotlib.use("svg")
matplotlib.rcParams["mathtext.fontset"] = "cm"   # Computer Modern → classic LaTeX look
matplotlib.rcParams["svg.fonttype"] = "path"     # embed glyphs as paths; no font dependency
import matplotlib.pyplot as plt

OUT = "public/blog"
os.makedirs(OUT, exist_ok=True)

INK = "#0b1220"     # near-black, for math + axis text (site background is white)
GREEN = "#098551"   # General Liquidity brand green
RED = "#dc2626"
GRAY = "#6b7280"


def render_eq(name: str, tex: str, fontsize: int = 24) -> None:
    fig = plt.figure(figsize=(0.1, 0.1))
    fig.text(0, 0, f"${tex}$", fontsize=fontsize, color=INK)
    fig.savefig(f"{OUT}/{name}.svg", bbox_inches="tight", pad_inches=0.08, transparent=True)
    plt.close(fig)


# ── equations ─────────────────────────────────────────────────────────────────
render_eq(
    "eq-psr",
    r"\widehat{PSR}(SR^{*}) \;=\; \Phi\left(\frac{(\widehat{SR}-SR^{*})\sqrt{n-1}}"
    r"{\sqrt{1-\hat{\gamma}_3\,\widehat{SR}+\frac{\hat{\gamma}_4-1}{4}\,\widehat{SR}^{2}}}\right)",
)
render_eq(
    "eq-deflation",
    r"SR^{*}_{0} \;=\; \sqrt{V[\{\widehat{SR}_n\}]}\,\left[(1-\gamma)\,Z^{-1}\left(1-\frac{1}{N}\right)"
    r"+\gamma\,Z^{-1}\left(1-\frac{1}{Ne}\right)\right]",
)
# pass^k and the eligibility gate are described in prose in the essay — mathtext's
# \left...\right parser is fussy about spacing there, and two display equations
# (the PSR and the deflation benchmark) carry the methodology visually.

# ── luck-demotion bar chart (real SharpeBench scores) ─────────────────────────
agents = ["skilled-momentum", "lucky-yolo", "ungated-bot"]
vals = [0.00202, 0.00411, 0.00202]
colors = [GREEN, RED, GRAY]
status = ["ranked #1", "fails pass$^{k}$", "risk-gate bypass"]

fig, ax = plt.subplots(figsize=(7.8, 4.7))
ax.bar(range(3), vals, color=colors, width=0.58, zorder=3)
ax.set_ylim(0, 0.0058)
# status + value stacked ABOVE each bar; agent names on the x-axis — nothing overlaps
for i, (v, s, c) in enumerate(zip(vals, status, colors)):
    ax.text(i, v + 0.00042, s, ha="center", va="bottom", fontsize=12, fontweight="bold", color=c)
    ax.text(i, v + 0.00014, f"{v:.5f}", ha="center", va="bottom", fontsize=10.5, color=c)
ax.set_xticks(range(3))
ax.set_xticklabels(agents, fontsize=11, color=INK)
ax.set_ylabel("raw return  /  period", fontsize=11.5, color=INK)
ax.tick_params(colors=INK, length=0)
for sp in ("top", "right"):
    ax.spines[sp].set_visible(False)
ax.spines["left"].set_color("#cbd5e1")
ax.spines["bottom"].set_color("#cbd5e1")
ax.set_axisbelow(True)
ax.grid(axis="y", color="#e8edf3", linewidth=0.9)
ax.margins(x=0.06)
fig.tight_layout()
fig.savefig(f"{OUT}/sharpebench-luck-demotion.svg", transparent=True, bbox_inches="tight")
plt.close(fig)

# ── deflation curve: the SAME track's deflated Sharpe vs. how many strategies were
# tried (N). Computed with the exact sharpebench-core formulas (stats.rs + deflated_sharpe.rs).
import math

GAMMA = 0.577_215_664_901_532_9


def _norm_cdf(x):
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


def _norm_ppf(p):  # Acklam's rational approximation — matches stats.rs::norm_ppf
    a = [-3.969683028665376e1, 2.209460984245205e2, -2.759285104469687e2,
         1.38357751867269e2, -3.066479806614716e1, 2.506628277459239e0]
    b = [-5.447609879822406e1, 1.615858368580409e2, -1.556989798598866e2,
         6.680131188771972e1, -1.328068155288572e1]
    c = [-7.784894002430293e-3, -3.223964580411365e-1, -2.400758277161838e0,
         -2.549732539343734e0, 4.374664141464968e0, 2.938163982698783e0]
    d = [7.784695709041462e-3, 3.224671290700398e-1, 2.445134137142996e0, 3.754408661907416e0]
    plow, phigh = 0.02425, 1 - 0.02425
    if p < plow:
        q = math.sqrt(-2 * math.log(p))
        return (((((c[0]*q+c[1])*q+c[2])*q+c[3])*q+c[4])*q+c[5]) / ((((d[0]*q+d[1])*q+d[2])*q+d[3])*q+1)
    if p <= phigh:
        q = p - 0.5; r = q*q
        return (((((a[0]*r+a[1])*r+a[2])*r+a[3])*r+a[4])*r+a[5])*q / (((((b[0]*r+b[1])*r+b[2])*r+b[3])*r+b[4])*r+1)
    q = math.sqrt(-2 * math.log(1 - p))
    return -(((((c[0]*q+c[1])*q+c[2])*q+c[3])*q+c[4])*q+c[5]) / ((((d[0]*q+d[1])*q+d[2])*q+d[3])*q+1)


def _expected_max_sharpe(sigma, n_trials):
    if n_trials <= 1 or sigma <= 0:
        return 0.0
    return sigma * ((1-GAMMA)*_norm_ppf(1-1/n_trials) + GAMMA*_norm_ppf(1-1/(n_trials*math.e)))


def _dsr(sr, track_len, n_trials, sigma_trials):
    sr_star = _expected_max_sharpe(sigma_trials, n_trials)
    denom = math.sqrt(max(1 - 0.0*sr + (3.0-1)/4*sr*sr, 1e-12))  # normal skew/kurt
    z = (sr - sr_star) * math.sqrt(track_len - 1) / denom
    return _norm_cdf(z)


SR, TRACK, SIGMA = 0.85, 150, 0.30   # an observed track strong enough to survive a few trials
Ns = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048]
dsrs = [_dsr(SR, TRACK, n, SIGMA) for n in Ns]
print("deflation curve (N, DSR):", [(n, round(v, 3)) for n, v in zip(Ns, dsrs)])

fig, ax = plt.subplots(figsize=(7.8, 4.3))
ax.plot(Ns, dsrs, color=GREEN, linewidth=2.4, marker="o", markersize=4.5, zorder=3)
ax.axhline(0.95, color=RED, linewidth=1.3, linestyle=(0, (5, 4)), zorder=2)
ax.text(Ns[-1], 0.96, "rank-eligibility bar", ha="right", va="bottom", fontsize=10.5, color=RED)
ax.set_xscale("log", base=2)
ax.set_xticks(Ns)
ax.set_xticklabels([str(n) for n in Ns], fontsize=9.5)
ax.set_xlabel("strategies / agents tried before this one was selected  (N)", fontsize=11, color=INK)
ax.set_ylabel("deflated Sharpe", fontsize=11.5, color=INK)
ax.set_ylim(0, 1.03)
ax.tick_params(colors=INK)
for sp in ("top", "right"):
    ax.spines[sp].set_visible(False)
ax.spines["left"].set_color("#cbd5e1")
ax.spines["bottom"].set_color("#cbd5e1")
ax.set_axisbelow(True)
ax.grid(color="#e8edf3", linewidth=0.9)
fig.tight_layout()
fig.savefig(f"{OUT}/sharpebench-deflation-curve.svg", transparent=True, bbox_inches="tight")
plt.close(fig)

print("wrote", sorted(f for f in os.listdir(OUT) if f.endswith(".svg")))
