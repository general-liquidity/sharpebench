# Forecast quality tutorial

This example is the independent consumer half of SharpeArena's executable
forecast-evidence tutorial. The JSON ledgers were produced by
`sharpearena/examples/forecast-quality/tutorial.py`; SharpeBench imports the
versioned artifact contract and recomputes every score from raw predictions and
outcomes.

The tutorial ships two fields built from the same synthetic forecasts:

| Field | Directory | Contracts | Settlement blocks | Comparison |
|---|---|---|---|---|
| supported | `fixtures/` | 12 | 6 | interval, p-value and Holm verdict reported |
| withheld | `fixtures/withheld/` | 8 | 2 | inference withheld, reason recorded |

The withheld field is the first eight questions of the supported field. It is
the pre-repair tutorial fixture, retained on purpose: with two resolution-time
blocks the block-resampling law places half its mass on single-block draws, so
a familywise level of 0.05 is finer than it can resolve, and the report says so
instead of publishing a zero-width interval and a p-value at the smoothing
floor. Four more blocks of the same forecasts are enough to clear that bar at
the default alpha; the supported field carries six, the same count as the
frozen prospective field under `paper/evidence/`.

Run the frozen analysis of the supported field from the SharpeBench repository
root:

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

The command must reproduce `fixtures/report.json`: twelve exact-common-support
questions in six blocks, a mean Brier loss of 0.1054 for `agent-alpha` against
0.2474 for `agent-beta`, an observed mean-loss difference of -0.1420 with a
90 percent percentile interval of [-0.2095, -0.0282], a two-sided
plus-one-corrected p-value of 4/401, and a familywise-significant Holm verdict
at alpha 0.05. The bootstrap has between-block variation to work with because
one block goes against `agent-alpha`. Pointing the same command at
`fixtures/withheld/` must reproduce `fixtures/withheld/report.json`, whose
comparison carries `inference_error` and no interval or p-value.

The core test suite checks both reports byte-for-byte and verifies each
producer artifact against its `manifest.json`.

This is a deterministic compatibility fixture, not evidence that either named
agent was evaluated prospectively. It also demonstrates that a retained late
revision is not scored, that a pre-open submission is rejected but kept, that
blind and consensus-visible exposures stay distinguishable, and that forecast
quality remains separate from the trading leaderboard.

To update the producer artifacts after an intentional contract change:

1. Regenerate them from the SharpeArena example. With `--sharpebench-dir`
   pointing at this checkout the script also writes both reports through the
   command above:

   ```bash
   python examples/forecast-quality/tutorial.py \
     --output-dir /tmp/sharpe-forecast-tutorial \
     --sharpebench-dir ../sharpebench
   ```

2. Copy the evidence files, manifests and reports for both fields into this
   directory, keeping the `withheld/` layout.
3. Run both products' tutorial tests before committing either repository.
