use dreamwell_types::{Game, GameDetail, GameTurn, GameTurnCheck, Settings};
use serde_json::json;

use crate::game_state::build_state_block;

const DECLARE_CHECKS_SYSTEM: &str = r#"You are a tabletop RPG rules assistant. Given a player action and current game state, declare which skill checks (if any) are needed.

Rules:
- Use 2d6 + modifier PbtA-style resolution
- Propose skill, modifier, stakes, and justification for each check
- Modifier is situational only (skill base is on the character sheet); keep modifiers modest
- Return empty checks array with no_check_reason for pure narrative actions
- Output ONLY valid JSON matching the schema"#;

const RESOLVE_SYSTEM: &str = r#"You are a tabletop RPG GM assistant. Given resolved dice results, produce scene beats and typed state changes.

Rules:
- Scene beats must honor the roll tiers (fail cannot be clean success)
- state_changes use targets: "pc" for player character, "world" for global
- kind: resource|condition|fact|clock; op: set|add|remove
- Resource/clock deltas are numeric; conditions/facts use value strings
- Output ONLY valid JSON matching the schema"#;

const PROSE_SYSTEM: &str = r#"You are a tabletop RPG narrator. Write vivid second-person prose rendering the scene beats.

Rules:
- Honor resolved roll tiers — a fail must not read as unqualified success
- Do not contradict established state or scene beats
- No JSON, no meta commentary — prose only"#;

const SCENE_SUMMARIZE_SYSTEM: &str = r#"Compress game turn prose into a dense fact summary for downstream context.

Rules:
- Short clauses or bullet lines only
- Include key events, character state, locations, unresolved threads
- Target ≤150 words
- Output only the summary text"#;

pub fn build_declare_checks_messages(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    guidance: &str,
) -> Vec<serde_json::Value> {
    let pc = detail.actors.iter().find(|a| a.role == "pc");
    let state_block = build_state_block(&detail.state, &detail.actors);
    let recent = recent_turn_context(&detail.turns, turn.id, 3);
    let mut user = format!(
        "World premise: {}\nSetting/tone: {}\nGM style: {}\n\nCurrent state:\n{state_block}\n\nRecent turns:\n{recent}\n\nPlayer action: {}",
        game.premise, game.setting, game.gm_style, turn.player_action
    );
    if let Some(pc) = pc {
        user.push_str(&format!("\n\nPC: {} — {}", pc.name, pc.description));
    }
    if !guidance.trim().is_empty() {
        user.push_str(&format!("\n\nGM guidance: {guidance}"));
    }
    vec![
        json!({ "role": "system", "content": DECLARE_CHECKS_SYSTEM }),
        json!({ "role": "user", "content": user }),
    ]
}

pub fn build_resolve_messages(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    checks: &[GameTurnCheck],
    guidance: &str,
) -> Vec<serde_json::Value> {
    let state_block = build_state_block(&detail.state, &detail.actors);
    let checks_text = if checks.is_empty() {
        "No checks — pure narration.".to_string()
    } else {
        checks
            .iter()
            .map(|c| {
                let tier = c
                    .tier
                    .map(|t| format!("{t:?}"))
                    .unwrap_or_else(|| "?".to_string());
                format!(
                    "- {} ({}+{}): rolled {:?} = {} → {tier} (margin {}) — stakes: {}",
                    c.label, c.skill, c.modifier, c.rolls, c.total, c.margin, c.stakes
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut user = format!(
        "Player action: {}\n\nResolved checks:\n{checks_text}\n\nCurrent state:\n{state_block}",
        turn.player_action
    );
    if !guidance.trim().is_empty() {
        user.push_str(&format!("\n\nGM guidance: {guidance}"));
    }
    let _ = game;
    vec![
        json!({ "role": "system", "content": RESOLVE_SYSTEM }),
        json!({ "role": "user", "content": user }),
    ]
}

pub fn build_prose_messages(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    checks: &[GameTurnCheck],
    guidance: &str,
    settings: &Settings,
) -> Vec<serde_json::Value> {
    let state_block = build_state_block(&detail.state, &detail.actors);
    let beats = turn.scene_beats.join("\n- ");
    let scene_summary = detail
        .scenes
        .iter()
        .find(|s| s.summary_valid)
        .map(|s| s.summary.as_str())
        .unwrap_or("");
    let recent_prose = recent_prose_context(&detail.turns, turn.id, settings);
    let tiers = checks
        .iter()
        .filter_map(|c| c.tier.map(|t| format!("{:?}", t)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut user = format!(
        "Scene beats:\n- {beats}\n\nRoll outcomes: {tiers}\n\nCurrent state:\n{state_block}\n\nPlayer action: {}\n\n{recent_prose}",
        turn.player_action
    );
    if !scene_summary.is_empty() {
        user.push_str(&format!("\n\nEarlier scene summary:\n{scene_summary}"));
    }
    if !guidance.trim().is_empty() {
        user.push_str(&format!("\n\nGM guidance: {guidance}"));
    }
    let _ = game;
    vec![
        json!({ "role": "system", "content": PROSE_SYSTEM }),
        json!({ "role": "user", "content": user }),
    ]
}

pub fn build_scene_summarize_messages(detail: &GameDetail) -> Vec<serde_json::Value> {
    let transcript: String = detail
        .turns
        .iter()
        .filter(|t| !t.prose.trim().is_empty())
        .map(|t| format!("Action: {}\n{}", t.player_action, t.prose.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    vec![
        json!({ "role": "system", "content": SCENE_SUMMARIZE_SYSTEM }),
        json!({ "role": "user", "content": format!("Turn transcript:\n{transcript}") }),
    ]
}

fn recent_turn_context(turns: &[GameTurn], before_id: i64, limit: usize) -> String {
    turns
        .iter()
        .filter(|t| t.id < before_id && !t.player_action.is_empty())
        .rev()
        .take(limit)
        .map(|t| {
            if t.prose.trim().is_empty() {
                format!("> {}", t.player_action)
            } else {
                format!("> {}\n{}", t.player_action, t.prose.trim())
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn recent_prose_context(turns: &[GameTurn], before_id: i64, settings: &Settings) -> String {
    let budget = if settings.context_tokens > 0 {
        (settings.context_tokens / 4).max(512) as usize
    } else {
        2048
    };
    let mut used = 0usize;
    let mut parts = Vec::new();
    for turn in turns.iter().filter(|t| t.id < before_id).rev() {
        let chunk = format!("Turn: {}\n{}", turn.player_action, turn.prose.trim());
        if used + chunk.len() > budget {
            break;
        }
        used += chunk.len();
        parts.push(chunk);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("Recent prose:\n{}", parts.join("\n\n"))
    }
}

pub fn declare_checks_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "checks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "skill": { "type": "string" },
                        "modifier": { "type": "integer" },
                        "stakes": { "type": "string" },
                        "justification": { "type": "string" }
                    },
                    "required": ["label", "skill", "modifier", "stakes", "justification"]
                }
            },
            "no_check_reason": { "type": ["string", "null"] }
        },
        "required": ["checks"]
    })
}

pub fn resolve_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "scene_beats": { "type": "array", "items": { "type": "string" } },
            "state_changes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "target": { "type": "string" },
                        "kind": { "type": "string", "enum": ["resource", "condition", "fact", "clock"] },
                        "key": { "type": "string" },
                        "op": { "type": "string", "enum": ["set", "add", "remove"] },
                        "value": { "type": "string" },
                        "delta": { "type": "integer" }
                    },
                    "required": ["target", "kind", "key", "op"]
                }
            }
        },
        "required": ["scene_beats", "state_changes"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declare_checks_schema_is_object_with_checks() {
        let schema = declare_checks_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["checks"].is_object());
    }

    #[test]
    fn resolve_schema_requires_scene_beats_and_state_changes() {
        let schema = resolve_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "scene_beats"));
        assert!(required.iter().any(|v| v == "state_changes"));
    }
}
