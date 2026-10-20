"""The drawing bench's results held to bench/limits.txt, and a line of both benches' results added to
bench/history.tsv. Run by ./bench.sh with the network bench's output and the drawing bench's JSON (empty when it
wasn't run); the network bench holds itself to its limits."""

import datetime
import json
import os
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))

# the history's columns, in order: a new one goes on the end, so old lines keep their meaning
COLUMNS = [
    "net_down_kbps", "net_down_peak_kbps", "net_server_out_kbps", "net_up_kbps",
    "net_snapshot_bytes", "net_snapshot_bytes_max", "net_snapshot_datagrams_max",
    "server_tick_ms", "server_tick_ms_p99", "server_tick_ms_max",
    "fps", "frame_ms", "frame_ms_p95", "code_ms", "code_ms_p95", "gpu_ms", "gpu_ms_p95",
    "draw_calls", "draw_calls_max", "triangles", "triangles_max", "shadow_draw_calls", "shadow_triangles",
]


def limits():
    out = {}
    for line in open(os.path.join(here, "limits.txt")):
        line = line.split("#")[0].strip()
        if line:
            name, value = line.split()
            out[name] = float(value)
    return out


def main(net_file, draw_file):
    results = {}
    for line in open(net_file):
        if line.startswith("bench "):
            _, name, value = line.split()[:3]
            results[name] = float(value)
    draw = {}
    if os.path.exists(draw_file) and os.path.getsize(draw_file):
        draw = json.load(open(draw_file))
        results.update({k: v for k, v in draw.items() if isinstance(v, (int, float)) and v is not None})

    over = []
    if draw:
        lim = limits()
        print(f"{draw['width']}x{draw['height']}, quality {draw['gfx']}, on {draw['gpu']}")
        print(f"{draw['players']} players against {draw['zombies']:.1f} zombies up on average, {draw['kills']} killed")
        for name in COLUMNS[10:]:
            value = draw.get(name)
            if value is None:
                print(f"bench {name} -  (not measured: the browser doesn't tell the GPU's time)")
                continue
            # the GPU's time from the start of a frame's work to its end: on Apple's GPUs one frame's work
            # overlaps the next's, so it can be more than a frame takes, and it is shown but not held to a limit
            limit = lim.get(name)
            # frames a second is the one where more is better
            bad = limit is not None and (value < limit if name == "fps" else value > limit)
            if bad:
                over.append(f"{name} {value:.2f} is past its limit of {limit:g}")
            mark = "OVER" if bad else ("ok" if limit is not None else "")
            print(f"bench {name} {value:.2f}  limit {'-' if limit is None else f'{limit:g}'} {mark}")

    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    dirty = subprocess.run(["git", "status", "--porcelain"], capture_output=True, text=True).stdout.strip()
    path = os.path.join(here, "history.tsv")
    new = not os.path.exists(path)
    with open(path, "a") as f:
        if new:
            f.write("\t".join(["date", "commit"] + COLUMNS) + "\n")
        cells = [datetime.datetime.now().strftime("%Y-%m-%d %H:%M"), commit + ("+" if dirty else "")]
        cells += [f"{results[c]:.2f}" if c in results else "" for c in COLUMNS]
        f.write("\t".join(cells) + "\n")
    print(f"added to {os.path.relpath(path)}")

    for o in over:
        print(o)
    return 1 if over else 0


sys.exit(main(*sys.argv[1:3]))
