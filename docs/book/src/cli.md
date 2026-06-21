# CLI reference

The `sharpebench` binary (crate `sb-cli`) is the command-line entry point.

```text
sharpebench run                       run reference agents through the sim and rank them
sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions
sharpebench commit <agent> <window> <digest> <salt>   forward-attestation pre-registration
sharpebench stress                    run the adversarial stress suite (contamination-masked)
sharpebench audit                     self-audit: prove the scorer resists gaming
sharpebench sign <subs.json> <key> <out.json>         score + sign a board to a file
sharpebench verify <board.json> <key> verify a signed board's chain
```

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
