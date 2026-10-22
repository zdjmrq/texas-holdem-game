#!/usr/bin/env bash
# Rust client <-> Rust server interoperability gate.
#
# Builds crates/poker-server/examples/serve, starts it on an ephemeral port,
# waits for the printed URL and runs poker-client's ignored interop smoke tests
# against it. One command, no Node and no reference repo required.
#
# Usage:
#   oracle/interop-rust-rust.sh
#
# Environment:
#   CARGO_TARGET_DIR  build directory (default /tmp/target-protocol)
#   POKER_LOG         server log level passed to examples/serve (default warn)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
# shellcheck disable=SC1091
source "$HOME/.cargo/env" 2>/dev/null || true

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/target-protocol}"
export POKER_LOG="${POKER_LOG:-warn}"
LOG="$(mktemp -t poker-server-serve-XXXXXX.log)"
SERVER_PID=""

cleanup() {
    if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -f "$LOG"
}
trap cleanup EXIT

echo "[interop-rust-rust] building poker-server example serve"
cargo build -p poker-server --example serve --offline

echo "[interop-rust-rust] starting Rust server on an ephemeral port"
POKER_HOST=127.0.0.1 POKER_PORT=0 "$CARGO_TARGET_DIR/debug/examples/serve" >"$LOG" 2>&1 &
SERVER_PID=$!

URL=""
for _ in $(seq 1 200); do
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "[interop-rust-rust] server exited during startup:" >&2
        cat "$LOG" >&2
        exit 1
    fi
    URL="$(grep -o 'ws://[^[:space:]]*' "$LOG" | head -n 1 || true)"
    if [[ -n "$URL" ]]; then
        break
    fi
    sleep 0.05
done
if [[ -z "$URL" ]]; then
    echo "[interop-rust-rust] server did not print a URL:" >&2
    cat "$LOG" >&2
    exit 1
fi
echo "[interop-rust-rust] server url: $URL"

POKER_INTEROP_URL="$URL" cargo test -p poker-client --offline --test interop -- --ignored --nocapture
echo "[interop-rust-rust] Rust client <-> Rust server interop OK"
