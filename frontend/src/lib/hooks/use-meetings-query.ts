"use client";

import { useQuery } from "@tanstack/react-query";

import { queryKeys } from "@/lib/service";

import { useBackendClient } from "./use-backend-client";

export function useMeetingsQuery(limit = 25, offset = 0) {
  const client = useBackendClient();

  return useQuery({
    queryKey: queryKeys.meetings(limit, offset),
    queryFn: () => client.listMeetings(limit, offset),
  });
}
