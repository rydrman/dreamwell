#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# Bind-mounted repo is owned by the host user; the dev container runs as root.
git config --global --add safe.directory "$(pwd)"

TRUNK="${CARGO_HOME:-/usr/local/cargo}/bin/trunk"

trunk_pid=

cleanup() {
  if [[ -n "${trunk_pid:-}" ]] && kill -0 "$trunk_pid" 2>/dev/null; then
    kill -TERM "$trunk_pid" 2>/dev/null || true
    wait "$trunk_pid" 2>/dev/null || true
  fi
}

trap 'cleanup; exit 130' INT TERM

make build-server

(cd crates/frontend && env -u NO_COLOR "$TRUNK" watch --release) &
trunk_pid=$!

export DREAMWELL_STATIC_DIR=.frontend-dist
export DREAMWELL_DATABASE_URL="${DREAMWELL_DATABASE_URL:-sqlite:/app/data/dreamwell.db}"

cargo watch \
  -w crates/server \
  -w crates/dreamwell-types \
  -w Cargo.toml \
  -w Cargo.lock \
  -d 2 \
  --no-restart \
  -x 'run --release -p dreamwell-server'

cleanup
