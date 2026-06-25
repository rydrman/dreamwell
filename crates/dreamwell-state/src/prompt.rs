macro_rules! state_target_rules_text {
    () => {
        r#"State target rules:
- "pc" (or "user"): the player character ONLY — their clothing, mood, inventory, injuries, held items
- "world": scene facts not owned by one person — location, weather, time, room layout, institution/quest stage
- Named character (e.g. "Maya", "Ryan"): NPCs — use the character's name as target; unknown names auto-create an NPC actor
- Do NOT store one character's attributes on world
- Do NOT pack multiple people or attributes into one key/value

BAD targeting / packing:
- target=world, key=clothing_state, value="Ryan's shirt is green"
- target=world, key=characters, value="Ryan is angry and Maya has a key"
- target=pc, key=scene_notes, value="Ryan talks to the bartender about the mine"

GOOD (actor-scoped, atomic):
- target=Ryan, key=shirt_color, value=green
- target=pc, key=mood, value=irritated
- target=world, key=location, value=tavern common room
- target=Maya, key=has_cellar_key, value=true"#
    };
}

pub const STATE_TARGET_RULES: &str = state_target_rules_text!();

macro_rules! state_kind_rules_text {
    () => {
        r#"State kind rules (one slot per target+key — pick the right kind once and keep it):
- fact (DEFAULT): durable text attribute — location, mood, inventory, traits, relationships, quest stage, appearance. Use set/remove with value strings. When unsure, use fact.
- condition: ONLY ephemeral status tags expected to clear soon (hidden, bleeding, inspired) — not for durable mood, location, or inventory.
- resource: ONLY when the key already appears as (resource): N/M in current state OR the schema defines it as resource. Numeric pool with max (stress 2/5, hit points). Never use resource for text attributes.
- clock: ONLY when the key already appears as (clock): N/M OR the schema defines it as clock. Stepped progress toward an outcome — not a substitute for facts.

BAD kind picks:
- resource/clock for mood, location, shirt_color, or any text attribute
- adjust_resource on a key not already tracked as (resource)

GOOD:
- fact: mood=excited, location=tavern, shirt_color=green
- resource: stress +1 when stress is already (resource): 2/5"#
    };
}

pub const STATE_KIND_RULES: &str = state_kind_rules_text!();

pub const STATE_CHANGE_RULES: &str = concat!(
    r#"State change rules:
- kind: resource|condition|fact|clock; op: set|add|remove
- One atomic scalar per (target, key) — keys are unique per target regardless of kind
- Use short snake_case keys for the attribute (shirt_color, location, mood) — not composite blobs (clothing_state, character_info)
- value is only the attribute's current value ("green", "tavern") — not "Ryan's shirt is green"
- Resource/clock use numeric delta or set values; conditions/facts use value strings
- Resource and clock values clamp to 0..max

"#,
    state_kind_rules_text!()
);

pub const STATE_CHANGE_PROMPT: &str = concat!(
    r#"State change rules:
- target: "pc" for the player character, "world" for global scope, or a named NPC
- kind: resource|condition|fact|clock; op: set|add|remove
- One atomic scalar per (target, key) — keys are unique per target regardless of kind
- Use short snake_case keys for the attribute (shirt_color, location, mood) — not composite blobs (clothing_state, character_info)
- value is only the attribute's current value ("green", "tavern") — not "Ryan's shirt is green"
- Prefer actor targets over world for anything about a specific person
- Resource/clock use numeric delta or set values; conditions/facts use value strings
- Resource and clock values clamp to 0..max

"#,
    state_kind_rules_text!(),
    "\n\n",
    state_target_rules_text!()
);

pub const PLAN_BEAT_RULES: &str = r#"Plan beat rules:
- Each beat is one concrete thing THIS output must cover — not a reusable template for any turn
- Ground every beat in the latest user turn and recent context: names, topics, questions, actions, and details they raised
- Use specific nouns and verbs from the conversation; prefer staging bullets the prose can follow in order
- BAD (too generic): "Respond to the user", "Stay in character", "Answer the question", "Continue the conversation", "React appropriately"
- GOOD (specific): "Answer whether the cellar door is still locked and mention the key on the windowsill", "Have the character agree to meet at the bridge at dusk", "Describe the smell of rain on the coat they asked about"
- One primary beat per turn — resolve the player's immediate action, not a montage of follow-on events
- Typically 1–3 beats for a normal reply; one beat when the user message is simple
- Do not plan future turns, unprompted plot twists, or beats that ignore what the user just said
- State changes should capture durable facts the beats establish — prefer set_fact and named actor targets; use resource/clock only for keys already numeric in state"#;

pub const RECHECK_SYSTEM_PROMPT: &str = r#"You review prose against typed session state.

Given the prose and current state, output ONLY a JSON object with state_changes that correct, add, or remove state entries that should persist.

Rules:
- Fix values that contradict the prose
- Add state for facts in prose but missing from state
- Do not repeat changes for values already correct
- Return {"state_changes": []} if no corrections are needed
- Output ONLY valid JSON matching the schema

When adding or correcting entries:
- Default to kind=fact for durable text attributes; use resource/clock only when the key is already numeric in state
- Prefer actor targets ("pc" or a named NPC) for facts about a specific person — not world
- One atomic attribute per key; split packed world entries onto the right actor
- Use short snake_case keys; values hold only the attribute, not full sentences"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_change_prompt_discourages_world_character_blobs() {
        assert!(STATE_CHANGE_PROMPT.contains("clothing_state"));
        assert!(STATE_CHANGE_PROMPT.contains("shirt_color"));
        assert!(STATE_CHANGE_PROMPT.contains("Prefer actor targets"));
        assert!(STATE_CHANGE_PROMPT.contains("fact (DEFAULT)"));
        assert!(STATE_CHANGE_PROMPT.contains("Never use resource for text"));
    }

    #[test]
    fn recheck_prompt_mentions_actor_targets() {
        assert!(RECHECK_SYSTEM_PROMPT.contains("named NPC"));
    }
}
