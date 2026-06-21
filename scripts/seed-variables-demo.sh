#!/usr/bin/env bash
set -euo pipefail

API="${DREAMWELL_API:-http://localhost:8080/api}"
COMPOSE=(docker compose -f docker-compose.yml)

echo "Seeding variable demo data via ${API} ..."

CHAR_ID="$(
	curl -sf "${API}/characters" \
		-H 'Content-Type: application/json' \
		-d '{
			"name": "Variable Demo",
			"description": "Demo character for the variables UI.",
			"personality": "Helpful and concise.",
			"scenario": "A fantasy tavern at dusk.",
			"first_message": "",
			"example_dialogue": "",
			"system_prompt": ""
		}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'
)"

CHAT_ID="$(
	curl -sf "${API}/chats" \
		-H 'Content-Type: application/json' \
		-d "{\"title\":\"Variable Demo\",\"character_id\":${CHAR_ID}}" \
		| python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'
)"

for payload in \
	'{"key":"mood","value":"calm"}' \
	'{"key":"quest","value":"find the amulet"}'; do
	curl -sf "${API}/chats/${CHAT_ID}/variables" \
		-X PUT \
		-H 'Content-Type: application/json' \
		-d "${payload}" >/dev/null
done

"${COMPOSE[@]}" exec -T dreamwell python3 - "${CHAT_ID}" <<'PY'
import json
import sqlite3
import sys
from datetime import datetime, timezone

chat_id = int(sys.argv[1])

def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f+00:00")

def insert_message(conn, role: str, content: str, updates) -> int:
    cur = conn.execute(
        """
        INSERT INTO messages (
            chat_id, role, content, is_summary, created_at,
            variable_updates, thought_content, thought_in_progress, in_summary
        ) VALUES (?, ?, ?, 0, ?, ?, '', 0, 0)
        """,
        (chat_id, role, content, now(), json.dumps(updates)),
    )
    return int(cur.lastrowid)

def upsert_variable(conn, key: str, value: str, source_message_id: int) -> None:
    conn.execute(
        """
        INSERT INTO chat_variables (chat_id, key, value, source_message_id, updated_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(chat_id, key, source_message_id)
        DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        """,
        (chat_id, key, value, source_message_id, now()),
    )

def delete_variable_scoped(conn, key: str, source_message_id: int) -> None:
    conn.execute(
        "DELETE FROM chat_variables WHERE chat_id = ? AND key = ? AND source_message_id = ?",
        (chat_id, key, source_message_id),
    )

conn = sqlite3.connect("/app/data/dreamwell.db")

insert_message(conn, "user", "Where are we right now?", [])
msg1 = insert_message(
    conn,
    "assistant",
    "The fire crackles in the hearth. You are in the **Rusty Tankard** tavern.",
    [{"key": "location", "value": "Rusty Tankard tavern"}],
)
upsert_variable(conn, "location", "Rusty Tankard tavern", msg1)

insert_message(conn, "user", "Let's slip out toward the forest.", [])
msg2 = insert_message(
    conn,
    "assistant",
    "You push through the side door into the cool night air. Pine needles crunch underfoot.",
    [
        {
            "key": "location",
            "value": "Whispering Forest",
            "previous_value": "Rusty Tankard tavern",
        }
    ],
)
upsert_variable(conn, "location", "Whispering Forest", msg2)

insert_message(conn, "user", "How are we feeling? Any coin on us?", [])
msg3 = insert_message(
    conn,
    "assistant",
    "Your pulse picks up as branches scrape the path. You count **50 gold** in your pouch.",
    [
        {"key": "gold", "value": "50"},
        {
            "key": "mood",
            "value": "tense",
            "previous_value": "calm",
        },
    ],
)
upsert_variable(conn, "gold", "50", msg3)
upsert_variable(conn, "mood", "tense", msg3)

insert_message(conn, "user", "Forget the side quest for now.", [])
insert_message(
    conn,
    "assistant",
    "You shove the amulet map deeper into your pack and focus on the trail ahead.",
    [
        {
            "key": "quest",
            "value": "",
            "previous_value": "find the amulet",
        }
    ],
)
delete_variable_scoped(conn, "quest", -1)

conn.execute("UPDATE chats SET updated_at = ? WHERE id = ?", (now(), chat_id))
conn.commit()
conn.close()
PY

echo
echo "Done."
echo "  Character id: ${CHAR_ID}"
echo "  Chat id:      ${CHAT_ID}"
echo "  Open:         http://localhost:8080/chats/${CHAT_ID}"
echo
echo "Expand the assistant messages to see the variables table with Previous links."
