"use client"

import * as React from "react"
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Pagination,
  PaginationContent,
  PaginationItem,
  PaginationNext,
  PaginationPrevious,
} from "@/components/ui/pagination"
import { useMeetingsQuery } from "@/lib/hooks/use-meetings-query"
import type { MeetingListItem } from "@/lib/types"

const PAGE_SIZE = 10

function formatTime(dateStr: string | null) {
  if (!dateStr) return null
  const date = new Date(dateStr)
  return date.toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  })
}

function formatRelativeDate(dateStr: string | null) {
  if (!dateStr) return "No date"
  const date = new Date(dateStr)
  const now = new Date()
  const isToday = date.toDateString() === now.toDateString()
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  const isYesterday = date.toDateString() === yesterday.toDateString()

  if (isToday) return "Today"
  if (isYesterday) return "Yesterday"
  return date.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
  })
}

function platformLabel(platform: string) {
  switch (platform) {
    case "google_meet":
      return "Google Meet"
    case "zoom":
      return "Zoom"
    case "microsoft_teams":
      return "Microsoft Teams"
    default:
      return platform
  }
}

function durationLabel(meeting: MeetingListItem) {
  if (!meeting.actual_start_at || !meeting.actual_end_at) return null
  const start = new Date(meeting.actual_start_at).getTime()
  const end = new Date(meeting.actual_end_at).getTime()
  const mins = Math.round((end - start) / 60000)
  if (mins < 1) return "< 1 min"
  if (mins < 60) return `${mins} min`
  const hrs = Math.floor(mins / 60)
  const remainder = mins % 60
  return remainder > 0 ? `${hrs}h ${remainder}m` : `${hrs}h`
}

function MeetingRow({ meeting }: { meeting: MeetingListItem }) {
  const effectiveDate = meeting.actual_start_at ?? meeting.scheduled_start_at ?? meeting.created_at
  const time = formatTime(effectiveDate)
  const relDate = formatRelativeDate(effectiveDate)
  const duration = durationLabel(meeting)
  const hasNotes = !!meeting.latest_note_summary

  return (
    <div className="flex items-center justify-between gap-4 rounded-lg border border-transparent px-4 py-3 transition-colors hover:bg-muted/50">
      <div className="flex flex-col gap-0.5 min-w-0">
        <span className="text-sm font-medium truncate">{meeting.title}</span>
        <span className="text-xs text-muted-foreground">
          {relDate}
          {time ? ` \u00b7 ${time}` : ""}
          {duration ? ` \u00b7 ${duration}` : ""}
          {" \u00b7 "}
          {platformLabel(meeting.platform)}
        </span>
      </div>
      <div className="shrink-0">
        {hasNotes ? (
          <Badge variant="outline" className="text-emerald-600 border-emerald-200 bg-emerald-50 dark:bg-emerald-950/30 dark:border-emerald-800">
            Notes ready
          </Badge>
        ) : (
          <Badge variant="outline" className="text-muted-foreground">
            Completed
          </Badge>
        )}
      </div>
    </div>
  )
}

export function RecentMeetings() {
  const [page, setPage] = React.useState(0)
  const { data, isLoading } = useMeetingsQuery(100, 0)
  const allMeetings = (data?.items ?? []).filter((m) => m.status === "completed")
  const totalPages = Math.ceil(allMeetings.length / PAGE_SIZE)
  const meetings = allMeetings.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)

  return (
    <Card>
      <CardHeader>
        <CardTitle>Recent Meetings</CardTitle>
      </CardHeader>
      <CardContent className="px-2 pb-4">
        {isLoading ? (
          <div className="flex flex-col gap-3 px-4">
            {Array.from({ length: 3 }).map((_, i) => (
              <div key={i} className="flex items-center justify-between gap-4">
                <div className="flex flex-col gap-1.5">
                  <Skeleton className="h-4 w-48" />
                  <Skeleton className="h-3 w-32" />
                </div>
                <Skeleton className="h-5 w-20" />
              </div>
            ))}
          </div>
        ) : allMeetings.length === 0 ? (
          <p className="px-4 py-6 text-center text-sm text-muted-foreground">
            No completed meetings yet.
          </p>
        ) : (
          <>
            <div className="flex flex-col divide-y">
              {meetings.map((meeting) => (
                <MeetingRow key={meeting.id} meeting={meeting} />
              ))}
            </div>
            {totalPages > 1 && (
              <Pagination className="mt-3">
                <PaginationContent>
                  <PaginationItem>
                    <PaginationPrevious
                      onClick={() => setPage((p) => Math.max(0, p - 1))}
                      className={page === 0 ? "pointer-events-none opacity-50" : "cursor-pointer"}
                    />
                  </PaginationItem>
                  <PaginationItem>
                    <span className="px-3 text-xs text-muted-foreground">
                      {page + 1} / {totalPages}
                    </span>
                  </PaginationItem>
                  <PaginationItem>
                    <PaginationNext
                      onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
                      className={page >= totalPages - 1 ? "pointer-events-none opacity-50" : "cursor-pointer"}
                    />
                  </PaginationItem>
                </PaginationContent>
              </Pagination>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}
