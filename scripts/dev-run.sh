#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

TRUNK="${CARGO_HOME:-/usr/local/cargo}/bin/trunk"

trunk_pid=

cleanup() {
  if [[ -n "${trunk_pid:-}" ]] && kill -0 "$trunk_pid" 2>/dev/null; then
    kill -TERM "$trunk_pid" 2>/dev/null || true
    wait "$trunk_pid" 2>/dev/null || true
  fi
}

trap 'cleanup; exit 130' INT TERM

make build

(cd crates/frontend && env -u NO_COLOR "$TRUNK" watch --release) &
trunk_pid=$!

export DREAMWELL_STATIC_DIR=crates/frontend/dist
export DREAMWELL_DATABASE_URL="${DREAMWELL_DATABASE_URL:-sqlite:/app/data/dreamwell.db}"

cargo watch \
  -w crates/server \
  -w crates/dreamwell-types \
  -w Cargo.toml \
  -w Cargo.lock \
  -x 'run --release -p dreamwell-server'

cleanup
