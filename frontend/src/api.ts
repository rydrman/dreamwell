import type {
  Character,
  Chat,
  ChatStreamPayload,
  Fact,
  Message,
  ModelInfo,
  QueueStatus,
  Settings,
} from "./types";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
    ...init,
  });
  if (!response.ok) {
    const detail = await response.text();
    throw new Error(detail || response.statusText);
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return response.json() as Promise<T>;
}

export const api = {
  health: () => request<{ status: string }>("/api/health"),

  listChats: () => request<Chat[]>("/api/chats"),
  createChat: (body: { title?: string; character_id?: number | null }) =>
    request<Chat>("/api/chats", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  updateChat: (id: number, body: { title?: string; character_id?: number | null }) =>
    request<Chat>(`/api/chats/${id}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  deleteChat: (id: number) =>
    request<{ ok: boolean }>(`/api/chats/${id}`, { method: "DELETE" }),
  getMessages: (chatId: number) =>
    request<Message[]>(`/api/chats/${chatId}/messages`),
  sendMessage: (chatId: number, content: string) =>
    request<Message>(`/api/chats/${chatId}/messages`, {
      method: "POST",
      body: JSON.stringify({ content }),
    }),
  getFacts: (chatId: number) => request<Fact[]>(`/api/chats/${chatId}/facts`),
  upsertFact: (chatId: number, key: string, value: string) =>
    request<Fact>(`/api/chats/${chatId}/facts`, {
      method: "PUT",
      body: JSON.stringify({ key, value }),
    }),
  deleteFact: (chatId: number, key: string) =>
    request<{ ok: boolean }>(`/api/chats/${chatId}/facts/${encodeURIComponent(key)}`, {
      method: "DELETE",
    }),
  getQueue: () => request<QueueStatus>("/api/chats/queue"),

  listCharacters: () => request<Character[]>("/api/characters"),
  createCharacter: (body: Partial<Character>) =>
    request<Character>("/api/characters", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  updateCharacter: (id: number, body: Partial<Character>) =>
    request<Character>(`/api/characters/${id}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  deleteCharacter: (id: number) =>
    request<{ ok: boolean }>(`/api/characters/${id}`, { method: "DELETE" }),
  importCharacter: async (file: File) => {
    const form = new FormData();
    form.append("file", file);
    const response = await fetch("/api/characters/import", {
      method: "POST",
      body: form,
    });
    if (!response.ok) {
      throw new Error(await response.text());
    }
    return response.json() as Promise<{ character: Character; source: string }>;
  },

  getSettings: () => request<Settings>("/api/settings"),
  updateSettings: (body: Partial<Settings>) =>
    request<Settings>("/api/settings", {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  listModels: () => request<ModelInfo[]>("/api/settings/models"),
};

export function openChatStream(
  chatId: number,
  onUpdate: (payload: ChatStreamPayload) => void,
  onIdle?: () => void,
): () => void {
  const source = new EventSource(`/api/chats/${chatId}/stream`);

  source.onmessage = (event) => {
    const payload = JSON.parse(event.data) as ChatStreamPayload;
    onUpdate(payload);
  };

  source.addEventListener("idle", () => {
    source.close();
    onIdle?.();
  });

  source.onerror = () => {
    source.close();
  };

  return () => source.close();
}
