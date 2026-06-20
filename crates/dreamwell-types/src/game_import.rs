use crate::{default_game_traits, Character, CharacterCreate, GameCreate, ScenarioCreate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameCharacterImportMode {
    /// Map the card as world/scenario text; PC fields stay empty unless filled in.
    World,
    /// Map the card as the player character sheet.
    PlayerCharacter,
}

fn join_nonempty_sections(sections: &[(&str, &str)]) -> String {
    sections
        .iter()
        .filter(|(_, body)| !body.is_empty())
        .map(|(label, body)| format!("{label}:\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn scenario_create_from_character(payload: CharacterCreate) -> ScenarioCreate {
    ScenarioCreate {
        title: payload.name,
        premise: join_nonempty_sections(&[("Scenario", payload.scenario.trim())]),
        setting: join_nonempty_sections(&[
            ("World", payload.description.trim()),
            ("Tone", payload.personality.trim()),
        ]),
        gm_style: join_nonempty_sections(&[
            ("GM instructions", payload.system_prompt.trim()),
            ("Example dialogue", payload.example_dialogue.trim()),
        ]),
        opening_message: payload.first_message.trim().to_string(),
        pc_name: String::new(),
        pc_description: String::new(),
        traits: default_game_traits(),
        character_id: None,
    }
}

pub fn scenario_create_from_character_record(character: &Character) -> ScenarioCreate {
    let mut create = scenario_create_from_character(CharacterCreate {
        name: character.name.clone(),
        description: character.description.clone(),
        personality: character.personality.clone(),
        scenario: character.scenario.clone(),
        first_message: character.first_message.clone(),
        example_dialogue: character.example_dialogue.clone(),
        system_prompt: character.system_prompt.clone(),
        avatar_url: character.avatar_url.clone(),
    });
    create.character_id = Some(character.id);
    create
}

pub fn game_create_from_character(
    payload: CharacterCreate,
    mode: GameCharacterImportMode,
    title: Option<String>,
    character_id: Option<i64>,
) -> GameCreate {
    let title = title.unwrap_or_else(|| payload.name.clone());
    let premise = join_nonempty_sections(&[("Scenario", payload.scenario.trim())]);
    let setting = join_nonempty_sections(&[
        ("World", payload.description.trim()),
        ("Tone", payload.personality.trim()),
    ]);
    let gm_style = join_nonempty_sections(&[
        ("GM instructions", payload.system_prompt.trim()),
        ("Example dialogue", payload.example_dialogue.trim()),
    ]);
    let opening_message = payload.first_message.trim().to_string();
    let (pc_name, pc_description) = match mode {
        GameCharacterImportMode::World => (String::new(), String::new()),
        GameCharacterImportMode::PlayerCharacter => (
            payload.name.clone(),
            join_nonempty_sections(&[
                ("Description", payload.description.trim()),
                ("Personality", payload.personality.trim()),
            ]),
        ),
    };
    GameCreate {
        title,
        premise,
        setting,
        gm_style,
        opening_message,
        character_id,
        scenario_id: None,
        pc_name,
        pc_description,
        pc_traits: default_game_traits(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_card() -> CharacterCreate {
        CharacterCreate {
            name: "Neon District".into(),
            description: "A rain-soaked cyberpunk sprawl.".into(),
            personality: "Gritty, noir, high-tech low-life.".into(),
            scenario: "A courier job goes wrong in Sector 7.".into(),
            first_message: "The alley reeks of ozone and fried wiring.".into(),
            example_dialogue: String::new(),
            system_prompt: "Keep tension high and consequences real.".into(),
            avatar_url: None,
        }
    }

    #[test]
    fn maps_world_card_fields_into_scenario() {
        let scenario = scenario_create_from_character(sample_card());
        assert_eq!(scenario.title, "Neon District");
        assert!(scenario.premise.contains("Sector 7"));
        assert!(!scenario.premise.contains("ozone"));
        assert_eq!(
            scenario.opening_message,
            "The alley reeks of ozone and fried wiring."
        );
        assert!(scenario.setting.contains("cyberpunk"));
        assert!(scenario.gm_style.contains("tension high"));
    }

    #[test]
    fn maps_saved_character_record_into_scenario() {
        let character = Character {
            id: 9,
            name: "Neon District".into(),
            description: "A rain-soaked cyberpunk sprawl.".into(),
            personality: "Gritty.".into(),
            scenario: "Sector 7.".into(),
            first_message: "Steam rises.".into(),
            example_dialogue: String::new(),
            system_prompt: "Noir.".into(),
            avatar_url: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let scenario = scenario_create_from_character_record(&character);
        assert_eq!(scenario.character_id, Some(9));
        assert_eq!(scenario.opening_message, "Steam rises.");
    }

    #[test]
    fn maps_world_card_into_game() {
        let game =
            game_create_from_character(sample_card(), GameCharacterImportMode::World, None, None);
        assert_eq!(game.title, "Neon District");
        assert_eq!(
            game.opening_message,
            "The alley reeks of ozone and fried wiring."
        );
        assert!(game.premise.contains("Sector 7"));
        assert!(game.pc_name.is_empty());
    }

    #[test]
    fn maps_character_card_as_pc() {
        let game = game_create_from_character(
            sample_card(),
            GameCharacterImportMode::PlayerCharacter,
            Some("Courier Run".into()),
            Some(42),
        );
        assert_eq!(game.title, "Courier Run");
        assert_eq!(game.pc_name, "Neon District");
        assert!(game.pc_description.contains("cyberpunk"));
        assert_eq!(game.character_id, Some(42));
    }
}
