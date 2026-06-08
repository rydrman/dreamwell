use crate::DEFAULT_USER_NAME;

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
            },
            None => Self {
                char_name: "Character",
                user_name,
                persona,
                description: "",
                personality: "",
                scenario: "",
                first_message: "",
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
    None
}

/// Replace SillyTavern-style `{{macro}}` placeholders (case-insensitive names).
/// Unknown macros are left unchanged. Nested macros are resolved in multiple passes.
pub fn substitute_macros(text: &str, ctx: &MacroContext<'_>) -> String {
    let mut current = text.to_string();
    for _ in 0..8 {
        let next = substitute_macros_once(&current, ctx);
        if next == current {
            break;
        }
        current = next;
    }
    current
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
}
