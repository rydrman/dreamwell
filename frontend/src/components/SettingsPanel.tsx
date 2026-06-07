import { useEffect, useState } from "react";
import { api } from "../api";
import type { ModelInfo, Settings } from "../types";

export function SettingsPanel() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [saving, setSaving] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);

  useEffect(() => {
    void api.getSettings().then(setSettings).catch(console.error);
  }, []);

  async function refreshModels() {
    setModelError(null);
    try {
      const list = await api.listModels();
      setModels(list);
    } catch (err) {
      setModelError(err instanceof Error ? err.message : "Failed to load models");
    }
  }

  async function save() {
    if (!settings) return;
    setSaving(true);
    try {
      const updated = await api.updateSettings(settings);
      setSettings(updated);
    } finally {
      setSaving(false);
    }
  }

  if (!settings) {
    return <div className="text-sm text-violet-200/50">Loading settings…</div>;
  }

  const field = (
    label: string,
    child: React.ReactNode,
    hint?: string,
  ) => (
    <label className="block text-sm">
      <span className="mb-1 block text-violet-200/80">{label}</span>
      {child}
      {hint ? <span className="mt-1 block text-xs text-violet-200/50">{hint}</span> : null}
    </label>
  );

  return (
    <div className="space-y-4">
      {field(
        "Inference server (OpenAI-compatible)",
        <input
          value={settings.inference_url}
          onChange={(e) =>
            setSettings({ ...settings, inference_url: e.target.value })
          }
          className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
          placeholder="http://localhost:11434/v1"
        />,
        "Works with Ollama, vLLM, text-generation-webui, etc.",
      )}

      <div className="flex gap-2">
        <div className="flex-1">
          {field(
            "Model",
            <select
              value={settings.model}
              onChange={(e) =>
                setSettings({ ...settings, model: e.target.value })
              }
              className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
            >
              <option value="">Select a model</option>
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name || m.id}
                </option>
              ))}
            </select>,
          )}
        </div>
        <button
          type="button"
          onClick={() => void refreshModels()}
          className="self-end rounded-lg border border-violet-700 px-3 py-2 text-sm hover:bg-violet-900/30"
        >
          Refresh
        </button>
      </div>
      {modelError ? (
        <p className="text-sm text-red-300">{modelError}</p>
      ) : null}

      <div className="grid grid-cols-2 gap-3">
        {field(
          "Temperature",
          <input
            type="number"
            step="0.05"
            value={settings.temperature}
            onChange={(e) =>
              setSettings({ ...settings, temperature: Number(e.target.value) })
            }
            className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
          />,
        )}
        {field(
          "Top P",
          <input
            type="number"
            step="0.05"
            value={settings.top_p}
            onChange={(e) =>
              setSettings({ ...settings, top_p: Number(e.target.value) })
            }
            className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
          />,
        )}
        {field(
          "Max tokens",
          <input
            type="number"
            value={settings.max_tokens}
            onChange={(e) =>
              setSettings({ ...settings, max_tokens: Number(e.target.value) })
            }
            className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
          />,
        )}
        {field(
          "Max concurrent jobs",
          <input
            type="number"
            min={1}
            value={settings.max_concurrent_jobs}
            onChange={(e) =>
              setSettings({
                ...settings,
                max_concurrent_jobs: Number(e.target.value),
              })
            }
            className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
          />,
          "Set to 1 to process chats slowly one at a time.",
        )}
      </div>

      {field(
        "System prompt prefix",
        <textarea
          rows={3}
          value={settings.system_prompt_prefix}
          onChange={(e) =>
            setSettings({ ...settings, system_prompt_prefix: e.target.value })
          }
          className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
        />,
      )}
      {field(
        "System prompt suffix",
        <textarea
          rows={3}
          value={settings.system_prompt_suffix}
          onChange={(e) =>
            setSettings({ ...settings, system_prompt_suffix: e.target.value })
          }
          className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
        />,
      )}

      <div className="rounded-xl border border-violet-900/30 p-4">
        <div className="mb-3 font-medium text-violet-100">Auto summarize</div>
        <label className="mb-3 flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={settings.summarize_enabled}
            onChange={(e) =>
              setSettings({ ...settings, summarize_enabled: e.target.checked })
            }
          />
          Enable summarization
        </label>
        <div className="grid grid-cols-2 gap-3">
          {field(
            "Summarize after N messages",
            <input
              type="number"
              value={settings.summarize_after_messages}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  summarize_after_messages: Number(e.target.value),
                })
              }
              className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
            />,
          )}
          {field(
            "Keep recent messages",
            <input
              type="number"
              value={settings.summarize_keep_recent}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  summarize_keep_recent: Number(e.target.value),
                })
              }
              className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
            />,
          )}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3">
        {field(
          "Max context messages",
          <input
            type="number"
            value={settings.max_context_messages}
            onChange={(e) =>
              setSettings({
                ...settings,
                max_context_messages: Number(e.target.value),
              })
            }
            className="w-full rounded-lg border border-violet-800/50 bg-[#0f0d12] px-3 py-2"
          />,
        )}
        <label className="flex items-end gap-2 pb-2 text-sm">
          <input
            type="checkbox"
            checked={settings.facts_enabled}
            onChange={(e) =>
              setSettings({ ...settings, facts_enabled: e.target.checked })
            }
          />
          Enable KV facts in prompts
        </label>
      </div>

      <button
        type="button"
        onClick={() => void save()}
        disabled={saving}
        className="rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium hover:bg-violet-500 disabled:opacity-50"
      >
        {saving ? "Saving…" : "Save settings"}
      </button>
    </div>
  );
}
