import { useCallback, useEffect, useState } from "react";
import { api, openChatStream } from "./api";
import { ChatSidebar } from "./components/ChatSidebar";
import { Composer } from "./components/Composer";
import { MessageList } from "./components/MessageList";
import { QueuePanel } from "./components/QueuePanel";
import { RightPanel } from "./components/RightPanel";
import type { Chat, Message, QueueStatus } from "./types";

export default function App() {
  const [chats, setChats] = useState<Chat[]>([]);
  const [selectedChatId, setSelectedChatId] = useState<number | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [queue, setQueue] = useState<QueueStatus | null>(null);
  const [loading, setLoading] = useState(true);

  const selectedChat = chats.find((c) => c.id === selectedChatId) ?? null;

  const refreshChats = useCallback(async () => {
    const list = await api.listChats();
    setChats(list);
    return list;
  }, []);

  const refreshQueue = useCallback(async () => {
    const status = await api.getQueue();
    setQueue(status);
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const list = await refreshChats();
        if (list[0]) setSelectedChatId(list[0].id);
      } finally {
        setLoading(false);
      }
    })();
  }, [refreshChats]);

  useEffect(() => {
    if (!selectedChatId) {
      setMessages([]);
      return;
    }

    let cancelled = false;
    void api.getMessages(selectedChatId).then((msgs) => {
      if (!cancelled) setMessages(msgs);
    });

    const close = openChatStream(
      selectedChatId,
      (payload) => {
        setMessages(payload.messages);
        setChats((prev) =>
          prev.map((chat) =>
            chat.id === payload.chat.id ? payload.chat : chat,
          ),
        );
      },
      () => {
        void refreshChats();
        void refreshQueue();
      },
    );

    return () => {
      cancelled = true;
      close();
    };
  }, [selectedChatId, refreshChats, refreshQueue]);

  useEffect(() => {
    void refreshQueue();
    const interval = setInterval(() => {
      void refreshQueue();
      void refreshChats();
    }, 3000);
    return () => clearInterval(interval);
  }, [refreshChats, refreshQueue]);

  async function handleNewChat() {
    const characterId = selectedChat?.character_id ?? null;
    const chat = await api.createChat({
      title: `Chat ${chats.length + 1}`,
      character_id: characterId,
    });
    await refreshChats();
    setSelectedChatId(chat.id);
  }

  async function handleDeleteChat(id: number) {
    await api.deleteChat(id);
    const list = await refreshChats();
    if (selectedChatId === id) {
      setSelectedChatId(list[0]?.id ?? null);
    }
  }

  async function handleSend(content: string) {
    if (!selectedChatId) return;
    await api.sendMessage(selectedChatId, content);
    const msgs = await api.getMessages(selectedChatId);
    setMessages(msgs);
    await refreshChats();
    await refreshQueue();
  }

  async function handleCharacterChange(characterId: number | null) {
    if (!selectedChatId) return;
    await api.updateChat(selectedChatId, { character_id: characterId });
    await refreshChats();
  }

  if (loading) {
    return (
      <div className="flex min-h-screen items-center justify-center text-violet-200/70">
        Loading Dreamwell…
      </div>
    );
  }

  return (
    <div className="flex h-screen overflow-hidden">
      <ChatSidebar
        chats={chats}
        selectedId={selectedChatId}
        onSelect={setSelectedChatId}
        onNew={() => void handleNewChat()}
        onDelete={(id) => void handleDeleteChat(id)}
      />

      <main className="flex min-w-0 flex-1 flex-col">
        <QueuePanel queue={queue} />
        <header className="flex items-center justify-between border-b border-violet-900/30 px-4 py-3">
          <div>
            <h1 className="text-lg font-semibold text-violet-50">
              {selectedChat?.title ?? "Select a chat"}
            </h1>
            <p className="text-sm text-violet-200/60">
              Responses stream on the server — switch chats freely while they
              generate.
            </p>
          </div>
          {selectedChat?.summary ? (
            <details className="max-w-md text-sm text-violet-200/70">
              <summary className="cursor-pointer">Conversation summary</summary>
              <p className="mt-2 whitespace-pre-wrap">{selectedChat.summary}</p>
            </details>
          ) : null}
        </header>

        <MessageList messages={messages} />
        <Composer disabled={!selectedChatId} onSend={handleSend} />
      </main>

      <RightPanel
        chatId={selectedChatId}
        characterId={selectedChat?.character_id ?? null}
        onCharacterChange={(id) => void handleCharacterChange(id)}
      />
    </div>
  );
}
