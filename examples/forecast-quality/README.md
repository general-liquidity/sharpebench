# Forecast quality tutorial

This example is the independent consumer half of SharpeArena's executable
forecast-evidence tutorial. The two JSON ledgers were produced by
`sharpearena/examples/forecast-quality/tutorial.py`; SharpeBench imports the
versioned artifact contract and recomputes every score from raw predictions and
outcomes.

Run the frozen analysis from the SharpeBench repository root:

```bash
cargo run -q -p sharpebench -- forecast-quality \
  examples/forecast-quality/fixtures/agent-alpha.json \
  examples/forecast-quality/fixtures/agent-beta.json \
  --bootstrap-samples 400 \
  --seed 23 \
  --confidence 0.9 \
  --alpha 0.05 \
  --bins 5 \
  --json
```

The command must reproduce `fixtures/report.json`. The core test suite checks
the report byte-for-byte and verifies each producer artifact against
`fixtures/manifest.json`.

This is a deterministic compatibility fixture, not evidence that either named
agent was evaluated prospectively. The synthetic field is intentionally small:
eight exact-common-support questions in two resolution-time blocks. It also
demonstrates that a retained late revision is not scored and that forecast
quality remains separate from the trading leaderboard.

To update the producer artifacts after an intentional contract change:

1. Regenerate them from the SharpeArena example.
2. Copy the two evidence files and manifest into this directory.
3. Run the command above and replace `report.json` with its complete output.
4. Run both products' tutorial tests before committing either repository.
