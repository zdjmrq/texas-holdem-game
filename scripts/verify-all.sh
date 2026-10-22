#!/usr/bin/env bash
# Full local verification for the Rust rewrite.
#
# Runs fmt/clippy/tests, then regenerates every golden corpus from the pinned
# reference JS project and fails on any drift. This is the script the Lead runs
# before declaring a milestone done; CI mirrors the same checks.
#
# Usage: scripts/verify-all.sh [--skip-oracle] [--skip-app-smoke]
set -euo pipefail
cd "$(dirname "$0")/.."

source "$HOME/.cargo/env"

SKIP_ORACLE=0
SKIP_APP_SMOKE=0
for arg in "$@"; do
  case "$arg" in
    --skip-oracle) SKIP_ORACLE=1 ;;
    --skip-app-smoke) SKIP_APP_SMOKE=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

echo "== cargo fmt =="
cargo fmt --all --check

echo "== cargo clippy (workspace, -D warnings) =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== cargo test (workspace) =="
cargo test --workspace

if [ "$SKIP_ORACLE" -eq 0 ]; then
  REFERENCE_REPO="${REFERENCE_REPO:-$PWD/../reference/texas-holdem-game}"
  export REFERENCE_REPO
  if [ ! -d "$REFERENCE_REPO" ]; then
    echo "!! reference JS repo not found at $REFERENCE_REPO - skipping oracle regeneration" >&2
    echo "   set REFERENCE_REPO or clone it first (see docs/RUST_REWRITE_HANDOFF.md)" >&2
  else
    echo "== golden corpus regeneration (oracle) =="
    for script in golden-eval golden-engine golden-equity golden-ai golden-protocol; do
      echo "-- node oracle/$script.js"
      node "oracle/$script.js" > "/tmp/$script.out" 2>&1 || {
        tail -40 "/tmp/$script.out" >&2
        exit 1
      }
    done
    if ! git diff --exit-code -- tests/golden > /dev/null; then
      echo "!! golden corpus drift detected:" >&2
      git diff --stat -- tests/golden >&2
      exit 1
    fi
    echo "golden corpora reproducible: OK"
  fi
fi

if [ "$SKIP_APP_SMOKE" -eq 0 ] && [ -n "${DISPLAY:-}" ]; then
  echo "== app smoke (headless scenario, 6s timeout) =="
  timeout 6s cargo run -q -p poker-app -- --scenario table > /tmp/app-smoke.log 2>&1 || status=$?
  if [ "${status:-0}" -ne 124 ] && [ "${status:-0}" -ne 0 ]; then
    tail -40 /tmp/app-smoke.log >&2
    exit 1
  fi
  echo "app smoke: OK"
fi

echo
echo "ALL CHECKS PASSED"
