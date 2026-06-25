# Game Mode — Development Plan

A new top-level **Game** mode for Dreamwell: a tabletop-RPG-style interactive
roleplay loop. The player proposes an action; the system resolves it through a
sequenced, multi-phase pipeline that mixes LLM reasoning with real backend dice
logic, then renders the result as prose. Game mode is a **new peer mode**
alongside Chats and Stories, reusing the existing queue, inference, SSE,
summarization, and autosave infrastructure.

> Status: **planning only**. This document is the agreed design. No code is
> built yet.

---

## 1. Locked decisions

| # | Decision | Choice |
|---|----------|--------|
| 1 | Resolution system | **2d6 / PbtA-style tiers** (fail ≤6 / mixed 7–9 / strong 10+). Engine kept generic enough to later support d20-vs-DC. |
| 2 | Difficulty / check authoring | **LLM-proposed.** The model declares which skill is tested, the modifier, the stakes, and a justification. The rules layer validates and clamps; the dice roll itself is always real and server-side. |
| 3 | State scope (v1) | **Minimal.** Resources (numeric, current/max, clamped), conditions/tags, freeform durable facts, and clocks (progress trackers). No full skill trees or inventory systems in v1. |
| 4 | Actors | **Single player character** for v1, but the schema is **extensible** to a party + NPCs (actor table with a role, not a single hardcoded sheet). |
| 5 | Build approach | **New `games` mode.** Share code aggressively (queue, inference, summarize utils, variable-parse helpers, SSE patterns, autosave) but do not overload stories. |
| 6 | Turn UX | **Auto-chained pipeline** with **incremental output**: each phase emits its own expandable bubble that stacks in the turn, streaming in as it completes. Final prose streams token-by-token. |

---

## 2. Why this fits the existing architecture

Game mode is essentially **Stories-with-an-interactive-turn-loop + real dice +
typed state**. The hard machinery already exists:

- **Layered artifacts before prose.** Stories already does
  `synopsis → mechanical (structured plan) → prose`
  (`story_beat_mechanical.rs`, `story_prompts.rs`). Our Phase 3 "scene bullets"
  is the mechanical-beat analog; Phase 4 "prose" is `StoryBeatProse`.
- **Condensed memory.** `story_summarize.rs` produces a dense
  `prose_summary` per chapter with a validity flag
  (`015_story_chapter_prose_summary.sql`); we reuse this pattern for scene
  compression.
- **Replayable, revertible state.** Story variables are replayed from a beat
  audit trail and reverted on regeneration
  (`revert_beat_variable_updates` in `stories_db.rs`,
  `variable_state.rs::story_state_at`). Game state deltas reuse this revert
  pattern.
- **Queue + SSE + streaming.** One job per user action, streamed to the DB and
  pushed over SSE (`queue.rs`, `routes/stories.rs` `stream_story`,
  `api.rs` `StoryStream`). Game turns reuse this exactly.

### The single most important improvement

Today, variables only update when the model *opportunistically* emits
`<var key="...">value</var>` tags mid-prose (`variables.rs::parse_variable_updates`,
`story_variables.rs`). That is why "variable usage isn't reliable." In Game
mode, **state changes become a mandatory, schema-validated pipeline phase** that
operates on the current typed state and returns explicit deltas. Reliability no
longer depends on the model remembering to emit tags.

---

## 2b. Implemented turn engine (`EngineMode::ToolsStructured`)

The shipped agent (`game_turn_agent.rs`) runs each turn as **three sequenced
LLM passes**, which is a refinement of §3's pipeline:

1. **Declare + roll checks** (`declare_and_roll_checks`) — JSON phase; dramatic
   checks are rolled in Rust.
2. **Mechanics resolution** (`build_mechanics_agent_messages`,
   `mechanics_agent_tool_specs`) — the model calls *only* the scenario mechanic
   tools (`roll_dice` / `board_move` / `draw_card`) plus `ask_pc_decision`, with
   **no prose**. Every dice/board/card outcome is therefore server-decided
   before any narration exists.
3. **Prose narration** (`build_prose_narration_messages`,
   `prose_agent_tool_specs`) — the model writes the turn prose from a
   "Resolved mechanics (canonical)" block (each line includes its `⟦mech:N⟧`
   reference) and embeds those markers at the narration point. The frontend
   expands markers into inline dice/card/board blocks from the canonical
   `mechanical_results`, so the player discovers outcomes as they read without
   the model restating numbers in prose. State / ask tools still run after the
   prose; outcome tools are not offered and any the model emits anyway are dropped.

**Why the split:** a single inline pass let weaker local models narrate a
fabricated dice result first ("you roll a 4…") and then emit a contradicting
`roll_dice` tool call, so the prose never matched the real roll. Resolving all
mechanics *before* narration removes the contradiction entirely. Inline markers
restore the "outcome as you read" feel: the marker is only a pointer — the UI
renders the real result from `mechanical_results`, not from model text. If the
model omits a marker, `ensure_inline_mech_markers` appends any missing ones so
the detached "Mechanics" panel stays hidden when possible.
See `crates/server/src/game_repro.rs` for the live reproduction/regression
harness (run with `--ignored`).

## 3. The turn pipeline

A single user action ("I try to pick the lock before the guard returns")
resolves through one **turn job** that sequences phases internally. The dice
roll happens in Rust *between* LLM calls so it is always real. Each phase
persists its artifact and pushes an SSE update so the UI can render incremental
bubbles.

```
Player action
   │
   ▼
Phase 1  DECLARE CHECKS        LLM → JSON: [{skill, modifier, stakes, justification, ...}]
   │                            (may be empty → no roll, pure narration)
   ▼
ROLL     (pure Rust)           2d6 + modifier per check → tier (fail/mixed/strong) + margin + seed
   │
   ▼
Phase 2  RESOLVE + STATE       LLM → JSON: typed state deltas (set/add/remove on resources,
   │     (merged 2+3 by         conditions, facts, clocks) + scene beats (bullet list)
   │      default for speed)    validated & clamped server-side
   ▼
Phase 3  SCENE PLAN            (bullets; merged into Phase 2 call by default, separable via setting)
   │
   ▼
Phase 4  PROSE                 LLM (streaming) → narrative rendering of the resolved beats
   │
   ▼
Turn complete → optional scene summary refresh
```

### Phase details

#### Phase 1 — Declare checks (`GameTurnCheck`)

- **Input:** player action, current typed state (sheet + resources + conditions
  + active clocks), recent turn context, world/system rules.
- **Output (strict JSON):**
  ```json
  {
    "checks": [
      {
        "label": "Pick the lock under time pressure",
        "skill": "Finesse",
        "modifier": 1,
        "stakes": "Fail: the guard returns and raises the alarm.",
        "justification": "Delicate manual task with a ticking clock."
      }
    ],
    "no_check_reason": null
  }
  ```
- `checks` may be **empty** (with `no_check_reason` set) → skip the roll, go
  straight to resolve. Needed for pure social/narrative actions and pacing.
- The model proposes `skill` and `modifier`; the **rules layer validates**
  (skill must exist on the sheet or map to a default) and **clamps** the
  modifier to a configured range.

#### Roll (pure Rust — `game_resolution.rs`)

- For each declared check: roll `2d6`, add the (validated) modifier, derive a
  **tier** and **margin**:
  - `total <= 6` → **fail**
  - `7 <= total <= 9` → **mixed** (success at a cost)
  - `total >= 10` → **strong** (full success)
  - `12+` (natural boon) and `2` (natural snag) flagged for optional flavor.
- Persist the **seed**, raw dice, modifier, total, tier, and margin so the turn
  is replayable/explainable and prose regeneration never re-rolls.
- Engine is generic: a `ResolutionSystem` enum keeps the door open for
  d20-vs-DC later, but v1 ships only `Pbta2d6`.

#### Phase 2 — Resolve & state delta (`GameTurnResolve`)

- **Input:** action, declared checks **with their resolved tiers/margins**,
  current typed state.
- **Output (strict, schema-validated JSON):**
  ```json
  {
    "scene_beats": [
      "The lock clicks open just as footsteps approach.",
      "You slip inside but leave the door ajar in your haste."
    ],
    "state_changes": [
      {"target": "pc", "kind": "resource", "key": "stress", "op": "add", "delta": 1},
      {"target": "pc", "kind": "condition", "key": "hidden", "op": "set", "value": "true"},
      {"target": "world", "kind": "clock", "key": "alarm", "op": "add", "delta": 1},
      {"target": "world", "kind": "fact", "key": "warehouse_side_door", "op": "set", "value": "unlocked"}
    ]
  }
  ```
- **Server validation & clamping:** resource deltas clamp to `[0, max]`;
  clock ticks clamp to `[0, segments]`; unknown targets/keys are rejected or
  created per policy. Every applied change stores `prev_value` for undo.
- By default this phase **also** returns `scene_beats` (Phase 3 merged in to
  save one round-trip on local models). A setting can split them.

#### Phase 3 — Scene plan (bullets)

- Merged into Phase 2 by default. When split (setting), it becomes a separate
  `GameTurnScenePlan` job that turns the resolved state changes + tiers into an
  ordered bullet list — directly analogous to `StoryBeatMechanical`.

#### Phase 4 — Prose (`GameTurnProse`, streaming)

- **Input:** scene beats, the resolved outcome tiers, current state (post-delta),
  recent prose, scene summary.
- **Output:** narrative prose, streamed token-by-token to the DB and SSE
  (reusing `stream_chat_completion` + `update_*` pattern from
  `run_beat_prose_generation_attempt` in `queue.rs`).
- The prose must honor the resolved tiers (a "fail" cannot be narrated as a
  clean success), enforced by prompt scope guards and an optional recheck pass.

### Auto-chaining and incremental output

Stories never auto-chains jobs (the user clicks each phase). Game mode **does**
auto-chain because a player expects a turn to resolve. Design:

- **One logical turn job** drives all phases in sequence inside the worker, with
  the Rust roll between Phase 1 and Phase 2.
- Each phase **persists its artifact immediately** and bumps the turn's
  `phase` marker, so the SSE payload reflects progress.
- The frontend renders **stacked, expandable bubbles** per phase:
  `Checks → Roll → State changes → Scene → Prose`. Each bubble appears as its
  phase completes; the prose bubble streams. Collapsed by default except the
  active/prose bubble.
- **Optional step mode** (setting / dev toggle): pause after each phase so the
  user (acting as GM) can inspect or edit the check, the roll, or the state
  delta before continuing. Cheap because artifacts already persist per phase.

---

## 4. Data model

New migration(s) under `crates/server/migrations/` (next sequential numbers).
SQLite, matching existing conventions (text timestamps, `ON DELETE CASCADE`).

### `games`
```sql
CREATE TABLE games (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    title           TEXT NOT NULL DEFAULT 'Untitled Game',
    premise         TEXT NOT NULL DEFAULT '',     -- world / scenario seed
    setting         TEXT NOT NULL DEFAULT '',     -- tone, genre, world rules
    gm_style        TEXT NOT NULL DEFAULT '',     -- GM voice / pacing notes
    resolution_system TEXT NOT NULL DEFAULT 'pbta_2d6',
    modifier_min    INTEGER NOT NULL DEFAULT -2,
    modifier_max    INTEGER NOT NULL DEFAULT 3,
    merge_resolve_scene INTEGER NOT NULL DEFAULT 1, -- Phase 2+3 merged
    step_mode       INTEGER NOT NULL DEFAULT 0,     -- pause between phases
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
```

### `game_actors` (extensible; v1 seeds exactly one PC)
```sql
CREATE TABLE game_actors (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'pc',  -- 'pc' | 'npc' | 'party'
    name        TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    skills      TEXT NOT NULL DEFAULT '{}',  -- JSON: { "Finesse": 1, "Force": 0, ... }
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
```
> v1 UI only exposes the single `pc` actor, but queries are actor-aware so a
> party/NPCs drop in without a migration.

### `game_state_entries` (typed, scoped, revertible)
```sql
CREATE TABLE game_state_entries (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    actor_id      INTEGER REFERENCES game_actors(id) ON DELETE CASCADE, -- NULL = world scope
    kind          TEXT NOT NULL,            -- 'resource' | 'condition' | 'fact' | 'clock'
    key           TEXT NOT NULL,
    value         TEXT NOT NULL DEFAULT '', -- string value / fact text / condition flag
    num_value     INTEGER,                  -- resources & clocks: current
    max_value     INTEGER,                  -- resources: cap; clocks: segments
    source_turn   INTEGER NOT NULL DEFAULT -1, -- -1 = manual / initial
    updated_at    TEXT NOT NULL,
    UNIQUE(game_id, actor_id, kind, key)
);
```

### `game_turns` (analog of a beat)
```sql
CREATE TABLE game_turns (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    player_action TEXT NOT NULL DEFAULT '',
    phase         TEXT NOT NULL DEFAULT 'pending', -- pending|checks|rolled|resolved|scene|prose|done|failed
    scene_beats   TEXT NOT NULL DEFAULT '[]',      -- JSON bullet list (Phase 3)
    prose         TEXT NOT NULL DEFAULT '',        -- Phase 4 (streamed)
    state_changes TEXT NOT NULL DEFAULT '[]',      -- applied deltas (audit + revert)
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
```

### `game_turn_checks` (declared + rolled)
```sql
CREATE TABLE game_turn_checks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    turn_id       INTEGER NOT NULL REFERENCES game_turns(id) ON DELETE CASCADE,
    label         TEXT NOT NULL DEFAULT '',
    skill         TEXT NOT NULL DEFAULT '',
    modifier      INTEGER NOT NULL DEFAULT 0,    -- post-clamp
    stakes        TEXT NOT NULL DEFAULT '',
    justification TEXT NOT NULL DEFAULT '',
    dice_expr     TEXT NOT NULL DEFAULT '2d6',
    seed          INTEGER NOT NULL DEFAULT 0,
    rolls         TEXT NOT NULL DEFAULT '[]',     -- raw dice JSON, e.g. [4,5]
    total         INTEGER NOT NULL DEFAULT 0,
    tier          TEXT NOT NULL DEFAULT '',       -- fail | mixed | strong
    margin        INTEGER NOT NULL DEFAULT 0,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL
);
```

### `game_scenes` (memory compression — reuse story summary pattern)
```sql
CREATE TABLE game_scenes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id         INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    title           TEXT NOT NULL DEFAULT '',
    summary         TEXT NOT NULL DEFAULT '',
    summary_valid   INTEGER NOT NULL DEFAULT 0,
    summary_at      TEXT,
    start_turn      INTEGER NOT NULL DEFAULT 0,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
```
> v1 can keep a single implicit scene per game and add scene-breaking later;
> the table exists so summary compression has a home from day one.

### `generation_jobs` extension
Add nullable columns `game_id` and `turn_id` (mirroring the existing
`story_id`/`chapter_id`/`beat_id` columns in `002_stories.sql`) plus the new
`job_type` values. Per-turn exclusivity via a
`COUNT(*) WHERE turn_id = ? AND status IN ('queued','running')` guard, exactly
like `has_active_beat_job`.

---

## 5. Shared types (`crates/dreamwell-types/src/lib.rs`)

Add alongside the existing `JobType` enum (currently lines ~20–34) and `Story*`
structs (~296–376):

- `enum ResolutionSystem { Pbta2d6 }` (extensible).
- `enum CheckTier { Fail, Mixed, Strong }`.
- `enum StateKind { Resource, Condition, Fact, Clock }`.
- `enum StateOp { Set, Add, Remove }`.
- Structs: `Game`, `GameActor`, `GameStateEntry`, `GameTurn`, `GameTurnCheck`,
  `GameScene`, `GameDetail { game, actors, state, turns, scenes }`,
  `GameStreamPayload { detail: GameDetail, active_job: Option<Job> }`.
- New `JobType` variants:
  `GameTurnCheck`, `GameTurnResolve`, `GameTurnScenePlan` (split mode only),
  `GameTurnProse`, `GameSceneSummarize`, and optional rechecks
  `GameProseRecheck`, `GameStateRecheck`.
- Extend `Job` with optional `game_id` / `turn_id`.

The check-declaration and state-delta JSON wire formats get dedicated
`#[derive(Serialize, Deserialize)]` request/response structs so parsing is
schema-checked rather than ad-hoc.

---

## 6. Server modules

Mirror the stories layout under `crates/server/src/`:

| New file | Mirrors | Responsibility |
|----------|---------|----------------|
| `game_db.rs` | `stories_db.rs` | CRUD for games/actors/turns/state/scenes; `prepare_*` gates; `enqueue_game_job`; per-turn job lock; `game_from_row` building `GameDetail`. |
| `game_resolution.rs` | (new) | Pure dice engine: dice-expr parse, seeded 2d6 roll, modifier clamp, tier + margin. Fully unit-tested, no LLM/DB. |
| `game_prompts.rs` | `story_prompts.rs` | Build messages for each phase with tight scope guards (declare-checks, resolve+state, scene plan, prose, scene summary). |
| `game_turn.rs` | `story_beat_mechanical.rs` + `queue.rs` prose path | The turn-job orchestrator: sequences phases, calls the roll, validates/clamps/applies state deltas, streams prose, advances `phase`, handles step-mode pauses. |
| `game_state.rs` | `variable_state.rs` + `story_variables.rs` | Apply / clamp / revert typed state deltas; compute current state; build the state block injected into prompts. |
| `game_summarize.rs` | `story_summarize.rs` | Scene summary compression + validity invalidation. |
| `routes/games.rs` | `routes/stories.rs` | REST endpoints + `/api/games/{id}/stream` SSE. |

Wire-ups:
- `queue.rs` `run_job` (dispatch ~lines 386–421): route the new `JobType`s to
  `game_turn.rs`. The turn job is a **single job** that internally walks phases;
  the roll is Rust between LLM calls. Prose streams via the existing streaming
  path. Reuse the JSON fence-strip + retry pattern already in
  `story_beat_mechanical.rs` for the structured phases.
- Per-story serialization analog: only one active job per `turn_id`; turns
  within a game are processed FIFO.

### Inference reliability (foundational)

Extend `inference.rs` `chat_completion` (line ~283) with an **optional
`response_format` / grammar** parameter and add a JSON-schema-validated parse +
**repair retry** (feed the parse error back to the model). llama.cpp (GBNF),
vLLM, and recent Ollama all support constrained JSON output; this is the
highest-leverage reliability work for the structured phases and is reused by the
existing JSON phases in stories. Keep the current behavior when no format is
requested.

---

## 7. Memory model

Layered context budget built in `game_prompts.rs` (reusing the summarize +
invalidation pattern from stories):

1. **Pinned (always in context):** PC sheet (name, description, skills),
   current resources, active conditions, active clocks, world/system rules.
2. **Recent turns verbatim:** last *N* turns' player actions + prose (token
   budget like `build_beat_prose_messages`, oldest dropped first).
3. **Rolling scene summary:** dense bullet summary from `game_summarize.rs`
   (analog of `prose_summary`), used once turns scroll out of the verbatim
   window. Invalidated when earlier turns change (mirror
   `invalidate_prose_summaries_from`).
4. **Durable facts:** `kind='fact'` state entries surface relevant world canon
   without replaying full prose. (Optional later: keyword/vector retrieval.)

This directly addresses "improved memory": typed durable state + compressed
scene summaries + a bounded verbatim window, instead of relying on the model to
re-derive everything from raw history.

---

## 8. Frontend (`crates/frontend/src/`)

Mirror the stories wiring (the map of stories' frontend is the template):

| Layer | Stories | Game |
|-------|---------|------|
| Mode enum | `AppMode::Stories` (`queue_ui.rs`) | add `AppMode::Game` |
| Route | `AppRoute::Stories` + `StoryNav` (`router.rs`) | `AppRoute::Games` + `GameNav` + `/games/...` parse/build |
| API | story fns + `StoryStream` (`api.rs`) | game fns + `GameStream` at `/api/games/{id}/stream` |
| Sync helpers | `story_sync.rs` | `game_sync.rs` (SSE replace guards, stale detection) |
| UI shell | `stories_ui.rs` `StoriesShell` | `game_ui.rs` `GameShell` |
| Sidebar | Chats/Stories tabs (`sidebar.rs`) | add a third Game tab |
| Dispatch | `if mode == Stories { <StoriesShell/> }` (`lib.rs`) | add Game branch + `list_games` on boot |
| Notices | `story_notice` (`generation_ui.rs`) | `game_notice` for `GameTurn*` job types |

### `GameShell` UI

- **Turn feed:** chronological list of turns. Each turn renders the player
  action followed by **stacked expandable bubbles** for its phases:
  `Checks` (skill, stakes, justification) → `Roll` (dice faces, total, tier
  badge with color: red fail / amber mixed / green strong) → `State changes`
  (typed deltas with prev→new) → `Scene` (bullets) → `Prose` (streamed).
  Bubbles appear as each phase completes via SSE; collapsed by default except
  the active phase and the final prose.
- **Action composer:** freeform text input at the bottom ("What do you do?") +
  optional guidance — submits a new turn (POST → enqueue → `bump_stream`),
  reusing the guidance + `StreamNudge` pattern from `StoriesShell`.
- **State panel:** side/overlay panel showing the PC sheet, resources (bars
  with current/max), conditions (chips), clocks (segmented), and facts —
  editable manually with autosave (`story_save.rs` `AutoSaveController`).
- **Controls:** regenerate-turn (reverts that turn's state deltas, **no
  re-roll** — the roll is canonical), and a step-mode toggle exposing
  continue/edit buttons between phases when enabled.

---

## 9. Phased build order

Engineering phases, decision-light foundations first. MVP = Phases 0–6.

- **Phase 0 — Types & design lock.** Add enums/structs and `JobType` variants in
  `dreamwell-types`. No behavior yet.
- **Phase 1 — Inference structured output.** Optional `response_format`/grammar
  in `chat_completion`; schema-validated parse + repair-retry helper.
- **Phase 2 — Data model & DB layer.** Migrations + `game_db.rs` CRUD,
  `prepare_*` gates, job enqueue, `GameDetail` assembly. Extend
  `generation_jobs`.
- **Phase 3 — Dice engine.** `game_resolution.rs`, fully unit-tested
  (tier boundaries, seeding/determinism, modifier clamping, dice-expr parsing).
- **Phase 4 — Turn pipeline + prompts.** `game_turn.rs` orchestrator,
  `game_state.rs` apply/clamp/revert, `game_prompts.rs`, `routes/games.rs`,
  `queue.rs` dispatch, SSE endpoint.
- **Phase 5 — Memory.** `game_summarize.rs` + layered context builder +
  invalidation.
- **Phase 6 — Frontend.** `AppMode::Game`, routing, `GameShell` with stacked
  phase bubbles + state panel, `api.rs`, `game_sync.rs`, sidebar tab, `lib.rs`
  dispatch.
- **Phase 7 — Quality & control.** Prose/state recheck passes (mirror
  `story_*_recheck.rs`), undo/regenerate-turn, step-mode UI, difficulty
  clamping policies.
- **Phase 8 — Polish.** Settings (resolution system, phase-merge toggle,
  per-phase model overrides), README/AGENTS docs, unit tests for engine +
  parsers, a Playwright e2e turn-loop test, then `make validate`.

---

## 10. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| Latency/cost: multiple LLM calls per turn on local models | Default-merge Phase 2+3 (3 calls/turn); allow smaller/faster model per structured phase; stream only prose. |
| Small local models produce malformed JSON | `response_format`/grammar constrained decoding (Phase 1 infra) + schema validation + repair retry (existing fence-strip/retry pattern). |
| LLM fudges difficulty to favor success | Rules layer validates skill + **clamps modifier**; dice are always real and server-rolled; prose recheck enforces that narration honors the tier. |
| Prose contradicts the resolved outcome | Scene beats derive from resolved tiers; prose prompt scope-guards on tiers; optional `GameProseRecheck`. |
| State drift / desync | Typed deltas only, clamped, with `prev_value` audit; regenerate reverts deltas (reuse story revert pattern). |
| Scope creep (sheets, inventory, party) | v1 is minimal + single PC; actor table and state kinds are extensible without migration churn. |
| Determinism on regenerate | Roll is persisted (seed + faces + tier); regeneration reuses the stored roll, never re-rolls. |

---

## 11. Open items to revisit during build

- Exact PbtA skill list for the default sheet (or fully freeform skills with a
  default `+0`).
- Whether clocks are global (world) only in v1 or also per-actor.
- Scene-break heuristics for when to start a new `game_scene` and summarize.
- Whether to expose `response_format` capability detection per backend, or just
  attempt-and-fallback.
