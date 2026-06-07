import json
from typing import AsyncIterator

from fastapi import APIRouter, Depends, HTTPException
from fastapi.responses import StreamingResponse
from sqlalchemy.orm import Session, joinedload

from app.config import settings
from app.database import SessionLocal, get_db
from app.models import (
    Chat,
    Character,
    Fact,
    GenerationJob,
    JobStatus,
    Message,
    MessageRole,
)
from app.queue import enqueue_generation
from app.schemas import (
    ChatCreate,
    ChatOut,
    ChatUpdate,
    FactOut,
    FactUpdate,
    JobOut,
    MessageOut,
    QueueStatus,
    SendMessageRequest,
)
from app.settings_service import get_or_create_settings

router = APIRouter(prefix="/chats", tags=["chats"])


def _job_out(job: GenerationJob) -> JobOut:
    return JobOut.model_validate(job)


def _chat_out(db: Session, chat: Chat) -> ChatOut:
    active = (
        db.query(GenerationJob)
        .filter(
            GenerationJob.chat_id == chat.id,
            GenerationJob.status.in_([JobStatus.queued, JobStatus.running]),
        )
        .order_by(GenerationJob.created_at.asc())
        .first()
    )
    queued = (
        db.query(GenerationJob)
        .filter(
            GenerationJob.chat_id == chat.id,
            GenerationJob.status == JobStatus.queued,
        )
        .count()
    )
    return ChatOut(
        id=chat.id,
        title=chat.title,
        character_id=chat.character_id,
        summary=chat.summary,
        created_at=chat.created_at,
        updated_at=chat.updated_at,
        active_job=_job_out(active) if active else None,
        queued_jobs=queued,
    )


@router.get("", response_model=list[ChatOut])
def list_chats(db: Session = Depends(get_db)):
    chats = db.query(Chat).order_by(Chat.updated_at.desc()).all()
    return [_chat_out(db, chat) for chat in chats]


@router.post("", response_model=ChatOut)
def create_chat(payload: ChatCreate, db: Session = Depends(get_db)):
    if payload.character_id:
        character = (
            db.query(Character).filter(Character.id == payload.character_id).first()
        )
        if not character:
            raise HTTPException(status_code=404, detail="Character not found")

    chat = Chat(title=payload.title, character_id=payload.character_id)
    db.add(chat)
    db.commit()
    db.refresh(chat)

    if payload.character_id:
        character = (
            db.query(Character).filter(Character.id == payload.character_id).first()
        )
        if character and character.first_message.strip():
            db.add(
                Message(
                    chat_id=chat.id,
                    role=MessageRole.assistant,
                    content=character.first_message.strip(),
                )
            )
            db.commit()

    return _chat_out(db, chat)


@router.get("/queue", response_model=QueueStatus)
def get_queue(db: Session = Depends(get_db)):
    running = (
        db.query(GenerationJob)
        .filter(GenerationJob.status == JobStatus.running)
        .order_by(GenerationJob.started_at.asc())
        .all()
    )
    queued = (
        db.query(GenerationJob)
        .filter(GenerationJob.status == JobStatus.queued)
        .order_by(GenerationJob.created_at.asc())
        .all()
    )
    return QueueStatus(
        running=[_job_out(j) for j in running],
        queued=[_job_out(j) for j in queued],
    )


@router.get("/{chat_id}", response_model=ChatOut)
def get_chat(chat_id: int, db: Session = Depends(get_db)):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")
    return _chat_out(db, chat)


@router.patch("/{chat_id}", response_model=ChatOut)
def update_chat(chat_id: int, payload: ChatUpdate, db: Session = Depends(get_db)):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")
    for key, value in payload.model_dump(exclude_unset=True).items():
        setattr(chat, key, value)
    db.commit()
    db.refresh(chat)
    return _chat_out(db, chat)


@router.delete("/{chat_id}")
def delete_chat(chat_id: int, db: Session = Depends(get_db)):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")
    db.delete(chat)
    db.commit()
    return {"ok": True}


@router.get("/{chat_id}/messages", response_model=list[MessageOut])
def list_messages(chat_id: int, db: Session = Depends(get_db)):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")

    messages = (
        db.query(Message)
        .options(joinedload(Message.job))
        .filter(Message.chat_id == chat_id)
        .order_by(Message.created_at.asc())
        .all()
    )
    result: list[MessageOut] = []
    for msg in messages:
        job_status = msg.job.status if msg.job else None
        result.append(
            MessageOut(
                id=msg.id,
                chat_id=msg.chat_id,
                role=msg.role,
                content=msg.content,
                is_summary=msg.is_summary,
                created_at=msg.created_at,
                job_status=job_status,
            )
        )
    return result


@router.post("/{chat_id}/messages", response_model=MessageOut)
def send_message(
    chat_id: int, payload: SendMessageRequest, db: Session = Depends(get_db)
):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")
    if not payload.content.strip():
        raise HTTPException(status_code=400, detail="Message cannot be empty")

    get_or_create_settings(db)

    user_msg = Message(
        chat_id=chat_id,
        role=MessageRole.user,
        content=payload.content.strip(),
    )
    db.add(user_msg)

    assistant_msg = Message(
        chat_id=chat_id,
        role=MessageRole.assistant,
        content="",
    )
    db.add(assistant_msg)
    db.commit()
    db.refresh(assistant_msg)

    job = enqueue_generation(db, chat_id, assistant_msg.id)
    chat.updated_at = assistant_msg.created_at
    db.commit()

    return MessageOut(
        id=assistant_msg.id,
        chat_id=assistant_msg.chat_id,
        role=assistant_msg.role,
        content=assistant_msg.content,
        is_summary=assistant_msg.is_summary,
        created_at=assistant_msg.created_at,
        job_status=job.status,
    )


@router.get("/{chat_id}/facts", response_model=list[FactOut])
def list_facts(chat_id: int, db: Session = Depends(get_db)):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")
    return (
        db.query(Fact).filter(Fact.chat_id == chat_id).order_by(Fact.key.asc()).all()
    )


@router.put("/{chat_id}/facts", response_model=FactOut)
def upsert_fact(chat_id: int, payload: FactUpdate, db: Session = Depends(get_db)):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")

    fact = (
        db.query(Fact)
        .filter(Fact.chat_id == chat_id, Fact.key == payload.key)
        .first()
    )
    if fact:
        fact.value = payload.value
    else:
        fact = Fact(chat_id=chat_id, key=payload.key, value=payload.value)
        db.add(fact)
    db.commit()
    db.refresh(fact)
    return fact


@router.delete("/{chat_id}/facts/{fact_key}")
def delete_fact(chat_id: int, fact_key: str, db: Session = Depends(get_db)):
    fact = (
        db.query(Fact).filter(Fact.chat_id == chat_id, Fact.key == fact_key).first()
    )
    if not fact:
        raise HTTPException(status_code=404, detail="Fact not found")
    db.delete(fact)
    db.commit()
    return {"ok": True}


async def _chat_event_stream(chat_id: int) -> AsyncIterator[str]:
    last_payload = ""
    while True:
        db = SessionLocal()
        try:
            chat = db.query(Chat).filter(Chat.id == chat_id).first()
            if not chat:
                yield f"event: error\ndata: {json.dumps({'detail': 'not found'})}\n\n"
                break

            messages = (
                db.query(Message)
                .options(joinedload(Message.job))
                .filter(Message.chat_id == chat_id)
                .order_by(Message.created_at.asc())
                .all()
            )
            active_job = (
                db.query(GenerationJob)
                .filter(
                    GenerationJob.chat_id == chat_id,
                    GenerationJob.status.in_(
                        [JobStatus.queued, JobStatus.running]
                    ),
                )
                .order_by(GenerationJob.created_at.desc())
                .first()
            )

            payload = json.dumps(
                {
                    "chat": _chat_out(db, chat).model_dump(mode="json"),
                    "messages": [
                        MessageOut(
                            id=m.id,
                            chat_id=m.chat_id,
                            role=m.role,
                            content=m.content,
                            is_summary=m.is_summary,
                            created_at=m.created_at,
                            job_status=m.job.status if m.job else None,
                        ).model_dump(mode="json")
                        for m in messages
                    ],
                    "active_job": _job_out(active_job).model_dump(mode="json")
                    if active_job
                    else None,
                },
                default=str,
            )

            if payload != last_payload:
                last_payload = payload
                yield f"data: {payload}\n\n"

            if not active_job:
                yield f"event: idle\ndata: {json.dumps({'chat_id': chat_id})}\n\n"
                break
        finally:
            db.close()

        import asyncio

        await asyncio.sleep(settings.sse_poll_interval)


@router.get("/{chat_id}/stream")
async def stream_chat(chat_id: int, db: Session = Depends(get_db)):
    chat = db.query(Chat).filter(Chat.id == chat_id).first()
    if not chat:
        raise HTTPException(status_code=404, detail="Chat not found")
    return StreamingResponse(
        _chat_event_stream(chat_id),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )
