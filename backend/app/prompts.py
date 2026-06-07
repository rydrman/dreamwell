from sqlalchemy.orm import Session

from app.models import AppSettings, Character, Chat, Fact, Message, MessageRole


def build_character_system_prompt(character: Character | None) -> str:
    if not character:
        return ""

    if character.system_prompt.strip():
        return character.system_prompt.strip()

    parts: list[str] = []
    if character.name:
        parts.append(f"You are {character.name}.")
    if character.description:
        parts.append(f"Description: {character.description}")
    if character.personality:
        parts.append(f"Personality: {character.personality}")
    if character.scenario:
        parts.append(f"Scenario: {character.scenario}")
    if character.example_dialogue:
        parts.append(f"Example dialogue:\n{character.example_dialogue}")
    return "\n\n".join(parts)


def format_facts(facts: list[Fact]) -> str:
    if not facts:
        return ""
    lines = [f"- {fact.key}: {fact.value}" for fact in facts]
    return "Known facts about this conversation:\n" + "\n".join(lines)


def facts_instruction() -> str:
    return (
        "You may update conversation facts using XML tags like "
        '<fact key="location">tavern</fact>. '
        "Only emit fact tags when information should be remembered."
    )


def build_messages_for_inference(
    db: Session,
    chat: Chat,
    settings: AppSettings,
) -> list[dict[str, str]]:
    character = chat.character
    facts = (
        db.query(Fact).filter(Fact.chat_id == chat.id).order_by(Fact.key).all()
        if settings.facts_enabled
        else []
    )

    system_parts: list[str] = []
    if settings.system_prompt_prefix.strip():
        system_parts.append(settings.system_prompt_prefix.strip())

    char_prompt = build_character_system_prompt(character)
    if char_prompt:
        system_parts.append(char_prompt)

    if chat.summary.strip():
        system_parts.append(f"Conversation summary so far:\n{chat.summary}")

    facts_text = format_facts(facts)
    if facts_text:
        system_parts.append(facts_text)

    if settings.facts_enabled:
        system_parts.append(facts_instruction())

    if settings.system_prompt_suffix.strip():
        system_parts.append(settings.system_prompt_suffix.strip())

    messages: list[dict[str, str]] = []
    if system_parts:
        messages.append({"role": "system", "content": "\n\n".join(system_parts)})

    db_messages = (
        db.query(Message)
        .filter(Message.chat_id == chat.id, Message.is_summary.is_(False))
        .order_by(Message.created_at.asc())
        .all()
    )

    if settings.max_context_messages > 0:
        db_messages = db_messages[-settings.max_context_messages :]

    for msg in db_messages:
        if msg.role == MessageRole.system:
            continue
        messages.append({"role": msg.role.value, "content": msg.content})

    return messages
