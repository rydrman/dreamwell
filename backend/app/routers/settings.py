from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session

from app.database import get_db
from app.inference import InferenceError, list_models
from app.schemas import ModelInfo, SettingsOut, SettingsUpdate
from app.settings_service import (
    get_or_create_settings,
    settings_to_schema,
    update_settings,
)

router = APIRouter(prefix="/settings", tags=["settings"])


@router.get("", response_model=SettingsOut)
def get_settings(db: Session = Depends(get_db)):
    row = get_or_create_settings(db)
    return SettingsOut(**settings_to_schema(row))


@router.patch("", response_model=SettingsOut)
def patch_settings(payload: SettingsUpdate, db: Session = Depends(get_db)):
    row = update_settings(db, payload)
    return SettingsOut(**settings_to_schema(row))


@router.get("/models", response_model=list[ModelInfo])
async def get_models(db: Session = Depends(get_db)):
    row = get_or_create_settings(db)
    try:
        return await list_models(row.inference_url)
    except InferenceError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
