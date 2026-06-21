use dreamwell_state::STATE_CHANGE_PROMPT;
use dreamwell_types::{Game, GameDetail, GameScene, GameTurn, GameTurnCheck, Settings};
use serde_json::json;

use crate::game_state::build_state_block;

/// Layered turn context for game prompts: long-term summary, compact recent beats,
/// and verbatim recent prose (newest-first within each tier's budget).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TurnContextTiers {
    pub long_term: String,
    pub recent_beats: String,
    pub recent_prose: String,
}

#[derive(Debug, Clone, Copy)]
struct TurnContextBudget {
    prose_chars: usize,
    beats_chars: usize,
}

fn turn_context_budget(settings: &Settings) -> TurnContextBudget {
    if settings.context_tokens > 0 {
        TurnContextBudget {
            prose_chars: (settings.context_tokens / 4).max(512) as usize,
            beats_chars: (settings.context_tokens / 2).max(1024) as usize,
        }
    } else {
        TurnContextBudget {
            prose_chars: 2048,
            beats_chars: 4096,
        }
    }
}

const DECLARE_CHECKS_SYSTEM: &str = r#"You are a tabletop RPG rules assistant for one specific scenario. Use the premise, setting/tone, and GM style to decide whether checks are needed.

Rules:
- Ground every decision in the defined scenario — genre, stakes, and tone come from premise, setting/tone, and GM style
- Prefer no check for low-stakes, conversational, exploratory, or slice-of-life actions that fit the scenario
- Do not invent danger, opposition, clocks, or escalation unless the scenario or player action calls for it
- When checks are needed: use 2d6 + modifier PbtA-style resolution
- Propose skill, modifier, stakes, and justification for each check; stakes must fit the scenario tone, not default adventure peril
- Modifier is situational only (trait base is on the character sheet); keep modifiers modest
- Only propose checks using trait names listed on the PC sheet
- Return empty checks array with no_check_reason when the action resolves narratively without a roll
- Output ONLY valid JSON matching the schema"#;

const RESOLVE_SYSTEM: &str = r#"You are a tabletop RPG GM assistant for one specific scenario. Given resolved dice results, produce scene beats and typed state changes that honor the defined premise, setting/tone, and GM style.

Rules:
- Scene beats must match the scenario's genre, scale, and tone — do not default to peril, combat, or action-movie escalation
- Scene beats must honor the roll tiers (fail cannot be clean success)
- State changes should reflect scenario-appropriate consequences; avoid health/stress harm or new threats unless warranted
- Output ONLY valid JSON matching the schema"#;

const PROSE_SYSTEM: &str = r#"You are a tabletop RPG narrator for one specific scenario. Write second-person prose rendering the scene beats.

Rules:
- Voice, pacing, mood, intimacy, and tension come from GM style and setting/tone — not from generic adventure defaults
- Do not inject peril, cliffhangers, or unexplained threats unless the scenario defines that genre or the beats require it
- Honor resolved roll tiers — a fail must not read as unqualified success
- Do not contradict established state, scenario parameters, or scene beats
- No JSON, no meta commentary — prose only"#;

const SCENE_SUMMARIZE_SYSTEM: &str = r#"Compress game turn prose into a dense fact summary for downstream context.

Rules:
- Short clauses or bullet lines only
- Preserve facts that matter for the defined scenario (relationships, goals, tone, location) — not only danger or combat
- Include key events, character state, locations, unresolved threads
- Target ≤150 words
- Output only the summary text"#;

/// Shared scenario parameters included in every GM phase prompt.
pub(crate) fn scenario_context_block(game: &Game) -> String {
    [
        format!("Premise / scenario:\n{}", game.premise.trim()),
        format!("Setting / tone:\n{}", game.setting.trim()),
        format!("GM style:\n{}", game.gm_style.trim()),
    ]
    .join("\n\n")
}

fn user_message_with_scenario(game: &Game, body: &str) -> String {
    format!(
        "Scenario parameters:\n{}\n\n{}",
        scenario_context_block(game),
        body
    )
}

pub fn build_declare_checks_messages(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    guidance: &str,
    settings: &Settings,
) -> Vec<serde_json::Value> {
    let pc = detail.actors.iter().find(|a| a.role == "pc");
    let state_block = build_state_block(&detail.state, &detail.actors);
    let context = build_turn_context_tiers(&detail.turns, &detail.scenes, turn.id, settings);
    let mut body = format!(
        "Current state:\n{state_block}\n\n{}\n\nPlayer action: {}",
        format_turn_context_sections(&context),
        turn.player_action
    );
    if let Some(pc) = pc {
        body.push_str(&format!("\n\nPC: {} — {}", pc.name, pc.description));
        if !pc.skills.is_empty() {
            let mut traits: Vec<_> = pc
                .skills
                .iter()
                .map(|(name, value)| format!("{name} ({value:+})"))
                .collect();
            traits.sort();
            body.push_str(&format!(
                "\n\nAvailable traits for checks (use only these names): {}",
                traits.join(", ")
            ));
        }
    }
    if !guidance.trim().is_empty() {
        body.push_str(&format!("\n\nGM guidance: {guidance}"));
    }
    let user = user_message_with_scenario(game, &body);
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
    settings: &Settings,
) -> Vec<serde_json::Value> {
    let state_block = build_state_block(&detail.state, &detail.actors);
    let budget = turn_context_budget(settings);
    let context = build_turn_context_tiers_with_budget(
        &detail.turns,
        &detail.scenes,
        turn.id,
        TurnContextBudget {
            prose_chars: budget.prose_chars / 2,
            beats_chars: budget.beats_chars,
        },
        0,
    );
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
    let context_block = format_turn_context_sections(&context);
    let mut body = format!(
        "Player action: {}\n\nResolved checks:\n{checks_text}\n\nCurrent state:\n{state_block}",
        turn.player_action
    );
    if !context_block.is_empty() {
        body.push_str(&format!("\n\n{context_block}"));
    }
    if !guidance.trim().is_empty() {
        body.push_str(&format!("\n\nGM guidance: {guidance}"));
    }
    let user = user_message_with_scenario(game, &body);
    vec![
        json!({
            "role": "system",
            "content": format!("{RESOLVE_SYSTEM}\n\n{STATE_CHANGE_PROMPT}"),
        }),
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
    let context = build_turn_context_tiers_with_budget(
        &detail.turns,
        &detail.scenes,
        turn.id,
        turn_context_budget(settings),
        1,
    );
    let tiers = checks
        .iter()
        .filter_map(|c| c.tier.map(|t| format!("{:?}", t)))
        .collect::<Vec<_>>()
        .join(", ");
    let context_block = format_turn_context_sections(&context);
    let mut body = format!(
        "Scene beats:\n- {beats}\n\nRoll outcomes: {tiers}\n\nCurrent state:\n{state_block}\n\nPlayer action: {}",
        turn.player_action
    );
    if !context_block.is_empty() {
        body.push_str(&format!("\n\n{context_block}"));
    }
    if !guidance.trim().is_empty() {
        body.push_str(&format!("\n\nGM guidance: {guidance}"));
    }
    let user = user_message_with_scenario(game, &body);
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
        .map(format_prior_prose_chunk)
        .collect::<Vec<_>>()
        .join("\n\n");
    let user = user_message_with_scenario(&detail.game, &format!("Turn transcript:\n{transcript}"));
    vec![
        json!({ "role": "system", "content": SCENE_SUMMARIZE_SYSTEM }),
        json!({ "role": "user", "content": user }),
    ]
}

fn long_term_memory_context(scenes: &[GameScene]) -> String {
    scenes
        .iter()
        .filter(|s| s.summary_valid && !s.summary.trim().is_empty())
        .map(|s| {
            let label = if s.title.trim().is_empty() {
                "Earlier scene".to_string()
            } else {
                format!("Earlier scene — {}", s.title.trim())
            };
            format!("{label}:\n{}", s.summary.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn build_turn_context_tiers(
    turns: &[GameTurn],
    scenes: &[GameScene],
    before_id: i64,
    settings: &Settings,
) -> TurnContextTiers {
    build_turn_context_tiers_with_budget(turns, scenes, before_id, turn_context_budget(settings), 0)
}

fn build_turn_context_tiers_with_budget(
    turns: &[GameTurn],
    scenes: &[GameScene],
    before_id: i64,
    budget: TurnContextBudget,
    min_recent_prose: usize,
) -> TurnContextTiers {
    TurnContextTiers {
        long_term: long_term_memory_context(scenes),
        recent_beats: recent_beats_context(turns, before_id, budget.beats_chars),
        recent_prose: recent_prose_context_with_budget(
            turns,
            before_id,
            budget.prose_chars,
            min_recent_prose,
        ),
    }
}

pub(crate) fn format_turn_context_sections(tiers: &TurnContextTiers) -> String {
    let mut sections = Vec::new();
    if !tiers.long_term.is_empty() {
        sections.push(format!(
            "Long-term memory (compressed earlier scenes):\n{}",
            tiers.long_term
        ));
    }
    if !tiers.recent_beats.is_empty() {
        sections.push(format!(
            "Recent turns (scene beats — compact staging notes):\n{}",
            tiers.recent_beats
        ));
    }
    if !tiers.recent_prose.is_empty() {
        sections.push(format!(
            "Recent turns (prose — canonical narration):\n{}",
            tiers.recent_prose
        ));
    }
    sections.join("\n\n")
}

fn format_prior_prose_chunk(turn: &GameTurn) -> String {
    if turn.is_opening {
        format!("Opening:\n{}", turn.prose.trim())
    } else {
        format!("Turn: {}\n{}", turn.player_action.trim(), turn.prose.trim())
    }
}

fn recent_beats_context(turns: &[GameTurn], before_id: i64, budget: usize) -> String {
    let mut sections = Vec::new();
    let mut used = 0usize;
    for turn in turns.iter().filter(|t| t.id < before_id).rev() {
        if turn.scene_beats.is_empty() {
            continue;
        }
        let beats = turn
            .scene_beats
            .iter()
            .map(|beat| format!("- {beat}"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = format!("Turn: {}\n{beats}", turn.player_action.trim(),);
        if used + chunk.len() > budget && !sections.is_empty() {
            break;
        }
        used += chunk.len();
        sections.push(chunk);
    }
    cap_turn_context_sections(sections, budget)
}

fn recent_prose_context_with_budget(
    turns: &[GameTurn],
    before_id: i64,
    budget: usize,
    min_sections: usize,
) -> String {
    let mut sections = Vec::new();
    let mut used = 0usize;
    for turn in turns
        .iter()
        .filter(|t| t.id < before_id && !t.prose.trim().is_empty())
        .rev()
    {
        let chunk = format_prior_prose_chunk(turn);
        if used + chunk.len() > budget && !sections.is_empty() {
            break;
        }
        used += chunk.len();
        sections.push(chunk);
    }

    let mut result = cap_turn_context_sections(sections, budget);
    if min_sections > 0 && prose_section_count(&result) < min_sections {
        if let Some(chunk) = most_recent_prior_prose(turns, before_id) {
            result = chunk;
        }
    }
    result
}

fn prose_section_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.matches("Turn:").count() + text.matches("Opening:").count()
    }
}

fn most_recent_prior_prose(turns: &[GameTurn], before_id: i64) -> Option<String> {
    turns
        .iter()
        .filter(|t| t.id < before_id && !t.prose.trim().is_empty())
        .next_back()
        .map(format_prior_prose_chunk)
}

fn cap_turn_context_sections(mut sections: Vec<String>, max_chars: usize) -> String {
    if sections.is_empty() {
        return String::new();
    }
    // sections are newest-first; drop oldest until within budget.
    let mut combined = sections.join("\n\n");
    while combined.len() > max_chars && sections.len() > 1 {
        sections.pop();
        combined = sections.join("\n\n");
    }
    if combined.len() <= max_chars {
        return combined;
    }
    truncate_context_from_start(&combined, max_chars)
}

fn truncate_context_from_start(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let skip = text.len() - max_chars;
    format!(
        "[…earlier context truncated…]\n\n{}",
        text.chars().skip(skip).collect::<String>()
    )
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
    dreamwell_state::resolve_schema()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use dreamwell_types::{GameActor, GameScene, ResolutionSystem};

    use super::*;

    fn sample_game() -> Game {
        Game {
            id: 1,
            title: "Tea Shop".into(),
            premise: "Run a quiet neighborhood tea shop for an afternoon.".into(),
            setting: "Cozy, low-stakes, warm and conversational.".into(),
            gm_style: "Gentle pacing; focus on small choices and character moments.".into(),
            opening_message: "Steam curls from the kettle.".into(),
            character_id: None,
            scenario_id: None,
            resolution_system: ResolutionSystem::Pbta2d6,
            modifier_min: -2,
            modifier_max: 3,
            merge_resolve_scene: true,
            step_mode: false,
            model_checks: String::new(),
            model_resolve: String::new(),
            model_prose: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            active_job: None,
            queued_jobs: 0,
        }
    }

    fn sample_opening_turn() -> GameTurn {
        GameTurn {
            id: 1,
            game_id: 1,
            sort_order: -1,
            player_action: String::new(),
            phase: "done".into(),
            scene_beats: vec![],
            prose: "Steam curls from the kettle.".into(),
            state_changes: vec![],
            checks: vec![],
            is_opening: true,
            generation_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sample_detail(game: Game) -> GameDetail {
        GameDetail {
            game,
            actors: vec![GameActor {
                id: 1,
                game_id: 1,
                role: "pc".into(),
                name: "Mira".into(),
                description: "Shopkeeper".into(),
                skills: Default::default(),
                sort_order: 0,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }],
            state: vec![],
            turns: vec![sample_opening_turn()],
            scenes: vec![],
        }
    }

    fn sample_turn() -> GameTurn {
        GameTurn {
            id: 2,
            game_id: 1,
            sort_order: 0,
            player_action: "I greet the regular at the counter.".into(),
            phase: "checks".into(),
            scene_beats: vec![],
            prose: String::new(),
            state_changes: vec![],
            checks: vec![],
            is_opening: false,
            generation_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sample_turn_with_id(id: i64) -> GameTurn {
        let mut turn = sample_turn();
        turn.id = id;
        turn.sort_order = id;
        turn
    }

    fn sample_scene(summary: &str, valid: bool) -> GameScene {
        GameScene {
            id: 1,
            game_id: 1,
            title: "Opening".into(),
            summary: summary.into(),
            summary_valid: valid,
            summary_at: None,
            start_turn: 0,
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn test_settings() -> Settings {
        Settings {
            inference_url: String::new(),
            model: String::new(),
            temperature: 0.7,
            top_p: 1.0,
            max_tokens: 1024,
            system_prompt_prefix: String::new(),
            system_prompt_suffix: String::new(),
            user_name: String::new(),
            persona_description: String::new(),
            summarize_enabled: false,
            summarize_adaptive: false,
            summarize_after_messages: 12,
            summarize_keep_recent: 4,
            variables_enabled: false,
            thought_blocks_enabled: false,
            max_context_messages: 0,
            context_tokens: 0,
            auto_context_on_model_change: false,
            max_concurrent_jobs: 1,
        }
    }

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

    #[test]
    fn declare_checks_includes_opening_turn_as_prior_prose() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let turn = sample_turn();
        let messages = build_declare_checks_messages(&game, &detail, &turn, "", &test_settings());
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Scenario parameters:"));
        assert!(user.contains("Cozy, low-stakes"));
        assert!(user.contains("Recent turns (prose"));
        assert!(user.contains("Opening:"));
        assert!(user.contains("Steam curls"));
    }

    #[test]
    fn resolve_omits_opening_from_scenario_but_includes_opening_turn() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let turn = sample_turn();
        let settings = test_settings();
        let resolve = build_resolve_messages(&game, &detail, &turn, &[], "", &settings);
        let user = resolve[1]["content"].as_str().unwrap();
        assert!(user.contains("Scenario parameters:"));
        assert!(user.contains("Gentle pacing"));
        assert!(user.contains("Opening:"));
        assert!(user.contains("Steam curls"));
    }

    #[test]
    fn prose_includes_at_least_one_prior_prose_for_continuity() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let turn = sample_turn();
        let settings = test_settings();
        let prose = build_prose_messages(&game, &detail, &turn, &[], "", &settings);
        let user = prose[1]["content"].as_str().unwrap();
        assert!(user.contains("Recent turns (prose"));
        assert!(user.contains("Steam curls"));

        let prior = GameTurn {
            prose: "You slide the cup across the counter.".into(),
            player_action: "I serve tea.".into(),
            ..sample_turn_with_id(3)
        };
        let mut turn2 = sample_turn_with_id(4);
        turn2.scene_beats = vec!["The guest smiles.".into()];
        let mut detail2 = sample_detail(game.clone());
        detail2.turns = vec![sample_opening_turn(), prior];
        let prose2 = build_prose_messages(&game, &detail2, &turn2, &[], "", &settings);
        let user2 = prose2[1]["content"].as_str().unwrap();
        assert!(user2.contains("slide the cup"));
    }

    #[test]
    fn opening_turn_ages_out_of_recent_prose_when_newer_turns_fill_budget() {
        let opening = sample_opening_turn();
        let mut turns = vec![opening];
        for id in 2..=7 {
            turns.push(GameTurn {
                id,
                sort_order: id,
                prose: format!("Prose chunk for turn {id} with enough text to consume budget."),
                player_action: format!("Action {id}"),
                is_opening: false,
                ..sample_turn()
            });
        }
        let tiers = build_turn_context_tiers_with_budget(
            &turns,
            &[],
            8,
            TurnContextBudget {
                prose_chars: 300,
                beats_chars: 4096,
            },
            0,
        );
        assert!(!tiers.recent_prose.contains("Steam curls"));
        assert!(tiers.recent_prose.contains("Prose chunk for turn 7"));
    }

    #[test]
    fn turn_context_tiers_include_long_term_beats_and_prose() {
        let prior = sample_turn_with_id(3);
        let prior = GameTurn {
            scene_beats: vec!["Mira pours tea.".into()],
            prose: "Steam rises as you pour the oolong.".into(),
            player_action: "I pour tea for the guest.".into(),
            ..prior
        };
        let current = sample_turn_with_id(4);
        let scenes = vec![sample_scene("Mira runs a quiet shop.", true)];
        let tiers = build_turn_context_tiers(
            &[sample_opening_turn(), prior],
            &scenes,
            current.id,
            &test_settings(),
        );
        assert!(tiers.long_term.contains("quiet shop"));
        assert!(tiers.recent_beats.contains("Mira pours tea"));
        assert!(tiers.recent_prose.contains("pour the oolong"));
    }

    #[test]
    fn format_turn_context_sections_orders_tiers() {
        let tiers = TurnContextTiers {
            long_term: "Old summary".into(),
            recent_beats: "Beat chunk".into(),
            recent_prose: "Prose chunk".into(),
        };
        let formatted = format_turn_context_sections(&tiers);
        let long_term_pos = formatted.find("Long-term memory").unwrap();
        let beats_pos = formatted.find("scene beats").unwrap();
        let prose_pos = formatted.find("prose — canonical").unwrap();
        assert!(long_term_pos < beats_pos);
        assert!(beats_pos < prose_pos);
    }

    #[test]
    fn resolve_prompt_includes_recent_beats_and_long_term_memory() {
        let game = sample_game();
        let prior = GameTurn {
            scene_beats: vec!["The bell chimes.".into()],
            prose: "A regular steps inside.".into(),
            player_action: "I watch the door.".into(),
            ..sample_turn_with_id(3)
        };
        let turn = sample_turn_with_id(4);
        let mut detail = sample_detail(game.clone());
        detail.turns = vec![sample_opening_turn(), prior];
        detail.scenes = vec![sample_scene("The shop has one regular.", true)];
        let messages = build_resolve_messages(&game, &detail, &turn, &[], "", &test_settings());
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Long-term memory"));
        assert!(user.contains("one regular"));
        assert!(user.contains("bell chimes"));
        assert!(user.contains("regular steps inside"));
    }

    #[test]
    fn prose_prompt_uses_tiered_context_sections() {
        let game = sample_game();
        let prior = GameTurn {
            scene_beats: vec!["Tea is served.".into()],
            prose: "You slide the cup across the counter.".into(),
            player_action: "I serve tea.".into(),
            ..sample_turn_with_id(3)
        };
        let mut turn = sample_turn_with_id(4);
        turn.scene_beats = vec!["The guest smiles.".into()];
        let mut detail = sample_detail(game.clone());
        detail.turns = vec![sample_opening_turn(), prior];
        detail.scenes = vec![sample_scene("Afternoon service continues.", true)];
        let messages = build_prose_messages(&game, &detail, &turn, &[], "", &test_settings());
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Long-term memory"));
        assert!(user.contains("Tea is served"));
        assert!(user.contains("slide the cup"));
        assert!(!user.contains("Earlier scene summary:"));
    }

    #[test]
    fn beats_budget_fits_more_turns_than_prose_budget() {
        let mut turns = vec![sample_opening_turn()];
        for id in 2..=7 {
            turns.push(GameTurn {
                id,
                sort_order: id,
                scene_beats: vec![format!(
                    "Beat detail for turn {id} with extra staging notes."
                )],
                prose: "x".repeat(500),
                player_action: format!("Action {id}"),
                is_opening: false,
                ..sample_turn()
            });
        }
        let current_id = 8;
        let settings = test_settings();
        let tiers = build_turn_context_tiers(&turns, &[], current_id, &settings);
        let beat_turn_count = tiers.recent_beats.matches("Turn:").count();
        let prose_turn_count = tiers.recent_prose.matches("Turn:").count();
        assert!(beat_turn_count > prose_turn_count);
    }

    #[test]
    fn system_prompts_discourage_default_peril() {
        assert!(DECLARE_CHECKS_SYSTEM.contains("Do not invent danger"));
        assert!(RESOLVE_SYSTEM.contains("do not default to peril"));
        assert!(PROSE_SYSTEM.contains("not from generic adventure defaults"));
    }
}
