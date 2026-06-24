use dynamo_parsers::tool_calling::parsers::{
    detect_and_parse_tool_call_with_recovery, detect_tool_call_start, find_tool_call_end_position,
    get_available_tool_parsers,
};
use dynamo_parsers::tool_calling::response::ToolCallResponse;
use dynamo_parsers::tool_calling::tools::try_tool_call_parse_aggregate;
use dynamo_parsers::tool_calling::ToolDefinition;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::inference::ToolCall;

const HOLDBACK_CHARS: usize = 64;
const BARE_CALL_PREFIX: &str = "call:";

#[derive(Debug, Clone)]
pub enum JailEvent {
    Prose(String),
    ToolCall(ToolCall),
}

#[derive(Debug, PartialEq, Eq)]
enum JailState {
    Prose,
    Jail,
}

pub struct ToolStreamJail {
    parser: Option<&'static str>,
    buffer: String,
    state: JailState,
}

impl ToolStreamJail {
    pub fn new(parser: Option<&'static str>) -> Self {
        Self {
            parser,
            buffer: String::new(),
            state: JailState::Prose,
        }
    }

    pub async fn push(
        &mut self,
        token: &str,
        tools: Option<&[ToolDefinition]>,
    ) -> AppResult<Vec<JailEvent>> {
        if self.parser.is_none() {
            self.buffer.push_str(token);
            return Ok(vec![]);
        }

        match self.state {
            JailState::Prose => self.push_prose(token, tools).await,
            JailState::Jail => self.push_jail(token, tools).await,
        }
    }

    pub async fn finish(&mut self, tools: Option<&[ToolDefinition]>) -> AppResult<Vec<JailEvent>> {
        if self.parser.is_none() {
            let tail = std::mem::take(&mut self.buffer);
            if tail.is_empty() {
                return Ok(vec![]);
            }
            let (calls, prose) = extract_tool_calls_from_text(&tail, None, tools).await?;
            return Ok(jail_events_from_extract(calls, prose));
        }

        let mut events = Vec::new();
        match self.state {
            JailState::Prose => {
                if !self.buffer.is_empty() {
                    let tail = std::mem::take(&mut self.buffer);
                    let (calls, prose) =
                        extract_tool_calls_from_text(&tail, self.parser, tools).await?;
                    events.extend(jail_events_from_extract(calls, prose));
                }
            }
            JailState::Jail => {
                events.extend(self.finalize_jail(tools).await?);
            }
        }
        Ok(events)
    }

    async fn push_prose(
        &mut self,
        token: &str,
        tools: Option<&[ToolDefinition]>,
    ) -> AppResult<Vec<JailEvent>> {
        self.buffer.push_str(token);
        let mut events = Vec::new();

        if only_partial_start_marker(&self.buffer, self.parser) {
            return Ok(events);
        }

        if detect_tool_call_start(&self.buffer, self.parser).unwrap_or(false) {
            if let Some(start) = find_tool_call_start(&self.buffer, self.parser) {
                let tail = self.buffer.split_off(start);
                if !self.buffer.is_empty() {
                    events.push(JailEvent::Prose(std::mem::take(&mut self.buffer)));
                }
                self.buffer = tail;
                self.state = JailState::Jail;
                events.extend(self.try_complete_jail(tools).await?);
                return Ok(events);
            }
        }

        events.extend(self.emit_prose_holdback());
        Ok(events)
    }

    async fn push_jail(
        &mut self,
        token: &str,
        tools: Option<&[ToolDefinition]>,
    ) -> AppResult<Vec<JailEvent>> {
        self.buffer.push_str(token);
        self.try_complete_jail(tools).await
    }

    async fn try_complete_jail(
        &mut self,
        tools: Option<&[ToolDefinition]>,
    ) -> AppResult<Vec<JailEvent>> {
        let mut events = Vec::new();
        while let Some(end) = find_tool_call_end_position(&self.buffer, self.parser) {
            let section = self.buffer[..end].to_string();
            let remainder = self.buffer[end..].to_string();
            let (calls, normal_text) = try_tool_call_parse_aggregate(&section, self.parser, tools)
                .await
                .map_err(|err| AppError::inference(err.to_string()))?;
            let (calls, normal_text) = merge_with_fallback(calls, normal_text, &section, tools);
            events.extend(tool_calls_to_events_from_calls(calls));
            self.buffer = remainder;
            self.state = JailState::Prose;
            if let Some(text) = normal_text.filter(|s| !s.is_empty()) {
                events.push(JailEvent::Prose(text));
            }
            if matches!(self.state, JailState::Prose) && !self.buffer.is_empty() {
                if detect_tool_call_start(&self.buffer, self.parser).unwrap_or(false) {
                    if let Some(start) = find_tool_call_start(&self.buffer, self.parser) {
                        let tail = self.buffer.split_off(start);
                        if !self.buffer.is_empty() {
                            events.push(JailEvent::Prose(std::mem::take(&mut self.buffer)));
                        }
                        self.buffer = tail;
                        self.state = JailState::Jail;
                        continue;
                    }
                }
                events.extend(self.emit_prose_holdback());
            }
            break;
        }
        Ok(events)
    }

    async fn finalize_jail(
        &mut self,
        tools: Option<&[ToolDefinition]>,
    ) -> AppResult<Vec<JailEvent>> {
        let section = std::mem::take(&mut self.buffer);
        self.state = JailState::Prose;
        if section.is_empty() {
            return Ok(vec![]);
        }
        let (calls, normal_text) =
            detect_and_parse_tool_call_with_recovery(&section, self.parser, tools)
                .await
                .map_err(|err| AppError::inference(err.to_string()))?;
        let (calls, normal_text) = merge_with_fallback(calls, normal_text, &section, tools);
        let mut events = tool_calls_to_events_from_calls(calls);
        if let Some(text) = normal_text.filter(|s| !s.is_empty()) {
            events.push(JailEvent::Prose(text));
        } else if events.is_empty() {
            events.push(JailEvent::Prose(section));
        }
        Ok(events)
    }

    fn emit_prose_holdback(&mut self) -> Vec<JailEvent> {
        if self.buffer.len() <= HOLDBACK_CHARS {
            return vec![];
        }
        let mut split = self.buffer.len() - HOLDBACK_CHARS;
        while split > 0 && !self.buffer.is_char_boundary(split) {
            split -= 1;
        }
        if split == 0 {
            return vec![];
        }
        let emit = self.buffer[..split].to_string();
        self.buffer = self.buffer[split..].to_string();
        if emit.is_empty() {
            vec![]
        } else {
            vec![JailEvent::Prose(emit)]
        }
    }
}

fn tool_calls_to_events_from_calls(calls: Vec<ToolCall>) -> Vec<JailEvent> {
    calls.into_iter().map(JailEvent::ToolCall).collect()
}

fn jail_events_from_extract(calls: Vec<ToolCall>, prose: Option<String>) -> Vec<JailEvent> {
    let mut events: Vec<JailEvent> = calls.into_iter().map(JailEvent::ToolCall).collect();
    if let Some(text) = prose.filter(|s| !s.is_empty()) {
        events.push(JailEvent::Prose(text));
    }
    events
}

async fn extract_tool_calls_from_text(
    text: &str,
    parser: Option<&'static str>,
    tools: Option<&[ToolDefinition]>,
) -> AppResult<(Vec<ToolCall>, Option<String>)> {
    let (dynamo_calls, normal_text) = if let Some(parser) = parser {
        detect_and_parse_tool_call_with_recovery(text, Some(parser), tools)
            .await
            .map_err(|err| AppError::inference(err.to_string()))?
    } else {
        let known = known_tool_names(tools);
        let (calls, prose) = fallback_extract_bare_calls(text, known.as_deref());
        return Ok((calls, if prose.is_empty() { None } else { Some(prose) }));
    };
    let (calls, prose) = merge_with_fallback(dynamo_calls, normal_text, text, tools);
    Ok((calls, prose))
}

fn merge_with_fallback(
    dynamo_calls: Vec<ToolCallResponse>,
    normal_text: Option<String>,
    section: &str,
    tools: Option<&[ToolDefinition]>,
) -> (Vec<ToolCall>, Option<String>) {
    if !dynamo_calls.is_empty() {
        return (
            dynamo_calls
                .into_iter()
                .map(|call| ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: call.function.arguments,
                })
                .collect(),
            normal_text,
        );
    }
    let known = known_tool_names(tools);
    let (fallback_calls, prose) = fallback_extract_bare_calls(section, known.as_deref());
    if fallback_calls.is_empty() {
        return (Vec::new(), normal_text);
    }
    (
        fallback_calls,
        if prose.is_empty() {
            normal_text
        } else {
            Some(prose)
        },
    )
}

fn known_tool_names(tools: Option<&[ToolDefinition]>) -> Option<Vec<&str>> {
    let tools = tools?;
    if tools.is_empty() {
        return None;
    }
    let mut names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    Some(names)
}

/// Parse gemma-style `call:name{key:value,...}` and, when tool defs are provided,
/// bare `name{key:value,...}` for known tool names.
fn fallback_extract_bare_calls(
    text: &str,
    known_tools: Option<&[&str]>,
) -> (Vec<ToolCall>, String) {
    let known = known_tools.unwrap_or(&[]);
    let mut calls = Vec::new();
    let mut prose = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(start) = find_next_fallback_start(text, cursor, known) else {
            prose.push_str(&text[cursor..]);
            break;
        };
        prose.push_str(&text[cursor..start]);
        let Some((call, consumed)) = parse_fallback_call_at(&text[start..], calls.len(), known)
        else {
            prose.push_str(&text[start..]);
            break;
        };
        calls.push(call);
        cursor = start + consumed;
    }
    (calls, prose)
}

fn find_next_fallback_start(text: &str, cursor: usize, known_tools: &[&str]) -> Option<usize> {
    let slice = &text[cursor..];
    let mut best: Option<usize> = None;

    if let Some(rel) = slice.find(BARE_CALL_PREFIX) {
        let start = cursor + rel;
        if is_bare_call_boundary(text, start) {
            best = Some(start);
        }
    }

    for name in known_tools {
        let mut search_from = 0usize;
        while let Some(rel) = slice[search_from..].find(name) {
            let start = cursor + search_from + rel;
            if is_direct_tool_call_start(text, start, name) {
                best = Some(best.map_or(start, |current| current.min(start)));
                break;
            }
            search_from += rel + name.len();
        }
    }

    best
}

fn is_direct_tool_call_start(text: &str, idx: usize, name: &str) -> bool {
    if !is_bare_call_boundary(text, idx) || !text[idx..].starts_with(name) {
        return false;
    }
    let after_name = idx + name.len();
    if text
        .get(after_name..)
        .is_none_or(|rest| !rest.starts_with('{'))
    {
        return false;
    }
    // `call:ask_pc_decision{...}` is handled by the `call:` path above.
    if idx >= BARE_CALL_PREFIX.len() && &text[idx - BARE_CALL_PREFIX.len()..idx] == BARE_CALL_PREFIX
    {
        return false;
    }
    true
}

fn parse_fallback_call_at(
    s: &str,
    index: usize,
    known_tools: &[&str],
) -> Option<(ToolCall, usize)> {
    if s.starts_with(BARE_CALL_PREFIX) {
        return parse_bare_call_at(s, index);
    }
    for name in known_tools {
        if let Some(call) = parse_direct_tool_call_at(s, name, index) {
            return Some(call);
        }
    }
    None
}

fn parse_direct_tool_call_at(s: &str, name: &str, index: usize) -> Option<(ToolCall, usize)> {
    if !s.starts_with(name) {
        return None;
    }
    let rest = &s[name.len()..];
    let (inner, close_len) = balanced_brace_content(rest)?;
    let arguments = bare_gemma_kv_to_json(inner)?;
    Some((
        ToolCall {
            id: format!("call_fallback_{index}"),
            name: name.to_string(),
            arguments,
        },
        name.len() + close_len,
    ))
}

fn is_bare_call_boundary(text: &str, idx: usize) -> bool {
    if idx == 0 {
        return true;
    }
    // `call:` must not be a substring of a longer token (e.g. `recall:`).
    text[..idx]
        .chars()
        .last()
        .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
}

fn parse_bare_call_at(s: &str, index: usize) -> Option<(ToolCall, usize)> {
    let rest = s.strip_prefix(BARE_CALL_PREFIX)?;
    let brace_idx = rest.find('{')?;
    let name = rest[..brace_idx].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return None;
    }
    let (inner, close_len) = balanced_brace_content(&rest[brace_idx..])?;
    let arguments = bare_gemma_kv_to_json(inner)?;
    Some((
        ToolCall {
            id: format!("call_fallback_{index}"),
            name: name.to_string(),
            arguments,
        },
        BARE_CALL_PREFIX.len() + brace_idx + close_len,
    ))
}

fn balanced_brace_content(s: &str) -> Option<(&str, usize)> {
    if !s.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

fn bare_gemma_kv_to_json(inner: &str) -> Option<String> {
    gemma_object_to_json_value(inner).and_then(|v| serde_json::to_string(&v).ok())
}

fn gemma_object_to_json_value(inner: &str) -> Option<serde_json::Value> {
    let mut map = serde_json::Map::new();
    for (key, value) in split_gemma_kv_pairs(inner) {
        if key.is_empty() {
            return None;
        }
        map.insert(key.to_string(), gemma_value_to_json_value(value)?);
    }
    Some(serde_json::Value::Object(map))
}

fn gemma_value_to_json_value(value: &str) -> Option<serde_json::Value> {
    let value = value.trim();
    if value.is_empty() {
        return Some(serde_json::Value::String(String::new()));
    }
    if value.starts_with('[') {
        return gemma_array_to_json_value(value);
    }
    if value.starts_with('{') {
        let (inner, _) = balanced_brace_content(value)?;
        return gemma_object_to_json_value(inner);
    }
    if value.len() >= 2 {
        if let Some(unquoted) = value
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
        {
            return Some(serde_json::Value::String(unquoted.to_string()));
        }
        if let Some(unquoted) = value
            .strip_prefix('\'')
            .and_then(|inner| inner.strip_suffix('\''))
        {
            return Some(serde_json::Value::String(unquoted.to_string()));
        }
    }
    Some(serde_json::Value::String(value.to_string()))
}

fn gemma_array_to_json_value(value: &str) -> Option<serde_json::Value> {
    let (inner, _) = balanced_bracket_content(value)?;
    let inner = inner.trim();
    if inner.is_empty() {
        return Some(serde_json::Value::Array(vec![]));
    }
    let items = split_gemma_array_items(inner);
    let mut arr = Vec::with_capacity(items.len());
    for item in items {
        arr.push(gemma_value_to_json_value(item)?);
    }
    Some(serde_json::Value::Array(arr))
}

fn split_gemma_kv_pairs(s: &str) -> Vec<(&str, &str)> {
    let mut pairs = Vec::new();
    let mut i = 0usize;
    while i < s.len() {
        while i < s.len() && s.as_bytes()[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        let rest = &s[i..];
        let Some(colon_rel) = rest.find(':') else {
            break;
        };
        let key = rest[..colon_rel].trim();
        let value_start = i + colon_rel + 1;
        let value_end = gemma_value_end(s, value_start);
        let value = s[value_start..value_end].trim();
        pairs.push((key, value));
        i = value_end;
        if i < s.len() && s.as_bytes()[i] == b',' {
            i += 1;
        }
    }
    pairs
}

fn gemma_value_end(s: &str, start: usize) -> usize {
    let rest = &s[start..];
    let leading_ws = rest.len() - rest.trim_start().len();
    let value_start = start + leading_ws;
    let trimmed = &s[value_start..];
    if trimmed.starts_with('[') {
        return value_start
            + balanced_bracket_content(trimmed)
                .map(|(_, len)| len)
                .unwrap_or(trimmed.len());
    }
    if trimmed.starts_with('{') {
        return value_start
            + balanced_brace_content(trimmed)
                .map(|(_, len)| len)
                .unwrap_or(trimmed.len());
    }
    for (rel, _) in trimmed.char_indices() {
        if trimmed.as_bytes().get(rel) != Some(&b',') {
            continue;
        }
        let after = trimmed[rel + 1..].trim_start();
        if let Some(colon) = after.find(':') {
            let key = after[..colon].trim();
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            {
                return value_start + rel;
            }
        }
    }
    s.len()
}

fn split_gemma_array_items(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                items.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        items.push(tail);
    }
    items
}

fn balanced_bracket_content(s: &str) -> Option<(&str, usize)> {
    if !s.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract remaining inline tool-call segments from text (e.g. prose leaked during streaming).
pub fn salvage_bare_tool_calls(
    text: &str,
    tools: Option<&[ToolDefinition]>,
) -> (Vec<ToolCall>, String) {
    let known = known_tool_names(tools);
    fallback_extract_bare_calls(text, known.as_deref())
}

fn only_partial_start_marker(buffer: &str, parser: Option<&'static str>) -> bool {
    if buffer.is_empty() {
        return false;
    }
    if detect_tool_call_start(buffer, parser).unwrap_or(false) {
        return false;
    }
    for end in 1..=buffer.len().min(HOLDBACK_CHARS) {
        let start = buffer.len() - end;
        if !buffer.is_char_boundary(start) {
            continue;
        }
        if detect_tool_call_start(&buffer[start..], parser).unwrap_or(false) {
            return true;
        }
    }
    false
}

fn find_tool_call_start(buffer: &str, parser: Option<&'static str>) -> Option<usize> {
    for (idx, _) in buffer.char_indices() {
        let suffix = &buffer[idx..];
        if detect_tool_call_start(suffix, parser).unwrap_or(false)
            && !only_partial_start_marker(suffix, parser)
        {
            return Some(idx);
        }
    }
    None
}

/// Resolve a dynamo tool-call parser from the connection's setting + model name.
/// Returns `None` when nothing applicable matches (caller relies on native `tool_calls`).
pub fn resolve_tool_parser(setting: &str, model: &str) -> Option<&'static str> {
    let available = get_available_tool_parsers();
    let pick = |name: &str| available.iter().copied().find(|p| *p == name);
    match setting {
        "" | "none" => None,
        "auto" => match_known_family(&model.to_lowercase()).and_then(pick),
        explicit => match pick(explicit) {
            Some(parser) => Some(parser),
            None => {
                tracing::warn!(
                    parser = explicit,
                    "unknown tool-call parser; using native only"
                );
                None
            }
        },
    }
}

pub fn list_tool_parsers() -> Vec<&'static str> {
    let mut parsers = get_available_tool_parsers();
    parsers.sort_unstable();
    parsers
}

/// Map OpenAI-style tool specs to dynamo [`ToolDefinition`] values.
pub fn tool_definitions_from_specs(tools: &[Value]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function")?;
            let name = function.get("name")?.as_str()?.to_string();
            let parameters = function.get("parameters").cloned();
            Some(ToolDefinition {
                name,
                parameters,
                strict: None,
            })
        })
        .collect()
}

fn match_known_family(model: &str) -> Option<&'static str> {
    let rules: &[(&[&str], &str)] = &[
        (&["gemma-4", "gemma4", "gemma"], "gemma4"),
        (&["qwen3-coder", "qwen3_coder"], "qwen3_coder"),
        (&["qwen2.5", "qwen25", "qwen-2.5"], "qwen25"),
        (&["qwen"], "hermes"),
        (&["llama-3", "llama3"], "llama3_json"),
        (&["mistral", "mixtral"], "mistral"),
        (&["deepseek-v4", "deepseekv4", "deepseek_v4"], "deepseek_v4"),
        (&["deepseek-v3.2", "deepseek_v3_2"], "deepseek_v3_2"),
        (&["deepseek-v3.1", "deepseek_v3_1"], "deepseek_v3_1"),
        (&["deepseek"], "deepseek_v3"),
        (&["hermes"], "hermes"),
        (&["phi-4", "phi4"], "phi4"),
        (&["glm-4.7", "glm47", "glm-4"], "glm47"),
        (&["kimi"], "kimi_k2"),
        (&["minimax"], "minimax_m2"),
        (&["jamba"], "jamba"),
        (&["nemotron-nano", "nemotron_nano"], "nemotron_nano"),
        (&["nemotron"], "nemotron_deci"),
        (&["gpt-oss", "harmony"], "harmony"),
        (&["python"], "pythonic"),
    ];
    for (patterns, parser) in rules {
        if patterns.iter().any(|needle| model.contains(needle)) {
            return Some(parser);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_auto_maps_gemma_family() {
        assert_eq!(
            resolve_tool_parser("auto", "llmfan46/gemma-4-31B-it"),
            Some("gemma4")
        );
    }

    #[test]
    fn resolve_none_is_native_only() {
        assert_eq!(resolve_tool_parser("none", "anything"), None);
        assert_eq!(resolve_tool_parser("", "anything"), None);
    }

    #[test]
    fn resolve_unknown_explicit_warns_and_returns_none() {
        assert_eq!(resolve_tool_parser("not_a_real_parser_xyz", "model"), None);
    }

    #[test]
    fn resolve_explicit_valid_parser() {
        assert_eq!(resolve_tool_parser("hermes", "anything"), Some("hermes"));
    }

    #[test]
    fn resolve_auto_unknown_model_returns_none() {
        assert_eq!(
            resolve_tool_parser("auto", "totally-unknown-model-xyz"),
            None
        );
    }

    #[test]
    fn fallback_parses_call_after_mech_marker() {
        let text = "⟦mech:0⟧call:board_move{actor:pc,board_id:main}";
        let (calls, prose) = fallback_extract_bare_calls(text, None);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "board_move");
        assert_eq!(prose, "⟦mech:0⟧");
    }

    #[test]
    fn fallback_parses_concatenated_calls() {
        let text = "call:roll_dice{dice_expr:1d6,label:test}call:draw_card{deck_id:transformation}";
        let (calls, prose) = fallback_extract_bare_calls(text, None);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "roll_dice");
        assert_eq!(calls[1].name, "draw_card");
        assert!(prose.is_empty());
    }

    #[test]
    fn fallback_parses_apply_state_changes_with_array() {
        let text = "call:apply_state_changes{changes:[{key:game_status,kind:fact,op:set,target:world,value:in progress}]}";
        let (calls, _) = fallback_extract_bare_calls(text, None);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "apply_state_changes");
        let args: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["changes"][0]["key"], "game_status");
        assert_eq!(args["changes"][0]["value"], "in progress");
    }

    #[test]
    fn fallback_parses_known_tool_without_call_prefix() {
        let text = r#"Ready to begin.

ask_pc_decision{question: "The game is set up and your friends are ready. Who will take the first turn?"}"#;
        let known = ["ask_pc_decision"];
        let (calls, prose) = fallback_extract_bare_calls(text, Some(&known));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ask_pc_decision");
        let args: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(
            args["question"],
            "The game is set up and your friends are ready. Who will take the first turn?"
        );
        assert_eq!(prose.trim(), "Ready to begin.");
    }

    #[test]
    fn fallback_prefers_call_prefix_over_embedded_tool_name() {
        let text = "call:ask_pc_decision{question: Who goes first?}";
        let known = ["ask_pc_decision"];
        let (calls, prose) = fallback_extract_bare_calls(text, Some(&known));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ask_pc_decision");
        assert!(prose.is_empty());
    }

    #[tokio::test]
    async fn dynamo_parses_bare_gemma4_call() {
        let (calls, _) = detect_and_parse_tool_call_with_recovery(
            "call:board_move{actor:pc}<tool_call|>",
            Some("gemma4"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "board_move");
    }

    #[tokio::test]
    async fn jail_streams_gemma4_bare_call_across_tokens() {
        let parser = resolve_tool_parser("auto", "gemma-4-test");
        let jail = &mut ToolStreamJail::new(parser);
        let mut events = Vec::new();
        for token in [
            "You shake the die. ",
            "call:board_move",
            "{actor:pc}",
            "<tool_call|>",
            " It lands.",
        ] {
            events.extend(jail.push(token, None).await.unwrap());
        }
        events.extend(jail.finish(None).await.unwrap());

        let prose: String = events
            .iter()
            .filter_map(|event| match event {
                JailEvent::Prose(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(prose.contains("You shake the die."));
        assert!(prose.contains("It lands."));
        let tool_names: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                JailEvent::ToolCall(call) => Some(call.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_names, vec!["board_move"]);
    }

    #[tokio::test]
    async fn dynamo_parses_roll_dice_bare_call_from_game_15() {
        let call_text = "call:roll_dice{dice_expr:1d6,label:Ryan's first move}";
        let (calls, _) = fallback_extract_bare_calls(call_text, None);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "roll_dice");
        let args: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["dice_expr"], "1d6");
        assert_eq!(args["label"], "Ryan's first move");
    }

    #[tokio::test]
    async fn jail_finish_executes_trailing_roll_dice_from_game_15() {
        let parser = resolve_tool_parser(
            "auto",
            "llmfan46/gemma-4-31B-it-qat-q4_0-unquantized-uncensored-heretic",
        );
        assert_eq!(parser, Some("gemma4"));
        let jail = &mut ToolStreamJail::new(parser);
        let prose_prefix = "I reach for the die and toss it.\n\n";
        let mut events = Vec::new();
        for token in [
            prose_prefix,
            "call:roll_dice",
            "{dice_expr:1d6,label:Ryan's first move}",
        ] {
            events.extend(jail.push(token, None).await.unwrap());
        }
        events.extend(jail.finish(None).await.unwrap());
        let tool_names: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                JailEvent::ToolCall(tc) => Some(tc.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_names, vec!["roll_dice"]);
        let prose: String = events
            .iter()
            .filter_map(|e| match e {
                JailEvent::Prose(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(!prose.contains("call:roll_dice"));
    }

    #[tokio::test]
    async fn jail_none_parser_passthrough() {
        let jail = &mut ToolStreamJail::new(None);
        jail.push("plain prose", None).await.unwrap();
        let events = jail.finish(None).await.unwrap();
        assert!(matches!(events.as_slice(), [JailEvent::Prose(text)] if text == "plain prose"));
    }
}
