from collections.abc import AsyncIterator

import httpx

from app.schemas import ModelInfo


class InferenceError(Exception):
    pass


async def list_models(base_url: str) -> list[ModelInfo]:
    url = base_url.rstrip("/") + "/models"
    async with httpx.AsyncClient(timeout=30.0) as client:
        try:
            response = await client.get(url)
            response.raise_for_status()
        except httpx.HTTPError as exc:
            raise InferenceError(f"Failed to list models: {exc}") from exc

    data = response.json()
    models = data.get("data", data if isinstance(data, list) else [])
    result: list[ModelInfo] = []
    for item in models:
        if isinstance(item, str):
            result.append(ModelInfo(id=item, name=item))
        elif isinstance(item, dict):
            model_id = item.get("id") or item.get("name") or ""
            result.append(ModelInfo(id=model_id, name=item.get("name")))
    return result


async def stream_chat_completion(
    *,
    base_url: str,
    model: str,
    messages: list[dict[str, str]],
    temperature: float,
    top_p: float,
    max_tokens: int,
) -> AsyncIterator[str]:
    url = base_url.rstrip("/") + "/chat/completions"
    payload = {
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": True,
    }

    async with httpx.AsyncClient(timeout=None) as client:
        try:
            async with client.stream("POST", url, json=payload) as response:
                if response.status_code >= 400:
                    body = await response.aread()
                    raise InferenceError(
                        f"Inference server returned {response.status_code}: "
                        f"{body.decode(errors='replace')}"
                    )

                async for line in response.aiter_lines():
                    if not line or not line.startswith("data: "):
                        continue
                    data = line[6:].strip()
                    if data == "[DONE]":
                        break
                    try:
                        import json

                        chunk = json.loads(data)
                        delta = chunk["choices"][0].get("delta", {})
                        token = delta.get("content", "")
                        if token:
                            yield token
                    except (KeyError, IndexError, json.JSONDecodeError):
                        continue
        except httpx.HTTPError as exc:
            raise InferenceError(f"Stream failed: {exc}") from exc


async def chat_completion(
    *,
    base_url: str,
    model: str,
    messages: list[dict[str, str]],
    temperature: float = 0.3,
    top_p: float = 0.9,
    max_tokens: int = 512,
) -> str:
    url = base_url.rstrip("/") + "/chat/completions"
    payload = {
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": False,
    }

    async with httpx.AsyncClient(timeout=120.0) as client:
        try:
            response = await client.post(url, json=payload)
            response.raise_for_status()
        except httpx.HTTPError as exc:
            raise InferenceError(f"Completion failed: {exc}") from exc

    data = response.json()
    return data["choices"][0]["message"]["content"]
