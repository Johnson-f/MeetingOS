"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";

export function useGoogleCalendarConnect() {
  const client = useBackendClient();

  return useMutation({
    mutationFn: async () => {
      const { url } = await client.getGoogleCalendarConnectUrl();
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
      queryClient.invalidateQueries({
        queryKey: ["backend", "google-calendar-status"],
      });
    },
  });
}

export function useGoogleCalendarResync() {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => client.resyncGoogleCalendar(),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: ["backend", "google-calendar-status"],
      });
      queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
    },
  });
}

export function useGoogleCalendarStatus() {
  const client = useBackendClient();

  return useQuery({
    queryKey: ["backend", "google-calendar-status"],
    queryFn: () => client.getGoogleCalendarStatus(),
    staleTime: 30_000,
  });
}
