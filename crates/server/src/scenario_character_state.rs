use dreamwell_state::STATE_CHANGE_RULES;
use dreamwell_types::{
    CharacterStateDef, GenerateCharacterStateRequest, GenerateCharacterStateResponse, Settings,
    StateKind,
};
use serde_json::json;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::model_fallback::has_inference_provider;

const GENERATE_CHARACTER_STATE_SYSTEM: &str = r#"You design initial typed game state for one character in a tabletop RPG scenario.

Each state entry tracks one durable attribute the GM can update during play:
- fact (DEFAULT): short textual truth — mood, role, employer, secret_goal, last_seen_location, inventory items
- condition: ephemeral status only (hidden, hostile, armed) — clears when resolved
- resource: numeric pool with current/max — ONLY when the scenario truly needs a depletable track (health, stress, ammo)
- clock: progress tracker with segments — ONLY when the scenario needs stepped countdown/progress (suspicion, alarm)

Rules:
- Prefer fact for most attributes; use resource/clock sparingly and only when numeric tracking is essential
- Use snake_case keys; one atomic attribute per key
- Prefer 3–8 entries that matter at scenario start
- Match the scenario genre, stakes, and tone
- Do not duplicate world-scoped keys from the world state schema on this character
- When existing state is provided, refine or extend it — keep useful keys, improve values/hints
- visibility: "player" when the player should see it in the UI, "gm" when hidden, or leave blank
- update_hints: brief guidance for when the GM should change this value during play"#;

#[derive(Debug, serde::Deserialize)]
struct LlmCharacterStateResponse {
    #[serde(default)]
    initial_state: Vec<CharacterStateDef>,
}

pub fn character_state_generation_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "initial_state": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" },
                        "kind": {
                            "type": "string",
                            "enum": ["resource", "condition", "fact", "clock"]
                        },
                        "description": { "type": "string" },
                        "initial_value": { "type": "string" },
                        "initial_num": { "type": ["integer", "null"] },
                        "initial_max": { "type": ["integer", "null"] },
                        "visibility": { "type": "string" },
                        "update_hints": { "type": "string" }
                    },
                    "required": ["key", "kind"]
                }
            }
        },
        "required": ["initial_state"]
    })
}

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn state_kind_label(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Resource => "resource",
        StateKind::Condition => "condition",
        StateKind::Fact => "fact",
        StateKind::Clock => "clock",
    }
}

fn format_traits(traits: &std::collections::HashMap<String, i64>) -> String {
    if traits.is_empty() {
        return String::new();
    }
    traits
        .iter()
        .map(|(name, value)| format!("{name}: {value:+}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_world_schema(schema: &[dreamwell_types::TrackedVarDef]) -> String {
    let world: Vec<_> = schema
        .iter()
        .filter(|def| {
            let target = def.target.trim();
            target.is_empty() || target.eq_ignore_ascii_case("world")
        })
        .collect();
    if world.is_empty() {
        return String::from("(none defined)");
    }
    world
        .iter()
        .map(|def| {
            let mut line = format!("- {} ({})", def.key, state_kind_label(def.kind));
            if !def.description.trim().is_empty() {
                line.push_str(&format!(": {}", def.description.trim()));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_cast_context(cast: &[dreamwell_types::ScenarioNpc], skip_name: &str) -> String {
    let lines: Vec<String> = cast
        .iter()
        .filter(|npc| npc.name.trim() != skip_name)
        .filter(|npc| !npc.name.trim().is_empty())
        .map(|npc| {
            let mut line = format!("- {} (NPC)", npc.name.trim());
            if !npc.content.trim().is_empty() {
                line.push_str(&format!(": {}", npc.content.trim()));
            }
            line
        })
        .collect();
    if lines.is_empty() {
        String::from("(none)")
    } else {
        lines.join("\n")
    }
}

fn format_existing_state(state: &[CharacterStateDef]) -> String {
    if state.is_empty() {
        return String::from("(none — propose a fresh set)");
    }
    state
        .iter()
        .map(|def| {
            let mut line = format!("- {} ({})", def.key, state_kind_label(def.kind));
            if let Some(num) = def.initial_num {
                if let Some(max) = def.initial_max {
                    line.push_str(&format!(": {num}/{max}"));
                } else {
                    line.push_str(&format!(": {num}"));
                }
            } else if !def.initial_value.trim().is_empty() {
                line.push_str(&format!(": {}", def.initial_value.trim()));
            }
            if !def.description.trim().is_empty() {
                line.push_str(&format!(" — {}", def.description.trim()));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_generate_character_state_messages(
    payload: &GenerateCharacterStateRequest,
) -> Vec<serde_json::Value> {
    let character = &payload.character;
    let traits = format_traits(&character.traits);
    let mut user = format!(
        "Scenario:\n- Title: {}\n- Premise: {}\n- Setting/tone: {}\n- GM style: {}\n",
        payload.title.trim(),
        payload.premise.trim(),
        payload.setting.trim(),
        payload.gm_style.trim(),
    );
    if !payload.objective.trim().is_empty() {
        user.push_str(&format!("- Objective: {}\n", payload.objective.trim()));
    }
    user.push_str(&format!(
        "\nWorld state schema (do not duplicate these keys on the character):\n{}\n\nOther cast (context only):\n{}\n\nTarget character:\n- Role: {}\n- Name: {}\n",
        format_world_schema(&payload.state_schema),
        format_cast_context(&payload.cast, character.name.trim()),
        character.role,
        character.name.trim(),
    ));
    if !character.description.trim().is_empty() {
        user.push_str(&format!(
            "- Description: {}\n",
            character.description.trim()
        ));
    }
    if !traits.is_empty() {
        user.push_str(&format!("- Traits: {traits}\n"));
    }
    user.push_str(&format!(
        "\nExisting state for this character (add keys or update by key):\n{}\n\nPropose initial_state entries for this character at scenario start.",
        format_existing_state(&payload.existing_state),
    ));

    vec![
        json!({
            "role": "system",
            "content": format!("{GENERATE_CHARACTER_STATE_SYSTEM}\n\n{STATE_CHANGE_RULES}"),
        }),
        json!({
            "role": "user",
            "content": user,
        }),
    ]
}

pub async fn generate_character_state(
    pool: &SqlitePool,
    payload: &GenerateCharacterStateRequest,
    settings: &Settings,
) -> AppResult<GenerateCharacterStateResponse> {
    if !has_inference_provider(settings) {
        return Err(AppError::bad_request(
            "Configure an inference model in Settings before generating character state",
        ));
    }
    if payload.character.name.trim().is_empty() {
        return Err(AppError::bad_request("Character name is required"));
    }

    let messages = build_generate_character_state_messages(payload);
    let token = CancellationToken::new();

    let llm: LlmCharacterStateResponse = db::chat_completion_json_for_connection(
        pool,
        settings,
        &messages,
        Some(&character_state_generation_schema()),
        max_retries(),
        &token,
        None,
        None,
        None,
        None,
    )
    .await?;

    let initial_state =
        dreamwell_types::merge_character_state(&payload.existing_state, &llm.initial_state);
    Ok(GenerateCharacterStateResponse { initial_state })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dreamwell_types::{GenerateCharacterStateTarget, TrackedVarDef};

    #[test]
    fn prompt_includes_scenario_and_character_context() {
        let payload = GenerateCharacterStateRequest {
            title: "Heist".into(),
            premise: "Steal the gem.".into(),
            setting: "Cyberpunk noir".into(),
            gm_style: "Punchy".into(),
            objective: "Get out alive".into(),
            state_schema: vec![TrackedVarDef {
                key: "alarm".into(),
                kind: StateKind::Clock,
                target: "world".into(),
                ..Default::default()
            }],
            cast: vec![dreamwell_types::ScenarioNpc {
                name: "Maya".into(),
                content: "The fixer".into(),
                ..Default::default()
            }],
            character: GenerateCharacterStateTarget {
                role: "npc".into(),
                name: "Guard".into(),
                description: "Stationed at the vault door".into(),
                traits: [("Force".to_string(), 1)].into(),
            },
            existing_state: vec![CharacterStateDef {
                key: "alertness".into(),
                kind: StateKind::Clock,
                initial_num: Some(1),
                initial_max: Some(4),
                ..Default::default()
            }],
        };
        let messages = build_generate_character_state_messages(&payload);
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Cyberpunk noir"));
        assert!(user.contains("alarm"));
        assert!(user.contains("Guard"));
        assert!(user.contains("alertness"));
        assert!(user.contains("Maya"));
        assert!(user.contains("Stationed at the vault door"));
    }

    #[test]
    fn schema_requires_initial_state() {
        let schema = character_state_generation_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "initial_state"));
    }
}
