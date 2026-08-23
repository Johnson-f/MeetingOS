"use client";

import { useMutation } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";

export function useMeetingSearch() {
  const client = useBackendClient();

  return useMutation({
    mutationFn: ({ query, meetingId }: { query: string; meetingId?: string }) =>
      client.searchMeetings(query, meetingId),
  });
}
