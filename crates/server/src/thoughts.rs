use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedThoughts {
    pub thought: String,
    pub reply: String,
    pub thought_complete: bool,
}

/// Strips model reasoning/thought blocks from generated text.
pub fn strip_thought_blocks(text: &str) -> String {
    parse_thought_blocks(text).reply
}

/// Splits reasoning blocks from the visible reply.
pub fn parse_thought_blocks(text: &str) -> ParsedThoughts {
    let (parts, remainder) = extract_complete_thoughts(text);
    let (partial, reply, has_unclosed) = extract_unclosed(&remainder);

    let mut thought_parts = parts;
    if has_unclosed && !partial.is_empty() {
        thought_parts.push(partial);
    }

    let thought = thought_parts.join(
        "

",
    );
    let reply_source = if has_unclosed { reply } else { remainder };
    let reply = collapse_spaces(reply_source.trim());

    let thought_complete = !has_unclosed;
    ParsedThoughts {
        thought,
        reply,
        thought_complete,
    }
}

fn extract_complete_thoughts(text: &str) -> (Vec<String>, String) {
    static PATTERNS: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?is)<think>(.*?)</think>").expect("think regex"),
            Regex::new(r"(?is)<thinking>(.*?)</thinking>").expect("thinking regex"),
            Regex::new(r"(?is)<thought>(.*?)</thought>").expect("thought regex"),
            Regex::new(r"(?is)<\|channel>thought\s*(.*?)<channel\|>").expect("gemma regex"),
        ]
    });

    let mut thoughts = Vec::new();
    let mut remainder = text.to_string();

    loop {
        let mut earliest: Option<(usize, usize, String)> = None;
        for re in patterns {
            if let Some(cap) = re.captures(&remainder) {
                let m = cap.get(0).expect("full match");
                let inner = cap
                    .get(1)
                    .map(|c| c.as_str().trim().to_string())
                    .unwrap_or_default();
                if earliest
                    .as_ref()
                    .is_none_or(|(start, _, _)| m.start() < *start)
                {
                    earliest = Some((m.start(), m.end(), inner));
                }
            }
        }

        let Some((start, end, inner)) = earliest else {
            break;
        };

        if !inner.is_empty() {
            thoughts.push(inner);
        }
        remainder.replace_range(start..end, "");
    }

    (thoughts, remainder)
}

fn extract_unclosed(text: &str) -> (String, String, bool) {
    let lower = text.to_lowercase();
    let mut open_pos: Option<(usize, usize)> = None;

    for (open, close) in [
        ("<think>", "</think>"),
        ("<thinking>", "</thinking>"),
        ("<thought>", "</thought>"),
        ("<|channel>thought", "<channel|>"),
    ] {
        if let Some(pos) = lower.rfind(open) {
            let after_open = &lower[pos..];
            if !after_open.contains(close) && (open_pos.is_none() || pos > open_pos.unwrap().0) {
                open_pos = Some((pos, open.len()));
            }
        }
    }

    let Some((pos, open_len)) = open_pos else {
        return (String::new(), text.to_string(), false);
    };

    (
        text[pos + open_len..].trim().to_string(),
        text[..pos].trim().to_string(),
        true,
    )
}

fn collapse_spaces(text: &str) -> String {
    static SPACES: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let spaces = SPACES.get_or_init(|| Regex::new(r" {2,}").expect("space collapse regex"));
    spaces.replace_all(text, " ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_think_tags() {
        let input = "Hello <think>hidden</think> world";
        assert_eq!(strip_thought_blocks(input), "Hello world");
    }

    #[test]
    fn extracts_think_and_reply() {
        let input = "Hello <think>hidden</think> world";
        let parsed = parse_thought_blocks(input);
        assert_eq!(parsed.thought, "hidden");
        assert_eq!(parsed.reply, "Hello world");
        assert!(parsed.thought_complete);
    }

    #[test]
    fn strips_thinking_tags() {
        let input = "<thinking>planning</thinking>

*smiles*";
        assert_eq!(strip_thought_blocks(input), "*smiles*");
    }

    #[test]
    fn strips_thought_tags() {
        let input = "Hi <thought>internal</thought> there";
        assert_eq!(strip_thought_blocks(input), "Hi there");
    }

    #[test]
    fn strips_multiple_blocks() {
        let input = "A <think>x</think> B <thinking>y</thinking> C";
        assert_eq!(strip_thought_blocks(input), "A B C");
    }

    #[test]
    fn extracts_multiple_thoughts() {
        let input = "A <think>x</think> B <thinking>y</thinking> C";
        let parsed = parse_thought_blocks(input);
        assert_eq!(
            parsed.thought,
            "x

y"
        );
        assert_eq!(parsed.reply, "A B C");
    }

    #[test]
    fn strips_unclosed_think_during_streaming() {
        let input = "Reply so far <think> still thinking";
        assert_eq!(strip_thought_blocks(input), "Reply so far");
    }

    #[test]
    fn extracts_unclosed_thought_streaming() {
        let input = "Reply so far <think> still thinking";
        let parsed = parse_thought_blocks(input);
        assert_eq!(parsed.thought, "still thinking");
        assert_eq!(parsed.reply, "Reply so far");
        assert!(!parsed.thought_complete);
    }

    #[test]
    fn strips_unclosed_thinking_during_streaming() {
        let input = "Partial <thinking>in progress";
        assert_eq!(strip_thought_blocks(input), "Partial");
    }

    #[test]
    fn strips_gemma_thought_channel() {
        let input = "<|channel>thought
planning here<channel|>*smiles*";
        assert_eq!(strip_thought_blocks(input), "*smiles*");
    }

    #[test]
    fn extracts_gemma_thought_channel() {
        let input = "<|channel>thought
planning here<channel|>*smiles*";
        let parsed = parse_thought_blocks(input);
        assert_eq!(parsed.thought, "planning here");
        assert_eq!(parsed.reply, "*smiles*");
        assert!(parsed.thought_complete);
    }

    #[test]
    fn strips_gemma_empty_thought_block() {
        let input = "<|channel>thought
<channel|>Hello there";
        assert_eq!(strip_thought_blocks(input), "Hello there");
    }

    #[test]
    fn strips_unclosed_gemma_during_streaming() {
        let input = "<|channel>thought
still reasoning";
        assert_eq!(strip_thought_blocks(input), "");
    }

    #[test]
    fn detects_opening_tag_only_as_in_progress() {
        for input in ["<|channel>thought\n", "<thinking>", "<|channel>thought"] {
            let parsed = parse_thought_blocks(input);
            assert_eq!(parsed.thought, "");
            assert_eq!(parsed.reply, "");
            assert!(!parsed.thought_complete);
        }
    }

    #[test]
    fn empty_complete_thought_block_has_no_thought_text() {
        for input in [
            "<thinking></thinking>Hello",
            "<|channel>thought\n<channel|>Hello",
            "<think>  </think>Hi",
        ] {
            let parsed = parse_thought_blocks(input);
            assert_eq!(parsed.thought, "", "input: {input}");
            assert!(parsed.thought_complete);
        }
    }

    #[test]
    fn leaves_text_without_blocks_unchanged() {
        let input = "Just a normal roleplay reply.";
        assert_eq!(strip_thought_blocks(input), input);
    }

    #[test]
    fn is_case_insensitive_for_tags() {
        let input = "<THINKING>notes</THINKING> Hi";
        assert_eq!(strip_thought_blocks(input), "Hi");
    }
}
