"use client";

import { useQuery } from "@tanstack/react-query";

import { queryKeys } from "@/lib/service";

import { useBackendClient } from "./use-backend-client";

export function useAnalyticsOverviewQuery() {
  const client = useBackendClient();

  return useQuery({
    queryKey: queryKeys.analytics,
    queryFn: () => client.getAnalyticsOverview(),
  });
}
