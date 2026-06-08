#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

make build
exec env DREAMWELL_STATIC_DIR=crates/frontend/dist \
  DREAMWELL_DATABASE_URL="${DREAMWELL_DATABASE_URL:-sqlite:/app/data/dreamwell.db}" \
  cargo run --release -p dreamwell-server
