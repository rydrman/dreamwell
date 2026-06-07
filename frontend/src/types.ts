export type JobStatus =
  | "queued"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export type MessageRole = "system" | "user" | "assistant";

export interface Character {
  id: number;
  name: string;
  description: string;
  personality: string;
  scenario: string;
  first_message: string;
  example_dialogue: string;
  system_prompt: string;
  avatar_url: string | null;
  created_at: string;
  updated_at: string;
}

export interface Chat {
  id: number;
  title: string;
  character_id: number | null;
  summary: string;
  created_at: string;
  updated_at: string;
  active_job: Job | null;
  queued_jobs: number;
}

export interface Message {
  id: number;
  chat_id: number;
  role: MessageRole;
  content: string;
  is_summary: boolean;
  created_at: string;
  job_status: JobStatus | null;
}

export interface Fact {
  id: number;
  chat_id: number;
  key: string;
  value: string;
  updated_at: string;
}

export interface Job {
  id: number;
  chat_id: number;
  message_id: number;
  status: JobStatus;
  error: string | null;
  position: number;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
}

export interface QueueStatus {
  running: Job[];
  queued: Job[];
}

export interface Settings {
  inference_url: string;
  model: string;
  temperature: number;
  top_p: number;
  max_tokens: number;
  system_prompt_prefix: string;
  system_prompt_suffix: string;
  summarize_enabled: boolean;
  summarize_after_messages: number;
  summarize_keep_recent: number;
  facts_enabled: boolean;
  max_context_messages: number;
  max_concurrent_jobs: number;
}

export interface ModelInfo {
  id: string;
  name?: string | null;
}

export interface ChatStreamPayload {
  chat: Chat;
  messages: Message[];
  active_job: Job | null;
}
