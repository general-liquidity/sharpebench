# Reference agent + the SharpeBench agent contract

SharpeBench agents are **external and language-agnostic**: a container or an
HTTP endpoint, in any language. The harness drives an agent through the same
two-message loop every decision step:

1. harness → agent: a **`MarketObservation`** (JSON), point-in-time.
2. agent → harness: a **`Decision`** (JSON).

`src/main.rs` here is the minimal honest implementation in Rust (equal-weight
buy-and-hold) using the typed `sharpebench-protocol`. Fork it, replace `decide`,
ship it. Any other language just matches the JSON shapes below.

## Transports

The harness supports two transports; the JSON payloads are identical across both.

### stdio (this reference agent)

One `MarketObservation` JSON object per line on **stdin**; one `Decision` JSON
object per line on **stdout**, flushed each line (the loop is line-synchronous).
Driven by `sharpebench_sim::ExternalAgent::spawn(program, args)`.

```bash
cargo run -p reference-agent                  # run it directly

# or containerize it (build from the repo root; it uses the workspace crate):
docker build -f examples/reference-agent/Dockerfile -t sharpebench-reference-agent .
docker run -i --rm sharpebench-reference-agent
```

### HTTP

A plain-HTTP endpoint that accepts `POST /decide` with a `MarketObservation`
body and returns a `Decision` body. Driven by `sharpebench_sim::HttpAgent::new("host:port")`
(loopback / in-sandbox; no TLS). Pseudocode:

```
POST /decide HTTP/1.1
Content-Type: application/json

{ ...MarketObservation... }   ->   200 OK   { ...Decision... }
```

The low-level `Agent` trait represents a failed call with an empty decision, but
transport health records whether that decision came from a fault. Checked field
paths convert the record into a typed failure: retryable runtime exhaustion
makes the sweep noncertifying, while an entrant protocol fault remains in the
pass^k denominator as a failing sentinel. A failed call is never ranked as an
ordinary hold.

## Wire format

### `MarketObservation` (harness → agent)

```json
{
  "date": "2025-01-02",
  "cash": 1.0,
  "symbols": [
    {
      "symbol": "AAPL",
      "close_history": [187.2, 188.0, 190.4],
      "fundamentals": { "pe": 28.1 },
      "news": ["Apple unveils ..."]
    }
  ],
  "portfolio": [
    { "symbol": "AAPL", "shares": 3.0, "avg_price": 188.0 }
  ]
}
```

- `close_history` is oldest-first and **point-in-time**: it only contains closes
  at or before `date`. `fundamentals` and `news` follow the same rule. Look-ahead
  is impossible by construction because the harness never sends future rows.

### `Decision` (agent → harness)

```json
{
  "orders": [
    { "symbol": "AAPL", "action": "buy", "target_weight": 0.5, "confidence": 0.7 }
  ],
  "reasoning": "optional free text, captured into the trajectory"
}
```

- `action` ∈ `"buy" | "sell" | "hold" | "close"` (lower-case).
- `target_weight` is the signed desired portfolio weight for the symbol in
  `[-1, 1]`; sizing is carried here, not by `action`.
- `confidence` ∈ `[0, 1]` (defaults to `0.5`) is your stated conviction. It is
  **scored for calibration** (Brier), so report it honestly: claiming 0.9 on
  coin-flips is penalized.
- `reasoning` is optional and captured for auditability.

Omitted symbols are left untouched. A `Decision` with no orders is a valid hold.
