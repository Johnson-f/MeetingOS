export const queryKeys = {
  status: ["backend", "status"] as const,
  analytics: ["backend", "analytics", "overview"] as const,
  currentUser: ["backend", "current-user"] as const,
  meetings: (limit: number, offset: number) =>
    ["backend", "meetings", limit, offset] as const,
  meeting: (meetingId: string) => ["backend", "meeting", meetingId] as const,
  note: (meetingId: string) => ["backend", "note", meetingId] as const,
  audio: (meetingId: string) => ["backend", "audio", meetingId] as const,
  participants: (meetingId: string) => ["participants", meetingId] as const,
  sharedMeeting: (token: string) => ["shared-meeting", token] as const,
};
