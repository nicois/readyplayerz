#!/bin/sh
# Builds the wasm client into web/pkg. Needs rustup's cargo (wasm32 target) and wasm-bindgen-cli.
set -e
export PATH=/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/arena.wasm

# The zombies mode is the same page with a flag set, served from its own path (/zombiess, and /zombies too).
# <base href="../"> keeps its relative script and wasm paths pointing at the shared files.
for dir in zombiess zombies; do
  mkdir -p web/$dir
  python3 - "$dir" <<'PY'
import sys
page = open("web/index.html").read()
tag = '<head>\n<base href="../">\n<script>window.MODE = "zombies";</script>'
open(f"web/{sys.argv[1]}/index.html", "w").write(page.replace("<head>", tag, 1))
PY
done
