"use client";

import { useEffect, useRef } from "react";
import { useAuth } from "@clerk/nextjs";
import { useQueryClient } from "@tanstack/react-query";

const BACKEND_URL =
  process.env.NEXT_PUBLIC_BACKEND_URL?.replace(/\/$/, "") ?? "";

export function useRealtimeEvents() {
  const { getToken } = useAuth();
  const queryClient = useQueryClient();
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    if (!BACKEND_URL) return;

    let cancelled = false;

    async function connect() {
      const token = await getToken();
      if (cancelled) return;

      const url = `${BACKEND_URL}/api/v1/events${token ? `?token=${token}` : ""}`;
      const es = new EventSource(url);
      eventSourceRef.current = es;

      es.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);

          if (data.type === "meeting_updated") {
            // Invalidate meetings list and analytics
            queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
            queryClient.invalidateQueries({ queryKey: ["backend", "analytics"] });

            // If a specific meeting_id, invalidate that too
            if (data.meeting_id) {
              queryClient.invalidateQueries({
                queryKey: ["backend", "meeting", data.meeting_id],
              });
              queryClient.invalidateQueries({
                queryKey: ["backend", "note", data.meeting_id],
              });
              queryClient.invalidateQueries({
                queryKey: ["backend", "audio", data.meeting_id],
              });
            }
          }
        } catch {
          // ignore malformed events
        }
      };

      es.onerror = () => {
        // EventSource auto-reconnects, but close and retry with fresh token
        es.close();
        if (!cancelled) {
          setTimeout(connect, 3000);
        }
      };
    }

    connect();

    return () => {
      cancelled = true;
      eventSourceRef.current?.close();
    };
  }, [getToken, queryClient]);
}
