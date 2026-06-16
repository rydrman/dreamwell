# Agent instructions

Before committing any changes, run:

```bash
make validate
```

This runs `fmt-check`, `clippy`, `test` (Rust unit/integration), and `e2e` (Playwright tab-resume tests) — the same checks as CI.

For a fast Rust-only loop during development:

```bash
make test
```

If formatting fails, run `make fmt` and re-run `make validate`.

To install the repo pre-commit hook locally:

```bash
make install-hooks
```
