import re

from sqlalchemy.orm import Session

from app.models import Fact, utcnow

FACT_PATTERN = re.compile(
    r"<fact\s+key=[\"']([^\"']+)[\"']\s*>(.*?)</fact>",
    re.IGNORECASE | re.DOTALL,
)


def extract_facts_from_text(text: str) -> tuple[str, list[tuple[str, str]]]:
    updates: list[tuple[str, str]] = []

    def replacer(match: re.Match[str]) -> str:
        key = match.group(1).strip()
        value = match.group(2).strip()
        if key:
            updates.append((key, value))
        return ""

    cleaned = FACT_PATTERN.sub(replacer, text)
    return cleaned.strip(), updates


def apply_fact_updates(db: Session, chat_id: int, updates: list[tuple[str, str]]) -> None:
    for key, value in updates:
        fact = (
            db.query(Fact)
            .filter(Fact.chat_id == chat_id, Fact.key == key)
            .first()
        )
        if fact:
            fact.value = value
            fact.updated_at = utcnow()
        else:
            db.add(Fact(chat_id=chat_id, key=key, value=value))
    if updates:
        db.commit()
