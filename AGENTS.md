# Agent instructions

Before committing any changes, run:

```bash
make validate
```

This runs `fmt-check`, `clippy`, and `test` — the same checks as CI. All three must pass.

If formatting fails, run `make fmt` and re-run `make validate`.

To install the repo pre-commit hook locally:

```bash
make install-hooks
```
