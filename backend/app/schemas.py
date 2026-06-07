from datetime import datetime
from typing import Any

from pydantic import BaseModel, Field

from app.models import JobStatus, MessageRole


class CharacterBase(BaseModel):
    name: str
    description: str = ""
    personality: str = ""
    scenario: str = ""
    first_message: str = ""
    example_dialogue: str = ""
    system_prompt: str = ""
    avatar_url: str | None = None


class CharacterCreate(CharacterBase):
    pass


class CharacterUpdate(BaseModel):
    name: str | None = None
    description: str | None = None
    personality: str | None = None
    scenario: str | None = None
    first_message: str | None = None
    example_dialogue: str | None = None
    system_prompt: str | None = None
    avatar_url: str | None = None


class CharacterOut(CharacterBase):
    id: int
    created_at: datetime
    updated_at: datetime

    class Config:
        from_attributes = True


class ChatCreate(BaseModel):
    title: str = "New Chat"
    character_id: int | None = None


class ChatUpdate(BaseModel):
    title: str | None = None
    character_id: int | None = None


class ChatOut(BaseModel):
    id: int
    title: str
    character_id: int | None
    summary: str
    created_at: datetime
    updated_at: datetime
    active_job: "JobOut | None" = None
    queued_jobs: int = 0

    class Config:
        from_attributes = True


class MessageOut(BaseModel):
    id: int
    chat_id: int
    role: MessageRole
    content: str
    is_summary: bool
    created_at: datetime
    job_status: JobStatus | None = None

    class Config:
        from_attributes = True


class SendMessageRequest(BaseModel):
    content: str


class FactOut(BaseModel):
    id: int
    chat_id: int
    key: str
    value: str
    updated_at: datetime

    class Config:
        from_attributes = True


class FactUpdate(BaseModel):
    key: str
    value: str


class SettingsOut(BaseModel):
    inference_url: str
    model: str
    temperature: float
    top_p: float
    max_tokens: int
    system_prompt_prefix: str
    system_prompt_suffix: str
    summarize_enabled: bool
    summarize_after_messages: int
    summarize_keep_recent: int
    facts_enabled: bool
    max_context_messages: int
    max_concurrent_jobs: int

    class Config:
        from_attributes = True


class SettingsUpdate(BaseModel):
    inference_url: str | None = None
    model: str | None = None
    temperature: float | None = None
    top_p: float | None = None
    max_tokens: int | None = None
    system_prompt_prefix: str | None = None
    system_prompt_suffix: str | None = None
    summarize_enabled: bool | None = None
    summarize_after_messages: int | None = None
    summarize_keep_recent: int | None = None
    facts_enabled: bool | None = None
    max_context_messages: int | None = None
    max_concurrent_jobs: int | None = None


class JobOut(BaseModel):
    id: int
    chat_id: int
    message_id: int
    status: JobStatus
    error: str | None
    position: int
    created_at: datetime
    started_at: datetime | None
    completed_at: datetime | None

    class Config:
        from_attributes = True


class QueueStatus(BaseModel):
    running: list[JobOut]
    queued: list[JobOut]


class ModelInfo(BaseModel):
    id: str
    name: str | None = None


class ImportCharacterResponse(BaseModel):
    character: CharacterOut
    source: str


ChatOut.model_rebuild()
