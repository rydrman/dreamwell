use dreamwell_types::{
    BoardDef, DeckDef, DeckFrom, DeckInstance, ElementInstances, GameElementsConfig,
    MechanicalData, MechanicalKind, MechanicalResult, MechanicalStep, MechanicalWhen,
};

use crate::game_resolution::roll_system_dice;

pub struct MechanicalContext<'a> {
    pub elements: &'a GameElementsConfig,
    pub instances: ElementInstances,
    pub active_actor: String,
    pub last_space_tags: Vec<String>,
    pub last_card_requires_roll: bool,
}

impl<'a> MechanicalContext<'a> {
    pub fn new(
        elements: &'a GameElementsConfig,
        instances: ElementInstances,
        active_actor: String,
    ) -> Self {
        Self {
            elements,
            instances,
            active_actor,
            last_space_tags: Vec::new(),
            last_card_requires_roll: false,
        }
    }
}

pub fn resolve_deck_from_tags(
    space_tags: &[String],
    elements: &GameElementsConfig,
) -> Option<String> {
    if space_tags.iter().any(|t| t == "truth") {
        elements
            .decks
            .iter()
            .find(|d| d.id == "truth")
            .map(|d| d.id.clone())
            .or_else(|| elements.decks.get(1).map(|d| d.id.clone()))
    } else {
        elements
            .decks
            .iter()
            .find(|d| d.id == "transformation")
            .map(|d| d.id.clone())
            .or_else(|| elements.decks.first().map(|d| d.id.clone()))
    }
}

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

pub fn execute_dice_roll(dice_expr: &str, label: &str) -> Option<MechanicalResult> {
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

pub fn execute_mechanicals(
    elements: &GameElementsConfig,
    instances: ElementInstances,
    steps: &[MechanicalStep],
    active_actor: &str,
) -> (Vec<MechanicalResult>, ElementInstances) {
    if steps.is_empty() {
        return (Vec::new(), instances);
    }

    let mut ctx = MechanicalContext::new(elements, instances, active_actor.to_string());
    let mut results = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        let result = match step {
            MechanicalStep::BoardMove { board, actor } => {
                let actor = actor.as_deref().unwrap_or(&ctx.active_actor);
                let board_def = ctx
                    .elements
                    .boards
                    .iter()
                    .find(|b| b.id == *board)
                    .or_else(|| ctx.elements.boards.first());
                let Some(board_def) = board_def else {
                    continue;
                };
                let r = execute_board_move(board_def, &mut ctx.instances, actor);
                if let Some(MechanicalData::BoardMove { space_tags, .. }) =
                    r.as_ref().map(|x| &x.data)
                {
                    ctx.last_space_tags = space_tags.clone();
                }
                r
            }
            MechanicalStep::CardDraw {
                deck_from,
                consume,
                deck_id,
            } => {
                let resolved_id = match deck_from {
                    DeckFrom::SpaceTag => {
                        resolve_deck_from_tags(&ctx.last_space_tags, ctx.elements)
                    }
                    DeckFrom::Literal => deck_id.clone(),
                };
                let Some(deck_id) = resolved_id else {
                    continue;
                };
                let deck_def = ctx.elements.decks.iter().find(|d| d.id == deck_id);
                let Some(deck_def) = deck_def else {
                    continue;
                };
                let r = execute_card_draw(deck_def, &mut ctx.instances, *consume);
                if let Some(MechanicalData::CardDraw { text, .. }) = r.as_ref().map(|x| &x.data) {
                    ctx.last_card_requires_roll = deck_def
                        .cards
                        .iter()
                        .find(|c| c.text == *text)
                        .map(|c| c.requires_roll)
                        .unwrap_or(false);
                }
                r
            }
            MechanicalStep::DiceRoll { dice, label, when } => {
                if matches!(when, Some(MechanicalWhen::CardRequiresRoll))
                    && !ctx.last_card_requires_roll
                {
                    continue;
                }
                execute_dice_roll(dice, label)
            }
        };

        if let Some(mut r) = result {
            r.sort_order = i as i64;
            results.push(r);
        }
    }

    (results, ctx.instances)
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

pub async fn sync_board_positions_to_state(
    pool: &sqlx::SqlitePool,
    game_id: i64,
    turn_id: i64,
    instances: &ElementInstances,
    state_schema: &[dreamwell_types::TrackedVarDef],
) -> crate::error::AppResult<()> {
    let position_keys = [
        "Board_position",
        "Game_piece_position",
        "Leading_piece_position",
    ];
    for key in position_keys {
        if !state_schema.iter().any(|d| d.key == key) {
            continue;
        }
        let pos = instances.board_positions.get("pc").copied().unwrap_or(0);
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE game_state_entries SET value=?1, num_value=?2, source_turn=?3, updated_at=?4 WHERE game_id=?5 AND key=?6",
        )
        .bind(pos.to_string())
        .bind(pos as i64)
        .bind(turn_id)
        .bind(&now)
        .bind(game_id)
        .bind(key)
        .execute(pool)
        .await?;
    }
    Ok(())
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
    game: &dreamwell_types::Game,
    results: &[MechanicalResult],
    instances: &ElementInstances,
) -> crate::error::AppResult<()> {
    use crate::db;
    use dreamwell_types::GameTurnSystemRoll;

    db::update_turn_mechanical_results(pool, turn_id, results).await?;
    db::update_game_element_instances(pool, game_id, instances).await?;
    sync_board_positions_to_state(pool, game_id, turn_id, instances, &game.state_schema).await?;

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
    use dreamwell_types::{CardDef, DeckDef, MechanicalStep};

    fn sample_elements() -> GameElementsConfig {
        GameElementsConfig {
            boards: vec![BoardDef {
                id: "main".to_string(),
                spaces: 80,
                move_dice: "1d6".to_string(),
                tag_rules: vec![dreamwell_types::BoardTagRule {
                    tag: "truth".to_string(),
                    spaces: vec![8, 14],
                }],
                default_tag: "transformation".to_string(),
            }],
            decks: vec![
                DeckDef {
                    id: "transformation".to_string(),
                    consume_on_draw: true,
                    cards: vec![CardDef {
                        id: "transformation:1".to_string(),
                        name: "Grow".to_string(),
                        text: "Grow effect".to_string(),
                        requires_roll: true,
                    }],
                },
                DeckDef {
                    id: "truth".to_string(),
                    consume_on_draw: true,
                    cards: vec![CardDef {
                        id: "truth:1".to_string(),
                        name: "Command".to_string(),
                        text: "Command effect".to_string(),
                        requires_roll: false,
                    }],
                },
            ],
            turn_mechanicals: dreamwell_types::default_board_game_mechanicals(),
        }
    }

    #[test]
    fn execute_mechanicals_runs_board_then_draw() {
        let elements = sample_elements();
        let instances = init_element_instances(&elements);
        let steps = vec![
            MechanicalStep::BoardMove {
                board: "main".to_string(),
                actor: Some("Chris".to_string()),
            },
            MechanicalStep::CardDraw {
                deck_from: DeckFrom::SpaceTag,
                consume: true,
                deck_id: None,
            },
        ];
        let (results, _) = execute_mechanicals(&elements, instances, &steps, "Chris");
        assert!(results.iter().any(|r| r.kind == MechanicalKind::BoardMove));
        assert!(results.iter().any(|r| r.kind == MechanicalKind::CardDraw));
    }

    #[test]
    fn resolve_deck_from_tags_picks_truth() {
        let elements = sample_elements();
        assert_eq!(
            resolve_deck_from_tags(&["truth".to_string()], &elements),
            Some("truth".to_string())
        );
    }
}
