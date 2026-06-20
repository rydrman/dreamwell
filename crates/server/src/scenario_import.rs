use dreamwell_types::{CharacterCreate, ScenarioCreate};

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
        premise: join_nonempty_sections(&[
            ("Scenario", payload.scenario.trim()),
            ("Opening hook", payload.first_message.trim()),
        ]),
        setting: join_nonempty_sections(&[
            ("World", payload.description.trim()),
            ("Tone", payload.personality.trim()),
        ]),
        gm_style: join_nonempty_sections(&[
            ("GM instructions", payload.system_prompt.trim()),
            ("Example dialogue", payload.example_dialogue.trim()),
        ]),
        pc_name: String::new(),
        pc_description: String::new(),
        character_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_world_card_fields_into_scenario() {
        let payload = CharacterCreate {
            name: "Neon District".into(),
            description: "A rain-soaked cyberpunk sprawl.".into(),
            personality: "Gritty, noir, high-tech low-life.".into(),
            scenario: "A courier job goes wrong in Sector 7.".into(),
            first_message: "The alley reeks of ozone and fried wiring.".into(),
            example_dialogue: String::new(),
            system_prompt: "Keep tension high and consequences real.".into(),
            avatar_url: None,
        };
        let scenario = scenario_create_from_character(payload);
        assert_eq!(scenario.title, "Neon District");
        assert!(scenario.premise.contains("Sector 7"));
        assert!(scenario.setting.contains("cyberpunk"));
        assert!(scenario.gm_style.contains("tension high"));
    }
}
