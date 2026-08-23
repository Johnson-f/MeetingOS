"use client";

import { useAuth } from "@clerk/nextjs";

import { createBackendClient } from "@/lib/backend_connection";

export function useBackendClient() {
  const { getToken } = useAuth();
  return createBackendClient({ getToken });
}
