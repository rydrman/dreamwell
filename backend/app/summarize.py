from sqlalchemy.orm import Session

from app.inference import InferenceError, chat_completion
from app.models import AppSettings, Chat, Message, MessageRole


async def maybe_summarize_chat(db: Session, chat: Chat, settings: AppSettings) -> None:
    if not settings.summarize_enabled:
        return

    messages = (
        db.query(Message)
        .filter(Message.chat_id == chat.id, Message.is_summary.is_(False))
        .order_by(Message.created_at.asc())
        .all()
    )

    non_system = [m for m in messages if m.role != MessageRole.system]
    if len(non_system) <= settings.summarize_after_messages:
        return

    keep = settings.summarize_keep_recent
    to_summarize = non_system[: -keep] if keep > 0 else non_system
    if not to_summarize:
        return

    transcript = "\n".join(
        f"{msg.role.value}: {msg.content}" for msg in to_summarize
    )
    prompt = [
        {
            "role": "system",
            "content": (
                "Summarize the following roleplay conversation. "
                "Preserve key plot points, relationships, and facts. "
                "Be concise."
            ),
        },
        {
            "role": "user",
            "content": (
                f"Previous summary:\n{chat.summary or '(none)'}\n\n"
                f"New messages to incorporate:\n{transcript}"
            ),
        },
    ]

    if not settings.model:
        return

    try:
        summary = await chat_completion(
            base_url=settings.inference_url,
            model=settings.model,
            messages=prompt,
            temperature=0.3,
            max_tokens=512,
        )
    except InferenceError:
        return

    chat.summary = summary.strip()
    for msg in to_summarize:
        db.delete(msg)
    db.commit()
