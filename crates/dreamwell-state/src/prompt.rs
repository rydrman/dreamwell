pub const STATE_CHANGE_RULES: &str = r#"State change rules:
- target: "pc" for the player character, "world" for global scope, or a named actor
- kind: resource|condition|fact|clock; op: set|add|remove
- Resource/clock use numeric delta or set values; conditions/facts use value strings
- One scalar value per (target, kind, key) — no multi-item lists in a single entry
- Resource and clock values clamp to 0..max"#;

pub const PLAN_BEAT_RULES: &str = r#"Plan beat rules:
- Each beat is one concrete thing THIS output must cover — not a reusable template for any turn
- Ground every beat in the latest user turn and recent context: names, topics, questions, actions, and details they raised
- Use specific nouns and verbs from the conversation; prefer staging bullets the prose can follow in order
- BAD (too generic): "Respond to the user", "Stay in character", "Answer the question", "Continue the conversation", "React appropriately"
- GOOD (specific): "Answer whether the cellar door is still locked and mention the key on the windowsill", "Have the character agree to meet at the bridge at dusk", "Describe the smell of rain on the coat they asked about"
- Typically 3–6 beats for a normal reply; fewer when the user message is simple
- Do not plan future turns, unprompted plot twists, or beats that ignore what the user just said
- State changes should capture durable facts/resources the beats establish — not restate beat wording"#;

pub const RECHECK_SYSTEM_PROMPT: &str = r#"You review prose against typed session state.

Given the prose and current state, output ONLY a JSON object with state_changes that correct, add, or remove state entries that should persist.

Rules:
- target: "pc" for the player character, "world" for global scope
- kind: resource|condition|fact|clock; op: set|add|remove
- Resource/clock deltas are numeric; conditions/facts use value strings
- Fix values that contradict the prose
- Add state for facts in prose but missing from state
- Do not repeat changes for values already correct
- Return {"state_changes": []} if no corrections are needed
- Output ONLY valid JSON matching the schema"#;
