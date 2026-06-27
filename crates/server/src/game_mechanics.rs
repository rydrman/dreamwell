use dreamwell_types::{
    BoardDef, DeckDef, DeckInstance, ElementInstances, GameElementsConfig, MechanicalData,
    MechanicalKind, MechanicalResult,
};

use crate::game_resolution::{parse_dice_expr, roll_system_dice};

pub fn board_space_tags(board: &BoardDef, space: u32) -> Vec<String> {
    for rule in &board.tag_rules {
        if rule.spaces.contains(&space) {
            return vec![rule.tag.clone()];
        }
    }
    vec![board.default_tag.clone()]
}

pub fn execute_board_move(
    board: &BoardDef,
    instances: &mut ElementInstances,
    actor: &str,
) -> Option<MechanicalResult> {
    let roll = roll_system_dice(&board.move_dice)?;
    let from = instances.board_positions.get(actor).copied().unwrap_or(0);
    let to = (from + roll.total as u32).min(board.spaces);
    instances.board_positions.insert(actor.to_string(), to);
    let space_tags = board_space_tags(board, to);
    Some(MechanicalResult {
        kind: MechanicalKind::BoardMove,
        label: format!("{actor} moves"),
        data: MechanicalData::BoardMove {
            actor: actor.to_string(),
            board_id: board.id.clone(),
            roll: roll.total,
            from_space: from,
            to_space: to,
            space_tags,
        },
        sort_order: 0,
    })
}

pub fn execute_card_draw(
    deck_def: &DeckDef,
    instances: &mut ElementInstances,
    consume: bool,
) -> Option<MechanicalResult> {
    let pile = instances
        .deck_piles
        .entry(deck_def.id.clone())
        .or_insert_with(|| DeckInstance {
            deck_id: deck_def.id.clone(),
            draw_pile: deck_def.cards.iter().map(|c| c.id.clone()).collect(),
            discard: Vec::new(),
        });

    let card_id = pile.draw_pile.first()?.clone();

    let card = deck_def.cards.iter().find(|c| c.id == card_id)?;

    if consume && deck_def.consume_on_draw {
        if let Some(pos) = pile.draw_pile.iter().position(|id| id == &card_id) {
            pile.draw_pile.remove(pos);
        }
        pile.discard.push(card_id.clone());
    }

    Some(MechanicalResult {
        kind: MechanicalKind::CardDraw,
        label: format!("Draw {}", card.name),
        data: MechanicalData::CardDraw {
            deck_id: deck_def.id.clone(),
            card_id: card.id.clone(),
            name: card.name.clone(),
            text: card.text.clone(),
            consumed: consume && deck_def.consume_on_draw,
        },
        sort_order: 0,
    })
}

/// Agent `roll_dice` tool: one physical die per call (`1d6`, `1d20`, …).
pub fn execute_dice_roll(dice_expr: &str, label: &str) -> Option<MechanicalResult> {
    let (count, _sides) = parse_dice_expr(dice_expr)?;
    if count != 1 {
        return None;
    }
    let roll = roll_system_dice(dice_expr)?;
    Some(MechanicalResult {
        kind: MechanicalKind::DiceRoll,
        label: label.to_string(),
        data: MechanicalData::DiceRoll {
            dice_expr: dice_expr.to_string(),
            rolls: roll.rolls,
            total: roll.total,
        },
        sort_order: 0,
    })
}

pub fn init_element_instances(elements: &GameElementsConfig) -> ElementInstances {
    use rand::seq::SliceRandom;
    let mut rng = rand::rng();
    let mut deck_piles = std::collections::HashMap::new();
    for deck in &elements.decks {
        let mut ids: Vec<String> = deck.cards.iter().map(|c| c.id.clone()).collect();
        ids.shuffle(&mut rng);
        deck_piles.insert(
            deck.id.clone(),
            DeckInstance {
                deck_id: deck.id.clone(),
                draw_pile: ids,
                discard: Vec::new(),
            },
        );
    }
    ElementInstances {
        board_positions: std::collections::HashMap::new(),
        deck_piles,
    }
}

/// Rebuild deck piles and board positions from completed turn mechanical results.
pub fn replay_element_instances(
    elements: &GameElementsConfig,
    turns: &[dreamwell_types::GameTurn],
) -> ElementInstances {
    if elements.boards.is_empty() && elements.decks.is_empty() {
        return ElementInstances::default();
    }
    let mut instances = init_element_instances(elements);
    for turn in turns {
        for result in &turn.mechanical_results {
            apply_mechanical_result_to_instances(&mut instances, result);
        }
    }
    instances
}

fn apply_mechanical_result_to_instances(
    instances: &mut ElementInstances,
    result: &MechanicalResult,
) {
    match &result.data {
        MechanicalData::BoardMove {
            actor, to_space, ..
        } => {
            instances.board_positions.insert(actor.clone(), *to_space);
        }
        MechanicalData::CardDraw {
            deck_id,
            card_id,
            consumed,
            ..
        } => {
            if !*consumed {
                return;
            }
            let Some(pile) = instances.deck_piles.get_mut(deck_id) else {
                return;
            };
            if let Some(pos) = pile.draw_pile.iter().position(|id| id == card_id) {
                pile.draw_pile.remove(pos);
            }
            if !pile.discard.iter().any(|id| id == card_id) {
                pile.discard.push(card_id.clone());
            }
        }
        MechanicalData::DiceRoll { .. } => {}
    }
}

pub async fn flush_turn_mechanicals_streaming(
    pool: &sqlx::SqlitePool,
    game_id: i64,
    turn_id: i64,
    results: &[MechanicalResult],
    instances: &ElementInstances,
) -> crate::error::AppResult<()> {
    use crate::db;
    db::update_turn_mechanical_results(pool, turn_id, results).await?;
    db::update_game_element_instances(pool, game_id, instances).await?;
    db::touch_game(pool, game_id).await?;
    Ok(())
}

pub async fn persist_turn_mechanicals(
    pool: &sqlx::SqlitePool,
    game_id: i64,
    turn_id: i64,
    _game: &dreamwell_types::Game,
    results: &[MechanicalResult],
    instances: &ElementInstances,
) -> crate::error::AppResult<()> {
    use crate::db;
    use dreamwell_types::GameTurnSystemRoll;

    db::update_turn_mechanical_results(pool, turn_id, results).await?;
    db::update_game_element_instances(pool, game_id, instances).await?;

    db::clear_system_rolls(pool, turn_id).await?;
    for (i, result) in results.iter().enumerate() {
        if let MechanicalData::DiceRoll {
            dice_expr, rolls, ..
        } = &result.data
        {
            let system_roll = GameTurnSystemRoll {
                id: 0,
                turn_id,
                label: result.label.clone(),
                dice_expr: dice_expr.clone(),
                rolls: rolls.clone(),
                outcome_key: result.label.clone(),
                outcome_summary: format!("Rolled {rolls:?} = {}", rolls.iter().sum::<i64>()),
                sort_order: i as i64,
                created_at: chrono::Utc::now(),
            };
            db::insert_system_roll(pool, turn_id, &system_roll).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dreamwell_types::{CardDef, DeckDef};

    #[test]
    fn board_space_tags_uses_rule_or_default() {
        let board = BoardDef {
            id: "main".to_string(),
            spaces: 80,
            move_dice: "1d6".to_string(),
            tag_rules: vec![dreamwell_types::BoardTagRule {
                tag: "truth".to_string(),
                spaces: vec![8, 14],
            }],
            default_tag: "space".to_string(),
        };
        assert_eq!(board_space_tags(&board, 8), vec!["truth".to_string()]);
        assert_eq!(board_space_tags(&board, 5), vec!["space".to_string()]);
    }

    #[test]
    fn execute_board_move_updates_position() {
        let board = BoardDef {
            id: "main".to_string(),
            spaces: 80,
            move_dice: "1d6".to_string(),
            tag_rules: vec![],
            default_tag: "space".to_string(),
        };
        let mut instances = ElementInstances::default();
        let result = execute_board_move(&board, &mut instances, "pc").expect("move");
        assert!(matches!(result.data, MechanicalData::BoardMove { .. }));
        assert!(instances.board_positions.contains_key("pc"));
    }

    #[test]
    fn execute_card_draw_returns_card() {
        let deck = DeckDef {
            id: "events".to_string(),
            consume_on_draw: true,
            cards: vec![CardDef {
                id: "events:1".to_string(),
                name: "Storm".to_string(),
                text: "A storm hits.".to_string(),
            }],
        };
        let mut instances = init_element_instances(&GameElementsConfig {
            decks: vec![deck.clone()],
            ..Default::default()
        });
        let result = execute_card_draw(&deck, &mut instances, true).expect("draw");
        assert!(matches!(result.data, MechanicalData::CardDraw { .. }));
    }

    #[test]
    fn execute_dice_roll_rejects_multi_die_expression() {
        assert!(execute_dice_roll("4d6", "group").is_none());
        assert!(execute_dice_roll("2d6", "pair").is_none());
    }

    #[test]
    fn execute_dice_roll_accepts_single_die() {
        let result = execute_dice_roll("1d6", "solo").expect("roll");
        assert!(matches!(result.data, MechanicalData::DiceRoll { .. }));
        if let MechanicalData::DiceRoll { rolls, total, .. } = result.data {
            assert_eq!(rolls.len(), 1);
            assert_eq!(total, rolls[0]);
        }
    }
}
