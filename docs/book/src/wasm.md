# Embedding the kernel (WASM, npm, MCP)

A benchmark whose scorer is re-implemented per consumer drifts. SharpeBench avoids
that by having **one** scoring kernel — `sharpebench-core` — and compiling it to
WebAssembly (`sharpebench-wasm`) for non-Rust hosts. A TypeScript trading agent, the
published npm package, and the canonical Rust CLI all score against byte-identical
math; there is no second implementation to disagree.

## The npm package

[`@general-liquidity/sharpebench`](https://www.npmjs.com/package/@general-liquidity/sharpebench)
is the kernel as a typed JS/TS package — no Rust toolchain required:

```ts
import { score, scoreAgent, selfAudit, greeks } from "@general-liquidity/sharpebench";

const board = score(submissions);          // ranked CompositeScore[]
selfAudit().all_defended;                   // the anti-gaming proof, in JS
greeks({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true });
```

The full surface: `score`, `scoreAgent`, `selfAudit`, `auditBriefing`,
`scoreAllocation`, `greeks`, `canary` — all fully typed and deterministic.

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
