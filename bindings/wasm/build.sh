#!/usr/bin/env bash
# Builds @kakemono/wasm: the module, then its JavaScript for Node (pkg/node,
# CommonJS) and for browsers and bundlers (pkg/web, an ES module).
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli of the same
# version as the wasm-bindgen crate (Cargo.toml).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release
module=target/wasm32-unknown-unknown/release/kakemono_wasm.wasm
wasm-bindgen --target nodejs --out-dir pkg/node "$module"
wasm-bindgen --target web --out-dir pkg/web "$module"
# The package is an ES module; the Node build is CommonJS.
printf '{"type":"commonjs"}\n' > pkg/node/package.json
