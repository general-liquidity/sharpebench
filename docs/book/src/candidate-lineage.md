# Candidate lineage diagnostics

SharpeArena can generate a bounded pool of non-executable strategy candidates
inside one recorded search. Its v2 candidate ledger binds four facts that a
flat leaderboard loses:

- the exact generator identity;
- a host-derived strategy-family preimage and digest;
- references to earlier raw candidates, resolved to their content digests;
- citations to exact source digests registered by the operator before
  generation, with optional locator, revision, authors, and license metadata.

SharpeBench verifies that sidecar independently. It recomputes canonical JSON
hashes for the raw candidate, manifest, generator identity, family, base binding,
and lineage binding. It also derives the family preimage again from the raw
SharpeArena DSL, resolves parent IDs against earlier valid rows, checks every
summary count, and requires validation scores for exactly the selectable
candidates. A malformed or partial artifact produces no report.

```bash
sharpebench lineage strategy-evidence.json --json
```

The input is one completed SharpeArena strategy-search evidence record. A JSONL
journal containing exactly one nonblank record is accepted. A multi-record
journal is refused instead of silently choosing a run; extract the run you want
to inspect first.

The report contains the verified in-harness trial count, candidate ancestry,
cited sources, and one robustness row per host-derived strategy family. For
each family it reports the best and median validation-split median Deflated
Sharpe, plus their gap. A wide gap is evidence that one tuned variant carries
the family; a narrow gap is consistent with robustness across the variants that
were actually proposed and scored.

## Guarantee boundary

Lineage is a diagnostic contract, not a scoring input. Family grouping never
merges proposals, changes the DSR trial count, changes eligibility, or moves the
rank key. Invalid and duplicate proposals remain in the observed-trial
denominator. The scoring kernel continues to consume ordinary submissions and
does not import SharpeArena.

The verifier establishes internal consistency and the declared chain of
custody. It does not establish that a cited source caused an idea, that a source
is scientifically sound, or that searches outside the recorded Arena run did
not happen. A source digest proves which bytes were named only when the source
preimage is available through the recorded locator or another evidence channel.
The family rule is specific to the current closed SharpeArena strategy DSL; a
future DSL change must update both derivations or verification fails.
