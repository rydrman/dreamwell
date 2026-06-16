#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DB_PATH="$(mktemp -u).db"
DIST_DIR="$(mktemp -d)/frontend-dist"
TARGET_DIR="$(mktemp -d)/target"
SERVER_PID=""
cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -f "$DB_PATH"
  rm -rf "$(dirname "$DIST_DIR")" "$TARGET_DIR"
}
trap cleanup EXIT

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
TRUNK="${TRUNK:-$CARGO_BIN/trunk}"
if [[ ! -x "$TRUNK" ]]; then
  make -C "$ROOT" install-trunk
fi

mkdir -p "$DIST_DIR"
export CARGO_TARGET_DIR="$TARGET_DIR"

if command -v fuser >/dev/null 2>&1; then
  fuser -k 8080/tcp >/dev/null 2>&1 || true
  sleep 0.5
fi

(cd crates/frontend && env -u NO_COLOR "$TRUNK" build --release --dist "$DIST_DIR")

cargo build --release -p dreamwell-server
SERVER_BIN="${CARGO_TARGET_DIR}/release/dreamwell-server"

DREAMWELL_E2E=1 \
DREAMWELL_DATABASE_URL="sqlite:${DB_PATH}" \
DREAMWELL_STATIC_DIR="${DIST_DIR}" \
DREAMWELL_HOST=127.0.0.1 \
DREAMWELL_PORT=8080 \
DREAMWELL_SSE_POLL_INTERVAL_MS=100 \
  "$SERVER_BIN" &
SERVER_PID=$!

for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:8080/api/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

if ! curl -fsS "http://127.0.0.1:8080/api/health" >/dev/null 2>&1; then
  echo "Server failed to start for e2e" >&2
  exit 1
fi

cd e2e
if [[ ! -d node_modules ]]; then
  npm install
fi
npx playwright install chromium
npm test
