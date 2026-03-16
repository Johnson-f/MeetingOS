"use client"

import * as React from "react"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
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
  useCancelMeetingMutation,
  useUpdateMeetingMutation,
} from "@/lib/hooks/use-meeting-mutations"
import type { MeetingListItem } from "@/lib/types"
import { platformLabel, statusBadge, toLocalDatetimeValue } from "./meeting-utils"

export function EditMeetingDialog({
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
            <Input id="edit-title" value={title} onChange={(e) => setTitle(e.target.value)} />
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
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
          <Button onClick={handleSave} disabled={!hasChanges || updateMeeting.isPending}>
            {updateMeeting.isPending ? "Saving..." : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
