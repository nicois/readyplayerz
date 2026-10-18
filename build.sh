#!/bin/sh
# Builds the wasm client into web/pkg. Needs rustup's cargo (wasm32 target) and wasm-bindgen-cli.
set -e
export PATH=/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/arena.wasm
