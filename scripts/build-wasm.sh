#!/usr/bin/env bash
# Build the SharpeBench scoring kernel (sharpebench-core, via sharpebench-wasm) to WASM so Gordon
# (Bun/TypeScript) can score with the IDENTICAL kernel as the public harness.
#
# Output: ./pkg with a JS module exposing `score(submissionsJson, configJson)`,
# which Gordon imports into its RULER eval harness.
set -euo pipefail

rustup target add wasm32-unknown-unknown
cargo build -p sharpebench-wasm --release --target wasm32-unknown-unknown
WASM="target/wasm32-unknown-unknown/release/sharpebench_wasm.wasm"

if command -v wasm-bindgen >/dev/null 2>&1; then
  wasm-bindgen "$WASM" --out-dir pkg --target bundler
  echo "wrote ./pkg  — import { score } from './pkg/sharpebench_wasm'"
else
  echo "built $WASM"
  echo "To generate the JS bindings, install the CLI then re-run:"
  echo "  cargo install wasm-bindgen-cli"
fi
