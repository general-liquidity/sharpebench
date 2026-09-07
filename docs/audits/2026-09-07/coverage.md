# Audit coverage and limits

> Historical audit evidence at [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
> Findings, quoted claims, test counts and references to "current" describe those
> reviewed baselines. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for later repair
> dispositions and remaining work; this report is not a current defect list.

This ledger separates discovery, source review, execution, and empirical replication. They are different kinds of evidence.

## Inventory

[The inventory](inventory.md) lists all 377 SharpeBench and 380 SharpeArena tracked files at the commits in the main report. All 25 Bench and 166 Arena tracked Python files were syntax-parsed. Arena's public Python package contains 68 tracked Python modules; these were partitioned across the root review and the interface/diagnostic reviewer slices.

An inventory or syntax parse does not count as a manual correctness review. Generated package glue, binaries, PDFs, lockfile entries, and data rows were not claimed as manually read source merely because their files were inventoried or hashed.

## Source review

| Area | Review performed |
| --- | --- |
| Bench scoring and statistics | Core eligibility, support filtering, seed pooling, per-run mandates, pass-k, DSR/PSR, bootstrap/FDR, comparison sets, forecast contracts/scoring, auxiliary diagnostic modules, edge honesty, and memory evaluators. Critical functions were traced through callers and bindings, not reviewed as isolated formulas. |
| Bench simulation and containment | Dataset parsing/transforms, windows, costs, target execution, environment, external stdio/HTTP, container launch/inspection/removal, failure accounting, resumable checkpoints, capture/replay, and reference/team agents. |
| Bench integrity and public surfaces | Commitments, sealed data, HMAC/Ed25519 chains, public board verification, candidate lineage, evidence coverage, CLI commands, Python, WASM, TypeScript wrapper and MCP server. |
| Arena native engine | Scenario generation, execution/noise, mandates, information cursor, market impact, LOB matching and queue priority, vector stepping, contract/transport boundary, seed derivation, cross-surface hash generation, and statistical confidence implementation. |
| Arena Python package | Environment/action/reset wrappers and integrations; dataset/prompt and training path; observation/causal transforms; checkpoints and functional replay; real-data sampling; task/baseline/metric diagnostics; strategy and edge manifests; forecast/paper execution; local field, operational accounting, and Bench bridge; trace promotion and identity guards. |
| Build, release and evidence integrity | Both release drivers, provenance generators/checkers and shared rules, release/version manifests, CI definitions and representative package smoke tests. Bench's complete `xtask` ingestion body and Python data acquisition/weekly-derivation scripts were read, without fetching data. |
| Documentation and formal scope | Both READMEs, main paper sections and appendices, relevant current operator/methodology/API documentation, and all authored Lean model files. Claims were compared with execution paths; historical and superseded material was distinguished from current capability. |
| Evidence producers | Both products' authored figure/evidence producers and Bench's harness examples receive their own final review appendices. Frozen principal Bench sweeps were independently checked for Cartesian-cell uniqueness/completeness. |

The root review combined full-module reading with focused tracing of large files. It did not repeat a complete line-by-line review of every inline test. Exact full-file lists for the independent slices, their category coverage, and their limits are preserved in the appendices rather than replaced with a blanket “100% audited” claim:

- [Bench recent, safety and methodology review](bench-reviewer.md).
- [Bench public interface review](bench-interfaces.md).
- [Arena recent accounting and interface review](arena-reviewer.md).
- [Arena diagnostics review](arena-diagnostics.md).
- [Bench evidence producers](bench-producers.md): all ten harness examples, the Python model shim, and four paper scripts, 4,393 lines.
- [Arena evidence producers](arena-producers.md): all fourteen `make-*.py` files, 4,230 lines, plus the 521-line provenance-common module.

## Executed checks

The main report's verification table is the authoritative result ledger. In particular:

- Rust product tests and Clippy used current source. Bench's developer-only `xtask` could not compile because local OpenSSL development files are absent; it was source-reviewed, not treated as tested.
- Arena's Python tests used current Python source with the existing Windows native extension. This is not a freshly rebuilt wheel-consumer check.
- npm tests used the existing built distribution/committed WASM. Fresh full cross-platform package rebuilds were not performed.
- A new unpublished Arena Cargo archive was created specifically to test source-versus-package semantic-hash identity. No registry write occurred.
- Both Lean projects compiled. Their proofs concern their explicit models and assumptions, not the complete deployed implementation.
- Synthetic finite examples, AST-extracted unchanged Python functions with explicit dependency doubles, and fresh local Rust/CLI builds reproduced selected findings. The report names the kind of evidence per finding.

## What was not established

1. No new model experiment, market-data acquisition, Docker resource-exhaustion trial, or hostile multi-tenant deployment was run.
2. No paper PDF was rebuilt, and the historical numerical evidence was not regenerated end to end. A confirmed arithmetic or validation bug does not automatically mean a published table changes.
3. The full transitive dependency trees, generated binaries, all historical releases, and every test fixture were not independently audited line by line.
4. Existing CI success establishes that those particular jobs passed. It neither proves an uncovered invariant nor substitutes for an independently observed live containment test.
5. The review does not establish redistribution rights, universal statistical validity, cryptographic security of an ad hoc construction, or absence of other defects.

## Disposition

Product files remain unchanged. Findings include repair directions and acceptance-test ideas, but repair and empirical reanalysis are separate work. “Confirmed” means source contradiction or reproduced behavior at this snapshot; it is not a claim that every affected route is used in a published experiment.
