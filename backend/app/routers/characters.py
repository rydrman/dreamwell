from fastapi import APIRouter, Depends, File, HTTPException, UploadFile
from sqlalchemy.orm import Session

from app.character_import import parse_character_import
from app.database import get_db
from app.models import Character
from app.schemas import (
    CharacterCreate,
    CharacterOut,
    CharacterUpdate,
    ImportCharacterResponse,
)

router = APIRouter(prefix="/characters", tags=["characters"])


@router.get("", response_model=list[CharacterOut])
def list_characters(db: Session = Depends(get_db)):
    return db.query(Character).order_by(Character.updated_at.desc()).all()


@router.post("", response_model=CharacterOut)
def create_character(payload: CharacterCreate, db: Session = Depends(get_db)):
    character = Character(**payload.model_dump())
    db.add(character)
    db.commit()
    db.refresh(character)
    return character


@router.get("/{character_id}", response_model=CharacterOut)
def get_character(character_id: int, db: Session = Depends(get_db)):
    character = db.query(Character).filter(Character.id == character_id).first()
    if not character:
        raise HTTPException(status_code=404, detail="Character not found")
    return character


@router.patch("/{character_id}", response_model=CharacterOut)
def update_character(
    character_id: int, payload: CharacterUpdate, db: Session = Depends(get_db)
):
    character = db.query(Character).filter(Character.id == character_id).first()
    if not character:
        raise HTTPException(status_code=404, detail="Character not found")
    for key, value in payload.model_dump(exclude_unset=True).items():
        setattr(character, key, value)
    db.commit()
    db.refresh(character)
    return character


@router.delete("/{character_id}")
def delete_character(character_id: int, db: Session = Depends(get_db)):
    character = db.query(Character).filter(Character.id == character_id).first()
    if not character:
        raise HTTPException(status_code=404, detail="Character not found")
    db.delete(character)
    db.commit()
    return {"ok": True}


@router.post("/import", response_model=ImportCharacterResponse)
async def import_character(
    file: UploadFile = File(...), db: Session = Depends(get_db)
):
    content = await file.read()
    try:
        data = parse_character_import(
            filename=file.filename or "character.json", content=content
        )
    except (ValueError, Exception) as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc

    character = Character(**data)
    db.add(character)
    db.commit()
    db.refresh(character)
    source = "png" if (file.filename or "").lower().endswith(".png") else "json"
    return ImportCharacterResponse(character=character, source=source)
