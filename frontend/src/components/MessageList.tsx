import type { Message } from "../types";

interface Props {
  messages: Message[];
}

export function MessageList({ messages }: Props) {
  return (
    <div className="flex-1 space-y-4 overflow-y-auto px-4 py-4">
      {messages.length === 0 ? (
        <div className="rounded-2xl border border-dashed border-violet-800/50 px-6 py-10 text-center text-violet-200/60">
          Send a message to queue a reply. You can switch chats while it
          generates server-side.
        </div>
      ) : (
        messages.map((message) => {
          const isUser = message.role === "user";
          const streaming =
            message.job_status === "running" || message.job_status === "queued";

          return (
            <div
              key={message.id}
              className={`flex ${isUser ? "justify-end" : "justify-start"}`}
            >
              <div
                className={`max-w-[85%] rounded-2xl px-4 py-3 shadow-lg ${
                  isUser
                    ? "bg-violet-700/80 text-white"
                    : "bg-[#221a2d] text-violet-50 ring-1 ring-violet-800/40"
                }`}
              >
                <div className="mb-1 text-xs uppercase tracking-wide opacity-60">
                  {message.role}
                  {streaming ? " · streaming on server" : ""}
                  {message.job_status === "failed" ? " · failed" : ""}
                </div>
                <div className="whitespace-pre-wrap leading-relaxed">
                  {message.content || (streaming ? "…" : "")}
                </div>
              </div>
            </div>
          );
        })
      )}
    </div>
  );
}
