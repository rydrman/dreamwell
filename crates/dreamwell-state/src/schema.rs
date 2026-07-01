use serde_json::json;

pub fn state_changes_schema() -> serde_json::Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "target": { "type": "string" },
                "kind": { "type": "string", "enum": ["measurement", "condition", "variable", "sequence"] },
                "key": { "type": "string" },
                "op": { "type": "string", "enum": ["set", "add", "remove", "setmin", "setmax", "step", "replace"] },
                "value": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "number" },
                        { "type": "boolean" },
                        { "type": "null" }
                    ]
                },
                "delta": { "type": "integer" },
                "float_value": { "type": "number" },
                "float_min": { "type": "number" },
                "float_max": { "type": "number" },
                "unit": { "type": "string" },
                "sequence_items": { "type": "array", "items": { "type": "string" } },
                "sequence_position": { "type": "integer" },
                "sequence_loop": { "type": "boolean" }
            },
            "required": ["target", "kind", "key", "op"]
        }
    })
}

/// Plan-phase JSON schema with configurable beats field name (`scene_beats`, `reply_beats`, etc.).
pub fn plan_schema(beats_field: &str) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            beats_field: { "type": "array", "items": { "type": "string" } },
            "state_changes": state_changes_schema()
        },
        "required": [beats_field, "state_changes"]
    })
}

/// Plan schema with beats only — state updates happen elsewhere (e.g. prose tools).
pub fn beats_only_plan_schema(beats_field: &str) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            beats_field: { "type": "array", "items": { "type": "string" } }
        },
        "required": [beats_field]
    })
}

pub fn resolve_schema() -> serde_json::Value {
    plan_schema("scene_beats")
}

pub fn state_recheck_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "state_changes": state_changes_schema()
        },
        "required": ["state_changes"]
    })
}
