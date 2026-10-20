#!/bin/sh
# ./bench.sh: the game under load, held to the limits in bench/limits.txt, with a line of the results added to
# bench/history.tsv so changes can be compared. Two benches, both six players against a full horde on the Suburb:
#
#   network  `cargo test` plays it on the server's own room and measures what goes over the wire (src/sim/load.rs)
#   drawing  headless Chrome plays it on the page, on this machine's GPU, and measures the frames (?horde)
#
# `./bench.sh net` runs only the first. The drawing bench builds the game first; CHROME says where Chrome is, and
# GFX the quality (0 low, 1 medium, 2 high; 2 by default) at 1920x1080. It fails if any limit is passed.
set -e
cd "$(dirname "$0")"
out=$(mktemp -d)
# on the way out, whatever the way, the page's server is stopped and the scratch files go; the script's own exit
# status is kept, not the stopped server's
trap 'code=$?; set +e; [ -n "$server" ] && { kill $server; wait $server; } 2>/dev/null; rm -rf "$out"; exit $code' EXIT
status=0

echo "== network"
cargo test --release --lib load -- --ignored --nocapture 2>&1 | grep -E "^(six|a snapshot|bench|test result|thread|  )" | tee "$out/net.txt"
grep -q "test result: ok" "$out/net.txt" || status=1

if [ "$1" != net ]; then
  echo "== drawing"
  ./build.sh >/dev/null 2>&1 || { echo "the build failed: run ./build.sh to see why"; exit 1; }
  port=8097
  python3 -m http.server "$port" --bind 127.0.0.1 --directory web >/dev/null 2>&1 &
  server=$!
  sleep 1
  chrome=${CHROME:-"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"}
  # on the real GPU, and without vsync or a frame cap, so a frame takes what it costs
  "$chrome" --headless=new --use-angle=metal --enable-webgl --ignore-gpu-blocklist --disable-gpu-vsync \
    --disable-frame-rate-limit --enable-logging=stderr --v=0 --user-data-dir="$out/profile" \
    --window-size=1920,1080 "http://127.0.0.1:$port/?horde&gfx=${GFX:-2}" >"$out/chrome.log" 2>&1 &
  browser=$!
  # warm-up and measuring take under half a minute; past a minute something is wrong
  for _ in $(seq 60); do
    grep -q '"bench {' "$out/chrome.log" && break
    sleep 1
  done
  kill $browser 2>/dev/null || true
  grep -o 'bench {.*}' "$out/chrome.log" | head -1 | sed 's/^bench //' >"$out/draw.json"
  if [ ! -s "$out/draw.json" ]; then
    echo "the drawing bench gave no results; Chrome's log:"
    grep -i "error\|console" "$out/chrome.log" | head -20
    status=1
  fi
fi

python3 bench/check.py "$out/net.txt" "$out/draw.json" || status=1
exit $status
