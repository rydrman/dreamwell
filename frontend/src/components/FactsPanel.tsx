import { useEffect, useState } from "react";
import { api } from "../api";
import type { Fact } from "../types";

interface Props {
  chatId: number | null;
}

export function FactsPanel({ chatId }: Props) {
  const [facts, setFacts] = useState<Fact[]>([]);
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");

  useEffect(() => {
    if (!chatId) {
      setFacts([]);
      return;
    }
    void api.getFacts(chatId).then(setFacts).catch(console.error);
  }, [chatId]);

  async function save() {
    if (!chatId || !key.trim()) return;
    const fact = await api.upsertFact(chatId, key.trim(), value);
    setFacts((prev) => {
      const others = prev.filter((f) => f.key !== fact.key);
      return [...others, fact].sort((a, b) => a.key.localeCompare(b.key));
    });
    setKey("");
    setValue("");
  }

  async function remove(factKey: string) {
    if (!chatId) return;
    await api.deleteFact(chatId, factKey);
    setFacts((prev) => prev.filter((f) => f.key !== factKey));
  }

  if (!chatId) {
    return (
      <div className="text-sm text-violet-200/50">
        Select a chat to view facts.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <p className="text-sm text-violet-200/70">
        Facts are injected into the prompt. The model can update them with{" "}
        <code className="rounded bg-black/30 px-1">
          {"<fact key=\"name\">value</fact>"}
        </code>
        .
      </p>

      <div className="space-y-2">
        {facts.length === 0 ? (
          <p className="text-sm text-violet-200/40">No facts yet.</p>
        ) : (
          facts.map((fact) => (
            <div
              key={fact.id}
              className="rounded-lg border border-violet-800/40 bg-black/20 p-3"
            >
              <div className="flex items-start justify-between gap-2">
                <div className="font-medium text-violet-200">{fact.key}</div>
                <button
                  type="button"
                  onClick={() => void remove(fact.key)}
                  className="text-xs text-red-300 hover:text-red-200"
                >
                  delete
                </button>
              </div>
              <div className="mt-1 whitespace-pre-wrap text-sm text-violet-50/90">
                {fact.value}
              </div>
            </div>
          ))
        )}
      </div>

      <div className="space-y-2 border-t border-violet-900/30 pt-4">
        <input
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="Key"
          className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2 text-sm"
        />
        <textarea
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="Value"
          rows={3}
          className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2 text-sm"
        />
        <button
          type="button"
          onClick={() => void save()}
          className="rounded-lg bg-violet-700 px-3 py-2 text-sm hover:bg-violet-600"
        >
          Save fact
        </button>
      </div>
    </div>
  );
}
