# Memory and retrieval benchmark

`sharpebench-memory` applies the suite's skill-versus-luck discipline to a memory
or retrieval layer. It is a pure library over caller-supplied outcomes; it does
not run an agent, retrieve documents, or own a store.

## Three-arm ablation

The required arms are:

1. **baseline** — no memory, the performance floor;
2. **retrieval** — the system under test; and
3. **oracle** — gold records only, the attainable ceiling.

The report includes retrieval lift, stationary-bootstrap significance through
`sharpebench-stats`, fraction of the oracle ceiling, and cost-normalized lift per
extra token and unit of latency. A larger raw lift can therefore rank below a
smaller one if it costs much more to obtain.

## Integrity legs

- **Poisoning:** behavior-integrity delta, attack success, and significance after
  corrupted records enter the retrieval set.
- **Multi-session dependency:** conditioned lift and dependency satisfaction when
  later sessions rely on facts written earlier.
- **Point-in-time correctness:** recall-audit counts and a hard leak flag for
  future information.
- **Confabulation:** regret from reinforced beliefs that were never retested and
  later resolved false.

All reductions and bootstrap seeds are deterministic. The crate currently has 40
unit tests and `#![forbid(unsafe_code)]`. Because it accepts outcome vectors rather
than executing an agent, any store or agent framework can feed it without becoming
a dependency of the benchmark.
