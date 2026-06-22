#!/usr/bin/env python3
"""Reference SharpeBench agent — the simplest thing that honors the protocol.

Transport: **stdio**. Reads one ``MarketObservation`` (JSON) per line on stdin
and writes one ``Decision`` (JSON) per line on stdout. Strategy: equal-weight
buy-and-hold — the baseline every real agent must beat. The point of this file
is the *contract*, not the alpha; fork it and replace ``decide``.

Run directly:        python3 agent.py
Or via the harness:  it spawns this as a subprocess (see ../README.md).
"""
import json
import sys


def decide(obs: dict) -> dict:
    """MarketObservation -> Decision. Replace this body with your strategy."""
    symbols = obs.get("symbols", [])
    n = len(symbols)
    weight = 1.0 / n if n else 0.0
    orders = [
        {
            "symbol": s["symbol"],
            "action": "buy",
            "target_weight": weight,
            "confidence": 0.5,
        }
        for s in symbols
    ]
    return {"orders": orders, "reasoning": "equal-weight buy-and-hold"}


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            decision = decide(json.loads(line))
        except Exception:  # any bad input degrades to a hold, never crashes
            decision = {"orders": [], "reasoning": "parse error -> hold"}
        sys.stdout.write(json.dumps(decision) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
