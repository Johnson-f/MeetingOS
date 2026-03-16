import type {
  AnalyticsOverview,
  CreateMeetingPayload,
  CurrentUserResponse,
  MeetingActionResponse,
  MeetingMutationResponse,
  MeetingResponse,
  MeetingsListResponse,
  NoteResponse,
  SearchResponse,
  ServiceStatusResponse,
  UpdateMeetingPayload,
} from "@/lib/types";

export class BackendApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "BackendApiError";
    this.status = status;
  }
}

export type TokenProvider = () => Promise<string | null>;

export interface BackendClientOptions {
  baseUrl?: string;
  getToken?: TokenProvider;
}

export class BackendClient {
  private readonly baseUrl: string;
  private readonly getToken?: TokenProvider;

  constructor(options: BackendClientOptions = {}) {
    this.baseUrl =
      options.baseUrl ??
      process.env.NEXT_PUBLIC_BACKEND_URL?.replace(/\/$/, "") ??
      "";
    this.getToken = options.getToken;
  }

  private resolveUrl(path: string) {
    if (!this.baseUrl) {
      throw new Error(
        "NEXT_PUBLIC_BACKEND_URL is not configured for the frontend backend client",
      );
    }

    return `${this.baseUrl}${path}`;
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const headers = new Headers(init.headers);
    headers.set("Accept", "application/json");

    if (init.body && !headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }

    if (this.getToken) {
      const token = await this.getToken();
      if (token) {
        headers.set("Authorization", `Bearer ${token}`);
      }
    }

    const response = await fetch(this.resolveUrl(path), {
      ...init,
      headers,
      cache: "no-store",
    });

    if (!response.ok) {
      let message = `Backend request failed with status ${response.status}`;
      try {
        const payload = (await response.json()) as { error?: string };
        if (payload.error) {
          message = payload.error;
        }
      } catch {}

      throw new BackendApiError(response.status, message);
    }

    if (response.status === 204) {
      return undefined as T;
    }

    return (await response.json()) as T;
  }

  getStatus() {
    return this.request<ServiceStatusResponse>("/api/v1/status");
  }

  getAnalyticsOverview() {
    return this.request<AnalyticsOverview>("/api/v1/analytics/overview");
  }

  getCurrentUser() {
    return this.request<CurrentUserResponse>("/api/v1/me");
  }

  listMeetings(limit = 25, offset = 0) {
    return this.request<MeetingsListResponse>(
      `/api/v1/meetings?limit=${limit}&offset=${offset}`,
    );
  }

  createMeeting(payload: CreateMeetingPayload) {
    return this.request<MeetingMutationResponse>("/api/v1/meetings", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  updateMeeting(meetingId: string, payload: UpdateMeetingPayload) {
    return this.request<MeetingResponse>(`/api/v1/meetings/${meetingId}`, {
      method: "PATCH",
      body: JSON.stringify(payload),
    });
  }

  getMeeting(meetingId: string) {
    return this.request<MeetingResponse>(`/api/v1/meetings/${meetingId}`);
  }

  getNote(meetingId: string) {
    return this.request<NoteResponse>(`/api/v1/notes/${meetingId}`);
  }

  cancelMeeting(meetingId: string) {
    return this.request<MeetingActionResponse>(
      `/api/v1/meetings/${meetingId}/cancel`,
      {
        method: "POST",
      },
    );
  }

  deleteMeeting(meetingId: string) {
    return this.request<void>(`/api/v1/meetings/${meetingId}`, {
      method: "DELETE",
    });
  }

  searchMeetings(query: string, meetingId?: string) {
    return this.request<SearchResponse>("/api/v1/meetings/search", {
      method: "POST",
      body: JSON.stringify({ query, meeting_id: meetingId }),
    });
  }

  getAudioUrl(meetingId: string) {
    return this.request<{ url: string }>(`/api/v1/meetings/${meetingId}/audio`);
  }

  // Google Calendar
  getGoogleCalendarConnectUrl() {
    return this.request<{ url: string }>("/api/v1/calendar/google/connect");
  }

  disconnectGoogleCalendar() {
    return this.request<void>("/api/v1/calendar/google/disconnect", {
      method: "POST",
    });
  }

  resyncGoogleCalendar() {
    return this.request<{ status: string }>("/api/v1/calendar/google/resync", {
      method: "POST",
    });
  }

  async fetchMeetingAudioBlob(meetingId: string) {
    const headers = new Headers();
    headers.set("Accept", "audio/mpeg");

    if (this.getToken) {
      const token = await this.getToken();
      if (token) {
        headers.set("Authorization", `Bearer ${token}`);
      }
    }

    const response = await fetch(
      this.resolveUrl(`/api/v1/meetings/${meetingId}/audio`),
      {
        method: "GET",
        headers,
        cache: "no-store",
      },
    );

    if (!response.ok) {
      let message = `Audio request failed with status ${response.status}`;
      try {
        const payload = (await response.json()) as { error?: string };
        if (payload.error) {
          message = payload.error;
        }
      } catch {}

      throw new BackendApiError(response.status, message);
    }

    return response.blob();
  }
}

export function createBackendClient(options: BackendClientOptions = {}) {
  return new BackendClient(options);
}
