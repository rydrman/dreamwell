use dreamwell_types::{
    is_scenario_export_value, parse_scenario_export_json, scenario_create_from_character,
    ScenarioCreate,
};
use serde_json::Value;

use crate::character_import::parse_character_import;
use crate::error::{AppError, AppResult};

pub use dreamwell_types::{game_create_from_character, GameCharacterImportMode};

pub fn scenario_create_from_import(filename: &str, content: &[u8]) -> AppResult<ScenarioCreate> {
    if filename.to_lowercase().ends_with(".png") {
        let card = parse_character_import(filename, content)?;
        return Ok(scenario_create_from_character(card));
    }

    let value: Value = serde_json::from_slice(content)
        .map_err(|e| AppError::bad_request(format!("Invalid JSON: {e}")))?;
    if is_scenario_export_value(&value) {
        let json = std::str::from_utf8(content)
            .map_err(|e| AppError::bad_request(format!("Invalid UTF-8: {e}")))?;
        return parse_scenario_export_json(json)
            .map_err(|e| AppError::bad_request(format!("Invalid scenario export: {e}")));
    }

    let card = parse_character_import(filename, content)?;
    Ok(scenario_create_from_character(card))
}

pub fn import_source_label(filename: &str, content: &[u8]) -> AppResult<&'static str> {
    if filename.to_lowercase().ends_with(".png") {
        return Ok("png");
    }
    let value: Value = serde_json::from_slice(content)
        .map_err(|e| AppError::bad_request(format!("Invalid JSON: {e}")))?;
    if is_scenario_export_value(&value) {
        Ok("scenario")
    } else {
        Ok("json")
    }
}
