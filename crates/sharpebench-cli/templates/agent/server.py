"""A minimal SharpeBench agent that speaks the HTTP protocol.

The harness sends `POST /decide` with one MarketObservation as the JSON body,
and the endpoint answers with one Decision as the JSON body. The harness opens
a fresh connection for every decision and may repeat a request after a
transport fault, so the same observation must always get the same decision.
Replace `decide` with your own policy.

Rules the harness checks:
- Every key in a decision must be one the schema names. An extra key is a
  protocol fault.
- At most one order per symbol, only for symbols the observation offered, with
  target_weight in [-1, 1] and an optional confidence in [0, 1].
- A decision must depend only on the observations of the current run and on
  your own earlier decisions in that run. Do not read the clock, draw ambient
  random numbers, or keep state between runs. The harness neither starts nor
  stops this endpoint, so anything it keeps in memory outlives every run.

Start it with `python server.py --port 8765`. It prints the address it listens
on, then serves until you stop it.
"""

import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

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


class DecideHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/decide":
            self.send_error(404, "only POST /decide is served")
            return
        length = int(self.headers.get("Content-Length", "0"))
        observation = json.loads(self.rfile.read(length))
        body = json.dumps(decide(observation)).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        # One line per decision on stderr would bury the startup line.
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765, help="0 picks a free port")
    options = parser.parse_args()
    server = HTTPServer((options.host, options.port), DecideHandler)
    host, port = server.server_address[:2]
    sys.stdout.write(f"listening on {host}:{port}\n")
    sys.stdout.flush()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
