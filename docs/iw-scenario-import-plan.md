# Infinite Worlds Scenario Import — Development Plan

Import community worlds from [Infinite Worlds](https://infiniteworlds.app) into
Dreamwell **scenarios**, then play them in **game mode** with clearer mechanics
than IW itself: real server-side dice, visible turn phases, and schema-validated
state — while preserving rich narrative setup (cast, rules, board-game loops).

> Status: **planning**. No implementation yet. Reference export:
> Shapes & Sizes v4.2 (`schemaVersion: 2.1`).

---

## 1. Goals

| # | Goal |
|---|------|
| 1 | **Import IW JSON** into reusable Dreamwell scenarios with faithful structure (not one giant text blob). |
| 2 | **Play mechanical scenarios better than IW** — especially board/card dice — by rolling in Rust, not asking the LLM to honor `<<1d6>>` placeholders. |
| 3 | **Keep our strengths** — PbtA 2d6 drama checks with visible tiers, persisted rolls, prose regeneration without re-roll. |
| 4 | **Defer IW features we don't need** — suggested action buttons, image generation pipeline, full trigger DSL clone. |

### Non-goals (v1)

- Scraping or API-fetching worlds from infiniteworlds.app (manual JSON export only).
- Pixel-perfect reproduction of every IW trigger effect type.
- NSFW image prompt import or in-game illustration.

---

## 2. What we learned from Shapes & Sizes

The export (`schemaVersion: 2.1`) differs from the generic IW wiki in important
ways:

| Finding | Implication for us |
|---------|------------------|
| `instructions` is **empty** | Rules live in **`instructionBlocks[]`** (named sections: Game Mechanics, Cards, Gameplay, Size Rules, etc.). |
| No `NPCs[]` — uses **`loreBookEntries[]`** | Cast is keyword-triggered lore (8 friends), not a formal NPC table. |
| **`charSelectText`** + setup tracked items | Pre-game: pick 3 invitees and relationships (`Character1/2/3`). |
| **`trackedItems[]`** (13 vars) | Board position, arousal, card queues, truth spaces — each with update hints and visibility. |
| **`triggerEvents[]`** | Conditional pivots (arousal threshold, leader position ≥ 30 swaps card deck + setting). |
| **`victoryCondition`** | Explicit win at board space 80. |
| **`secretInfo` instruction block** | GM planning spec: game state, next `1d6`, next card, NPC decision tree — but IW leaves actual rolling to the AI. |
| World-specific **`skills[]`** | Boldness, Curiosity, Persuasion, Experimentation, Adaptability — not our default PbtA five. |
| Two dice systems in prose | **1d6 board movement** + **1d6 card effect** — separate from social/skill uncertainty. |

**Core IW weakness this world exposes:** mechanical fidelity depends on the AI
honestly rolling dice and updating a dozen tracked variables in freeform
`secretInfo`. Our server-side roll + typed state pipeline is the differentiator.

---

## 3. Design thesis: two roll lanes

```
Player action
   │
   ├─► SYSTEM ROLLS (new)          board die, card die, lotteries
   │      Rules / Plan phase declares what to roll
   │      Rust rolls 1d6 / NdM — no PbtA tier
   │      Lookup table → numeric state delta
   │
   ├─► DRAMA CHECKS (existing)     persuasion, hesitation, social friction
   │      LLM declares check → Rust 2d6 + trait → tier
   │
   └─► RESOLVE + PROSE              state deltas + narrative (rolls canonical)
```

IW conflates both into narration. We separate **scenario mechanics** (system
rolls) from **character drama** (PbtA checks).

---

## 4. Locked decisions

| # | Decision | Choice |
|---|----------|--------|
| 1 | Import source | **Manual JSON upload** (`POST /api/scenarios/import-iw`). No scraping. |
| 2 | Rules storage | **`rules_blocks: Vec<{name, content}>`** on scenario — preserve IW `instructionBlocks` structure. |
| 3 | Cast model | **`cast[]`** from `loreBookEntries`; **`pc_options[]`** from `possibleCharacters`. |
| 4 | Traits | **Scenario-defined `trait_defs[]`**; fall back to PbtA defaults when empty. |
| 5 | State seeding | **`state_schema[]`** on scenario → seed `game_state_entries` at game creation. |
| 6 | System rolls | **New `game_turn_system_rolls` table** — same visibility as skill checks, no tier. |
| 7 | Plan phase | **Structured JSON** (GM plan) before rolls — replaces IW `secretInfo` prose planning. |
| 8 | Suggested actions | **Skip v1** — freeform player input only; optional turn-template chips later. |
| 9 | Images | **Skip v1** — drop IW image fields on import. |

---

## 5. Schema changes

### 5.1 Scenario template (new / extended columns)

Migration `025_iw_scenario_fields.sql` (number TBD at implementation):

```sql
ALTER TABLE scenarios ADD COLUMN rules_blocks TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scenarios ADD COLUMN objective TEXT NOT NULL DEFAULT '';
ALTER TABLE scenarios ADD COLUMN setup_text TEXT NOT NULL DEFAULT '';
ALTER TABLE scenarios ADD COLUMN trait_defs TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scenarios ADD COLUMN cast_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scenarios ADD COLUMN pc_options_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scenarios ADD COLUMN state_schema_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scenarios ADD COLUMN win_condition_json TEXT;
ALTER TABLE scenarios ADD COLUMN content_flags_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE scenarios ADD COLUMN source_meta_json TEXT;
ALTER TABLE scenarios ADD COLUMN scenario_triggers_json TEXT NOT NULL DEFAULT '[]';
```

Rust types in `dreamwell-types` (sketch):

```rust
pub struct RulesBlock { pub name: String, pub content: String }

pub struct ScenarioNpc {
    pub name: String,
    pub content: String,
    pub keywords: Vec<String>,
}

pub struct PcOption {
    pub name: String,
    pub description: String,
    pub traits: HashMap<String, i64>,
    pub portrait_url: Option<String>,
    pub setup_vars: Vec<SetupVarChoice>,
}

pub struct TrackedVarDef {
    pub key: String,
    pub kind: StateKind,
    pub description: String,
    pub initial_value: String,
    pub initial_num: Option<i64>,
    pub visibility: String,
    pub update_hints: String,
}

pub struct WinCondition {
    pub condition: String,
    pub epilogue_text: String,
}

pub struct ContentFlags {
    pub mature: bool,
    pub nsfw: bool,
    pub warnings: Vec<String>,
}

pub struct SourceMeta {
    pub platform: String,
    pub schema_version: f64,
    pub original_version: String,
}
```

Extend `Scenario`, `ScenarioCreate`, `ScenarioUpdate` accordingly.

### 5.2 Game runtime (system rolls)

Migration `026_game_system_rolls.sql`:

```sql
CREATE TABLE game_turn_system_rolls (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    turn_id     INTEGER NOT NULL REFERENCES game_turns(id) ON DELETE CASCADE,
    label       TEXT NOT NULL,
    dice_expr   TEXT NOT NULL DEFAULT '1d6',
    rolls       TEXT NOT NULL DEFAULT '[]',
    outcome_key TEXT NOT NULL DEFAULT '',
    outcome_summary TEXT NOT NULL DEFAULT '',
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL
);
```

Extend `GameTurn` with `system_rolls: Vec<GameTurnSystemRoll>`.

### 5.3 Structured plan artifact

Store on `game_turns` as JSON or a side table:

```rust
pub struct TurnPlan {
    pub round: Option<i64>,
    pub active_player: Option<String>,
    pub board_positions: HashMap<String, i64>,
    pub card_drawn: Option<String>,
    pub system_rolls_needed: Vec<SystemRollRequest>,
    pub npc_decision_summary: Option<String>,
}

pub struct SystemRollRequest {
    pub label: String,
    pub dice_expr: String,
    pub purpose: String,  // "board" | "card" | "lottery"
}
```

---

## 6. IW import mapping

Endpoint: `POST /api/scenarios/import-iw` (multipart `file`).

| IW field | Dreamwell field | Notes |
|----------|-----------------|-------|
| `title` | `title` | Direct |
| `background` + `objective` | `premise` | Hook + win goal |
| `description` | append to `premise` or `setup_text` | Player blurb / changelog |
| `instructionBlocks[]` | `rules_blocks` | Preserve names |
| `authorStyle` + `descriptionRequest` + "Writing Style" block | `gm_style` | Voice + output constraints |
| `firstInput` | `opening_message` | May contain `<<character1>>` macros |
| `charSelectText` | `setup_text` | Pre-play configuration |
| `possibleCharacters[]` | `pc_options` | Skills → traits; setup tracked items |
| `loreBookEntries[]` | `cast` | Keywords + content |
| `skills[]` | `trait_defs` | Override PbtA defaults when non-empty |
| `trackedItems[]` | `state_schema` | Map dataType, initialValue, updateInstructions |
| `victoryCondition` | `win_condition` | |
| `triggerEvents[]` | `scenario_triggers` | Simplified (see §8) |
| `mature`, `nsfw`, `contentWarnings` | `content_flags` | |
| `schemaVersion`, `version` | `source_meta` | |
| Image fields | *dropped* | |

Implementation: `crates/dreamwell-types/src/iw_import.rs` +
`crates/server/src/scenario_import.rs` + `routes/scenarios.rs`.

---

## 7. Turn pipeline changes

Current pipeline (`game_turn.rs`):

```
DECLARE CHECKS → ROLL (2d6) → RESOLVE + STATE → PROSE
```

Target pipeline for mechanical scenarios:

```
PLAN (JSON) → SYSTEM ROLLS (1d6/…) → DECLARE CHECKS (optional) → ROLL (2d6) → RESOLVE + STATE → PROSE
```

### PLAN (new)

- **Input:** player action, `rules_blocks`, current state, cast, turn history.
- **Output:** `TurnPlan` — active player, system rolls needed, card/space logic.
- **Prompts:** inject relevant `rules_blocks` by name, not one blob.

### SYSTEM ROLLS (new)

- `roll_dice(expr, 0)` — **no tier**.
- Persist to `game_turn_system_rolls`; distinct UI from 2d6 checks.
- v1: Resolve applies outcomes using rules text; v2: structured card decks.

### DECLARE CHECKS / ROLL (unchanged)

- Validate against scenario `trait_defs`.
- Skip when Plan reports no dramatic uncertainty.

### RESOLVE + STATE (extended)

- Apply board position, arousal, measurements from plan + system rolls.
- Evaluate simplified triggers; check `win_condition`.

### PROSE (unchanged)

- Honor system roll faces and 2d6 tiers; no re-rolling.

### Shapes & Sizes example

```
Player: "I roll and take my turn"
  → Plan: active player, board 1d6, Transformation card
  → System roll: [4] → card table outcome
  → Drama check: (optional) Boldness
  → Resolve: update state
  → Prose: narrate (rolls fixed)
```

---

## 8. Simplified scenario triggers

```rust
pub struct ScenarioTrigger {
    pub name: String,
    pub conditions: Vec<TriggerCondition>,
    pub effects: Vec<TriggerEffect>,
}

pub enum TriggerEffect {
    InjectRulesBlock { block_name: String },
    SetState { key: String, value: String },
    AppendGmInstruction { text: String },
}
```

Shapes & Sizes targets:

| Trigger | Condition | Effect |
|---------|-----------|--------|
| Arousal interrupt | `Most_Aroused >= 93` AND `Sexual interaction == No` | Inject pause-game GM instruction |
| Change cards | `Game_Leader >= 30` | Swap Cards block; island pivot rules |

---

## 9. Game creation from IW scenario

1. Copy text fields (existing behavior).
2. Setup wizard if `pc_options` + `setup_text` present.
3. Create PC from chosen option; NPC actors from cast selections.
4. Seed `game_state_entries` from `state_schema`.
5. Snapshot `rules_blocks` + `win_condition` onto game row.

---

## 10. UI work

| Area | Change |
|------|--------|
| Scenarios | Import IW JSON; content-warning badge |
| Scenario editor | Rules blocks; cast; trait defs |
| Setup wizard (new) | `setup_text` + PC + 3 cast picks |
| Game turn UI | Plan bubble; system rolls in Roll phase |
| State sidebar | Labels from `state_schema` |
| Cast panel | NPC lore on click |

---

## 11. Module / file plan

| File | Role |
|------|------|
| `crates/dreamwell-types/src/iw_import.rs` | `iw_world_to_scenario` |
| `crates/dreamwell-types/src/iw_types.rs` | IW JSON serde subset |
| `crates/server/migrations/025_*.sql` | Scenario columns |
| `crates/server/migrations/026_*.sql` | System rolls |
| `crates/server/src/scenario_db.rs` | CRUD for new fields |
| `crates/server/src/routes/scenarios.rs` | `import-iw` route |
| `crates/server/src/game_turn.rs` | Plan + system roll phases |
| `crates/server/src/game_prompts.rs` | Rules block injection |
| `crates/server/src/game_resolution.rs` | System roll helper |
| `crates/frontend/src/scenario_ui.rs` | Import + editor |
| `crates/frontend/src/game_setup_ui.rs` | Setup wizard |
| `crates/frontend/src/game_ui.rs` | Plan + system roll UI |

---

## 12. Implementation phases

### Phase A — Import fidelity (P0)

Import Shapes & Sizes as a scenario; playable with existing loop.

| Task | Description |
|------|-------------|
| A.1 | `iw_types.rs` serde structs |
| A.2 | `iw_import.rs` mapping (§6) |
| A.3 | Migration + `Scenario` extensions |
| A.4 | `POST /api/scenarios/import-iw` |
| A.5 | Frontend import button + rules block viewer |
| A.6 | Unit test with Shapes & Sizes fixture |

### Phase B — Party setup & state seeding (P1)

| Task | Description |
|------|-------------|
| B.1 | Setup wizard UI |
| B.2 | Resolve `<<characterN>>` macros in opening |
| B.3 | NPC actors from cast at game start |
| B.4 | Seed state from `state_schema` |
| B.5 | `trait_defs` in check validation |

### Phase C — System rolls & plan phase (P2)

| Task | Description |
|------|-------------|
| C.1 | `game_turn_system_rolls` migration |
| C.2 | Plan phase LLM + JSON schema |
| C.3 | System roll execution in Rust |
| C.4 | UI: Plan + system roll bubbles |
| C.5 | Resolve consumes plan + rolls |
| C.6 | Card lookup from rules block |

### Phase D — Triggers & win condition (P3)

| Task | Description |
|------|-------------|
| D.1 | Import simplified triggers |
| D.2 | Evaluate after state resolve |
| D.3 | Win check + epilogue |
| D.4 | Shapes & Sizes pivots |

### Phase E — Polish (P4)

| Task | Description |
|------|-------------|
| E.1 | Cast panel |
| E.2 | Step mode for Plan / system rolls |
| E.3 | Prose length presets |
| E.4 | Native scenario export |

---

## 13. Testing strategy

| Level | Coverage |
|-------|----------|
| Unit | `iw_import`; `roll_dice("1d6")`; trait validation; triggers |
| Integration | Import endpoint; game create with seeded state |
| Fixture | `tests/fixtures/shapes_sizes_v4_2.json` |
| E2E | Import → setup → turn with Plan/rolls (Phase C+) |

Run `make validate` before merge per `AGENTS.md`.

---

## 14. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| IW schema drift | `source_meta.schema_version`; versioned mapper |
| Card rules too complex to parse | v1: LLM + rules text in Resolve; v2: structured decks |
| Prompt token bloat | Inject only relevant `rules_blocks` per phase |
| NSFW content | `content_flags` + UI warning |
| 4-player board vs single PC | NPC actors + Plan tracks board turn order |

---

## 15. References

- `docs/game-mode-plan.md` — turn pipeline (implemented)
- `crates/dreamwell-types/src/game_import.rs` — character → scenario import
- [IW JSON export (wiki)](https://infiniteworlds.mywikis.wiki/wiki/Misc_Advanced_Features)
- [IW Game Creation Guide](https://github.com/sabreking/IWGameCreationGuide)

---

## 16. Priority summary

| Priority | Phases | Outcome |
|----------|--------|---------|
| **P0** | A | Import Shapes & Sizes; edit rules blocks |
| **P1** | B | Party setup + seeded state |
| **P2** | C | Real 1d6 board/card dice |
| **P3** | D | Mid-game pivots + win condition |
| **P4** | E | Polish |
