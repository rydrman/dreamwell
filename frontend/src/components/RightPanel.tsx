import { useState } from "react";
import { CharacterPanel } from "./CharacterPanel";
import { FactsPanel } from "./FactsPanel";
import { SettingsPanel } from "./SettingsPanel";

type Tab = "character" | "facts" | "settings";

interface Props {
  chatId: number | null;
  characterId: number | null;
  onCharacterChange: (id: number | null) => void;
}

export function RightPanel({ chatId, characterId, onCharacterChange }: Props) {
  const [tab, setTab] = useState<Tab>("character");

  const tabs: Array<{ id: Tab; label: string }> = [
    { id: "character", label: "Character" },
    { id: "facts", label: "Facts" },
    { id: "settings", label: "Settings" },
  ];

  return (
    <aside className="flex h-full w-96 shrink-0 flex-col border-l border-violet-900/40 bg-[#15111c]/90">
      <div className="flex border-b border-violet-900/30">
        {tabs.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => setTab(item.id)}
            className={`flex-1 px-3 py-3 text-sm ${
              tab === item.id
                ? "border-b-2 border-violet-400 text-violet-100"
                : "text-violet-200/60 hover:text-violet-100"
            }`}
          >
            {item.label}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto p-4">
        {tab === "character" ? (
          <CharacterPanel
            selectedCharacterId={characterId}
            onCharacterChange={onCharacterChange}
          />
        ) : null}
        {tab === "facts" ? <FactsPanel chatId={chatId} /> : null}
        {tab === "settings" ? <SettingsPanel /> : null}
      </div>
    </aside>
  );
}
