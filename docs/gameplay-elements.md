# Gameplay Elements

Server-authoritative board, deck, and dice primitives invoked by the inline-prose agent via tools.

## Element types

### Board

- `id`, `spaces`, `move_dice` (e.g. `1d6`)
- `tag_rules`: map tag → space numbers (e.g. `truth` → `[8, 11, 14, …]`)
- `default_tag` when no rule matches

### Deck

- `id`, `consume_on_draw`, `cards[]`
- Each card: `id`, `name`, `text`

### Game snapshot

- `game_elements`: static board and deck definitions
- `element_instances`: runtime board positions and deck draw piles

## Agent tools (generic primitives)

The structured inline-prose agent calls these tools when scenario rules require mechanics:

| Tool | Purpose |
|------|---------|
| `board_move` | Roll the board move die, advance an actor, return from/to space and `space_tags` |
| `draw_card` | Draw the top card from a named deck (`deck_id` required); returns canonical card text |
| `roll_dice` | Roll a dice expression (e.g. `1d6`); returns rolls and total |

Turn sequencing, deck selection (e.g. mapping space tags to decks), and when to call each tool are defined in the scenario's **rules blocks** — not in the engine.

Results persist on `GameTurn.mechanical_results` and feed cumulative prompts. Inline prose markers (`⟦mech:N⟧`) anchor result blocks in streamed narration.

## Engine mode

All game turns use `tools_structured`: dramatic checks are rolled first, then a single prose pass with inline tool calls for mechanics and state updates.

## Scenario import/export

- Native format: `dreamwell.scenario.v1` JSON documents with a top-level `format` field and scenario fields.
- `/api/scenarios/import` accepts native scenario JSON, SillyTavern-style character JSON, or PNG cards.
- `/api/scenarios/:id/export` downloads a native scenario JSON document.
- Instances seeded at game create (shuffled decks, positions at 0)

## Notes

- `scenario_triggers` remain dormant in v1 (not evaluated at runtime).
- System rolls table kept for UI compat; dice mechanicals also populate `system_rolls`.
