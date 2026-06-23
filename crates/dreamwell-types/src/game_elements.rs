use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// How a game turn is orchestrated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineMode {
    #[default]
    Pipeline,
    ToolsMechanics,
    ToolsStructured,
}

impl EngineMode {
    pub fn from_db(s: &str) -> Self {
        match s {
            "tools_mechanics" => Self::ToolsMechanics,
            "tools_structured" => Self::ToolsStructured,
            _ => Self::Pipeline,
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Pipeline => "pipeline",
            Self::ToolsMechanics => "tools_mechanics",
            Self::ToolsStructured => "tools_structured",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameElementsConfig {
    #[serde(default)]
    pub boards: Vec<BoardDef>,
    #[serde(default)]
    pub decks: Vec<DeckDef>,
    #[serde(default)]
    pub turn_mechanicals: Vec<MechanicalStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardDef {
    pub id: String,
    #[serde(default = "default_board_spaces")]
    pub spaces: u32,
    #[serde(default = "default_move_dice")]
    pub move_dice: String,
    #[serde(default)]
    pub tag_rules: Vec<BoardTagRule>,
    #[serde(default = "default_board_tag")]
    pub default_tag: String,
}

fn default_board_spaces() -> u32 {
    80
}

fn default_move_dice() -> String {
    "1d6".to_string()
}

fn default_board_tag() -> String {
    "transformation".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardTagRule {
    pub tag: String,
    #[serde(default)]
    pub spaces: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckDef {
    pub id: String,
    #[serde(default = "default_true")]
    pub consume_on_draw: bool,
    #[serde(default)]
    pub cards: Vec<CardDef>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardDef {
    pub id: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub requires_roll: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MechanicalStep {
    BoardMove {
        #[serde(default = "default_main_board")]
        board: String,
        #[serde(default)]
        actor: Option<String>,
    },
    CardDraw {
        #[serde(default = "default_deck_from")]
        deck_from: DeckFrom,
        #[serde(default = "default_true")]
        consume: bool,
        #[serde(default)]
        deck_id: Option<String>,
    },
    DiceRoll {
        #[serde(default = "default_move_dice")]
        dice: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        when: Option<MechanicalWhen>,
    },
}

fn default_main_board() -> String {
    "main".to_string()
}

fn default_deck_from() -> DeckFrom {
    DeckFrom::SpaceTag
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeckFrom {
    SpaceTag,
    Literal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MechanicalWhen {
    CardRequiresRoll,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ElementInstances {
    #[serde(default)]
    pub board_positions: HashMap<String, u32>,
    #[serde(default)]
    pub deck_piles: HashMap<String, DeckInstance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckInstance {
    pub deck_id: String,
    #[serde(default)]
    pub draw_pile: Vec<String>,
    #[serde(default)]
    pub discard: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MechanicalKind {
    DiceRoll,
    BoardMove,
    CardDraw,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MechanicalResult {
    pub kind: MechanicalKind,
    #[serde(default)]
    pub label: String,
    pub data: MechanicalData,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MechanicalData {
    DiceRoll {
        dice_expr: String,
        rolls: Vec<i64>,
        total: i64,
    },
    BoardMove {
        actor: String,
        board_id: String,
        roll: i64,
        from_space: u32,
        to_space: u32,
        space_tags: Vec<String>,
    },
    CardDraw {
        deck_id: String,
        card_id: String,
        name: String,
        text: String,
        consumed: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnObservability {
    pub engine_mode: EngineMode,
    #[serde(default)]
    pub llm_call_count: u32,
    #[serde(default)]
    pub tool_call_count: u32,
    #[serde(default)]
    pub tool_iterations: u32,
    #[serde(default)]
    pub phase_timings_ms: HashMap<String, u64>,
}

/// Default turn template for board game scenarios.
pub fn default_board_game_mechanicals() -> Vec<MechanicalStep> {
    vec![
        MechanicalStep::BoardMove {
            board: "main".to_string(),
            actor: None,
        },
        MechanicalStep::CardDraw {
            deck_from: DeckFrom::SpaceTag,
            consume: true,
            deck_id: None,
        },
        MechanicalStep::DiceRoll {
            dice: "1d6".to_string(),
            label: "card_effect".to_string(),
            when: Some(MechanicalWhen::CardRequiresRoll),
        },
    ]
}

/// Parse "Card N: Name - effect text" lines from a rules block into deck sections.
pub fn parse_cards_from_rules_block(content: &str) -> (Vec<CardDef>, Vec<CardDef>) {
    let mut transformation = Vec::new();
    let mut truth = Vec::new();
    let mut current: Option<&str> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Transformation Cards:") {
            current = Some("transformation");
            continue;
        }
        if trimmed.starts_with("Truth Cards:") {
            current = Some("truth");
            continue;
        }
        if let Some(caps) = parse_card_line(trimmed) {
            let (num, name, text) = caps;
            let requires_roll = text.contains("roll one die")
                || text.contains("roll a six-sided die")
                || text.contains("roll one die.");
            let card = CardDef {
                id: format!("{}:{num}", current.unwrap_or("unknown")),
                name: name.to_string(),
                text: text.to_string(),
                requires_roll,
            };
            match current {
                Some("transformation") => transformation.push(card),
                Some("truth") => truth.push(card),
                _ => {}
            }
        }
    }
    (transformation, truth)
}

fn parse_card_line(line: &str) -> Option<(u32, &str, &str)> {
    let rest = line.strip_prefix("Card ")?;
    let (num_part, rest) = rest.split_once(':')?;
    let num: u32 = num_part.trim().parse().ok()?;
    let (name, text) = rest.split_once('-')?;
    Some((num, name.trim(), text.trim()))
}

/// Build gameplay elements from Infinite Worlds scenario data (board-game scenarios).
pub fn build_game_elements_from_iw(
    rules_blocks: &[crate::RulesBlock],
    truth_spaces_value: Option<&str>,
) -> GameElementsConfig {
    let cards_block = rules_blocks
        .iter()
        .find(|b| b.name == "Cards and probabilities")
        .map(|b| b.content.as_str())
        .unwrap_or("");

    let (transformation_cards, truth_cards) = parse_cards_from_rules_block(cards_block);

    let truth_spaces = truth_spaces_value
        .map(parse_truth_spaces)
        .unwrap_or_default();

    let mut tag_rules = Vec::new();
    if !truth_spaces.is_empty() {
        tag_rules.push(BoardTagRule {
            tag: "truth".to_string(),
            spaces: truth_spaces,
        });
    }

    if transformation_cards.is_empty() && truth_cards.is_empty() && tag_rules.is_empty() {
        return GameElementsConfig::default();
    }

    GameElementsConfig {
        boards: vec![BoardDef {
            id: "main".to_string(),
            spaces: 80,
            move_dice: "1d6".to_string(),
            tag_rules,
            default_tag: "transformation".to_string(),
        }],
        decks: vec![
            DeckDef {
                id: "transformation".to_string(),
                consume_on_draw: true,
                cards: transformation_cards,
            },
            DeckDef {
                id: "truth".to_string(),
                consume_on_draw: true,
                cards: truth_cards,
            },
        ],
        turn_mechanicals: default_board_game_mechanicals(),
    }
}

/// Parse semicolon-separated truth space numbers.
pub fn parse_truth_spaces(value: &str) -> Vec<u32> {
    value
        .split(';')
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_mode_round_trips_db() {
        assert_eq!(EngineMode::from_db("pipeline"), EngineMode::Pipeline);
        assert_eq!(
            EngineMode::from_db("tools_mechanics"),
            EngineMode::ToolsMechanics
        );
        assert_eq!(EngineMode::from_db("bogus"), EngineMode::Pipeline);
        assert_eq!(EngineMode::Pipeline.as_db(), "pipeline");
    }

    #[test]
    fn parse_card_line_extracts_fields() {
        let line = "Card 3: Comparative Growth - Choose a player and a body part.";
        let (n, name, text) = parse_card_line(line).unwrap();
        assert_eq!(n, 3);
        assert_eq!(name, "Comparative Growth");
        assert!(text.contains("Choose a player"));
    }

    #[test]
    fn parse_truth_spaces_splits_list() {
        assert_eq!(parse_truth_spaces("8; 11; 14"), vec![8, 11, 14]);
    }
}
