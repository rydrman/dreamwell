import type { Chat } from "../types";

interface Props {
  chats: Chat[];
  selectedId: number | null;
  onSelect: (id: number) => void;
  onNew: () => void;
  onDelete: (id: number) => void;
}

function statusLabel(chat: Chat): string | null {
  if (!chat.active_job) return null;
  if (chat.active_job.status === "running") return "writing…";
  if (chat.active_job.status === "queued") {
    return chat.queued_jobs > 1 ? `queued (${chat.queued_jobs})` : "queued";
  }
  return chat.active_job.status;
}

export function ChatSidebar({
  chats,
  selectedId,
  onSelect,
  onNew,
  onDelete,
}: Props) {
  return (
    <aside className="flex h-full w-72 shrink-0 flex-col border-r border-violet-900/40 bg-[#15111c]/90">
      <div className="flex items-center justify-between border-b border-violet-900/30 px-4 py-4">
        <div>
          <div className="text-xs uppercase tracking-[0.2em] text-violet-300/70">
            Dreamwell
          </div>
          <div className="text-lg font-semibold text-violet-100">Chats</div>
        </div>
        <button
          type="button"
          onClick={onNew}
          className="rounded-lg bg-violet-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-violet-600"
        >
          New
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-2">
        {chats.length === 0 ? (
          <p className="px-3 py-6 text-sm text-violet-200/50">
            No chats yet. Start one and queue replies across threads.
          </p>
        ) : (
          chats.map((chat) => {
            const status = statusLabel(chat);
            const selected = chat.id === selectedId;
            return (
              <div
                key={chat.id}
                className={`group mb-1 flex items-start gap-2 rounded-xl px-3 py-2 ${
                  selected
                    ? "bg-violet-800/40 ring-1 ring-violet-500/40"
                    : "hover:bg-violet-900/20"
                }`}
              >
                <button
                  type="button"
                  onClick={() => onSelect(chat.id)}
                  className="min-w-0 flex-1 text-left"
                >
                  <div className="truncate font-medium text-violet-50">
                    {chat.title}
                  </div>
                  <div className="mt-1 flex items-center gap-2 text-xs text-violet-200/60">
                    <span>{new Date(chat.updated_at).toLocaleString()}</span>
                    {status ? (
                      <span className="rounded-full bg-amber-500/20 px-2 py-0.5 text-amber-200">
                        {status}
                      </span>
                    ) : null}
                  </div>
                </button>
                <button
                  type="button"
                  onClick={() => onDelete(chat.id)}
                  className="hidden rounded px-2 py-1 text-xs text-red-300 group-hover:block hover:bg-red-900/30"
                  title="Delete chat"
                >
                  ✕
                </button>
              </div>
            );
          })
        )}
      </div>
    </aside>
  );
}
