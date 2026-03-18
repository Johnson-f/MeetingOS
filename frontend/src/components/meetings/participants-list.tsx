"use client"

import { Skeleton } from "@/components/ui/skeleton"
import { useParticipantsQuery } from "@/lib/hooks/use-participants-query"

export function ParticipantsList({ meetingId }: { meetingId: string }) {
  const { data, isLoading, isError } = useParticipantsQuery(meetingId)

  if (isLoading) {
    return (
      <div className="flex flex-col gap-2">
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="flex items-center gap-3">
            <Skeleton className="size-7 rounded-full" />
            <div className="flex-1">
              <Skeleton className="h-3 w-32 mb-1" />
              <Skeleton className="h-2.5 w-24" />
            </div>
          </div>
        ))}
      </div>
    )
  }

  if (isError) {
    return (
      <p className="text-xs text-muted-foreground">
        Could not load participants.
      </p>
    )
  }

  const participants = data?.participants ?? []

  if (participants.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">No participants recorded.</p>
    )
  }

  return (
    <div className="flex flex-col gap-2">
      {participants.map((p) => {
        const hasJoined = Boolean(p.first_joined_at)
        const name = p.display_name ?? p.email ?? "Unknown"
        return (
          <div key={p.id} className="flex items-start gap-3 py-1">
            <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-secondary text-xs font-semibold uppercase text-secondary-foreground">
              {name.charAt(0)}
            </div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-1.5">
                <span className="text-sm font-medium truncate">{name}</span>
                {p.is_host && (
                  <span className="inline-flex items-center rounded-full bg-primary/10 px-1.5 py-0.5 text-[0.6rem] font-semibold uppercase tracking-wide text-primary">
                    Host
                  </span>
                )}
              </div>
              {p.email && p.display_name && (
                <p className="text-xs text-muted-foreground truncate">{p.email}</p>
              )}
              <p className="text-xs text-muted-foreground">
                {hasJoined ? "Joined" : "Invited"}
              </p>
            </div>
          </div>
        )
      })}
    </div>
  )
}
