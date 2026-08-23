"use client"

import * as React from "react"
import { toast } from "sonner"
import { HugeiconsIcon } from "@hugeicons/react"
import { PencilEdit01Icon, Delete02Icon, ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import { useDeleteMeetingMutation } from "@/lib/hooks/use-meeting-mutations"
import type { MeetingListItem } from "@/lib/types"
import {
  formatTime,
  formatRelativeDate,
  platformLabel,
  durationLabel,
  statusBadge,
  isEditable,
} from "./meeting-utils"
import { MeetingDetail } from "./meeting-detail"

function RowHeader({
  meeting,
  onEdit,
}: {
  meeting: MeetingListItem
  onEdit: () => void
}) {
  const [deleteOpen, setDeleteOpen] = React.useState(false)
  const deleteMeeting = useDeleteMeetingMutation()
  const effectiveDate = meeting.actual_start_at ?? meeting.scheduled_start_at ?? meeting.created_at
  const time = formatTime(effectiveDate)
  const relDate = formatRelativeDate(effectiveDate)
  const duration = durationLabel(meeting)
  const hasNotes = !!meeting.latest_note_summary
  const editable = isEditable(meeting)

  return (
    <div className="group flex items-center justify-between gap-4 rounded-lg px-4 py-3 transition-colors hover:bg-muted/50">
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
      <div className="flex items-center gap-1 shrink-0">
        {editable && (
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
                  <Button variant="outline" size="sm" onClick={() => setDeleteOpen(false)}>
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
        )}
        {meeting.status === "completed" && hasNotes ? (
          <Badge variant="outline" className="text-emerald-600 border-emerald-200 bg-emerald-50 dark:bg-emerald-950/30 dark:border-emerald-800">
            Notes ready
          </Badge>
        ) : (
          statusBadge(meeting.status)
        )}
      </div>
    </div>
  )
}

export function MeetingRow({
  meeting,
  onEdit,
}: {
  meeting: MeetingListItem
  onEdit: () => void
}) {
  const [open, setOpen] = React.useState(false)
  const isCompleted = meeting.status === "completed"

  if (!isCompleted) {
    return <RowHeader meeting={meeting} onEdit={onEdit} />
  }

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger className="w-full text-left cursor-pointer">
        <div className="flex items-center">
          <div className="flex-1">
            <RowHeader meeting={meeting} onEdit={onEdit} />
          </div>
          <div className="pr-4 shrink-0">
            <HugeiconsIcon
              icon={ArrowDown01Icon}
              strokeWidth={2}
              className={`size-4 text-muted-foreground transition-transform duration-200 ${open ? "rotate-180" : ""}`}
            />
          </div>
        </div>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <MeetingDetail meetingId={meeting.id} />
      </CollapsibleContent>
    </Collapsible>
  )
}
