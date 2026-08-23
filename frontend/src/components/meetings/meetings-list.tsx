"use client"

import * as React from "react"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Pagination,
  PaginationContent,
  PaginationItem,
  PaginationNext,
  PaginationPrevious,
} from "@/components/ui/pagination"
import type { MeetingListItem } from "@/lib/types"
import { isEditable } from "./meeting-utils"
import { MeetingRow } from "./meeting-row"
import { EditMeetingDialog } from "./edit-meeting-dialog"

const PAGE_SIZE = 10

export function MeetingsList({ meetings }: { meetings: MeetingListItem[] }) {
  const [page, setPage] = React.useState(0)
  const [editMeeting, setEditMeeting] = React.useState<MeetingListItem | null>(null)
  const totalPages = Math.ceil(meetings.length / PAGE_SIZE)
  const paged = meetings.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)

  React.useEffect(() => {
    setPage(0)
  }, [meetings.length])

  if (meetings.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        No meetings found.
      </p>
    )
  }

  return (
    <>
      <div className="flex flex-col divide-y">
        {paged.map((meeting) => (
          <MeetingRow
            key={meeting.id}
            meeting={meeting}
            onEdit={() => {
              if (isEditable(meeting)) setEditMeeting(meeting)
            }}
          />
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
      {editMeeting && (
        <EditMeetingDialog
          meeting={editMeeting}
          open={!!editMeeting}
          onOpenChange={(open) => { if (!open) setEditMeeting(null) }}
        />
      )}
    </>
  )
}

export function LoadingSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      {Array.from({ length: 5 }).map((_, i) => (
        <div key={i} className="flex items-center justify-between gap-4 px-4 py-3">
          <div className="flex flex-col gap-1.5">
            <Skeleton className="h-4 w-56" />
            <Skeleton className="h-3 w-36" />
          </div>
          <Skeleton className="h-5 w-20" />
        </div>
      ))}
    </div>
  )
}
