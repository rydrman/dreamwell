# Dreamwell

A lightweight SillyTavern-style roleplay client with **server-side streaming** and a **shared generation queue**.

Send messages across many chats, switch away, and come back later — responses keep generating on the server and are persisted as they stream in.

## Stack

- **Backend**: Rust (Axum, SQLx/SQLite, Tokio job queue)
- **Frontend**: Rust (Yew WASM via Trunk)
- **CI**: rustfmt, clippy, tests, Docker build
- **Publish**: private GHCR image on every `main` push (SHA tag) and on git tags (tag name)

## Features

- Custom OpenAI-compatible inference server with model selection
- Character cards (create, edit, import JSON/PNG)
- Configurable auto-summarize to keep context small
- Per-chat KV facts the model can update via `<fact key="...">...</fact>`
- Prompt prefix/suffix and model parameters
- Shared backend queue (default concurrency: 1)

## Development

```bash
# Lint / format
make fmt-check
make clippy

# Build frontend + server
make build

# Run locally (serves UI + API on :8080)
make run
```

Point **Settings → Inference server** at your OpenAI-compatible endpoint (e.g. `http://localhost:11434/v1` for Ollama), refresh models, and pick one.

## Environment

| Variable | Default | Description |
|----------|---------|-------------|
| `DREAMWELL_DATABASE_URL` | `sqlite:dreamwell.db` | SQLite database path |
| `DREAMWELL_STATIC_DIR` | `crates/frontend/dist` | Built Yew assets |
| `DREAMWELL_HOST` | `0.0.0.0` | Listen host |
| `DREAMWELL_PORT` | `8080` | Listen port |
| `DREAMWELL_MAX_CONCURRENT_JOBS` | `1` | Queue concurrency |

## Docker

```bash
make docker
docker run --rm -p 8080:8080 -v dreamwell-data:/app/data dreamwell:local
```

Images are published privately to `ghcr.io/<owner>/dreamwell` on every push to `main` (tagged with the commit SHA) and on git tag pushes (tagged with the tag name).

## Architecture

```
User sends message → job queued → worker streams tokens → SQLite updated continuously
                                         ↓
                         SSE polls DB → live updates; full history on return
```
