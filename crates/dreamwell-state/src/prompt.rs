macro_rules! state_target_rules_text {
    () => {
        r#"State target rules:
- "pc" (or "user"): the player character ONLY — their clothing, mood, inventory, injuries, held items
- "world": scene variables not owned by one person — location, weather, time, room layout, institution/quest stage
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
- Pick by the value's shape, not the topic:
  - Text or a label (excited, tavern, green, has_key, allied) → variable (or condition if it clears soon)
  - A number with a maximum (3/5 stress, 2/4 countdown) → resource or clock
- variable (DEFAULT): durable text attribute — location, mood, inventory, traits, relationships, quest stage, appearance, body measurements. Use set/remove with value strings. When unsure, use variable.
- condition: ephemeral status tags expected to clear soon (hidden, bleeding, inspired) — not durable mood, location, or inventory.
- resource: a numeric track with a ceiling — stress 2/5, hit points, supply. Use only when the value is a count with a max; never for a text attribute like mood, location, or measurements.
- clock: numeric progress in segments toward an outcome — countdown 2/4, investigation clock.
- Keep the kind stable: if a key already shows in current state as (resource)/(clock)/(variable)/(condition), keep using that same kind — do not flip a text variable into a resource or vice versa.

BAD kind picks:
- resource/clock for mood, location, shirt_color, body measurements, or any text attribute
- changing a key's kind away from what current state already shows

GOOD:
- variable: mood=excited, location=tavern, shirt_color=green, height=6'2"
- resource: stress +1 (a counted track with a max)"#
    };
}

pub const STATE_CHANGE_RULES: &str = concat!(
    r#"State change rules:
- kind: resource|condition|variable|clock; op: set|add|remove
- One atomic scalar per (target, key) — keys are unique per target regardless of kind
- Use short snake_case keys for the attribute (shirt_color, location, mood) — not composite blobs (clothing_state, character_info)
- value is only the attribute's current value ("green", "tavern") — not "Ryan's shirt is green"
- Resource/clock use numeric delta or set values; conditions/variables use value strings
- Resource and clock values clamp to 0..max

"#,
    state_kind_rules_text!()
);

pub const STATE_CHANGE_PROMPT: &str = concat!(
    r#"State change rules:
- target: "pc" for the player character, "world" for global scope, or a named NPC
- kind: resource|condition|variable|clock; op: set|add|remove
- One atomic scalar per (target, key) — keys are unique per target regardless of kind
- Use short snake_case keys for the attribute (shirt_color, location, mood) — not composite blobs (clothing_state, character_info)
- value is only the attribute's current value ("green", "tavern") — not "Ryan's shirt is green"
- Prefer actor targets over world for anything about a specific person
- Resource/clock use numeric delta or set values; conditions/variables use value strings
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
- Typically 3–6 beats for a normal reply; fewer when the user message is simple
- Do not plan future turns, unprompted plot twists, or beats that ignore what the user just said
- State changes should capture durable variables the beats establish — prefer set_variable and named actor targets; use resource/clock only for genuinely numeric tracks (a count with a max), one attribute per key"#;

pub const RECHECK_SYSTEM_PROMPT: &str = r#"You review prose against typed session state.

Given the prose and current state, output ONLY a JSON object with state_changes that correct, add, or remove state entries that should persist.

Rules:
- Fix values that contradict the prose
- Add state for variables in prose but missing from state
- Do not repeat changes for values already correct
- Return {"state_changes": []} if no corrections are needed
- Output ONLY valid JSON matching the schema

When adding or correcting entries:
- Default to kind=variable for durable text attributes; use resource/clock only for genuinely numeric tracks (a count with a max), and keep a key's existing kind stable
- Prefer actor targets ("pc" or a named NPC) for variables about a specific person — not world
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
        assert!(STATE_CHANGE_PROMPT.contains("variable (DEFAULT)"));
        assert!(STATE_CHANGE_PROMPT.contains("never for a text attribute"));
    }

    #[test]
    fn recheck_prompt_mentions_actor_targets() {
        assert!(RECHECK_SYSTEM_PROMPT.contains("named NPC"));
    }
}
