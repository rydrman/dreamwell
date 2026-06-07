# Dreamwell

A lightweight SillyTavern-style roleplay client focused on **server-side streaming** and a **shared generation queue**.

Send messages across many chats, switch away, and come back later — responses keep generating on the server and are persisted as they stream in.

## Features

- **Custom inference server** — OpenAI-compatible API (Ollama, vLLM, text-generation-webui, etc.) with model picker
- **Character cards** — create, edit, and import JSON or Tavern PNG cards
- **Auto-summarize** — configurable threshold to compress old context
- **KV facts** — per-chat key/value memory the model can update via `<fact key="...">...</fact>` tags
- **Prompt & model settings** — temperature, top-p, max tokens, system prefix/suffix, concurrency
- **Shared backend queue** — one slow worker by default; bump concurrency in settings to run more in parallel

## Quick start

### Backend

```bash
cd backend
pip install -r requirements.txt
uvicorn app.main:app --reload --host 0.0.0.0 --port 8000
```

### Frontend

```bash
cd frontend
npm install
npm run dev
```

Open http://localhost:5173

### Inference

Point **Settings → Inference server** at your OpenAI-compatible endpoint, e.g.:

- Ollama: `http://localhost:11434/v1`
- Then click **Refresh** and pick a model.

## Architecture

```
User sends message → job queued → worker streams tokens → DB updated continuously
                                         ↓
Frontend SSE polls DB → live updates while viewing; full history on return
```

Jobs are processed by a background asyncio worker. Each token is committed to SQLite, so partial responses survive page reloads and tab switches.

## Environment

| Variable | Default | Description |
|----------|---------|-------------|
| `DREAMWELL_DATABASE_URL` | `sqlite:///./dreamwell.db` | SQLAlchemy database URL |
| `DREAMWELL_MAX_CONCURRENT_JOBS` | `1` | Default queue concurrency |

## API

- `GET /api/chats` — list chats with active job status
- `POST /api/chats/{id}/messages` — send message, queue generation
- `GET /api/chats/{id}/stream` — SSE stream of message updates
- `GET /api/chats/queue` — global queue status
- `GET /api/characters` — character cards
- `POST /api/characters/import` — import JSON/PNG
- `GET/PATCH /api/settings` — app configuration
