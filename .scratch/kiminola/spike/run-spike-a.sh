#!/usr/bin/env bash
# Ticket 11 Spike A runner: verifies WASAPI loopback packet flow while a tone plays.
# Usage: ./run-spike-a.sh [x86_64-pc-windows-msvc]   (default: native aarch64)
set -uo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

TARGET="${1:-aarch64-pc-windows-msvc}"
BIN="wasapi-loopback-spike/target/${TARGET}/release/wasapi-loopback-spike.exe"

echo "== building for $TARGET =="
(cd wasapi-loopback-spike && cargo build --release --target "$TARGET" 2>&1 | tail -2)

"$BIN" genwav tone.wav 30

rm -f ps-pid.txt
powershell.exe -NoProfile -ExecutionPolicy Bypass -File play-tone.ps1 "$(cygpath -w "$PWD/tone.wav")" &
PSBG=$!
for i in $(seq 1 50); do [ -f ps-pid.txt ] && break; sleep 0.2; done
PID=$(tr -d '\r\n ' < ps-pid.txt)
echo "== playback process pid: $PID =="
sleep 1  # let playback actually start

RC_ALL=0
echo "== classic whole-endpoint loopback =="
"$BIN" classic 8 || RC_ALL=1
echo
echo "== process loopback (pid $PID) =="
"$BIN" process "$PID" 8 || RC_ALL=1

powershell.exe -NoProfile -Command "Stop-Process -Id $PID -Force -ErrorAction SilentlyContinue" 2>/dev/null
kill "$PSBG" 2>/dev/null
exit $RC_ALL
