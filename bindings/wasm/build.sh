#!/usr/bin/env bash
# Builds @kakemono/wasm: the module, then its JavaScript for Node (pkg/node,
# CommonJS) and for browsers and bundlers (pkg/web, an ES module).
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli of the same
# version as the wasm-bindgen crate (Cargo.toml).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --locked
module=target/wasm32-unknown-unknown/release/kakemono_wasm.wasm
wasm-bindgen --target nodejs --out-dir pkg/node "$module"
wasm-bindgen --target web --out-dir pkg/web "$module"
# The package is an ES module; the Node build is CommonJS.
printf '{"type":"commonjs"}\n' > pkg/node/package.json
# An ES module entry for Node, with the same default `init` as the web build
# (a no-op: the Node build is ready once loaded), so that one import works in
# both.
cat > pkg/node/index.mjs <<'JS'
import wasm from './kakemono_wasm.js'
export const { Document, addFont, version } = wasm
export default async function init() {}
export function initSync() {}
JS
cp pkg/node/kakemono_wasm.d.ts pkg/node/index.d.mts
printf '%s\n' 'export default function init(): Promise<void>' 'export function initSync(): void' >> pkg/node/index.d.mts
