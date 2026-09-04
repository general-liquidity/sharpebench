# Formal model

This Lean project verifies selected invariants behind SharpeBench's prospective
forecast-quality report:

- exact common support is an intersection;
- attaching a forecast report cannot alter the trading-rank projection;
- sorted fixed-point Holm adjusted values cannot decrease and stay capped; and
- the finite-bootstrap plus-one numerator and denominator are positive, with
  the numerator bounded by the denominator.

Build it with:

```console
cd formal
lake build
```

The proofs model decisions in `crates/sharpebench-core/src/forecast.rs`. They
are not an extraction of Rust semantics. Executable conformance tests remain
responsible for connecting the formal model to the implementation.
