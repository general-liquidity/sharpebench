"""A minimal SharpeBench agent that speaks the stdio protocol.

The harness writes one MarketObservation as a JSON object per line on stdin.
The agent answers each one with one Decision as a JSON object per line on
stdout, and flushes after every line. Replace `decide` with your own policy.

Rules the harness checks:
- Every key in a decision must be one the schema names. An extra key is a
  protocol fault.
- At most one order per symbol, only for symbols the observation offered, with
  target_weight in [-1, 1] and an optional confidence in [0, 1].
- A decision must depend only on the observations of the current run and on
  your own earlier decisions in that run. Do not read the clock, draw ambient
  random numbers, or keep state in files between runs.
"""

import json
import sys

# How many closes the trend filter compares against.
LOOKBACK = 10


def is_rising(closes):
    """True when the last close is above the mean of the previous LOOKBACK closes."""
    if len(closes) <= LOOKBACK:
        return False
    previous = closes[-LOOKBACK - 1 : -1]
    return closes[-1] > sum(previous) / len(previous)


def decide(observation):
    """Hold an equal weight in every symbol whose close is above its recent mean."""
    symbols = observation["symbols"]
    if not symbols:
        return {"orders": []}
    weight = 1.0 / len(symbols)
    orders = []
    for snapshot in symbols:
        rising = is_rising(snapshot["close_history"])
        orders.append(
            {
                "symbol": snapshot["symbol"],
                "action": "buy" if rising else "close",
                "target_weight": weight if rising else 0.0,
            }
        )
    return {"orders": orders}


def main():
    for line in sys.stdin:
        if not line.strip():
            continue
        decision = decide(json.loads(line))
        sys.stdout.write(json.dumps(decision) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
