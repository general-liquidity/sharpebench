# Reference agent + the SharpeBench agent contract

SharpeBench agents are **external and language-agnostic** — a container or an
HTTP endpoint, in any language. The harness drives an agent through the same
two-message loop every decision step:

1. harness → agent: a **`MarketObservation`** (JSON), point-in-time.
2. agent → harness: a **`Decision`** (JSON).

`agent.py` here is the minimal honest implementation (equal-weight buy-and-hold).
Fork it, replace `decide`, ship it.

## Transports

The harness supports two transports; the JSON payloads are identical across both.

### stdio (this reference agent)

One `MarketObservation` JSON object per line on **stdin**; one `Decision` JSON
object per line on **stdout**. Keep stdout unbuffered/flushed (the loop is
line-synchronous). Driven by `sharpebench_sim::ExternalAgent::spawn(program, args)`.

```bash
docker build -t sharpebench-reference-agent .
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

On any connection or parse error, both transports degrade to a **hold** (empty
orders) — they never crash the harness.

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
  is impossible by construction — the harness never sends future rows.

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
- `target_weight` is the desired portfolio weight for the symbol in `[0, 1]`
  (signed for shorts); sizing is carried here, not by `action`.
- `confidence` ∈ `[0, 1]` (defaults to `0.5`) is your stated conviction — it is
  **scored for calibration** (Brier), so report it honestly: claiming 0.9 on
  coin-flips is penalized.
- `reasoning` is optional and captured for auditability.

Omitted symbols are left untouched. A `Decision` with no orders is a valid hold.
