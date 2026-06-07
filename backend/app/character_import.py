import json
from io import BytesIO
from typing import Any

from PIL import Image
from pydantic import BaseModel


class TavernCardV2(BaseModel):
    spec: str | None = None
    spec_version: str | None = None
    data: dict[str, Any]


def _extract_from_png(data: bytes) -> dict[str, Any] | None:
    image = Image.open(BytesIO(data))
    for key in ("chara", "ccv3"):
        if key in image.info:
            raw = image.info[key]
            if isinstance(raw, bytes):
                raw = raw.decode("utf-8", errors="replace")
            return json.loads(raw)
    return None


def parse_character_import(
    *,
    filename: str,
    content: bytes,
) -> dict[str, Any]:
    lower = filename.lower()
    if lower.endswith(".png"):
        card = _extract_from_png(content)
        if not card:
            raise ValueError("No character data found in PNG")
        return _normalize_card(card)

    text = content.decode("utf-8")
    card = json.loads(text)
    return _normalize_card(card)


def _normalize_card(card: dict[str, Any]) -> dict[str, Any]:
    if "data" in card and isinstance(card["data"], dict):
        data = card["data"]
    else:
        data = card

    return {
        "name": data.get("name") or data.get("char_name") or "Unnamed",
        "description": data.get("description") or data.get("char_persona") or "",
        "personality": data.get("personality") or "",
        "scenario": data.get("scenario") or data.get("world_scenario") or "",
        "first_message": data.get("first_mes")
        or data.get("first_message")
        or data.get("greeting")
        or "",
        "example_dialogue": data.get("mes_example")
        or data.get("example_dialogue")
        or "",
        "system_prompt": data.get("system_prompt") or "",
        "avatar_url": data.get("avatar") or None,
    }
