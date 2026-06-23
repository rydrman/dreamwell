# Gameplay Elements

Server-authoritative board-game mechanics (board moves, deck draws, dice rolls) with three engine modes for local comparison.

## Element types

### Board

- `id`, `spaces`, `move_dice` (e.g. `1d6`)
- `tag_rules`: map tag → space numbers (e.g. `truth` → `[8, 11, 14, …]`)
- Default tag when no rule matches (e.g. `transformation`)

### Deck

- `id`, `consume_on_draw`, `cards[]`
- Each card: `id`, `name`, `text`, `requires_roll`

### Game snapshot

- `game_elements`: static defs + `turn_mechanicals` step template
- `element_instances`: runtime board positions, deck draw piles

## Mechanical steps (bulk execution)

Executed in order by `execute_mechanicals()`:

1. `board_move` — roll, update position, emit `space_tags`
2. `card_draw` — `deck_from: space_tag` selects deck; optional `consume`
3. `dice_roll` — effect roll when last card has `requires_roll`

Results persist on `GameTurn.mechanical_results` and feed cumulative prompts.

## Engine modes

| Mode | ID | Mechanics | LLM phases |
|------|-----|-----------|------------|
| Pipeline | `pipeline` | Bulk after plan | Plan → checks → resolve → prose |
| Tools mechanics | `tools_mechanics` | Tool loop after plan | Same as pipeline |
| Tools structured | `tools_structured` | Full tool loop | Prose only |

Prose always streams last in all modes.

## Comparison metrics

Per turn (see `TurnObservability`):

- `engine_mode`, `llm_call_count`, `tool_call_count`, `tool_iterations`
- `mechanical_results` summary
- Phase timings when logged

## IW import

- Cards parsed from `instructionBlocks["Cards and probabilities"]`
- Board from Game Mechanics + `Truth_Spaces` tracked item
- Instances seeded at game create (shuffled decks, positions at 0)

## Notes

- `scenario_triggers` remain dormant in v1 (not evaluated at runtime).
- Legacy `Transformation_card_drawn` state facts are deprecated once mechanical results exist.
- System rolls table kept for UI compat; dice mechanicals also populate `system_rolls` during transition.
