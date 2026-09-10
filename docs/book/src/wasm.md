# Embedding the kernel (WASM, npm, MCP)

A benchmark whose scorer is re-implemented per consumer drifts. SharpeBench avoids
that by having **one** scoring kernel, `sharpebench-core`, and compiling it to
WebAssembly (`sharpebench-wasm`) for non-Rust hosts. A TypeScript trading agent, the
published npm package, and the canonical Rust CLI all call the same kernel; there
is no second scoring implementation. Ubuntu CI compares the built npm/WASM
package with the committed golden, while the Rust host facade is checked on the
three CI operating systems. That evidence is limited to the tested fixtures and
hosts.

## The npm package

[`@general-liquidity/sharpebench`](https://www.npmjs.com/package/@general-liquidity/sharpebench)
is the kernel as a typed JS/TS package. No Rust toolchain is required:

```ts
import { score, scoreAgent, selfAudit, greeks } from "@general-liquidity/sharpebench";

const board = score(submissions);          // ranked CompositeScore[]
selfAudit().all_defended;                   // results of the named regression attacks
greeks({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true });
```

The full surface is `score`, `scoreAgent`, `selfAudit`, `auditBriefing`,
`scoreAllocation`, `greeks`, `canary`, `isMySharpeReal`,
`isMySharpeRealFull`, `percentileSelection`, `decomposeUncertainty`,
`crowdingHalfLife`, `classifyDisqualification`, and `regimeCompare`. All are
typed and deterministic.

## Board declarations and explanations

`score(submissions, config)` accepts an optional `declared_mandate` on each
submission, using the [same declaration format as the CLI](submitting.md).
`declared_passed_k`, `declared_mandate_eligible` and the within-mandate ordinal
report a second verdict. They do not change host `rank_eligible`, ordering or
`rank_ordinal`. Malformed declarations, duplicate agent IDs and whitespace-only
IDs are errors; identity comparison is exact and case-sensitive.

Use the same complete field and configuration for scoring and explanation.
Both calls below use the defaults:

```ts
import { score, classifyDisqualification } from "@general-liquidity/sharpebench";

const board = score(submissions);
const explanations = classifyDisqualification(submissions);
```

Omitting config, or passing `{}`, selects defaults in npm. A nonempty config
object is not a partial override: it must include `n_trials`, `trials_sr_std`,
`dsr_bar`, `per_run_psr_bar`, `alpha`, `bootstrap_seed`, `n_boot` and `block_prob`.
The TypeScript type permits omission of these fields, but the runtime parser
requires them. Preserve a complete config when changing frequency or other
controls, and supply that same config to both calls.

`classifyDisqualification` classifies the ranked field's scores, preserving
relative-benchmark and measured-dispersion context. Each result contains
`agent_id`, host `rank_eligible` and `reasons`. Reasons describe the host score,
including its host mandate, and do not describe the declared verdict. Advisory
flags do not change eligibility. The WASM exports call the Rust `score_json`
and `classify_disqualification_json` entry points; the npm wrapper throws on
their errors.

Python's `rank_board(submissions_json, config_json=None)` uses the same declared
field parser and returns the ranked board as a JSON string. Invalid declarations
or ambiguous agent IDs raise `ValueError`. Single-submission `scoreAgent` and
Python `score_one` do not supply a ranked field; use the board APIs when a
declaration or benchmark depends on other entrants.

TypeScript `Run.outcomes` is `boolean[]`, matching the Rust/JSON submission
contract. Convert known binary observations to booleans before submission;
numeric `0`/`1` arrays are not that field's wire format. This does not change the
separate uncertainty API's documented binary-input format.

These checks bind agent identity, not run timing. Legacy `Run` arrays still
require the caller to align window, seed and period order across entrants.

## Statistical unavailability and migration

The npm wrapper preserves the kernel's error fields rather than dropping them
while converting names to camel case:

| API | Field | Meaning |
|---|---|---|
| `isMySharpeReal` and `full.honesty` | `statisticsError` | Deflation was not estimated. Do not interpret its numeric fallback as measured no-skill performance. |
| `isMySharpeRealFull` | `snoopingError` | The whole fieldwise family was withheld; p-values of 1 and all-false `stepDown` are conservative sentinels. |
| `isMySharpeRealFull` | `pboError` | PBO is unavailable and serializes as `null`. |
| `percentileSelection` | `input_error` | At least one candidate or parameter is unsupported. Both winner indices are null. |

Absent optional error fields mean only that the corresponding kernel computation
accepted its inputs, not that the statistical assumptions hold. Nonfinite
floating-point diagnostics serialize as JSON null. The honesty and PBO types now
represent those nulls explicitly; callers that previously assumed every field
was numeric need a null check. An infinite minimum track record length, for
example, is not zero required observations.

The full verdict also exposes its existing kernel `hlz` diagnostic as
`{tStat, tThreshold, passed, explanation}`. It is separate from the headline
honesty verdict. Board explanations include `deflation_unavailable`,
`bootstrap_unavailable` and `selection_unavailable`. The first two are hard,
because the scorer gates on the statistics they name. `selection_unavailable` is
advisory: the scorer reports `selection_gap` and never consults it in
`rank_eligible`, so a submission carrying only that reason is still
rank-eligible. Read `rank_eligible` for the verdict rather than the presence of
a reason.

The same refusal regressions execute through the committed WASM wrapper and
through an offline-installed npm tarball. A source-only fix is not sufficient:
rebuild `npm/pkg/` when the kernel changes and run both `npm test` and
`npm run smoke-install` from `npm/`.

## The MCP server

[`@general-liquidity/sharpebench-mcp`](https://www.npmjs.com/package/@general-liquidity/sharpebench-mcp)
exposes the same kernel as Model-Context-Protocol tools, so Claude (or any MCP
client) can deflate a Sharpe / check pass^k / audit a briefing in its tool loop:

```json
{ "mcpServers": { "sharpebench": { "command": "npx", "args": ["-y", "@general-liquidity/sharpebench-mcp"] } } }
```

## Building from source

```sh
wasm-pack build crates/sharpebench-wasm --target nodejs --out-dir ../../npm/pkg --out-name sharpebench
```

This produces the `.wasm` plus JS/TS bindings under `npm/pkg/`, which the npm
package wraps with a typed API.

## Host-testable shim

Each entry point is a plain Rust `*_json` function (not gated on `wasm32`), so the
binding logic is unit-tested on the host toolchain, while the `wasm-bindgen`
exports are compiled only for `wasm32`. Same code path, testable without a browser.
