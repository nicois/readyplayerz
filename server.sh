#!/bin/sh
# Builds the game and runs the game server: the page on http://localhost:8080 and the rooms on UDP port 4433.
# See server/src/main.rs for the settings (ports, a real certificate).
set -e
export PATH=$HOME/.cargo/bin:$PATH
cd "$(dirname "$0")"
./build.sh
cargo build --release -p server
exec target/release/server
