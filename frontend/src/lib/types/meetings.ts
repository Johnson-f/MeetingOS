export type MeetingTimeMode = "future" | "starting_now" | "already_started";

export interface RecallBotView {
  id: string;
  recall_bot_id: string;
  bot_name: string;
  join_at: string | null;
  status: string;
  status_sub_code: string | null;
  failure_detail: string | null;
  joined_at: string | null;
  left_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface RecordingView {
  id: string;
  recall_recording_id: string | null;
  status: string;
  duration_seconds: number | null;
  started_at: string | null;
  ended_at: string | null;
  audio_asset_status: string | null;
  audio_playback_ready: boolean;
}

export interface TranscriptSegmentView {
  id: string;
  seq: number;
  speaker_label: string | null;
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface TranscriptionView {
  id: string;
  provider: string;
  model: string;
  language: string | null;
  status: string;
  full_text: string | null;
  segments: TranscriptSegmentView[];
  started_at: string | null;
  completed_at: string | null;
  error_message: string | null;
}

export interface ActionItemView {
  id: string;
  description: string;
  assignee_name: string | null;
  assignee_email: string | null;
  due_date: string | null;
  priority: string | null;
  status: string;
  sort_order: number;
}

export interface NoteView {
  id: string;
  provider: string;
  model: string;
  prompt_version: string;
  status: string;
  title: string | null;
  summary_markdown: string | null;
  key_points: string[];
  decisions: string[];
  risks: string[];
  action_items: ActionItemView[];
  generated_at: string | null;
}

export interface MeetingDetail {
  id: string;
  title: string;
  platform: string;
  source: string;
  meeting_time_mode: string;
  original_meeting_url: string;
  normalized_meeting_url: string;
  dedup_key: string;
  status: string;
  processing_status: string;
  scheduled_start_at: string | null;
  scheduled_end_at: string | null;
  actual_start_at: string | null;
  actual_end_at: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
  bot: RecallBotView | null;
  recording: RecordingView | null;
  transcription: TranscriptionView | null;
  note: NoteView | null;
}

export interface MeetingListItem {
  id: string;
  title: string;
  platform: string;
  meeting_time_mode: string;
  status: string;
  processing_status: string;
  scheduled_start_at: string | null;
  actual_start_at: string | null;
  actual_end_at: string | null;
  recall_bot_id: string | null;
  latest_note_summary: string | null;
  created_at: string;
  updated_at: string;
}

export interface MeetingsListResponse {
  items: MeetingListItem[];
  limit: number;
  offset: number;
}

export interface CreateMeetingPayload {
  meeting_url: string;
  title?: string;
  meeting_time_mode?: MeetingTimeMode;
  join_at?: string;
  bot_name?: string;
  auto_share_enabled?: boolean;
}

export interface UpdateMeetingPayload {
  title?: string;
  scheduled_start_at?: string;
  bot_name?: string;
  auto_share_enabled?: boolean;
}

export interface MeetingMutationResponse {
  meeting: MeetingDetail;
  created: boolean;
}

export interface MeetingActionResponse {
  meeting_id: string;
  status: string;
  processing_status: string;
}

export interface ParticipantView {
  id: string;
  display_name: string | null;
  email: string | null;
  is_host: boolean;
  provider_participant_id: string | null;
  first_joined_at: string | null;
  last_left_at: string | null;
}

export interface ParticipantsResponse {
  participants: ParticipantView[];
}

export interface ShareMeetingPayload {
  emails: string[];
}

export interface ShareMeetingResponse {
  status: string;
  recipient_count: number;
}

export interface SharedMeetingResponse {
  meeting: {
    title: string;
    scheduled_start_at: string | null;
    actual_start_at: string | null;
    actual_end_at: string | null;
    platform: string;
    note: NoteView | null;
    transcription: TranscriptionView | null;
  };
  participants: ParticipantView[];
  audio_url: string | null;
}

export interface MeetingResponse {
  meeting: MeetingDetail;
}

export interface NoteResponse {
  note: NoteView | null;
}

export interface SearchSource {
  meeting_id: string;
  meeting_title: string;
  text: string;
  start_ms: number;
  end_ms: number;
  speaker_label: string | null;
  relevance_score: number;
}

export interface SearchResponse {
  answer: string;
  sources: SearchSource[];
}

export interface ChatThread {
  id: string;
  user_id: string;
  workspace_id: string;
  title: string | null;
  created_at: string;
  updated_at: string;
}

export interface ChatMessageRecord {
  id: string;
  thread_id: string;
  role: "user" | "assistant";
  content: string;
  sources_json: string | null;
  created_at: string;
}

export interface ThreadsResponse {
  threads: ChatThread[];
}

export interface ThreadMessagesResponse {
  messages: ChatMessageRecord[];
}
