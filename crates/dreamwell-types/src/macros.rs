use crate::DEFAULT_USER_NAME;

pub fn empty_setup_vars() -> &'static std::collections::HashMap<String, String> {
    use std::sync::OnceLock;
    static EMPTY: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();
    EMPTY.get_or_init(std::collections::HashMap::new)
}

/// Values available for SillyTavern-style `{{macro}}` substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroContext<'a> {
    pub char_name: &'a str,
    pub user_name: &'a str,
    pub persona: &'a str,
    pub description: &'a str,
    pub personality: &'a str,
    pub scenario: &'a str,
    pub first_message: &'a str,
    pub setup_vars: &'a std::collections::HashMap<String, String>,
}

impl<'a> MacroContext<'a> {
    pub fn effective_user_name(&self) -> &str {
        let name = self.user_name.trim();
        if name.is_empty() {
            DEFAULT_USER_NAME
        } else {
            name
        }
    }

    pub fn from_game_detail(
        detail: &'a crate::GameDetail,
        user_name: &'a str,
        persona: &'a str,
    ) -> Self {
        let pc = detail.actors.iter().find(|actor| actor.role == "pc");
        let char_name = pc
            .map(|actor| actor.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                let title = detail.game.title.trim();
                if title.is_empty() {
                    "Character"
                } else {
                    title
                }
            });
        Self {
            char_name,
            user_name,
            persona,
            description: pc.map(|actor| actor.description.as_str()).unwrap_or(""),
            personality: "",
            scenario: detail.game.premise.as_str(),
            first_message: detail.game.opening_message.as_str(),
            setup_vars: empty_setup_vars(),
        }
    }

    pub fn from_game_detail_and_settings(
        detail: &'a crate::GameDetail,
        settings: &'a crate::Settings,
    ) -> Self {
        Self::from_game_detail(
            detail,
            settings.user_name.as_str(),
            settings.persona_description.as_str(),
        )
    }

    pub fn from_character_and_settings(
        character: Option<&'a crate::Character>,
        user_name: &'a str,
        persona: &'a str,
    ) -> Self {
        match character {
            Some(c) => Self {
                char_name: c.name.as_str(),
                user_name,
                persona,
                description: c.description.as_str(),
                personality: c.personality.as_str(),
                scenario: c.scenario.as_str(),
                first_message: c.first_message.as_str(),
                setup_vars: empty_setup_vars(),
            },
            None => Self {
                char_name: "Character",
                user_name,
                persona,
                description: "",
                personality: "",
                scenario: "",
                first_message: "",
                setup_vars: empty_setup_vars(),
            },
        }
    }
}

fn resolve_macro<'a>(key: &str, ctx: &'a MacroContext<'a>) -> Option<&'a str> {
    if key.eq_ignore_ascii_case("user") {
        return Some(ctx.effective_user_name());
    }
    if key.eq_ignore_ascii_case("char") {
        return Some(ctx.char_name);
    }
    if key.eq_ignore_ascii_case("persona") {
        return Some(ctx.persona);
    }
    if key.eq_ignore_ascii_case("description") {
        return Some(ctx.description);
    }
    if key.eq_ignore_ascii_case("personality") {
        return Some(ctx.personality);
    }
    if key.eq_ignore_ascii_case("scenario") {
        return Some(ctx.scenario);
    }
    if key.eq_ignore_ascii_case("charfirstmessage") {
        return Some(ctx.first_message);
    }
    if let Some(value) = ctx.setup_vars.get(key) {
        return Some(value.as_str());
    }
    for (var_key, value) in ctx.setup_vars {
        if var_key.eq_ignore_ascii_case(key) {
            return Some(value.as_str());
        }
    }
    None
}

/// Replace SillyTavern-style `{{macro}}` placeholders (case-insensitive names).
/// Unknown macros are left unchanged. Nested macros are resolved in multiple passes.
pub fn substitute_macros(text: &str, ctx: &MacroContext<'_>) -> String {
    let mut current = substitute_angle_macros(text, ctx);
    for _ in 0..8 {
        let next = substitute_macros_once(&current, ctx);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn substitute_angle_macros(text: &str, ctx: &MacroContext<'_>) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<<") {
        result.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        if let Some(end) = rest.find(">>") {
            let key = rest[..end].trim();
            if let Some(value) = resolve_macro(key, ctx) {
                result.push_str(value);
            } else {
                result.push_str("<<");
                result.push_str(key);
                result.push_str(">>");
            }
            rest = &rest[end + 2..];
        } else {
            result.push_str("<<");
            break;
        }
    }
    result.push_str(rest);
    result
}

fn substitute_macros_once(text: &str, ctx: &MacroContext<'_>) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        if let Some(end) = rest.find("}}") {
            let key = rest[..end].trim();
            if let Some(value) = resolve_macro(key, ctx) {
                result.push_str(value);
            } else {
                result.push_str("{{");
                result.push_str(key);
                result.push_str("}}");
            }
            rest = &rest[end + 2..];
        } else {
            result.push_str("{{");
            break;
        }
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MacroContext<'static> {
        MacroContext {
            char_name: "Seraphina",
            user_name: "Alex",
            persona: "A curious traveler.",
            description: "A forest guardian.",
            personality: "Kind and wise.",
            scenario: "An enchanted forest.",
            first_message: "Welcome, {{user}}.",
            setup_vars: empty_setup_vars(),
        }
    }

    #[test]
    fn replaces_user_and_char_case_insensitively() {
        let out = substitute_macros("Hello {{char}}, this is {{USER}} and {{User}}.", &ctx());
        assert_eq!(out, "Hello Seraphina, this is Alex and Alex.");
    }

    #[test]
    fn replaces_field_macros() {
        let out = substitute_macros(
            "{{persona}} | {{description}} | {{charFirstMessage}}",
            &ctx(),
        );
        assert_eq!(
            out,
            "A curious traveler. | A forest guardian. | Welcome, Alex."
        );
    }

    #[test]
    fn leaves_unknown_macros() {
        let out = substitute_macros("{{unknown}} stays", &ctx());
        assert_eq!(out, "{{unknown}} stays");
    }

    #[test]
    fn empty_user_name_falls_back() {
        let ctx = MacroContext {
            user_name: "  ",
            ..ctx()
        };
        assert_eq!(substitute_macros("Hi {{user}}", &ctx), "Hi User");
    }

    #[test]
    fn from_game_detail_uses_pc_and_settings() {
        use crate::{Game, GameActor, GameDetail};
        let detail = GameDetail {
            game: Game {
                id: 1,
                title: "Tea Shop".into(),
                premise: "Serve {{user}} at the counter.".into(),
                setting: "Cozy.".into(),
                gm_style: "Gentle.".into(),
                opening_message: "Hello {{User}}, welcome to {{char}}.".into(),
                character_id: None,
                scenario_id: None,
                resolution_system: crate::ResolutionSystem::Pbta2d6,
                modifier_min: -2,
                modifier_max: 3,
                merge_resolve_scene: true,
                step_mode: false,
                model_checks: String::new(),
                model_resolve: String::new(),
                model_prose: String::new(),
                rules_blocks: vec![],
                state_schema: vec![],
                win_condition: None,
                scenario_triggers: vec![],
                trait_defs: vec![],
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                archived_at: None,
                active_job: None,
                queued_jobs: 0,
            },
            actors: vec![GameActor {
                id: 1,
                game_id: 1,
                role: "pc".into(),
                name: "Mira".into(),
                description: "Shopkeeper".into(),
                skills: Default::default(),
                sort_order: 0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
            state: vec![],
            turns: vec![],
            scenes: vec![],
        };
        let ctx = MacroContext::from_game_detail(&detail, "Alex", "A traveler.");
        assert_eq!(
            substitute_macros(&detail.game.opening_message, &ctx),
            "Hello Alex, welcome to Mira."
        );
        assert_eq!(
            substitute_macros(&detail.game.premise, &ctx),
            "Serve Alex at the counter."
        );
    }
}
