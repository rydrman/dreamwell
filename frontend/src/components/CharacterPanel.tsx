import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Character } from "../types";

const emptyCharacter = (): Partial<Character> => ({
  name: "",
  description: "",
  personality: "",
  scenario: "",
  first_message: "",
  example_dialogue: "",
  system_prompt: "",
});

interface Props {
  selectedCharacterId: number | null;
  onCharacterChange: (id: number | null) => void;
}

export function CharacterPanel({
  selectedCharacterId,
  onCharacterChange,
}: Props) {
  const [characters, setCharacters] = useState<Character[]>([]);
  const [draft, setDraft] = useState<Partial<Character>>(emptyCharacter());
  const [editingId, setEditingId] = useState<number | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void api.listCharacters().then(setCharacters).catch(console.error);
  }, []);

  useEffect(() => {
    if (!selectedCharacterId) {
      setEditingId(null);
      setDraft(emptyCharacter());
      return;
    }
    const character = characters.find((c) => c.id === selectedCharacterId);
    if (character) {
      setEditingId(character.id);
      setDraft(character);
    }
  }, [selectedCharacterId, characters]);

  async function save() {
    if (!draft.name?.trim()) return;
    if (editingId) {
      const updated = await api.updateCharacter(editingId, draft);
      setCharacters((prev) =>
        prev.map((c) => (c.id === updated.id ? updated : c)),
      );
    } else {
      const created = await api.createCharacter(draft);
      setCharacters((prev) => [created, ...prev]);
      setEditingId(created.id);
      onCharacterChange(created.id);
    }
  }

  async function remove(id: number) {
    await api.deleteCharacter(id);
    setCharacters((prev) => prev.filter((c) => c.id !== id));
    if (editingId === id) {
      setEditingId(null);
      setDraft(emptyCharacter());
      onCharacterChange(null);
    }
  }

  async function importFile(file: File) {
    const result = await api.importCharacter(file);
    setCharacters((prev) => [result.character, ...prev]);
    setEditingId(result.character.id);
    setDraft(result.character);
    onCharacterChange(result.character.id);
  }

  const fields: Array<{ key: keyof Character; label: string; rows?: number }> = [
    { key: "name", label: "Name" },
    { key: "description", label: "Description", rows: 3 },
    { key: "personality", label: "Personality", rows: 3 },
    { key: "scenario", label: "Scenario", rows: 3 },
    { key: "first_message", label: "First message", rows: 3 },
    { key: "example_dialogue", label: "Example dialogue", rows: 4 },
    { key: "system_prompt", label: "System prompt override", rows: 4 },
  ];

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => {
            setEditingId(null);
            setDraft(emptyCharacter());
          }}
          className="rounded-lg bg-violet-700 px-3 py-2 text-sm hover:bg-violet-600"
        >
          New character
        </button>
        <button
          type="button"
          onClick={() => fileRef.current?.click()}
          className="rounded-lg border border-violet-700 px-3 py-2 text-sm hover:bg-violet-900/30"
        >
          Import JSON/PNG
        </button>
        <input
          ref={fileRef}
          type="file"
          accept=".json,.png"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) void importFile(file);
            e.target.value = "";
          }}
        />
      </div>

      <div className="max-h-40 overflow-y-auto rounded-lg border border-violet-900/30">
        {characters.map((character) => (
          <button
            key={character.id}
            type="button"
            onClick={() => {
              setEditingId(character.id);
              setDraft(character);
              onCharacterChange(character.id);
            }}
            className={`flex w-full items-center justify-between px-3 py-2 text-left text-sm hover:bg-violet-900/20 ${
              editingId === character.id ? "bg-violet-800/30" : ""
            }`}
          >
            <span>{character.name}</span>
            <span
              role="button"
              onClick={(e) => {
                e.stopPropagation();
                void remove(character.id);
              }}
              className="text-xs text-red-300"
            >
              delete
            </span>
          </button>
        ))}
      </div>

      <div className="space-y-3">
        {fields.map(({ key, label, rows }) => (
          <label key={key} className="block text-sm">
            <span className="mb-1 block text-violet-200/80">{label}</span>
            {rows ? (
              <textarea
                value={(draft[key] as string) ?? ""}
                onChange={(e) =>
                  setDraft((prev) => ({ ...prev, [key]: e.target.value }))
                }
                rows={rows}
                className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
              />
            ) : (
              <input
                value={(draft[key] as string) ?? ""}
                onChange={(e) =>
                  setDraft((prev) => ({ ...prev, [key]: e.target.value }))
                }
                className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
              />
            )}
          </label>
        ))}
      </div>

      <button
        type="button"
        onClick={() => void save()}
        className="rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium hover:bg-violet-500"
      >
        Save character
      </button>
    </div>
  );
}
