"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";

export function useGoogleCalendarConnect() {
  const client = useBackendClient();

  return useMutation({
    mutationFn: async () => {
      const { url } = await client.getGoogleCalendarConnectUrl();
      // Redirect the user to Google OAuth
      window.location.href = url;
    },
  });
}

export function useGoogleCalendarDisconnect() {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => client.disconnectGoogleCalendar(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["backend", "current-user"] });
      queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
    },
  });
}

export function useGoogleCalendarResync() {
  const client = useBackendClient();

  return useMutation({
    mutationFn: () => client.resyncGoogleCalendar(),
  });
}
