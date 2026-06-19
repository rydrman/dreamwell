# Agent instructions

Before committing any changes, run:

```bash
make validate
```

This runs `fmt-check`, `clippy`, `test` (Rust unit/integration), and `e2e` (Playwright tab-resume tests for chats and games) — the same checks as CI.

Game mode code lives under `crates/server/src/game_*.rs`, `crates/server/src/routes/games.rs`, and `crates/frontend/src/game_*.rs`. See `docs/game-mode-plan.md` for the turn pipeline and phased build plan.

For a fast Rust-only loop during development:

```bash
make test
```

If formatting fails, run `make fmt` and re-run `make validate`.

To install the repo pre-commit hook locally:

```bash
make install-hooks
```
