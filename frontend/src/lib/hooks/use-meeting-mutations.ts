"use client";

import { useMutation, useQueryClient } from "@tanstack/react-query";

import { queryKeys } from "@/lib/service";
import type { CreateMeetingPayload } from "@/lib/types";

import { useBackendClient } from "./use-backend-client";

export function useCreateMeetingMutation() {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CreateMeetingPayload) =>
      client.createMeeting(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
    },
  });
}

export function useCancelMeetingMutation() {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (meetingId: string) => client.cancelMeeting(meetingId),
    onSuccess: (_, meetingId) => {
      queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
      queryClient.invalidateQueries({ queryKey: queryKeys.meeting(meetingId) });
    },
  });
}

export function useDeleteMeetingMutation() {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (meetingId: string) => client.deleteMeeting(meetingId),
    onSuccess: (_, meetingId) => {
      queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
      queryClient.removeQueries({ queryKey: queryKeys.meeting(meetingId) });
      queryClient.removeQueries({ queryKey: queryKeys.note(meetingId) });
      queryClient.removeQueries({ queryKey: queryKeys.audio(meetingId) });
    },
  });
}
