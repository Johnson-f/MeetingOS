"use client";

import { useQuery } from "@tanstack/react-query";

import { queryKeys } from "@/lib/service";

import { useBackendClient } from "./use-backend-client";

export function useCurrentUserQuery() {
  const client = useBackendClient();

  return useQuery({
    queryKey: queryKeys.currentUser,
    queryFn: () => client.getCurrentUser(),
  });
}
