#!/usr/bin/env python3
"""Repair a dev DB after removing migration 039 (models_json).

Applies migration 040 schema changes if needed, removes the applied 039
record from _sqlx_migrations, and records 040 so sqlx stays in sync.

Usage (inside dev container or with DREAMWELL_DATABASE_URL set):
  python3 scripts/repair-dev-db-after-inference-migration-drop.py

Default DB path: /app/data/dreamwell.db
"""
from __future__ import annotations

import hashlib
import os
import shutil
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DB = Path("/app/data/dreamwell.db")
MIGRATION_040 = (
    REPO_ROOT / "crates/server/migrations/040_inference_connection_fallback.sql"
)


def db_path() -> Path:
    url = os.environ.get("DREAMWELL_DATABASE_URL", f"sqlite:{DEFAULT_DB}")
    if url.startswith("sqlite:"):
        return Path(url.removeprefix("sqlite:"))
    raise SystemExit(f"Unsupported database URL: {url}")


def has_column(cur: sqlite3.Cursor, table: str, column: str) -> bool:
    cur.execute(f"PRAGMA table_info({table})")
    return any(row[1] == column for row in cur.fetchall())


def main() -> int:
    path = db_path()
    if not path.exists():
        raise SystemExit(f"Database not found: {path}")

    if not MIGRATION_040.exists():
        raise SystemExit(f"Missing migration file: {MIGRATION_040}")

    backup = path.with_suffix(path.suffix + ".pre-inference-repair.bak")
    shutil.copy2(path, backup)
    print(f"Backup written to {backup}")

    conn = sqlite3.connect(path)
    cur = conn.cursor()

    cur.execute(
        "SELECT version, description FROM _sqlx_migrations WHERE version IN (39, 40) ORDER BY version"
    )
    before = cur.fetchall()
    print(f"Migration rows before repair: {before}")

    if not has_column(cur, "inference_connections", "enabled"):
        cur.execute(
            "ALTER TABLE inference_connections ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1"
        )
        print("Added inference_connections.enabled")

    if not has_column(cur, "inference_connections", "sort_order"):
        cur.execute(
            "ALTER TABLE inference_connections ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0"
        )
        print("Added inference_connections.sort_order")

    if not has_column(cur, "messages", "generation_notice"):
        cur.execute(
            "ALTER TABLE messages ADD COLUMN generation_notice TEXT NOT NULL DEFAULT ''"
        )
        print("Added messages.generation_notice")

    cur.execute("UPDATE inference_connections SET sort_order = id")
    print(f"Set sort_order for {cur.rowcount} inference connection(s)")

    cur.execute("DELETE FROM _sqlx_migrations WHERE version = 39")
    if cur.rowcount:
        print("Removed applied migration 039 from _sqlx_migrations")

    checksum = hashlib.sha384(MIGRATION_040.read_bytes()).digest()
    cur.execute("SELECT 1 FROM _sqlx_migrations WHERE version = 40")
    if cur.fetchone() is None:
        cur.execute(
            """
            INSERT INTO _sqlx_migrations
                (version, description, installed_on, success, checksum, execution_time)
            VALUES (?, ?, ?, 1, ?, 0)
            """,
            (
                40,
                "inference connection fallback",
                datetime.now(timezone.utc).isoformat(),
                checksum,
            ),
        )
        print("Recorded migration 040 in _sqlx_migrations")
    else:
        print("Migration 040 already recorded")

    conn.commit()

    cur.execute(
        "SELECT version, description FROM _sqlx_migrations WHERE version >= 38 ORDER BY version"
    )
    print(f"Migration rows after repair: {cur.fetchall()}")

    cur.execute("PRAGMA table_info(inference_connections)")
    cols = [row[1] for row in cur.fetchall()]
    print(f"inference_connections columns: {', '.join(cols)}")

    for table in ("chats", "messages", "characters", "inference_connections", "games"):
        cur.execute(f"SELECT COUNT(*) FROM {table}")
        print(f"{table}: {cur.fetchone()[0]}")

    conn.close()
    print("Repair complete.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
