#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameTonePreset {
    pub id: &'static str,
    pub label: &'static str,
    pub setting: &'static str,
    pub gm_style: &'static str,
}

pub const GAME_TONE_PRESETS: &[GameTonePreset] = &[
    GameTonePreset {
        id: "cozy",
        label: "Cozy slice-of-life",
        setting: "Cozy, low-stakes, warm and conversational.",
        gm_style: "Gentle pacing; focus on small choices and character moments. Consequences are emotional or social, not physical.",
    },
    GameTonePreset {
        id: "mystery",
        label: "Mystery",
        setting: "Slow-burn intrigue; clues and atmosphere over action.",
        gm_style: "Reveal information gradually. Consequences are reputational and informational. Avoid combat unless the player seeks danger.",
    },
    GameTonePreset {
        id: "adventure",
        label: "High-stakes adventure",
        setting: "Perilous, propulsive, consequential adventure.",
        gm_style: "Keep tension high and stakes real. Escalate when the player takes risks. Failures should cost something meaningful.",
    },
    GameTonePreset {
        id: "romance",
        label: "Romance / social drama",
        setting: "Intimate, relationship-forward; feelings and subtext matter.",
        gm_style: "Focus on dialogue, chemistry, and emotional beats. Conflict is interpersonal. Prefer no mechanical harm.",
    },
    GameTonePreset {
        id: "exploration",
        label: "Exploration & discovery",
        setting: "Curious, wonder-driven; the world is the draw.",
        gm_style: "Reward observation and curiosity. Describe places vividly. Danger is optional and clearly signposted.",
    },
    GameTonePreset {
        id: "horror",
        label: "Horror / dread",
        setting: "Uneasy, atmospheric; dread builds through implication.",
        gm_style: "Use restraint and ambiguity. Let implication do the work. Consequences can be psychological or physical when earned.",
    },
];

pub fn game_tone_preset_by_id(id: &str) -> Option<&'static GameTonePreset> {
    GAME_TONE_PRESETS.iter().find(|preset| preset.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_unique_ids_and_nonempty_copy() {
        let mut ids = std::collections::HashSet::new();
        for preset in GAME_TONE_PRESETS {
            assert!(ids.insert(preset.id));
            assert!(!preset.label.is_empty());
            assert!(!preset.setting.is_empty());
            assert!(!preset.gm_style.is_empty());
        }
    }

    #[test]
    fn lookup_by_id_works() {
        let cozy = game_tone_preset_by_id("cozy").unwrap();
        assert_eq!(cozy.label, "Cozy slice-of-life");
        assert!(game_tone_preset_by_id("missing").is_none());
    }
}
