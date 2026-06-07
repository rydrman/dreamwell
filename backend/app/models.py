import enum
from datetime import datetime, timezone

from sqlalchemy import (
    Boolean,
    DateTime,
    Enum,
    Float,
    ForeignKey,
    Integer,
    String,
    Text,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base


def utcnow():
    return datetime.now(timezone.utc)


class JobStatus(str, enum.Enum):
    queued = "queued"
    running = "running"
    completed = "completed"
    failed = "failed"
    cancelled = "cancelled"


class Character(Base):
    __tablename__ = "characters"

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    name: Mapped[str] = mapped_column(String(255))
    description: Mapped[str] = mapped_column(Text, default="")
    personality: Mapped[str] = mapped_column(Text, default="")
    scenario: Mapped[str] = mapped_column(Text, default="")
    first_message: Mapped[str] = mapped_column(Text, default="")
    example_dialogue: Mapped[str] = mapped_column(Text, default="")
    system_prompt: Mapped[str] = mapped_column(Text, default="")
    avatar_url: Mapped[str | None] = mapped_column(String(512), nullable=True)
    created_at: Mapped[datetime] = mapped_column(DateTime, default=utcnow)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime, default=utcnow, onupdate=utcnow
    )

    chats: Mapped[list["Chat"]] = relationship(back_populates="character")


class Chat(Base):
    __tablename__ = "chats"

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    title: Mapped[str] = mapped_column(String(255), default="New Chat")
    character_id: Mapped[int | None] = mapped_column(
        Integer, ForeignKey("characters.id"), nullable=True
    )
    summary: Mapped[str] = mapped_column(Text, default="")
    created_at: Mapped[datetime] = mapped_column(DateTime, default=utcnow)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime, default=utcnow, onupdate=utcnow
    )

    character: Mapped[Character | None] = relationship(back_populates="chats")
    messages: Mapped[list["Message"]] = relationship(
        back_populates="chat", cascade="all, delete-orphan"
    )
    facts: Mapped[list["Fact"]] = relationship(
        back_populates="chat", cascade="all, delete-orphan"
    )
    jobs: Mapped[list["GenerationJob"]] = relationship(
        back_populates="chat", cascade="all, delete-orphan"
    )


class MessageRole(str, enum.Enum):
    system = "system"
    user = "user"
    assistant = "assistant"


class Message(Base):
    __tablename__ = "messages"

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    chat_id: Mapped[int] = mapped_column(Integer, ForeignKey("chats.id"))
    role: Mapped[MessageRole] = mapped_column(Enum(MessageRole))
    content: Mapped[str] = mapped_column(Text, default="")
    is_summary: Mapped[bool] = mapped_column(Boolean, default=False)
    created_at: Mapped[datetime] = mapped_column(DateTime, default=utcnow)

    chat: Mapped[Chat] = relationship(back_populates="messages")
    job: Mapped["GenerationJob | None"] = relationship(
        back_populates="message", uselist=False
    )


class Fact(Base):
    __tablename__ = "facts"

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    chat_id: Mapped[int] = mapped_column(Integer, ForeignKey("chats.id"))
    key: Mapped[str] = mapped_column(String(255))
    value: Mapped[str] = mapped_column(Text, default="")
    updated_at: Mapped[datetime] = mapped_column(
        DateTime, default=utcnow, onupdate=utcnow
    )

    chat: Mapped[Chat] = relationship(back_populates="facts")


class GenerationJob(Base):
    __tablename__ = "generation_jobs"

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    chat_id: Mapped[int] = mapped_column(Integer, ForeignKey("chats.id"))
    message_id: Mapped[int] = mapped_column(Integer, ForeignKey("messages.id"))
    status: Mapped[JobStatus] = mapped_column(
        Enum(JobStatus), default=JobStatus.queued
    )
    error: Mapped[str | None] = mapped_column(Text, nullable=True)
    position: Mapped[int] = mapped_column(Integer, default=0)
    created_at: Mapped[datetime] = mapped_column(DateTime, default=utcnow)
    started_at: Mapped[datetime | None] = mapped_column(DateTime, nullable=True)
    completed_at: Mapped[datetime | None] = mapped_column(DateTime, nullable=True)

    chat: Mapped[Chat] = relationship(back_populates="jobs")
    message: Mapped[Message] = relationship(back_populates="job")


class AppSettings(Base):
    __tablename__ = "app_settings"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, default=1)
    inference_url: Mapped[str] = mapped_column(
        String(512), default="http://localhost:11434/v1"
    )
    model: Mapped[str] = mapped_column(String(255), default="")
    temperature: Mapped[float] = mapped_column(Float, default=0.8)
    top_p: Mapped[float] = mapped_column(Float, default=0.9)
    max_tokens: Mapped[int] = mapped_column(Integer, default=512)
    system_prompt_prefix: Mapped[str] = mapped_column(Text, default="")
    system_prompt_suffix: Mapped[str] = mapped_column(Text, default="")
    summarize_enabled: Mapped[bool] = mapped_column(Boolean, default=True)
    summarize_after_messages: Mapped[int] = mapped_column(Integer, default=20)
    summarize_keep_recent: Mapped[int] = mapped_column(Integer, default=8)
    facts_enabled: Mapped[bool] = mapped_column(Boolean, default=True)
    max_context_messages: Mapped[int] = mapped_column(Integer, default=40)
