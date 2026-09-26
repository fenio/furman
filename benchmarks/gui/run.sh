#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BUILD_DIR="$ROOT/benchmarks/gui/.build"
ENTRY_COUNT=${1:-100000}
RUNS=${2:-3}
BUFFER_ROWS=${3:-10}
PORT=${BENCHMARK_PORT:-4178}
RESULTS_FILE="$BUILD_DIR/results.ndjson"
IFS=',' read -r -a WEB_TARGETS <<< "${BENCHMARK_WEB_TARGETS:-svelte-current,svelte-optimized,solid,vanilla}"

mkdir -p "$BUILD_DIR"
: > "$RESULTS_FILE"

swiftc -O -framework AppKit \
  "$ROOT/benchmarks/gui/native/main.swift" \
  -o "$BUILD_DIR/appkit-file-list"

"$ROOT/node_modules/.bin/vite" build \
  --config "$ROOT/benchmarks/gui/web/vite.config.ts"

"$ROOT/node_modules/.bin/vite" preview \
  --config "$ROOT/benchmarks/gui/web/vite.config.ts" \
  --host 127.0.0.1 \
  --port "$PORT" \
  > "$BUILD_DIR/vite-preview.log" 2>&1 &
SERVER_PID=$!
PROFILE_DIR=''
cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  if [[ -n "$PROFILE_DIR" ]]; then
    rm -rf -- "$PROFILE_DIR"
  fi
}
trap cleanup EXIT

for _ in {1..100}; do
  if curl -fsS "http://127.0.0.1:$PORT" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

for run in $(seq 1 "$RUNS"); do
  printf 'run=%s target=appkit\n' "$run"
  result=$(node "$ROOT/benchmarks/gui/run-native.mjs" \
    "$BUILD_DIR/appkit-file-list" \
    "$ENTRY_COUNT")
  printf '%s\n' "$result" | tee -a "$RESULTS_FILE"

  target_index=0
  for target in "${WEB_TARGETS[@]}"; do
    printf 'run=%s target=%s\n' "$run" "$target"
    PROFILE_DIR="$BUILD_DIR/chrome-profile-$target-$run-$$"
    result=$(node "$ROOT/benchmarks/gui/run-web.mjs" \
      "http://127.0.0.1:$PORT/?entries=$ENTRY_COUNT&run=$run&target=$target&buffer=$BUFFER_ROWS" \
      "$PROFILE_DIR" \
      "$((9228 + (run - 1) * ${#WEB_TARGETS[@]} + target_index))")
    printf '%s\n' "$result" | tee -a "$RESULTS_FILE"
    rm -rf -- "$PROFILE_DIR"
    PROFILE_DIR=''
    target_index=$((target_index + 1))
  done
done

node "$ROOT/benchmarks/gui/summarize.mjs" "$RESULTS_FILE"
