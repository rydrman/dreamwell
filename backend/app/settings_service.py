from sqlalchemy.orm import Session

from app.config import settings as app_config
from app.models import AppSettings
from app.queue import job_queue
from app.schemas import SettingsUpdate


def get_or_create_settings(db: Session) -> AppSettings:
    row = db.query(AppSettings).filter(AppSettings.id == 1).first()
    if not row:
        row = AppSettings(id=1)
        db.add(row)
        db.commit()
        db.refresh(row)
    return row


def settings_to_schema(row: AppSettings) -> dict:
    return {
        "inference_url": row.inference_url,
        "model": row.model,
        "temperature": row.temperature,
        "top_p": row.top_p,
        "max_tokens": row.max_tokens,
        "system_prompt_prefix": row.system_prompt_prefix,
        "system_prompt_suffix": row.system_prompt_suffix,
        "summarize_enabled": row.summarize_enabled,
        "summarize_after_messages": row.summarize_after_messages,
        "summarize_keep_recent": row.summarize_keep_recent,
        "facts_enabled": row.facts_enabled,
        "max_context_messages": row.max_context_messages,
        "max_concurrent_jobs": app_config.max_concurrent_jobs,
    }


def update_settings(db: Session, payload: SettingsUpdate) -> AppSettings:
    row = get_or_create_settings(db)
    data = payload.model_dump(exclude_unset=True)
    max_jobs = data.pop("max_concurrent_jobs", None)
    for key, value in data.items():
        setattr(row, key, value)
    db.commit()
    db.refresh(row)
    if max_jobs is not None:
        app_config.max_concurrent_jobs = max_jobs
        job_queue.set_max_concurrent(max_jobs)
    return row
