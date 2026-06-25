use dreamwell_state::{state_kind_str, STATE_CHANGE_PROMPT};
use dreamwell_types::{
    substitute_macros, Game, GameActor, GameDetail, GameScene, GameTurn, GameTurnCheck,
    MacroContext, Settings, PROSE_CHECK_MARKER_OPEN, PROSE_INLINE_MARKER_CLOSE,
    PROSE_MECH_MARKER_OPEN, PROSE_STATE_MARKER_OPEN,
};
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
- Checks add gameplay even in cozy, intimate, or slice-of-life scenarios — use them when the outcome is uncertain or when success/failure would change the scene
- In gentle or low-peril scenarios, stakes can be social, emotional, craft, memory, composure, or subtle consequence — not combat, injury, or alarm
- Skip a check only when the action is purely observational or already guaranteed with no meaningful uncertainty
- When the scenario resolves the action through its own board/deck/dice mechanics (rolling the game's die, moving a piece, drawing or resolving a card), do NOT add a separate dramatic check for that routine mechanical step — return an empty checks array; reserve checks for genuinely uncertain dramatic, social, or interpersonal moments the game's own mechanics do not already cover
- Do not invent danger, opposition, clocks, or escalation unless the scenario, player action, or GM guidance calls for it
- When GM guidance is present, treat it as mandatory human direction — use it with the player action, or as the sole direction when the player action is empty
- When checks are needed: use 2d6 + modifier PbtA-style resolution
- Propose skill, modifier, stakes, and justification for each check; stakes must fit the scenario tone, not default adventure peril
- Modifier is situational only (trait base is on the character sheet); keep modifiers modest
- Only propose checks using trait names listed for the PC in the Characters block
- Return empty checks array with no_check_reason only when a roll would add no tension or uncertainty
- Keep string fields concise so the JSON response stays complete
- Output ONLY valid JSON matching the schema

Use these exact field names (do not rename or nest differently):
{
  "checks": [
    {
      "label": "short name for the roll",
      "skill": "exact trait name from Characters",
      "modifier": 0,
      "stakes": "what happens on failure",
      "justification": "why this roll is needed"
    }
  ],
  "no_check_reason": null
}
When no checks are needed: "checks": [] and set no_check_reason to a brief reason (not null)."#;

/// Compact hint appended to JSON repair retries for the declare-checks phase.
pub fn declare_checks_repair_hint() -> &'static str {
    r#"Required JSON shape (exact keys):
{"checks":[{"label":"...","skill":"TraitName","modifier":0,"stakes":"...","justification":"..."}],"no_check_reason":null}
- checks: array (may be empty); each item needs label, skill, modifier (integer), stakes, justification
- no_check_reason: string when checks is empty, otherwise null
- Do not use alternate key names like trait, name, mod, or dramatic_checks"#
}

const INLINE_PROSE_AGENT_SYSTEM: &str = r#"You are a tabletop RPG narrator for one specific scenario. Write second-person prose that resolves the player's action and moves the scene forward, calling tools inline to fire any real game mechanic.

POV (overrides GM style):
- Always narrate the PC in second person: "you"/"your" — NEVER "I"/"my"/"me" for the PC, even if GM style says first person.
- GM style first-person instructions apply to tone, pacing, and detail level only — not pronouns.
- NPCs and their dialogue use third person or quoted speech as usual.

GM guidance (human player direction):
- When the user message includes GM guidance, treat it as mandatory — the human player's explicit direction for this turn.
- GM guidance can stand alone when the player action is empty; when both are present, both must shape the turn.
- Prose and tool use must visibly reflect GM guidance; do not ignore it in favor of default scenario momentum.

How to narrate:
- Narrate the world, NPCs, environment, and the consequences of the player's stated action and GM guidance — in second person ("you").
- Follow GM style and scenario rules for pacing, length, and level of detail.
- Prefer plain action and dialogue over lyrical description and stacked metaphors unless GM style calls for richer prose.
- Honor any resolved check tiers already rolled for this turn — a fail must not read as unqualified success.

Inline mechanics (use tools, never invent outcomes):
- board_move, draw_card, and roll_dice are generic primitives — use them whenever the scenario rules call for board movement, a deck draw, or a dice roll.
- Follow the scenario's rules blocks for turn sequencing, deck selection, and when each mechanic applies.
- draw_card requires an explicit deck_id — choose the deck per scenario rules (e.g. map space tags from board_move to the correct deck).
- One mechanic per cycle: write prose describing the lead-up, call exactly one matching tool, then write prose from the tool's result before starting the next mechanic. Never batch board_move, draw_card, or roll_dice in the same assistant message.
- Narrate the lead-up to the mechanic — the PC reaching for the deck, the die leaving the hand, the step toward the next space — then call the matching tool BEFORE narrating how it lands. Stop the prose at the moment of action and let the tool decide the outcome.
- Never narrate the result (the card's face, the rolled number, the space landed on) before the tool returns — call the tool first, then continue from the actual outcome (canonical card text, rolled numbers, board position).
- After a tool returns, its result is inserted into the narration as a visible block; resume the prose from that real outcome.
- Do not fabricate dice numbers, card text, or board movement.

Example rhythm (follow this interleaved pattern):
GOOD — prose, then one tool, then outcome prose, repeat:
  You hook a finger under the top card of the events deck and flip it face-up.
  → draw_card(deck_id="events")
  The card reads "Shortcut — skip ahead one space."
  → apply_state_changes(changes=[{target:"pc", kind:"condition", key:"has_shortcut", op:"set", value:"true"}])
  You pocket the slip and reach for the die.
  You scoop up the move die and toss it across the board.
  → board_move(actor="pc")
  It clatters to a four; you advance four spaces toward the finish line.
  → apply_state_changes(changes=[{target:"world", kind:"fact", key:"location", op:"set", value:"space 36, near finish"}, {target:"pc", kind:"fact", key:"mood", op:"set", value:"excited"}])
  Your pulse kicks up as the crowd cheers from the sidelines.

BAD — do not do this:
  → draw_card(...) and board_move(...) together before any prose
  You roll a four and draw Shortcut. (outcomes invented before tools run)
  You're excited and standing near the finish line now. (location and mood only in prose — no apply_state_changes)

PC agency:
- When a card or scene requires a choice the player has not made, call ask_pc_decision with a concrete question BEFORE resolving the effect.
- Do not pick targets, options, or strategic decisions for the PC.

Tracked state (apply_state_changes tool):
- When the scene establishes or changes durable tracked facts OR resolves a card or mechanic effect, call apply_state_changes — do not only mention them in prose.
- The tool is the source of truth, not the prose alone.

Ending the turn:
- When the PC must make a choice the player did not specify, call ask_pc_decision with a direct second-person question, then stop.
- Narrate up to the next decision point, then stop — do not narrate the PC's next choice for them.
- Plain prose and tool calls only — no JSON, no meta commentary, no headers."#;

const PC_AGENCY_RULES: &str = r#"PC agency (critical — applies in every phase):
- The player action is the PC's intent when present. GM guidance is mandatory direction from the human running the game — not optional flavor.
- When the player action is empty but GM guidance is present, the guidance IS the turn direction; honor it fully in checks and prose.
- Only resolve outcomes for the PC that follow directly from what the player action and/or GM guidance explicitly states.
- Do not invent new choices, targets, preferences, dialogue, or strategic decisions for the PC beyond what the player action or GM guidance authorizes.
- When the PC must make a choice the player did not specify, stop at revealing what needs deciding — do not pick for them.
- Partial or vague player actions authorize only what they explicitly request; do not extrapolate unstated follow-on choices for the PC.
- NPC decisions are fair game: decide freely for NPCs per scenario rules."#;

fn game_system_prompt(base: &str) -> String {
    format!("{base}\n\n{PC_AGENCY_RULES}")
}

const SCENE_SUMMARIZE_SYSTEM: &str = r#"Compress game turn prose into a dense fact summary for downstream context.

Rules:
- Short clauses or bullet lines only
- Preserve facts that matter for the defined scenario (relationships, goals, tone, location) — not only danger or combat
- Include key events, character state, locations, unresolved threads
- Target ≤150 words
- Output only the summary text"#;

/// Shared scenario parameters included in every GM phase prompt.
pub(crate) fn scenario_context_block(game: &Game, ctx: &MacroContext<'_>) -> String {
    [
        format!(
            "Premise / scenario:\n{}",
            substitute_macros(game.premise.trim(), ctx)
        ),
        format!(
            "Setting / tone:\n{}",
            substitute_macros(game.setting.trim(), ctx)
        ),
        format!(
            "GM style:\n{}",
            substitute_macros(game.gm_style.trim(), ctx)
        ),
    ]
    .join("\n\n")
}

fn user_message_with_scenario(game: &Game, body: &str, ctx: &MacroContext<'_>) -> String {
    format!(
        "Scenario parameters:\n{}\n\n{}",
        scenario_context_block(game, ctx),
        body
    )
}

fn actor_role_rank(role: &str) -> i32 {
    if role == "pc" {
        0
    } else {
        1
    }
}

fn actor_display_name(actor: &GameActor) -> String {
    if !actor.name.trim().is_empty() {
        return actor.name.trim().to_string();
    }
    if actor.role == "pc" {
        "Player character".to_string()
    } else {
        "Unnamed NPC".to_string()
    }
}

fn actor_role_label(role: &str) -> &'static str {
    if role == "pc" {
        "PC"
    } else {
        "NPC"
    }
}

/// Canonical roster block for PC and NPC actors, included in every game prompt phase.
pub(crate) fn build_characters_block(actors: &[GameActor]) -> String {
    let mut ordered: Vec<&GameActor> = actors
        .iter()
        .filter(|actor| actor.role == "pc" || actor.role == "npc")
        .collect();
    ordered.sort_by(|a, b| {
        actor_role_rank(&a.role)
            .cmp(&actor_role_rank(&b.role))
            .then(a.sort_order.cmp(&b.sort_order))
            .then(a.id.cmp(&b.id))
    });

    let sections: Vec<String> = ordered
        .into_iter()
        .map(|actor| {
            let mut lines = vec![format!(
                "## {} ({})",
                actor_display_name(actor),
                actor_role_label(&actor.role)
            )];
            if !actor.description.trim().is_empty() {
                lines.push(actor.description.trim().to_string());
            }
            if !actor.skills.is_empty() {
                let mut traits: Vec<_> = actor
                    .skills
                    .iter()
                    .map(|(name, value)| format!("{name} ({value:+})"))
                    .collect();
                traits.sort();
                lines.push(format!("Traits: {}", traits.join(", ")));
            }
            lines.join("\n")
        })
        .collect();

    if sections.is_empty() {
        String::new()
    } else {
        format!("Characters:\n{}", sections.join("\n\n"))
    }
}

fn append_characters_section(body: &mut String, actors: &[GameActor]) {
    let block = build_characters_block(actors);
    if !block.is_empty() {
        body.push_str(&format!("\n\n{block}"));
    }
}

/// Turn pipeline phase — each later phase includes all context from earlier phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnPromptPhase {
    DeclareChecks,
    /// Single-pass narration that calls scenario mechanic tools inline (structured engine mode).
    ProseInline,
}

/// Inputs shared by every turn-pipeline prompt builder.
struct TurnPromptInputs<'a> {
    game: &'a Game,
    detail: &'a GameDetail,
    turn: &'a GameTurn,
    checks: &'a [GameTurnCheck],
    guidance: &'a str,
    settings: &'a Settings,
    ctx: &'a MacroContext<'a>,
}

/// Generous guardrail for total scenario-rules size. History has its own
/// independent budget, so this only protects against a pathologically large
/// rule set blowing the model context; it is set high enough that normal
/// scenarios (including dense IW imports) are never truncated.
fn rules_budget_chars(settings: &Settings) -> usize {
    if settings.context_tokens > 0 {
        (settings.context_tokens as usize)
            .saturating_mul(4)
            .max(32_768)
    } else {
        usize::MAX
    }
}

fn format_rules_blocks(game: &Game, settings: &Settings) -> String {
    let budget = rules_budget_chars(settings);
    let mut blocks = Vec::new();
    let mut used = 0usize;
    let mut truncated = false;
    for b in relevant_rules_blocks(game) {
        let block = format!("## {}\n{}", b.name, b.content);
        // Drop whole trailing blocks rather than cutting a block mid-text, so
        // rules like card tables are never partially shown.
        if used + block.len() > budget && !blocks.is_empty() {
            truncated = true;
            break;
        }
        used += block.len();
        blocks.push(block);
    }
    let mut text = blocks.join("\n\n");
    if truncated {
        text.push_str("\n\n[…additional scenario rules omitted to fit context…]");
    }
    text
}

fn push_section(body: &mut String, section: &str) {
    if section.is_empty() {
        return;
    }
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(section);
}

/// Prefer guidance persisted on the turn; fall back to the queued job payload.
pub(crate) fn effective_turn_guidance(turn: &GameTurn, job_guidance: &str) -> String {
    if !turn.guidance_notes.trim().is_empty() {
        turn.guidance_notes.trim().to_string()
    } else {
        job_guidance.trim().to_string()
    }
}

fn append_turn_direction(body: &mut String, turn: &GameTurn, job_guidance: &str) {
    let guidance = effective_turn_guidance(turn, job_guidance);
    let action = turn.player_action.trim();

    if guidance.is_empty() && action.is_empty() {
        return;
    }

    if !guidance.is_empty() {
        let label = if action.is_empty() {
            "GM guidance (this turn's direction — no separate player action; honor this fully)"
        } else {
            "GM guidance (mandatory — must shape this turn alongside the player action)"
        };
        push_section(body, &format!("{label}:\n{guidance}"));
    }

    if !action.is_empty() {
        push_section(body, &format!("Player action: {action}"));
    }
}

fn format_resolved_checks(checks: &[GameTurnCheck]) -> String {
    if checks.is_empty() {
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
    }
}

fn history_context_for_phase(
    detail: &GameDetail,
    turn: &GameTurn,
    phase: TurnPromptPhase,
    settings: &Settings,
    ctx: &MacroContext<'_>,
) -> String {
    let budget = turn_context_budget(settings);
    let (budget, min_recent_prose) = match phase {
        TurnPromptPhase::ProseInline => (budget, 1),
        TurnPromptPhase::DeclareChecks => (budget, 0),
    };
    let tiers = build_turn_context_tiers_with_budget(
        &detail.turns,
        &detail.scenes,
        turn.id,
        budget,
        min_recent_prose,
        ctx,
    );
    format_turn_context_sections(&tiers)
}

/// Shared cumulative user-message body for turn pipeline prompts.
fn build_cumulative_turn_body(phase: TurnPromptPhase, inputs: &TurnPromptInputs<'_>) -> String {
    let TurnPromptInputs {
        game,
        detail,
        turn,
        checks,
        guidance,
        settings,
        ctx,
    } = inputs;

    // Ordered stable → volatile so the leading scenario rules + characters form
    // a byte-identical prefix across phases and turns, maximizing KV/prefix
    // cache reuse on the inference backend. Per-turn content (state, plan,
    // checks, beats, player action) trails so it never breaks that prefix.
    let mut body = String::new();

    let rules_text = format_rules_blocks(game, settings);
    if !rules_text.is_empty() {
        push_section(&mut body, &format!("Scenario rules:\n{rules_text}"));
    }

    push_section(&mut body, &build_characters_block(&detail.actors));

    let state_block = build_state_block(&detail.state, &detail.actors);
    push_section(&mut body, &format!("Current state:\n{state_block}"));

    if phase == TurnPromptPhase::ProseInline {
        let schema = format_tracked_state_schema(&game.state_schema);
        if !schema.is_empty() {
            push_section(&mut body, &schema);
        }
        let elements = format_game_elements_context(game);
        if !elements.is_empty() {
            push_section(&mut body, &elements);
        }
    }

    let history = history_context_for_phase(detail, turn, phase, settings, ctx);
    push_section(&mut body, &history);

    push_section(&mut body, &format_plan_and_system_rolls(turn));
    let mechanics = format_mechanical_results(turn);
    if !mechanics.is_empty() {
        push_section(&mut body, &mechanics);
    }

    if phase == TurnPromptPhase::ProseInline {
        let checks_text = format_resolved_checks(checks);
        push_section(&mut body, &format!("Resolved checks:\n{checks_text}"));
        if let Some(section) = card_in_play_section(detail, turn) {
            push_section(&mut body, &section);
        }
    }

    append_turn_direction(&mut body, turn, guidance);

    if phase == TurnPromptPhase::DeclareChecks {
        push_section(
            &mut body,
            "Decide whether this action needs dramatic checks. \
             Respond with JSON only — use the exact field names from the system message \
             (checks, label, skill, modifier, stakes, justification, no_check_reason).",
        );
    }

    if phase == TurnPromptPhase::ProseInline {
        let guidance_present = !effective_turn_guidance(turn, guidance).is_empty();
        let mut instruction = String::from(
            "Narrate this turn now in second person (\"you\"). \
             Follow the scenario rules for turn sequencing and when to call board_move, draw_card, or roll_dice. \
             If a pending effect from a previous turn is listed above, resolve it before starting new mechanics. \
             For each mechanic: prose lead-up → one tool call → prose from the result — never batch multiple mechanics or call tools before describing the action; \
             call tools inline for mechanics and tracked state; stop with ask_pc_decision when the PC owes a choice.",
        );
        if guidance_present {
            instruction.push_str(
                " If GM guidance is present above, it is mandatory human direction — the turn must visibly follow it.",
            );
        }
        push_section(&mut body, &instruction);
    }

    body
}

/// When the immediately previous turn ended on a freshly drawn card whose effect was
/// never resolved, surface that card so the inline-prose agent finishes resolving it.
fn card_in_play_section(detail: &GameDetail, current: &GameTurn) -> Option<String> {
    use dreamwell_types::MechanicalData;
    let prev = detail
        .turns
        .iter()
        .filter(|t| t.id != current.id && t.sort_order < current.sort_order)
        .filter(|t| !t.mechanical_results.is_empty())
        .max_by_key(|t| t.sort_order)?;
    let MechanicalData::CardDraw { name, text, .. } = &prev.mechanical_results.last()?.data else {
        return None;
    };
    Some(format!(
        "Pending effect (from previous turn, not yet resolved):\n- {name}: {text}\n\
         If the player's action this turn addresses this effect, resolve it before starting new mechanics.",
    ))
}

fn format_game_elements_context(game: &Game) -> String {
    let elements = &game.game_elements;
    if elements.boards.is_empty() && elements.decks.is_empty() {
        return String::new();
    }

    let mut lines = vec!["Game elements (runtime state):".to_string()];

    for board in &elements.boards {
        let mut board_line = format!(
            "- Board \"{}\": {} spaces, move die {}, default tag \"{}\"",
            board.id, board.spaces, board.move_dice, board.default_tag
        );
        if !board.tag_rules.is_empty() {
            let rules: Vec<String> = board
                .tag_rules
                .iter()
                .map(|rule| format!("{} → spaces {:?}", rule.tag, rule.spaces))
                .collect();
            board_line.push_str(&format!("; tag rules: {}", rules.join("; ")));
        }
        lines.push(board_line);
        for (actor, pos) in &game.element_instances.board_positions {
            lines.push(format!(
                "  - {actor} position on \"{}\": space {pos}",
                board.id
            ));
        }
        if game.element_instances.board_positions.is_empty() {
            lines.push(format!("  - No positions recorded yet on \"{}\"", board.id));
        }
    }

    if !elements.decks.is_empty() {
        let deck_ids: Vec<&str> = elements.decks.iter().map(|d| d.id.as_str()).collect();
        lines.push(format!("- Available decks: {}", deck_ids.join(", ")));
        for deck in &elements.decks {
            let remaining = game
                .element_instances
                .deck_piles
                .get(&deck.id)
                .map(|pile| pile.draw_pile.len())
                .unwrap_or(deck.cards.len());
            lines.push(format!(
                "  - \"{}\": {remaining} cards remaining in draw pile",
                deck.id
            ));
        }
    }

    lines.join("\n")
}

fn format_tracked_state_schema(schema: &[dreamwell_types::TrackedVarDef]) -> String {
    if schema.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = schema
        .iter()
        .map(|def| {
            let scope = match def.scope {
                dreamwell_types::StateScope::World => "world",
                dreamwell_types::StateScope::Pc => "pc",
                dreamwell_types::StateScope::Npc => "npc",
            };
            let mut line = format!("- {} ({}, {})", def.key, state_kind_str(def.kind), scope);
            if def.scope == dreamwell_types::StateScope::Npc {
                if let Some(name) = def.actor_name.as_deref().filter(|n| !n.trim().is_empty()) {
                    line.push_str(&format!(" [{name}]"));
                }
            }
            if !def.description.trim().is_empty() {
                line.push_str(&format!(": {}", def.description.trim()));
            }
            if !def.update_hints.trim().is_empty() {
                line.push_str(&format!(" [hint: {}]", def.update_hints.trim()));
            }
            line
        })
        .collect();
    format!(
        "Tracked state schema (use apply_state_changes to update these):\n{}",
        lines.join("\n")
    )
}

pub fn build_declare_checks_messages(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    guidance: &str,
    settings: &Settings,
) -> Vec<serde_json::Value> {
    let ctx = MacroContext::from_game_detail_and_settings(detail, settings);
    let inputs = TurnPromptInputs {
        game,
        detail,
        turn,
        checks: &[],
        guidance,
        settings,
        ctx: &ctx,
    };
    let body = build_cumulative_turn_body(TurnPromptPhase::DeclareChecks, &inputs);
    let user = user_message_with_scenario(game, &body, &ctx);
    vec![
        json!({ "role": "system", "content": game_system_prompt(DECLARE_CHECKS_SYSTEM) }),
        json!({ "role": "user", "content": user }),
    ]
}

/// Messages for the single-pass inline-prose tool agent (structured engine mode):
/// the model narrates and calls scenario mechanic tools inline.
pub fn build_inline_prose_agent_messages(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    checks: &[GameTurnCheck],
    guidance: &str,
    settings: &Settings,
) -> Vec<serde_json::Value> {
    let ctx = MacroContext::from_game_detail_and_settings(detail, settings);
    let inputs = TurnPromptInputs {
        game,
        detail,
        turn,
        checks,
        guidance,
        settings,
        ctx: &ctx,
    };
    let body = build_cumulative_turn_body(TurnPromptPhase::ProseInline, &inputs);
    let user = user_message_with_scenario(game, &body, &ctx);
    vec![
        json!({
            "role": "system",
            "content": format!(
                "{}\n\n{STATE_CHANGE_PROMPT}",
                game_system_prompt(INLINE_PROSE_AGENT_SYSTEM)
            ),
        }),
        json!({ "role": "user", "content": user }),
    ]
}

pub fn build_scene_summarize_messages(
    detail: &GameDetail,
    settings: &Settings,
) -> Vec<serde_json::Value> {
    let ctx = MacroContext::from_game_detail_and_settings(detail, settings);
    let transcript: String = detail
        .turns
        .iter()
        .filter(|t| !t.prose.trim().is_empty())
        .map(|turn| format_prior_prose_chunk(turn, &ctx))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut body = format!("Turn transcript:\n{transcript}");
    append_characters_section(&mut body, &detail.actors);
    let user = user_message_with_scenario(&detail.game, &body, &ctx);
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

#[cfg(test)]
fn build_turn_context_tiers(
    turns: &[GameTurn],
    scenes: &[GameScene],
    before_id: i64,
    settings: &Settings,
    ctx: &MacroContext<'_>,
) -> TurnContextTiers {
    build_turn_context_tiers_with_budget(
        turns,
        scenes,
        before_id,
        turn_context_budget(settings),
        0,
        ctx,
    )
}

fn build_turn_context_tiers_with_budget(
    turns: &[GameTurn],
    scenes: &[GameScene],
    before_id: i64,
    budget: TurnContextBudget,
    min_recent_prose: usize,
    ctx: &MacroContext<'_>,
) -> TurnContextTiers {
    TurnContextTiers {
        long_term: long_term_memory_context(scenes),
        recent_beats: recent_beats_context(turns, before_id, budget.beats_chars),
        recent_prose: recent_prose_context_with_budget(
            turns,
            before_id,
            budget.prose_chars,
            min_recent_prose,
            ctx,
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

/// Remove inline `⟦mech:N⟧` / `⟦state:N⟧` / `⟦check:N⟧` markers from stored prose so they never
/// leak into history context or summaries (the surrounding narration already conveys
/// the outcome). Collapses the blank lines the markers were padded with.
fn strip_prose_inline_markers(prose: &str) -> String {
    let marker_opens = [
        PROSE_MECH_MARKER_OPEN,
        PROSE_STATE_MARKER_OPEN,
        PROSE_CHECK_MARKER_OPEN,
    ];
    let mut out = String::with_capacity(prose.len());
    let mut rest = prose;
    while let Some((open_idx, open_tag)) = marker_opens
        .iter()
        .filter_map(|tag| rest.find(tag).map(|idx| (idx, *tag)))
        .min_by_key(|(idx, _)| *idx)
    {
        out.push_str(&rest[..open_idx]);
        let after = &rest[open_idx + open_tag.len()..];
        match after.find(PROSE_INLINE_MARKER_CLOSE) {
            Some(close) => rest = &after[close + PROSE_INLINE_MARKER_CLOSE.len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
}

fn strip_prose_mech_markers(prose: &str) -> String {
    strip_prose_inline_markers(prose)
}

fn format_prior_prose_chunk(turn: &GameTurn, ctx: &MacroContext<'_>) -> String {
    let cleaned = strip_prose_mech_markers(turn.prose.trim());
    let prose = substitute_macros(cleaned.trim(), ctx);
    if turn.is_opening {
        format!("Opening:\n{prose}")
    } else {
        format!("Turn: {}\n{}", turn.player_action.trim(), prose)
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
    ctx: &MacroContext<'_>,
) -> String {
    let mut sections = Vec::new();
    let mut used = 0usize;
    for turn in turns
        .iter()
        .filter(|t| t.id < before_id && !t.prose.trim().is_empty())
        .rev()
    {
        let chunk = format_prior_prose_chunk(turn, ctx);
        if used + chunk.len() > budget && !sections.is_empty() {
            break;
        }
        used += chunk.len();
        sections.push(chunk);
    }

    let mut result = cap_turn_context_sections(sections, budget);
    if min_sections > 0 && prose_section_count(&result) < min_sections {
        if let Some(chunk) = most_recent_prior_prose(turns, before_id, ctx) {
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

fn most_recent_prior_prose(
    turns: &[GameTurn],
    before_id: i64,
    ctx: &MacroContext<'_>,
) -> Option<String> {
    turns
        .iter()
        .rfind(|t| t.id < before_id && !t.prose.trim().is_empty())
        .map(|turn| format_prior_prose_chunk(turn, ctx))
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

pub fn relevant_rules_blocks(game: &Game) -> Vec<&dreamwell_types::RulesBlock> {
    game.rules_blocks.iter().collect()
}

pub fn format_plan_and_system_rolls(turn: &GameTurn) -> String {
    let mut parts = Vec::new();
    if let Some(plan) = &turn.plan {
        if !plan.summary_beats.is_empty() {
            parts.push(format!("Turn plan:\n- {}", plan.summary_beats.join("\n- ")));
        }
        if let Some(summary) = &plan.npc_decision_summary {
            if !summary.is_empty() {
                parts.push(format!("NPC decisions: {summary}"));
            }
        }
        if let Some(card) = &plan.card_drawn {
            if !card.is_empty() {
                parts.push(format!("Card drawn: {card}"));
            }
        }
    }
    if !turn.system_rolls.is_empty() {
        let rolls = turn
            .system_rolls
            .iter()
            .map(|r| {
                format!(
                    "- {} ({}): {:?} = {}",
                    r.label,
                    r.dice_expr,
                    r.rolls,
                    r.rolls.iter().sum::<i64>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!("System rolls (canonical):\n{rolls}"));
    }
    parts.join("\n\n")
}

pub fn format_mechanical_results(turn: &GameTurn) -> String {
    if turn.mechanical_results.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = turn
        .mechanical_results
        .iter()
        .map(format_mechanical_result_line)
        .collect();
    format!("Mechanical results (canonical):\n{}", lines.join("\n"))
}

fn format_mechanical_result_line(result: &dreamwell_types::MechanicalResult) -> String {
    use dreamwell_types::{MechanicalData, MechanicalKind};
    match (&result.kind, &result.data) {
        (
            MechanicalKind::BoardMove,
            MechanicalData::BoardMove {
                actor,
                roll,
                from_space,
                to_space,
                space_tags,
                ..
            },
        ) => {
            format!(
                "- Board move: {actor} rolled {roll}, moved {from_space} → {to_space} (tags: {})",
                space_tags.join(", ")
            )
        }
        (
            MechanicalKind::CardDraw,
            MechanicalData::CardDraw {
                name,
                text,
                deck_id,
                ..
            },
        ) => {
            format!("- Card draw ({deck_id}): {name} — {text}")
        }
        (
            MechanicalKind::DiceRoll,
            MechanicalData::DiceRoll {
                dice_expr,
                rolls,
                total,
                ..
            },
        ) => {
            format!("- {} ({}): {:?} = {total}", result.label, dice_expr, rolls)
        }
        _ => format!("- {}", result.label),
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
            engine_mode: dreamwell_types::EngineMode::ToolsStructured,
            game_elements: dreamwell_types::GameElementsConfig::default(),
            element_instances: dreamwell_types::ElementInstances::default(),
            model_checks: String::new(),
            model_resolve: String::new(),
            model_prose: String::new(),
            rules_blocks: vec![],
            state_schema: vec![],
            win_condition: None,
            scenario_triggers: vec![],
            trait_defs: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
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
            guidance_notes: String::new(),
            phase: "done".into(),
            scene_beats: vec![],
            prose: "Steam curls from the kettle.".into(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
            state_changes: vec![],
            checks: vec![],
            system_rolls: vec![],
            plan: None,
            mechanical_results: vec![],
            observability: Default::default(),
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
            guidance_notes: String::new(),
            phase: "checks".into(),
            scene_beats: vec![],
            prose: String::new(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
            state_changes: vec![],
            checks: vec![],
            system_rolls: vec![],
            plan: None,
            mechanical_results: vec![],
            observability: Default::default(),
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
            active_connection_id: None,
            connections: Vec::new(),
            model: String::new(),
            temperature: 0.7,
            top_p: 1.0,
            max_tokens: 1024,
            system_prompt_prefix: String::new(),
            system_prompt_suffix: String::new(),
            user_name: "Alex".into(),
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

    fn test_macro_ctx<'a>(detail: &'a GameDetail, settings: &'a Settings) -> MacroContext<'a> {
        MacroContext::from_game_detail_and_settings(detail, settings)
    }

    #[test]
    fn declare_checks_schema_is_object_with_checks() {
        let schema = declare_checks_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["checks"].is_object());
    }

    #[test]
    fn inline_prose_system_describes_generic_primitives() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let turn = sample_turn();
        let settings = test_settings();
        let msgs = build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings);
        let system = msgs[0]["content"].as_str().unwrap();
        assert!(system.contains(r#"second person ("you")"#));
        assert!(system.contains("generic primitives"));
        assert!(system.contains("deck_id"));
        assert!(system.contains("ask_pc_decision"));
        assert!(system.contains("Example rhythm"));
        assert!(system.contains("apply_state_changes"));
        assert!(system.contains("no apply_state_changes"));
        assert!(system.contains("One mechanic per cycle"));
        let user = msgs[1]["content"].as_str().unwrap();
        assert!(user.contains("Follow the scenario rules"));
        assert!(user.contains("never batch multiple mechanics"));
    }

    #[test]
    fn inline_prose_includes_prior_prose_for_continuity() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let turn = sample_turn();
        let settings = test_settings();
        let msgs = build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings);
        let user = msgs[1]["content"].as_str().unwrap();
        assert!(user.contains("Recent turns (prose"));
        assert!(user.contains("Steam curls"));

        let prior = GameTurn {
            prose: "You slide the cup across the counter.".into(),
            player_action: "I serve tea.".into(),
            ..sample_turn_with_id(3)
        };
        let mut detail2 = sample_detail(game.clone());
        detail2.turns = vec![sample_opening_turn(), prior];
        let turn2 = sample_turn_with_id(4);
        let msgs2 = build_inline_prose_agent_messages(&game, &detail2, &turn2, &[], "", &settings);
        let user2 = msgs2[1]["content"].as_str().unwrap();
        assert!(user2.contains("slide the cup"));
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
            &MacroContext {
                char_name: "Mira",
                user_name: "Alex",
                persona: "",
                description: "",
                personality: "",
                scenario: "",
                first_message: "",
                setup_vars: dreamwell_types::empty_setup_vars(),
            },
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
        let settings = test_settings();
        let detail = sample_detail(sample_game());
        let tiers = build_turn_context_tiers(
            &[sample_opening_turn(), prior],
            &scenes,
            current.id,
            &settings,
            &test_macro_ctx(&detail, &settings),
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
    fn inline_prose_prompt_includes_recent_beats_and_long_term_memory() {
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
        let messages =
            build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &test_settings());
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Long-term memory"));
        assert!(user.contains("one regular"));
        assert!(user.contains("bell chimes"));
        assert!(user.contains("regular steps inside"));
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
        let detail = sample_detail(sample_game());
        let tiers = build_turn_context_tiers(
            &turns,
            &[],
            current_id,
            &settings,
            &test_macro_ctx(&detail, &settings),
        );
        let beat_turn_count = tiers.recent_beats.matches("Turn:").count();
        let prose_turn_count = tiers.recent_prose.matches("Turn:").count();
        assert!(beat_turn_count > prose_turn_count);
    }

    #[test]
    fn scenario_context_expands_user_macros() {
        let mut game = sample_game();
        game.setting = "Welcome {{User}} to the shop.".into();
        game.opening_message = "Hello {{user}}, says {{char}}.".into();
        let mut detail = sample_detail(game.clone());
        detail.turns = vec![GameTurn {
            prose: "Hello {{user}}, says {{char}}.".into(),
            is_opening: true,
            ..sample_opening_turn()
        }];
        let settings = test_settings();
        let messages = build_declare_checks_messages(&game, &detail, &sample_turn(), "", &settings);
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Welcome Alex to the shop."));
        assert!(user.contains("Opening:"));
        assert!(user.contains("Hello Alex, says Mira."));
        assert!(!user.contains("{{User}}"));
        assert!(!user.contains("{{user}}"));
    }

    #[test]
    fn characters_block_lists_pc_before_npcs_with_traits() {
        let pc = GameActor {
            id: 1,
            game_id: 1,
            role: "pc".into(),
            name: "Mira".into(),
            description: "Shopkeeper".into(),
            skills: [("Flair".to_string(), 1)].into_iter().collect(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let npc = GameActor {
            id: 2,
            game_id: 1,
            role: "npc".into(),
            name: "Brennan".into(),
            description: "Night watchman".into(),
            skills: Default::default(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let block = build_characters_block(&[npc.clone(), pc.clone()]);
        let pc_pos = block.find("Mira (PC)").unwrap();
        let npc_pos = block.find("Brennan (NPC)").unwrap();
        assert!(pc_pos < npc_pos);
        assert!(block.contains("Traits: Flair (+1)"));
        assert!(block.contains("Night watchman"));
    }

    #[test]
    fn turn_prompts_include_characters_block() {
        let game = sample_game();
        let mut detail = sample_detail(game.clone());
        detail.actors.push(GameActor {
            id: 2,
            game_id: 1,
            role: "npc".into(),
            name: "Regular".into(),
            description: "A familiar face at the counter.".into(),
            skills: Default::default(),
            sort_order: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        let turn = sample_turn();
        let settings = test_settings();
        for messages in [
            build_declare_checks_messages(&game, &detail, &turn, "", &settings),
            build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings),
        ] {
            let user = messages[1]["content"].as_str().unwrap();
            assert!(user.contains("Characters:"));
            assert!(user.contains("Mira (PC)"));
            assert!(user.contains("Regular (NPC)"));
        }
    }

    #[test]
    fn scene_summarize_includes_characters_block() {
        let game = sample_game();
        let detail = sample_detail(game);
        let messages = build_scene_summarize_messages(&detail, &test_settings());
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Characters:"));
        assert!(user.contains("Mira (PC)"));
    }

    #[test]
    fn system_prompts_discourage_default_peril() {
        assert!(DECLARE_CHECKS_SYSTEM.contains("Do not invent danger"));
        assert!(INLINE_PROSE_AGENT_SYSTEM.contains("Honor any resolved check tiers"));
    }

    #[test]
    fn declare_checks_encourages_cozy_gameplay() {
        assert!(DECLARE_CHECKS_SYSTEM.contains("cozy, intimate"));
        assert!(DECLARE_CHECKS_SYSTEM.contains("social, emotional"));
        assert!(!DECLARE_CHECKS_SYSTEM.contains("Prefer no check for low-stakes"));
    }

    #[test]
    fn declare_checks_system_lists_exact_json_field_names() {
        assert!(DECLARE_CHECKS_SYSTEM.contains(r#""checks""#));
        assert!(DECLARE_CHECKS_SYSTEM.contains(r#""label""#));
        assert!(DECLARE_CHECKS_SYSTEM.contains(r#""skill""#));
        assert!(DECLARE_CHECKS_SYSTEM.contains(r#""modifier""#));
        assert!(DECLARE_CHECKS_SYSTEM.contains(r#""no_check_reason""#));
    }

    #[test]
    fn declare_checks_user_message_reminds_about_json_fields() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let turn = sample_turn();
        let messages = build_declare_checks_messages(&game, &detail, &turn, "", &test_settings());
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("exact field names"));
        assert!(user.contains("no_check_reason"));
    }

    #[test]
    fn format_mechanical_results_includes_card_text() {
        use dreamwell_types::{MechanicalData, MechanicalKind, MechanicalResult};
        let turn = GameTurn {
            mechanical_results: vec![MechanicalResult {
                kind: MechanicalKind::CardDraw,
                label: "draw".into(),
                data: MechanicalData::CardDraw {
                    deck_id: "transformation".into(),
                    card_id: "transformation:1".into(),
                    name: "Grow".into(),
                    text: "Choose a body part to enlarge.".into(),
                    consumed: true,
                },
                sort_order: 0,
            }],
            ..sample_turn()
        };
        let formatted = format_mechanical_results(&turn);
        assert!(formatted.contains("Grow"));
        assert!(formatted.contains("Choose a body part"));
    }

    #[test]
    fn format_game_elements_context_lists_boards_and_decks() {
        let mut game = sample_game();
        game.game_elements = dreamwell_types::GameElementsConfig {
            boards: vec![dreamwell_types::BoardDef {
                id: "main".into(),
                spaces: 80,
                move_dice: "1d6".into(),
                tag_rules: vec![dreamwell_types::BoardTagRule {
                    tag: "truth".into(),
                    spaces: vec![8, 14],
                }],
                default_tag: "event".into(),
            }],
            decks: vec![dreamwell_types::DeckDef {
                id: "events".into(),
                consume_on_draw: true,
                cards: vec![dreamwell_types::CardDef {
                    id: "events:1".into(),
                    name: "Boost".into(),
                    text: "Move forward.".into(),
                }],
            }],
        };
        game.element_instances
            .board_positions
            .insert("pc".into(), 12);
        let ctx = format_game_elements_context(&game);
        assert!(ctx.contains("Board \"main\""));
        assert!(ctx.contains("truth"));
        assert!(ctx.contains("Available decks: events"));
        assert!(ctx.contains("pc position"));
    }

    #[test]
    fn relevant_rules_blocks_includes_all_scenario_blocks() {
        let mut game = sample_game();
        game.rules_blocks = vec![
            dreamwell_types::RulesBlock {
                name: "Gameplay".into(),
                content: "One action per turn.".into(),
            },
            dreamwell_types::RulesBlock {
                name: "Writing Style".into(),
                content: "Be concise.".into(),
            },
        ];
        let blocks = relevant_rules_blocks(&game);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].name, "Gameplay");
        assert_eq!(blocks[1].name, "Writing Style");
    }

    #[test]
    fn turn_prompts_include_pc_agency_rules() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let turn = sample_turn();
        let settings = test_settings();
        for messages in [
            build_declare_checks_messages(&game, &detail, &turn, "", &settings),
            build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings),
        ] {
            let system = messages[0]["content"].as_str().unwrap();
            assert!(
                system.contains("PC agency"),
                "expected PC agency rules in system prompt"
            );
            assert!(system.contains("Do not invent new choices"));
        }
    }

    #[test]
    fn cumulative_context_includes_rules_in_turn_prompts() {
        let mut game = sample_game();
        game.rules_blocks = vec![dreamwell_types::RulesBlock {
            name: "Cards and probabilities".into(),
            content: "Card 1: Grow - Choose a player and a body part.".into(),
        }];
        let detail = sample_detail(game.clone());
        let settings = test_settings();
        let turn = sample_turn();

        for messages in [
            build_declare_checks_messages(&game, &detail, &turn, "", &settings),
            build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings),
        ] {
            let user = messages[1]["content"].as_str().unwrap();
            assert!(user.contains("Scenario rules:"));
            assert!(user.contains("Card 1: Grow - Choose a player"));
        }
    }

    #[test]
    fn declare_checks_includes_plan_when_present() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let settings = test_settings();
        let mut turn = sample_turn();
        turn.plan = Some(dreamwell_types::TurnPlan {
            card_drawn: Some("Card 2: Shrink".into()),
            summary_beats: vec!["NPC rolls and draws.".into()],
            ..Default::default()
        });

        let messages = build_declare_checks_messages(&game, &detail, &turn, "", &settings);
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("Card drawn: Card 2: Shrink"));
    }

    #[test]
    fn cumulative_body_orders_stable_prefix_before_volatile_content() {
        let mut game = sample_game();
        game.rules_blocks = vec![dreamwell_types::RulesBlock {
            name: "Gameplay".into(),
            content: "One action per turn.".into(),
        }];
        let detail = sample_detail(game.clone());
        let settings = test_settings();
        let turn = sample_turn();

        let messages = build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings);
        let user = messages[1]["content"].as_str().unwrap();

        let rules_pos = user.find("Scenario rules:").unwrap();
        let characters_pos = user.find("Characters:").unwrap();
        let state_pos = user.find("Current state:").unwrap();
        let action_pos = user.find("Player action:").unwrap();

        // Stable scenario rules + characters lead; volatile per-turn content trails.
        assert!(rules_pos < characters_pos);
        assert!(characters_pos < state_pos);
        assert!(state_pos < action_pos);
    }

    #[test]
    fn rules_blocks_truncate_at_block_boundary_when_over_budget() {
        let mut game = sample_game();
        let big = "x".repeat(40_000);
        game.rules_blocks = vec![
            dreamwell_types::RulesBlock {
                name: "First".into(),
                content: big.clone(),
            },
            dreamwell_types::RulesBlock {
                name: "Second".into(),
                content: "Should be dropped.".into(),
            },
        ];
        let mut settings = test_settings();
        settings.context_tokens = 4000; // budget ~= max(16k, 32k) = 32_768 chars

        let rules = format_rules_blocks(&game, &settings);
        assert!(rules.contains("## First"));
        assert!(!rules.contains("Should be dropped."));
        assert!(rules.contains("additional scenario rules omitted"));
    }

    #[test]
    fn rules_blocks_unbounded_when_no_context_budget() {
        let mut game = sample_game();
        game.rules_blocks = vec![
            dreamwell_types::RulesBlock {
                name: "First".into(),
                content: "x".repeat(40_000),
            },
            dreamwell_types::RulesBlock {
                name: "Second".into(),
                content: "Kept.".into(),
            },
        ];
        let settings = test_settings(); // context_tokens = 0 → unbounded

        let rules = format_rules_blocks(&game, &settings);
        assert!(rules.contains("## Second"));
        assert!(rules.contains("Kept."));
        assert!(!rules.contains("additional scenario rules omitted"));
    }

    #[test]
    fn effective_turn_guidance_prefers_turn_over_job() {
        let mut turn = sample_turn();
        turn.guidance_notes = "Keep the scene cozy.".into();
        assert_eq!(
            super::effective_turn_guidance(&turn, "ignored job note"),
            "Keep the scene cozy."
        );
    }

    #[test]
    fn guidance_only_turn_omits_empty_player_action_line() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let settings = test_settings();
        let mut turn = sample_turn();
        turn.player_action.clear();
        turn.guidance_notes = "Skip the card draw and stay at the counter.".into();

        let messages = build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings);
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("honor this fully"));
        assert!(user.contains("Skip the card draw and stay at the counter."));
        assert!(!user.contains("Player action:"));
        assert!(user.contains("mandatory human direction"));
    }

    #[test]
    fn guidance_with_action_includes_both_sections() {
        let game = sample_game();
        let detail = sample_detail(game.clone());
        let settings = test_settings();
        let mut turn = sample_turn();
        turn.guidance_notes = "Keep the tone gentle.".into();

        let messages = build_inline_prose_agent_messages(&game, &detail, &turn, &[], "", &settings);
        let user = messages[1]["content"].as_str().unwrap();
        assert!(user.contains("mandatory — must shape this turn alongside the player action"));
        assert!(user.contains("Keep the tone gentle."));
        assert!(user.contains("Player action: I greet the regular at the counter."));
    }
}
