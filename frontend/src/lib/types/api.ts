export interface ServiceStatusResponse {
  api: {
    name: string;
    environment: string;
    version: string;
  };
  bind_address: string;
  role: string;
  recall_ai_ready: boolean;
  groq_ready: boolean;
  storage_ready: boolean;
  turso_ready: boolean;
}

export interface CurrentUserResponse {
  user_id: string;
  workspace_id: string;
}

export interface GoogleCalendarStatusResponse {
  connected: boolean;
  status?: "connected" | "auth_required" | "disconnected";
  calendars_count?: number;
  last_synced_at?: string | null;
  connected_at?: string | null;
}
