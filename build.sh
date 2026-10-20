#!/bin/sh
# Builds the wasm client into web/pkg. Needs rustup's cargo (wasm32 target) and wasm-bindgen-cli.
set -e
export PATH=$HOME/.cargo/bin:$PATH
cd "$(dirname "$0")"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "error: wasm-bindgen not found on PATH." >&2
  echo "Install wasm-bindgen-cli (version must match wasm-bindgen in Cargo.toml):" >&2
  echo "  cargo install wasm-bindgen-cli --version 0.2.129 --profile dev" >&2
  exit 1
fi

cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/arena.wasm
