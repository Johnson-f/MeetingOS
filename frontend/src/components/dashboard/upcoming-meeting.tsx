"use client"

import * as React from "react"
import { toast } from "sonner"
import { HugeiconsIcon } from "@hugeicons/react"
import { PencilEdit01Icon, Delete02Icon } from "@hugeicons/core-free-icons"
import {
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Pagination,
  PaginationContent,
  PaginationItem,
  PaginationNext,
  PaginationPrevious,
} from "@/components/ui/pagination"
import { useMeetingsQuery } from "@/lib/hooks/use-meetings-query"
import {
  useDeleteMeetingMutation,
  useCancelMeetingMutation,
  useUpdateMeetingMutation,
} from "@/lib/hooks/use-meeting-mutations"
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
  const tomorrow = new Date(now)
  tomorrow.setDate(tomorrow.getDate() + 1)
  const isTomorrow = date.toDateString() === tomorrow.toDateString()

  if (isToday) return "Today"
  if (isTomorrow) return "Tomorrow"
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

function statusBadge(status: string) {
  switch (status) {
    case "draft":
    case "scheduled":
      return (
        <Badge variant="outline" className="text-emerald-600 border-emerald-200 bg-emerald-50 dark:bg-emerald-950/30 dark:border-emerald-800">
          Scheduled
        </Badge>
      )
    case "joining":
      return (
        <Badge variant="outline" className="text-blue-600 border-blue-200 bg-blue-50 dark:bg-blue-950/30 dark:border-blue-800">
          Joining
        </Badge>
      )
    case "recording":
      return (
        <Badge variant="outline" className="text-red-600 border-red-200 bg-red-50 dark:bg-red-950/30 dark:border-red-800">
          Recording
        </Badge>
      )
    case "processing":
      return (
        <Badge variant="outline" className="text-amber-600 border-amber-200 bg-amber-50 dark:bg-amber-950/30 dark:border-amber-800">
          Processing
        </Badge>
      )
    case "completed":
      return (
        <Badge variant="outline" className="text-emerald-600 border-emerald-200 bg-emerald-50 dark:bg-emerald-950/30 dark:border-emerald-800">
          Completed
        </Badge>
      )
    case "failed":
      return <Badge variant="destructive">Failed</Badge>
    case "cancelled":
      return (
        <Badge variant="outline" className="text-muted-foreground">
          Cancelled
        </Badge>
      )
    default:
      return <Badge variant="outline">{status}</Badge>
  }
}

function toLocalDatetimeValue(dateStr: string | null) {
  if (!dateStr) return ""
  const d = new Date(dateStr)
  const offset = d.getTimezoneOffset()
  const local = new Date(d.getTime() - offset * 60000)
  return local.toISOString().slice(0, 16)
}

function EditMeetingDialog({
  meeting,
  open,
  onOpenChange,
}: {
  meeting: MeetingListItem
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [title, setTitle] = React.useState(meeting.title)
  const [scheduledAt, setScheduledAt] = React.useState(
    toLocalDatetimeValue(meeting.scheduled_start_at)
  )
  const updateMeeting = useUpdateMeetingMutation()
  const cancelMeeting = useCancelMeetingMutation()

  React.useEffect(() => {
    if (open) {
      setTitle(meeting.title)
      setScheduledAt(toLocalDatetimeValue(meeting.scheduled_start_at))
    }
  }, [open, meeting.title, meeting.scheduled_start_at])

  const hasChanges =
    title !== meeting.title ||
    scheduledAt !== toLocalDatetimeValue(meeting.scheduled_start_at)

  function handleSave() {
    updateMeeting.mutate(
      {
        meetingId: meeting.id,
        payload: {
          title: title !== meeting.title ? title : undefined,
          scheduled_start_at:
            scheduledAt && scheduledAt !== toLocalDatetimeValue(meeting.scheduled_start_at)
              ? new Date(scheduledAt).toISOString()
              : undefined,
        },
      },
      {
        onSuccess: () => {
          toast.success("Meeting updated")
          onOpenChange(false)
        },
        onError: (err: Error) => toast.error(err.message || "Failed to update"),
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Edit Meeting</DialogTitle>
          <DialogDescription>Update meeting details.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3">
          <div className="grid gap-1.5">
            <Label htmlFor="edit-title">Title</Label>
            <Input
              id="edit-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </div>
          <div className="grid gap-1.5">
            <Label>Platform</Label>
            <Input value={platformLabel(meeting.platform)} readOnly className="opacity-70" />
          </div>
          <div className="grid gap-1.5">
            <Label>Status</Label>
            <div>{statusBadge(meeting.status)}</div>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="edit-scheduled">Scheduled Time</Label>
            <Input
              id="edit-scheduled"
              type="datetime-local"
              value={scheduledAt}
              onChange={(e) => setScheduledAt(e.target.value)}
            />
          </div>
        </div>
        <DialogFooter className="pt-2 flex gap-2">
          {["draft", "scheduled", "joining"].includes(meeting.status) && (
            <Button
              variant="destructive"
              onClick={() => {
                cancelMeeting.mutate(meeting.id, {
                  onSuccess: () => {
                    toast.success("Meeting cancelled")
                    onOpenChange(false)
                  },
                  onError: (err: Error) => toast.error(err.message || "Failed to cancel"),
                })
              }}
              disabled={cancelMeeting.isPending}
            >
              {cancelMeeting.isPending ? "Cancelling..." : "Cancel Meeting"}
            </Button>
          )}
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
          >
            Close
          </Button>
          <Button
            onClick={handleSave}
            disabled={!hasChanges || updateMeeting.isPending}
          >
            {updateMeeting.isPending ? "Saving..." : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function MeetingRow({
  meeting,
  onEdit,
}: {
  meeting: MeetingListItem
  onEdit: () => void
}) {
  const [deleteOpen, setDeleteOpen] = React.useState(false)
  const deleteMeeting = useDeleteMeetingMutation()
  const effectiveDate = meeting.scheduled_start_at ?? meeting.actual_start_at ?? meeting.created_at
  const time = formatTime(effectiveDate)
  const relDate = formatRelativeDate(effectiveDate)

  return (
    <div className="group flex items-center justify-between gap-4 rounded-lg border border-transparent px-4 py-3 transition-colors hover:bg-muted/50">
      <div className="flex flex-col gap-0.5 min-w-0">
        <span className="text-sm font-medium truncate">{meeting.title}</span>
        <span className="text-xs text-muted-foreground">
          {relDate}
          {time ? ` \u00b7 ${time}` : ""}
          {" \u00b7 "}
          {platformLabel(meeting.platform)}
        </span>
      </div>
      <div className="flex items-center gap-1 shrink-0">
        <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={(e) => {
              e.stopPropagation()
              onEdit()
            }}
            className="size-7"
          >
            <HugeiconsIcon icon={PencilEdit01Icon} strokeWidth={2} className="size-3.5" />
            <span className="sr-only">Edit</span>
          </Button>
          <Popover open={deleteOpen} onOpenChange={setDeleteOpen}>
            <PopoverTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="size-7 text-destructive hover:text-destructive"
                  onClick={(e) => e.stopPropagation()}
                />
              }
            >
              <HugeiconsIcon icon={Delete02Icon} strokeWidth={2} className="size-3.5" />
              <span className="sr-only">Delete</span>
            </PopoverTrigger>
            <PopoverContent className="w-56" side="bottom" align="end">
              <PopoverHeader>
                <PopoverTitle>Delete meeting?</PopoverTitle>
                <PopoverDescription>This action cannot be undone.</PopoverDescription>
              </PopoverHeader>
              <div className="flex justify-end gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setDeleteOpen(false)}
                >
                  Cancel
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  disabled={deleteMeeting.isPending}
                  onClick={() => {
                    deleteMeeting.mutate(meeting.id, {
                      onSuccess: () => {
                        toast.success("Meeting deleted")
                        setDeleteOpen(false)
                      },
                      onError: (err: Error) => toast.error(err.message || "Failed to delete"),
                    })
                  }}
                >
                  {deleteMeeting.isPending ? "Deleting..." : "Delete"}
                </Button>
              </div>
            </PopoverContent>
          </Popover>
        </div>
        {statusBadge(meeting.status)}
      </div>
    </div>
  )
}

export function UpcomingMeetings() {
  const [page, setPage] = React.useState(0)
  const [editMeeting, setEditMeeting] = React.useState<MeetingListItem | null>(null)
  const { data, isLoading } = useMeetingsQuery(100, 0)
  const activeStatuses = new Set(["draft", "scheduled", "joining", "recording", "processing"])
  const allMeetings = (data?.items ?? []).filter((m) => activeStatuses.has(m.status))
  const totalPages = Math.ceil(allMeetings.length / PAGE_SIZE)
  const meetings = allMeetings.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)
  const todayCount = allMeetings.filter((m) => {
    const d = m.scheduled_start_at ?? m.actual_start_at ?? m.created_at
    return d && new Date(d).toDateString() === new Date().toDateString()
  }).length


  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>Upcoming</CardTitle>
          {todayCount > 0 && (
            <CardAction>
              <Badge variant="outline">{todayCount} today</Badge>
            </CardAction>
          )}
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
              No meetings yet. Click &quot;New Meeting&quot; to get started.
            </p>
          ) : (
            <>
              <div className="flex flex-col divide-y">
                {meetings.map((meeting) => (
                  <MeetingRow
                    key={meeting.id}
                    meeting={meeting}
                    onEdit={() => setEditMeeting(meeting)}
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
            </>
          )}
        </CardContent>
      </Card>
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
