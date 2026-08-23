"use client"

import { useRealtimeEvents } from "@/lib/hooks/use-realtime-events"

export function RealtimeProvider({ children }: { children: React.ReactNode }) {
  useRealtimeEvents()
  return <>{children}</>
}
