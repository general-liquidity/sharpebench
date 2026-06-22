# Embedding the kernel (WASM)

A benchmark whose scorer is re-implemented per consumer drifts. SharpeBench avoids
that by having **one** scoring kernel — `sharpebench-core` — and compiling it to WebAssembly
(`sharpebench-wasm`) for non-Rust hosts. A TypeScript trading agent and the canonical Rust
CLI then score against byte-identical math; there is no second implementation to
disagree.

## Building

```sh
scripts/build-wasm.sh         # wasm-bindgen → pkg/ (release .wasm + JS bindings)
```

This produces `sharpebench_wasm_bg.wasm` plus JS/TS bindings. The exported `score` entry
takes the same submissions JSON the CLI's `score` accepts and returns the ranked
board as JSON.

## Consuming from a host (TypeScript/Bun)

```ts
import { scoreSharpeBench } from "./sharpebench"; // thin wrapper over the wasm pkg
const board = scoreSharpeBench(submissionsJson);
```

The host loads the vendored `.wasm`, calls the same `rank` the CLI calls, and gets
the same verdicts. Keep the host integration **opt-in** and pinned to a published
`sharpebench-wasm` version: the kernel is the source of truth, and the production path is to
depend on the published package rather than a vendored copy that can fall behind.

## Host-testable shim

`sharpebench_wasm::score_json` is a plain Rust function (not gated on `wasm32`) so the
binding logic is unit-tested on the host toolchain, while the `wasm-bindgen`
`score` export is compiled only for `wasm32`. Same code path, testable without a
browser.
