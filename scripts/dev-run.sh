#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

make_pid=
cleanup() {
  if [[ -n "${make_pid:-}" ]] && kill -0 "$make_pid" 2>/dev/null; then
    kill -INT "$make_pid" 2>/dev/null || true
    wait "$make_pid" 2>/dev/null || true
  fi
  exit 130
}
trap cleanup INT TERM

make build &
make_pid=$!
wait "$make_pid"
make_pid=
trap - INT TERM

exec env DREAMWELL_STATIC_DIR=crates/frontend/dist \
  DREAMWELL_DATABASE_URL="${DREAMWELL_DATABASE_URL:-sqlite:/app/data/dreamwell.db}" \
  cargo run --release -p dreamwell-server
