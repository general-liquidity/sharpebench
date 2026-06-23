# CLI reference

The `sharpebench` binary (crate `sharpebench-cli`) is the command-line entry point.

```text
sharpebench run                       run reference agents through the sim and rank them
sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions
sharpebench commit <agent> <window> <digest> <salt>   forward-attestation pre-registration
sharpebench stress                    run the adversarial stress suite (contamination-masked)
sharpebench audit                     self-audit: prove the scorer resists gaming
sharpebench sign <subs.json> <key> <out.json>         score + sign a board to a file
sharpebench verify <board.json> <key> verify a signed board's chain
sharpebench capture <agent> <out.json>                capture an agent's raw-decision trajectory
sharpebench verify-trajectory <traj.json>             replay a trajectory → recompute its score
sharpebench audit-briefing <briefing.json>            audit a shared briefing for salience bias
sharpebench canary <seed>                             derive a do-not-train contamination tripwire
sharpebench score-allocation <alloc.json>             score a weight-vector trajectory (turnover)
sharpebench greeks <spot> <strike> <t> <r> <vol> <call|put>   Black-Scholes price + Greeks + tail-risk
```

Add `--json` to any command for machine-readable output.

## `run`

Runs the reference agents (buy-and-hold, momentum) through the point-in-time
simulator over multiple windows × seeds with costs on, and prints the ranked
board. The teaching demo: watch deflation and pass^k in action.

## `score`

Ranks a JSON field of pre-computed submissions (see
[Submitting an agent](submitting.md)). The board shows DSR, PSR, pass^k, process,
bootstrap p, and raw return, with a footer naming how many of the submitted agents
are eligible.

## `stress`

Runs the adversarial stress suite (flash-crash, whipsaw, …) with
contamination-masking so an agent can't fingerprint the scenario.

## `audit`

Runs the [benchmark self-audit](integrity.md). Exits non-zero if any known attack
is not demoted.

## `commit` / `sign` / `verify`

The [forward-attestation](attestation.md) surface: pre-register a strategy digest,
sign a published board, and verify a board's tamper-evident chain.

## `capture` / `verify-trajectory`

Capture an agent's raw per-seed×window decision trajectory to JSON, then have a
separate verifier replay it through the sim and recompute the score from the raw
decisions — a forged trajectory recomputes to a different number.

## `audit-briefing` / `canary` / `score-allocation` / `greeks`

Standalone analysis surfaces over the kernel: lint a shared briefing for
input-side salience bias, derive a do-not-train contamination tripwire, score a
target-allocation weight-vector trajectory (validity + L1 turnover), and price an
option with its Greeks and short-gamma/vega tail-risk classification.
