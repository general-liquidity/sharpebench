"""Annualized Sharpe of each US equity index in the frozen dataset.

Uses the kernel's own convention: mean over sample standard deviation (ddof 1)
on per-period simple returns, zero risk-free rate, scaled by sqrt(252). This is
`sharpebench_stats::deflated_sharpe::sharpe_ratio` annualized, so the printed
numbers are directly comparable to the annualized-equivalent bars in Table 2.

Usage: python paper/evidence/index_sharpe.py
"""

import csv
import math
import statistics
from collections import defaultdict

PERIODS_PER_YEAR = 252.0
DATASET = "data/us-indices-1d.csv"


def main() -> None:
    closes = defaultdict(list)
    with open(DATASET, newline="") as fh:
        for row in csv.DictReader(fh):
            closes[row["symbol"]].append((row["date"], float(row["close"])))

    for symbol in sorted(closes):
        prices = [px for _, px in sorted(closes[symbol])]
        returns = [prices[i] / prices[i - 1] - 1.0 for i in range(1, len(prices))]
        sharpe = statistics.mean(returns) / statistics.stdev(returns)
        print(f"{symbol}\t{len(returns)}\t{sharpe * math.sqrt(PERIODS_PER_YEAR):.4f}")


if __name__ == "__main__":
    main()
