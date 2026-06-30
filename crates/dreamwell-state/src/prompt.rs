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
  - Text or a label (excited, tavern, green, has_key, allied, athletic) → variable (or condition if it clears soon)
  - A float number (height, stress level, arousal) → measurement
  - An ordered list with a cursor (turn order, quest steps, queue) → sequence
- variable (DEFAULT): durable text attribute — location, mood, inventory, traits, relationships, quest stage, appearance, body descriptions. Use set/remove with value strings. When unsure, use variable.
- condition: ephemeral status tags expected to clear soon (hidden, bleeding, inspired) — not durable mood, location, or inventory.
- measurement: a float value, unbounded by default. Optional unit label (cm, kg, stress, ft, in, lb — common imperial abbreviations are accepted). Values are decimal in that unit — height=182 unit=cm, height=71 unit=in, not feet+inches like 5.11 for 5′11″. Use set_measurement_min/max only when bounds matter; clear_measurement wipes value, bounds, and unit.
- sequence: ordered string items with an active cursor. Use set_sequence (non-empty items), step_sequence to advance, clear_sequence to remove.
- Keep the kind stable: if a key already shows in current state as (measurement)/(sequence)/(variable)/(condition), keep using that same kind.

BAD kind picks:
- measurement/sequence for mood, location, shirt_color, or prose descriptions (use variable)
- changing a key's kind away from what current state already shows

GOOD:
- variable: mood=excited, location=tavern, build=athletic, shirt_color=green
- measurement: height=182 unit=cm; stress=2.5; height=71 unit=in (five foot eleven in inches)
- sequence: items=[pc, Maya, guard] for turn order"#
    };
}

pub const STATE_CHANGE_RULES: &str = concat!(
    r#"State change rules:
- kind: measurement|condition|variable|sequence; op: set|add|remove|setmin|setmax|step
- One atomic scalar per (target, key) — keys are unique per target regardless of kind
- Use short snake_case keys for the attribute (shirt_color, location, mood) — not composite blobs (clothing_state, character_info)
- value is only the attribute's current value ("green", "tavern") — not "Ryan's shirt is green"
- Measurements use float values; variables/conditions use value strings

"#,
    state_kind_rules_text!()
);

pub const STATE_CHANGE_PROMPT: &str = concat!(
    r#"State change rules:
- target: "pc" for the player character, "world" for global scope, or a named NPC
- kind: measurement|condition|variable|sequence; op: set|add|remove|setmin|setmax|step
- One atomic scalar per (target, key) — keys are unique per target regardless of kind
- Use short snake_case keys for the attribute (shirt_color, location, mood) — not composite blobs (clothing_state, character_info)
- value is only the attribute's current value ("green", "tavern") — not "Ryan's shirt is green"
- Prefer actor targets over world for anything about a specific person
- Measurements use float values; variables/conditions use value strings

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
- GOOD (specific): "Answer whether the cellar door is still locked and mention the key on the windowsill", "Have the character agree to meet at the bridge at dusk", "Have Maya fidget with her cuff and deflect with a joke", "Describe the smell of rain on the coat they asked about"
- Prefer beats that stage concrete actions and spoken lines over beats that only call for describing a character's mood, traits, or appearance
- BAD (describing instead of acting): "Describe how nervous Maya looks", "Convey Ryan's anger through narration"
- GOOD (acting/speaking): "Have Maya fidget and change the subject", "Have Ryan refuse and walk toward the door"
- Typically 3–6 beats for a normal reply; fewer when the user message is simple
- Do not plan future turns, unprompted plot twists, or beats that ignore what the user just said
- State changes should capture durable variables the beats establish — prefer set_variable and named actor targets; use measurement/sequence only when genuinely numeric or ordered lists"#;

pub const CHARACTER_ACTION_RULES: &str = r#"Character presentation:
- Prefer action and spoken lines over exposition about moods, traits, or appearance
- Show characters doing and saying things; do not summarize them in adjective lists or narrate their inner state when action or dialogue would carry it
- BAD: "Maya was nervous and beautiful, clearly worried about the meeting."
- GOOD: "Maya picks at the napkin. 'I shouldn't have come,' she says.""#;

pub const RECHECK_SYSTEM_PROMPT: &str = r#"You review prose against typed session state.

Given the prose and current state, output ONLY a JSON object with state_changes that correct, add, or remove state entries that should persist.

Rules:
- Fix values that contradict the prose
- Add state for variables in prose but missing from state
- Do not repeat changes for values already correct
- Return {"state_changes": []} if no corrections are needed
- Output ONLY valid JSON matching the schema

When adding or correcting entries:
- Default to kind=variable for durable text attributes; use measurement/sequence only when appropriate, and keep a key's existing kind stable
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
        assert!(STATE_CHANGE_PROMPT.contains("measurement/sequence for mood"));
    }

    #[test]
    fn plan_beat_rules_discourage_character_description_beats() {
        assert!(PLAN_BEAT_RULES.contains("Describe how nervous Maya looks"));
        assert!(PLAN_BEAT_RULES.contains("Have Maya fidget"));
    }

    #[test]
    fn character_action_rules_prefer_showing_over_telling() {
        assert!(CHARACTER_ACTION_RULES.contains("action and spoken lines"));
        assert!(CHARACTER_ACTION_RULES.contains("Maya was nervous"));
        assert!(CHARACTER_ACTION_RULES.contains("Maya picks at the napkin"));
    }

    #[test]
    fn recheck_prompt_mentions_actor_targets() {
        assert!(RECHECK_SYSTEM_PROMPT.contains("named NPC"));
    }
}
