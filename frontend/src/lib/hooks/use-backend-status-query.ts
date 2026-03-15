"use client";

import { useQuery } from "@tanstack/react-query";

import { createBackendClient } from "@/lib/backend_connection";
import { queryKeys } from "@/lib/service";

export function useBackendStatusQuery() {
  const client = createBackendClient();

  return useQuery({
    queryKey: queryKeys.status,
    queryFn: () => client.getStatus(),
  });
}
