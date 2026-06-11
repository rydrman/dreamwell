use regex::Regex;
use std::sync::OnceLock;

const TAG: &str = r"(?:var|fact|variable)";
const IDENT: &str = r#"(?:key|name)\s*=\s*["']?([^"'>\s]+)["']?"#;

/// Strips variable markup from message text for display.
pub fn strip_variables_for_display(text: &str, streaming: bool) -> String {
    let mut working = text.to_string();
    working = strip_delete_tags(&working);
    working = strip_set_value_tags(&working);
    working = strip_set_tags(&working);
    working = strip_orphan_closing_tags(&working);
    working = strip_incomplete_variable_tags(&working, streaming);
    collapse_spaces(working.trim())
}

fn delete_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Regex::new(&format!(
                r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*\bdelete\b(?:\s*=\s*["']?(?:true|1)["']?)?[^>]*/>"#
            ))
            .expect("delete self-closing regex"),
            Regex::new(&format!(
                r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*\bdelete\b[^>]*>\s*</{TAG}\s*>"#
            ))
            .expect("delete empty element regex"),
        ]
    })
}

fn set_value_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*\bvalue\s*=\s*["']?([^"'>\s]*)["']?[^>]*/>"#
        ))
        .expect("set value self-closing regex")
    })
}

fn set_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*>(.*?)</{TAG}\s*>"#
        ))
        .expect("set regex")
    })
}

fn strip_delete_tags(text: &str) -> String {
    let mut working = text.to_string();
    for re in delete_patterns() {
        working = re.replace_all(&working, "").into_owned();
    }
    working
}

fn strip_set_value_tags(text: &str) -> String {
    set_value_pattern().replace_all(text, "").into_owned()
}

fn strip_set_tags(text: &str) -> String {
    set_pattern().replace_all(text, "").into_owned()
}

fn strip_orphan_closing_tags(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(&format!(r"(?is)</{TAG}\s*>")).expect("orphan close regex"));
    re.replace_all(text, "").into_owned()
}

fn strip_incomplete_variable_tags(text: &str, hold_incomplete: bool) -> String {
    let (mut visible, has_unclosed) = split_unclosed_variable_tag(text);
    if has_unclosed && !hold_incomplete {
        return visible;
    }

    if hold_incomplete {
        let holdback = trailing_partial_var_prefix(&visible);
        let visible_len = visible.len().saturating_sub(holdback);
        visible = visible[..visible_len].trim_end().to_string();
    }
    visible
}

fn split_unclosed_variable_tag(text: &str) -> (String, bool) {
    let lower = text.to_lowercase();
    let mut last_unclosed: Option<usize> = None;

    for open in ["<variable", "<fact", "<var"] {
        if let Some(pos) = lower.rfind(open) {
            if !variable_tag_is_complete(&lower[pos..])
                && last_unclosed.is_none_or(|existing| pos > existing)
            {
                last_unclosed = Some(pos);
            }
        }
    }

    match last_unclosed {
        Some(pos) => (text[..pos].trim_end().to_string(), true),
        None => (text.to_string(), false),
    }
}

fn variable_tag_is_complete(lower: &str) -> bool {
    if let Some(slash_end) = lower.find("/>") {
        if !lower[..slash_end].contains('>') {
            return true;
        }
    }

    lower.contains("</var>") || lower.contains("</fact>") || lower.contains("</variable>")
}

fn trailing_partial_var_prefix(text: &str) -> usize {
    const PREFIXES: &[&str] = &[
        "</variable>",
        "</fact>",
        "</var>",
        "<variable",
        "<fact",
        "<var",
        "</",
        "<",
    ];
    let mut max_len = 0;
    for prefix in PREFIXES {
        for i in 1..prefix.len() {
            if text.ends_with(&prefix[..i]) {
                max_len = max_len.max(i);
            }
        }
    }
    max_len
}

fn collapse_spaces(text: &str) -> String {
    static SPACES: OnceLock<Regex> = OnceLock::new();
    let spaces = SPACES.get_or_init(|| Regex::new(r" {2,}").expect("space collapse regex"));
    spaces.replace_all(text, " ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_name_attribute_tags() {
        assert_eq!(
            strip_variables_for_display(r#"*smiles* <var name="hp">80</var>"#, false),
            "*smiles*"
        );
    }

    #[test]
    fn strips_variable_element_name() {
        assert_eq!(
            strip_variables_for_display(r#"<variable name="location">tavern</variable>"#, false),
            ""
        );
    }

    #[test]
    fn strips_key_attribute_after_other_attributes() {
        assert_eq!(
            strip_variables_for_display(r#"<var id="1" name="location">tavern</var>"#, false),
            ""
        );
    }

    #[test]
    fn strips_unquoted_name_attribute() {
        assert_eq!(
            strip_variables_for_display(r#"<var name=location>tavern</var>"#, false),
            ""
        );
    }

    #[test]
    fn strips_spaced_equals_in_name_attribute() {
        assert_eq!(
            strip_variables_for_display(r#"<var name = "location">tavern</var>"#, false),
            ""
        );
    }

    #[test]
    fn strips_var_tags_from_display_text() {
        assert_eq!(
            strip_variables_for_display(r#"Hello <var key="location">tavern</var> world"#, false),
            "Hello world"
        );
    }

    #[test]
    fn strips_orphan_closing_tags() {
        assert_eq!(
            strip_variables_for_display(
                r#"*narrates* <var key="hp">80</var>
</var>"#,
                false
            ),
            "*narrates*"
        );
    }

    #[test]
    fn strips_duplicate_closing_tags() {
        assert_eq!(
            strip_variables_for_display(r#"<var key="location">tavern</var></var>"#, false),
            ""
        );
    }

    #[test]
    fn strips_self_closing_value_tags() {
        assert_eq!(
            strip_variables_for_display(r#"Hi <var key="hp" value="50"/> there"#, false),
            "Hi there"
        );
    }

    #[test]
    fn strips_incomplete_tags_when_complete() {
        assert_eq!(
            strip_variables_for_display(r#"Visible only <var key="hp">50</var"#, false),
            "Visible only"
        );
    }

    #[test]
    fn holds_back_incomplete_tags_during_streaming() {
        assert_eq!(
            strip_variables_for_display(r#"Visible only <var key="hp">50</var"#, true),
            "Visible only"
        );
        assert_eq!(
            strip_variables_for_display(r#"Visible only <var key="hp">50"#, true),
            "Visible only"
        );
        assert_eq!(
            strip_variables_for_display(r#"Visible only <v"#, true),
            "Visible only"
        );
    }

    #[test]
    fn is_case_insensitive_for_tags() {
        assert_eq!(
            strip_variables_for_display(r#"<VAR key="x">y</VAR>"#, false),
            ""
        );
    }
}
