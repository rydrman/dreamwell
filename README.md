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
- Per-chat variables the model can update via `<var key="...">...</var>`
- Prompt prefix/suffix and model parameters
- Shared backend queue (default concurrency: 1)
- **Game mode**: tabletop-RPG-style turns with real 2d6 dice, typed state, and auto-chained phases (checks → roll → resolve → prose)

## Game mode

Game mode is a third top-level tab alongside Chats and Stories. Each turn:

1. The player submits an action.
2. The backend declares checks, rolls real dice, resolves state changes, and streams prose.
3. The UI shows stacked phase bubbles (checks, roll, state, scene, prose) updated over SSE.

Use **step mode** in the state panel to pause between phases. **Regenerate (keep roll)** re-runs resolve/prose without re-rolling. **Align prose** and **Recheck state** run optional quality passes after a turn completes.

Per-game settings (modifier range, merge resolve+scene, per-phase model overrides) live in the state panel under **Game settings**. Empty model overrides use the global Settings model.

See `docs/game-mode-plan.md` for the full design.

## Development

```bash
# One-shot CI checks (required before every commit)
make validate

# Install git hook so commits are blocked unless validate passes
make install-hooks

# Individual checks
make fmt-check
make clippy
make test

# Build frontend + server
make build

# Run locally (serves UI + API on :8080)
# Uses Docker with persistent cargo/target volumes so rebuilds are incremental.
make run

# Run without Docker (requires local Rust + wasm32 target)
make run-local
```

**Commit policy:** `rustfmt`, `clippy`, and `cargo test` must pass before committing. Run `make validate` manually, or `make install-hooks` once to enforce this via a pre-commit hook.

Point **Settings → Inference server** at your OpenAI-compatible endpoint (e.g. `http://localhost:11434/v1` for Ollama), refresh models, and pick one.

When using `make run` (Docker), the app runs inside a container — use `http://host.docker.internal:11434/v1` to reach Ollama on the host, not `localhost`.

Click the **queue bar** at the top of Chats or Stories to open the queue page, inspect running/waiting jobs, and cancel them. Jobs interrupted by a server restart are automatically requeued.

## Environment

| Variable | Default | Description |
|----------|---------|-------------|
| `DREAMWELL_DATABASE_URL` | `sqlite:dreamwell.db` | SQLite database path |
| `DREAMWELL_STATIC_DIR` | `crates/frontend/dist` | Built Yew assets |
| `DREAMWELL_HOST` | `0.0.0.0` | Listen host |
| `DREAMWELL_PORT` | `8080` | Listen port |
| `DREAMWELL_MAX_CONCURRENT_JOBS` | `1` | Queue concurrency |

Inference HTTP timeouts use a 10 minute connect timeout. Non-streaming requests also have a 15 minute total timeout. Streaming requests have no total timeout; instead they fail if no data arrives for 10 minutes.

## Docker

```bash
make docker
docker run --rm -p 8080:8080 -v dreamwell-data:/app/data dreamwell:local
```

Images are published privately to `ghcr.io/<owner>/dreamwell` on every push to `main` (tagged with the commit SHA and `latest`) and on git tag pushes (tagged with the tag name).

## Kubernetes

Manifests live in `deploy/`. The cluster needs a `ghcr-credentials` pull secret in the `dreamwell` namespace (managed in the homelab repo).

```bash
# 1. Ensure a main-branch image exists in GHCR (push to main or wait for CI)
# 2. Apply the GHCR pull secret via homelab OpenTofu (see homelab/charts/ghcr.tf)
kubectl --kubeconfig=~/work/homelab/kube_config_talos.yaml apply -f deploy/namespace.yaml

# 3. Deploy (tracks :latest; Keel polls GHCR every 10m and rolls out on digest change)
make deploy

# Pin a specific tag instead of auto-update
make deploy IMAGE_TAG=abc1234
```

Ingress is at `https://dreamwell.bottriell.ca` with Authelia (`*.bottriell.ca` one-factor policy) and a 1Gi Longhorn PVC for SQLite data.

## Architecture

```
User sends message → job queued → worker streams tokens → SQLite updated continuously
                                         ↓
                         SSE polls DB → live updates; full history on return
```
