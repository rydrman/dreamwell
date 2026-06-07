import { useState } from "react";

interface Props {
  disabled?: boolean;
  onSend: (content: string) => Promise<void>;
}

export function Composer({ disabled, onSend }: Props) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);

  async function handleSubmit() {
    const content = text.trim();
    if (!content || sending || disabled) return;
    setSending(true);
    try {
      await onSend(content);
      setText("");
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="border-t border-violet-900/30 bg-[#15111c]/90 p-4">
      <div className="flex gap-3">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void handleSubmit();
            }
          }}
          rows={3}
          placeholder="Write your message… (Enter to send, Shift+Enter for newline)"
          className="min-h-[80px] flex-1 resize-y rounded-xl border border-violet-800/50 bg-[#0f0d12] px-4 py-3 text-violet-50 outline-none focus:border-violet-500"
          disabled={disabled || sending}
        />
        <button
          type="button"
          onClick={() => void handleSubmit()}
          disabled={disabled || sending || !text.trim()}
          className="self-end rounded-xl bg-violet-600 px-5 py-3 font-medium text-white hover:bg-violet-500 disabled:opacity-40"
        >
          {sending ? "Queuing…" : "Send"}
        </button>
      </div>
    </div>
  );
}
