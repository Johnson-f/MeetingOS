"use client";

import { useQuery } from "@tanstack/react-query";

import { queryKeys } from "@/lib/service";

import { useBackendClient } from "./use-backend-client";

export function useMeetingQuery(meetingId: string) {
  const client = useBackendClient();

  return useQuery({
    queryKey: queryKeys.meeting(meetingId),
    queryFn: () => client.getMeeting(meetingId),
    enabled: Boolean(meetingId),
  });
}

export function useMeetingNoteQuery(meetingId: string) {
  const client = useBackendClient();

  return useQuery({
    queryKey: queryKeys.note(meetingId),
    queryFn: () => client.getNote(meetingId),
    enabled: Boolean(meetingId),
  });
}

export function useMeetingAudioBlobQuery(meetingId: string, enabled = true) {
  const client = useBackendClient();

  return useQuery({
    queryKey: queryKeys.audio(meetingId),
    queryFn: () => client.fetchMeetingAudioBlob(meetingId),
    enabled: enabled && Boolean(meetingId),
  });
}
