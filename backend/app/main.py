from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.database import init_db
from app.queue import job_queue
from app.routers import characters, chats, settings
from app.settings_service import get_or_create_settings
from app.database import SessionLocal


@asynccontextmanager
async def lifespan(app: FastAPI):
    init_db()
    db = SessionLocal()
    try:
        get_or_create_settings(db)
    finally:
        db.close()
    job_queue.start()
    yield
    await job_queue.stop()


app = FastAPI(title="Dreamwell", version="0.1.0", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(characters.router, prefix="/api")
app.include_router(chats.router, prefix="/api")
app.include_router(settings.router, prefix="/api")


@app.get("/api/health")
def health():
    return {"status": "ok"}
