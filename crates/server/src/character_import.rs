use std::io::Cursor;

use base64::Engine as _;
use dreamwell_types::CharacterCreate;
use png::Info;
use serde_json::Value;

use crate::error::{AppError, AppResult};

pub fn parse_character_import(filename: &str, content: &[u8]) -> AppResult<CharacterCreate> {
    let card = if filename.to_lowercase().ends_with(".png") {
        parse_png_card(content)?
    } else {
        serde_json::from_slice::<Value>(content)?
    };
    Ok(normalize_card(&card))
}

fn parse_png_card(content: &[u8]) -> AppResult<Value> {
    let decoder = png::Decoder::new(Cursor::new(content));
    let mut reader = decoder
        .read_info()
        .map_err(|e| AppError::bad_request(e.to_string()))?;
    reader
        .finish()
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    let info = reader.info();
    if let Some(card) = find_card_in_info(info, "ccv3") {
        return Ok(card);
    }
    find_card_in_info(info, "chara")
        .ok_or_else(|| AppError::bad_request("No character data found in PNG"))
}

fn find_card_in_info(info: &Info<'_>, keyword: &str) -> Option<Value> {
    for text in &info.uncompressed_latin1_text {
        if text.keyword == keyword {
            return parse_card_text(&text.text).ok();
        }
    }
    for text in &info.compressed_latin1_text {
        if text.keyword == keyword {
            let decoded = text.get_text().ok()?;
            return parse_card_text(&decoded).ok();
        }
    }
    for text in &info.utf8_text {
        if text.keyword == keyword {
            let decoded = text.get_text().ok()?;
            return parse_card_text(&decoded).ok();
        }
    }
    None
}

fn parse_card_text(text: &str) -> AppResult<Value> {
    let trimmed = text.trim();
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(trimmed) {
        if let Ok(value) = serde_json::from_slice(&decoded) {
            return Ok(value);
        }
    }
    serde_json::from_str(trimmed).map_err(AppError::from)
}

fn normalize_card(card: &Value) -> CharacterCreate {
    let data = card.get("data").unwrap_or(card);
    CharacterCreate {
        name: data
            .get("name")
            .or_else(|| data.get("char_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unnamed")
            .to_string(),
        description: data
            .get("description")
            .or_else(|| data.get("char_persona"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        personality: data
            .get("personality")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        scenario: data
            .get("scenario")
            .or_else(|| data.get("world_scenario"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        first_message: data
            .get("first_mes")
            .or_else(|| data.get("first_message"))
            .or_else(|| data.get("greeting"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        example_dialogue: data
            .get("mes_example")
            .or_else(|| data.get("example_dialogue"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        system_prompt: data
            .get("system_prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        avatar_url: data
            .get("avatar")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_card_text_accepts_base64_json() {
        let card = serde_json::json!({
            "spec": "chara_card_v2",
            "data": { "name": "Test Character" }
        });
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_string(&card).unwrap());
        let parsed = parse_card_text(&encoded).unwrap();
        assert_eq!(parsed["data"]["name"], "Test Character");
    }

    #[test]
    fn parse_sample_character_png() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sample_character_card.png");
        let content = std::fs::read(path).unwrap();
        let character = parse_character_import("sample_character_card.png", &content).unwrap();
        assert_eq!(character.name, "River Guide");
        assert!(character.description.contains("park ranger"));
    }
}
