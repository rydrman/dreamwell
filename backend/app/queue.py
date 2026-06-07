import asyncio
from datetime import datetime, timezone

from sqlalchemy.orm import Session

from app.config import settings
from app.database import SessionLocal
from app.facts import apply_fact_updates, extract_facts_from_text
from app.inference import InferenceError, stream_chat_completion
from app.models import AppSettings, Chat, GenerationJob, JobStatus, Message, utcnow
from app.prompts import build_messages_for_inference
from app.summarize import maybe_summarize_chat


class JobQueue:
    def __init__(self) -> None:
        self._task: asyncio.Task | None = None
        self._stop_event = asyncio.Event()
        self._wake_event = asyncio.Event()
        self._running_count = 0
        self._max_concurrent = settings.max_concurrent_jobs
        self._lock = asyncio.Lock()

    def start(self) -> None:
        if self._task is None or self._task.done():
            self._stop_event.clear()
            self._task = asyncio.create_task(self._loop())

    async def stop(self) -> None:
        self._stop_event.set()
        self._wake_event.set()
        if self._task:
            await self._task

    def wake(self) -> None:
        self._wake_event.set()

    def set_max_concurrent(self, value: int) -> None:
        self._max_concurrent = max(1, value)
        self._wake_event.set()

    async def _loop(self) -> None:
        while not self._stop_event.is_set():
            try:
                await self._process_available_slots()
            except Exception:
                pass

            try:
                await asyncio.wait_for(self._wake_event.wait(), timeout=1.0)
            except asyncio.TimeoutError:
                pass
            self._wake_event.clear()

    async def _process_available_slots(self) -> None:
        async with self._lock:
            available = self._max_concurrent - self._running_count
            if available <= 0:
                return

            jobs = self._claim_jobs(available)
            for job_id in jobs:
                self._running_count += 1
                asyncio.create_task(self._run_job(job_id))

    def _claim_jobs(self, limit: int) -> list[int]:
        db = SessionLocal()
        try:
            running = (
                db.query(GenerationJob)
                .filter(GenerationJob.status == JobStatus.running)
                .count()
            )
            slots = max(0, self._max_concurrent - running)
            if slots <= 0:
                return []

            jobs = (
                db.query(GenerationJob)
                .filter(GenerationJob.status == JobStatus.queued)
                .order_by(GenerationJob.created_at.asc())
                .limit(min(limit, slots))
                .all()
            )

            claimed: list[int] = []
            for job in jobs:
                job.status = JobStatus.running
                job.started_at = utcnow()
                claimed.append(job.id)
            db.commit()
            return claimed
        finally:
            db.close()

    async def _run_job(self, job_id: int) -> None:
        try:
            await self._execute_job(job_id)
        finally:
            self._running_count -= 1
            self._wake_event.set()

    async def _execute_job(self, job_id: int) -> None:
        db = SessionLocal()
        try:
            job = db.query(GenerationJob).filter(GenerationJob.id == job_id).first()
            if not job or job.status != JobStatus.running:
                return

            message = db.query(Message).filter(Message.id == job.message_id).first()
            chat = db.query(Chat).filter(Chat.id == job.chat_id).first()
            app_settings = db.query(AppSettings).filter(AppSettings.id == 1).first()

            if not message or not chat or not app_settings:
                job.status = JobStatus.failed
                job.error = "Missing message, chat, or settings"
                job.completed_at = utcnow()
                db.commit()
                return

            if not app_settings.model:
                job.status = JobStatus.failed
                job.error = "No model selected in settings"
                job.completed_at = utcnow()
                db.commit()
                return

            messages = build_messages_for_inference(db, chat, app_settings)
            accumulated = ""

            try:
                async for token in stream_chat_completion(
                    base_url=app_settings.inference_url,
                    model=app_settings.model,
                    messages=messages,
                    temperature=app_settings.temperature,
                    top_p=app_settings.top_p,
                    max_tokens=app_settings.max_tokens,
                ):
                    accumulated += token
                    message.content = accumulated
                    chat.updated_at = utcnow()
                    db.commit()

                cleaned, fact_updates = extract_facts_from_text(accumulated)
                if fact_updates and app_settings.facts_enabled:
                    message.content = cleaned
                    apply_fact_updates(db, chat.id, fact_updates)
                    db.commit()

                job.status = JobStatus.completed
                job.completed_at = utcnow()
                db.commit()

                await maybe_summarize_chat(db, chat, app_settings)

            except InferenceError as exc:
                job.status = JobStatus.failed
                job.error = str(exc)
                job.completed_at = utcnow()
                if not message.content:
                    message.content = f"[Generation failed: {exc}]"
                db.commit()
        finally:
            db.close()


job_queue = JobQueue()


def enqueue_generation(db: Session, chat_id: int, message_id: int) -> GenerationJob:
    queued_count = (
        db.query(GenerationJob)
        .filter(GenerationJob.status == JobStatus.queued)
        .count()
    )
    job = GenerationJob(
        chat_id=chat_id,
        message_id=message_id,
        status=JobStatus.queued,
        position=queued_count + 1,
    )
    db.add(job)
    db.commit()
    db.refresh(job)
    job_queue.wake()
    return job
