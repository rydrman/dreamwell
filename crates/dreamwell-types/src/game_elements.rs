use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Inline marker emitted into turn prose to anchor a mechanical-result block at the
/// exact point in the narration where the model triggered it. The frontend splits
/// prose on these markers and renders the matching `MechanicalResult` inline.
pub const PROSE_MECH_MARKER_OPEN: &str = "\u{27E6}mech:";
pub const PROSE_STATE_MARKER_OPEN: &str = "\u{27E6}state:";
pub const PROSE_CHECK_MARKER_OPEN: &str = "\u{27E6}check:";
pub const PROSE_INLINE_MARKER_CLOSE: &str = "\u{27E7}";
/// Legacy alias — all inline marker types share the same closing delimiter.
pub const PROSE_MECH_MARKER_CLOSE: &str = PROSE_INLINE_MARKER_CLOSE;

/// Build the inline prose marker for the mechanical result with the given sort order.
pub fn prose_mech_marker(sort_order: i64) -> String {
    format!("{PROSE_MECH_MARKER_OPEN}{sort_order}{PROSE_INLINE_MARKER_CLOSE}")
}

/// Anchor an applied state change at a point in the narration.
pub fn prose_state_marker(index: i64) -> String {
    format!("{PROSE_STATE_MARKER_OPEN}{index}{PROSE_INLINE_MARKER_CLOSE}")
}

/// Anchor a dramatic check (rolled before prose) at the start of narration.
pub fn prose_check_marker(sort_order: i64) -> String {
    format!("{PROSE_CHECK_MARKER_OPEN}{sort_order}{PROSE_INLINE_MARKER_CLOSE}")
}

/// How a game turn is orchestrated (tool-calling inline prose agent).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineMode {
    #[default]
    #[serde(alias = "pipeline", alias = "tools_mechanics")]
    ToolsStructured,
}

impl EngineMode {
    pub fn from_db(_s: &str) -> Self {
        Self::ToolsStructured
    }

    pub fn as_db(self) -> &'static str {
        "tools_structured"
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameElementsConfig {
    #[serde(default)]
    pub boards: Vec<BoardDef>,
    #[serde(default)]
    pub decks: Vec<DeckDef>,
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
    "space".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_mode_round_trips_db() {
        assert_eq!(EngineMode::from_db("pipeline"), EngineMode::ToolsStructured);
        assert_eq!(
            EngineMode::from_db("tools_mechanics"),
            EngineMode::ToolsStructured
        );
        assert_eq!(
            EngineMode::from_db("tools_structured"),
            EngineMode::ToolsStructured
        );
        assert_eq!(EngineMode::ToolsStructured.as_db(), "tools_structured");
    }
}
