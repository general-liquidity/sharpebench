# SharpeBench documentation

The root [`README`](../README.md) is the task-first product entry point. The
mdBook contains the benchmark reference; the documents beside it cover project
operations and engineering assessments.

## Use and methodology

| Topic | Document |
|---|---|
| Book contents | [mdBook summary](book/src/SUMMARY.md) |
| CLI and transports | [CLI reference](book/src/cli.md) |
| Submission format | [Submitting an agent](book/src/submitting.md) |
| Scoring and statistical gates | [Methodology](book/src/methodology.md) |
| Process and lifecycle checks | [Process discipline](book/src/methodology-process.md) |
| Forward attestation | [Attestation](book/src/attestation.md) |
| Sandboxed entrants and forward league | [The arena](book/src/arena.md) |
| Integrity, self-audit, and provenance | [Integrity](book/src/integrity.md) |
| Memory/retrieval benchmark | [Memory benchmark](book/src/memory.md) |

Build or serve the book from the repository root:

```bash
mdbook build docs/book
mdbook serve docs/book
```

## Project operations

| Topic | Document |
|---|---|
| Current state and roadmap | [Plan](PLAN.md) |
| Contributing | [`CONTRIBUTING.md`](../CONTRIBUTING.md) |
| Governance | [Governance](GOVERNANCE.md) |
| Release history | [`CHANGELOG.md`](../CHANGELOG.md) |
| Release procedure | [`RELEASING.md`](../RELEASING.md) |
| Registry authentication and recovery | [Publishing model](PUBLISHING.md) |
| smolvm extraction audit | [smolvm assessment](SMOLVM_ASSESSMENT.md) |
| Methodology paper and committed evidence | [`paper/`](../paper/) |

Package-specific READMEs remain beside the Rust crates, Python distribution,
npm package, MCP server, and reference agent so registry consumers do not need
the monorepo layout to use one surface.
