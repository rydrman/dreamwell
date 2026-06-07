use std::io::Cursor;

use dreamwell_types::CharacterCreate;
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
    let reader = decoder
        .read_info()
        .map_err(|e| AppError::bad_request(e.to_string()))?;
    for text in &reader.info().uncompressed_latin1_text {
        if text.keyword == "chara" || text.keyword == "ccv3" {
            return serde_json::from_str(&text.text).map_err(AppError::from);
        }
    }
    Err(AppError::bad_request("No character data found in PNG"))
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
