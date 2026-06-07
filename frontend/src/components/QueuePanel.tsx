import type { QueueStatus } from "../types";

interface Props {
  queue: QueueStatus | null;
}

export function QueuePanel({ queue }: Props) {
  if (!queue) return null;
  const total = queue.running.length + queue.queued.length;
  if (total === 0) return null;

  return (
    <div className="border-b border-violet-900/30 bg-[#1a1424]/80 px-4 py-2 text-sm text-violet-100/80">
      <span className="font-medium text-violet-200">Queue:</span>{" "}
      {queue.running.length > 0 ? (
        <span>
          running chat #{queue.running.map((j) => j.chat_id).join(", #")}
        </span>
      ) : null}
      {queue.running.length > 0 && queue.queued.length > 0 ? " · " : null}
      {queue.queued.length > 0 ? (
        <span>{queue.queued.length} waiting</span>
      ) : null}
    </div>
  );
}
