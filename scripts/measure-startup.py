#!/usr/bin/env python3
"""Measures the budgets that need a running app: startup and idle memory.

See ideas/04-performance-budget.md.

Startup is reported two ways because neither alone is honest. The in-process
figure starts at `main` and so misses `exec` and dynamic linking — which for a
WebView app is not a rounding error. Wall clock from spawn captures everything
the user actually waits for, but includes OS overhead the app does not control.
The true cost sits between them, and both are printed rather than picking one.

Written in Python rather than shell so the timestamps are taken in one process:
shelling out for each one added roughly a hundred milliseconds of interpreter
startup to a figure being compared against a 400ms budget.
"""
from __future__ import annotations

import subprocess
import sys
import time
from pathlib import Path

STARTUP_TARGET_MS = 400
STARTUP_LIMIT_MS = 700
RSS_TARGET_MB = 150
RSS_LIMIT_MB = 250

MARKER = "EASYPDF_STARTUP_MS="
SETTLE_SECONDS = 3.0


def verdict(name: str, measured: float, target: float, limit: float, unit: str) -> bool:
    if measured <= target:
        state = "within target"
    elif measured <= limit:
        state = "OVER TARGET"
    else:
        state = "HARD FAIL"
    print(f"{name:34} {measured:>8.1f} {unit}  (target {target:g}, limit {limit:g})  {state}")
    return measured <= limit


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    app = Path(sys.argv[1]) if len(sys.argv) > 1 else (
        root / "target/release/bundle/macos/EasyPDF.app"
    )
    binary = app / "Contents/MacOS/easypdf-desktop"

    if not binary.is_file():
        print(f"error: {binary} not found — build it with:", file=sys.stderr)
        print("  npm --prefix apps/desktop run tauri build -- --bundles app", file=sys.stderr)
        return 1

    launched_at = time.perf_counter()
    process = subprocess.Popen(
        [str(binary)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        env={**__import__("os").environ, "EASYPDF_MEASURE_STARTUP": "1"},
        text=True,
    )

    in_process_ms = None
    ready_at = None
    deadline = launched_at + 30.0

    # Read stderr line by line: blocking on the pipe is far more precise than
    # polling a file, and the marker is written the moment the window is ready.
    assert process.stderr is not None
    while time.perf_counter() < deadline:
        line = process.stderr.readline()
        if line == "":
            break
        if MARKER in line:
            ready_at = time.perf_counter()
            in_process_ms = float(line.split(MARKER, 1)[1].split()[0])
            break

    if ready_at is None or in_process_ms is None:
        print("error: the app never reported readiness", file=sys.stderr)
        process.kill()
        return 1

    wall_ms = (ready_at - launched_at) * 1000.0

    time.sleep(SETTLE_SECONDS)
    if process.poll() is not None:
        print("error: the app exited during startup", file=sys.stderr)
        return 1

    rss = subprocess.run(
        ["ps", "-o", "rss=", "-p", str(process.pid)],
        capture_output=True, text=True, check=False,
    ).stdout.strip()
    rss_mb = int(rss) / 1024 if rss.isdigit() else 0.0

    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()

    print(f"measuring {app}\n")
    ok = True
    ok &= verdict("startup, in-process", in_process_ms, STARTUP_TARGET_MS, STARTUP_LIMIT_MS, "ms")
    ok &= verdict("startup, wall clock", wall_ms, STARTUP_TARGET_MS, STARTUP_LIMIT_MS, "ms")
    ok &= verdict("idle RSS, no document", rss_mb, RSS_TARGET_MB, RSS_LIMIT_MB, "MB")

    launch_overhead = wall_ms - in_process_ms
    print(f"\nexec + dynamic linking before main: {launch_overhead:.1f} ms")

    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
