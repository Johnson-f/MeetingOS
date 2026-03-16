"use client"

import * as React from "react"
import { toast } from "sonner"

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { useCreateMeetingMutation } from "@/lib/hooks/use-meeting-mutations"
import type { MeetingTimeMode } from "@/lib/types"

export function NewMeetingDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [meetingUrl, setMeetingUrl] = React.useState("")
  const [title, setTitle] = React.useState("")
  const [botName, setBotName] = React.useState("")
  const [scheduleLater, setScheduleLater] = React.useState(false)
  const [joinAt, setJoinAt] = React.useState("")

  const createMeeting = useCreateMeetingMutation()

  function resetForm() {
    setMeetingUrl("")
    setTitle("")
    setBotName("")
    setScheduleLater(false)
    setJoinAt("")
  }

  function handleSubmit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault()

    if (!meetingUrl.trim()) return

    const meetingTimeMode: MeetingTimeMode = scheduleLater
      ? "future"
      : "starting_now"

    createMeeting.mutate(
      {
        meeting_url: meetingUrl.trim(),
        title: title.trim() || undefined,
        bot_name: botName.trim() || undefined,
        meeting_time_mode: meetingTimeMode,
        join_at:
          scheduleLater && joinAt
            ? new Date(joinAt).toISOString()
            : undefined,
      },
      {
        onSuccess: () => {
          toast.success(
            scheduleLater ? "Meeting scheduled" : "Bot sent to meeting"
          )
          resetForm()
          onOpenChange(false)
        },
        onError: (error) => {
          toast.error(error.message || "Failed to create meeting")
        },
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New Meeting</DialogTitle>
          <DialogDescription>
            Paste a meeting link to send a bot that records and takes notes.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-3">
          <div className="grid gap-1.5">
            <Label htmlFor="meeting-url">Meeting URL</Label>
            <Input
              id="meeting-url"
              placeholder="Paste meeting link (Zoom, Google Meet, etc.)"
              value={meetingUrl}
              onChange={(e) => setMeetingUrl(e.target.value)}
              required
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="title">Title (optional)</Label>
            <Input
              id="title"
              placeholder="Meeting title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="bot-name">Bot Name (optional)</Label>
            <Input
              id="bot-name"
              placeholder="Bot display name"
              value={botName}
              onChange={(e) => setBotName(e.target.value)}
            />
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="schedule-later"
              checked={scheduleLater}
              onChange={(e) => setScheduleLater(e.target.checked)}
              className="size-3.5 rounded border border-input accent-primary"
            />
            <Label htmlFor="schedule-later">Schedule for later</Label>
          </div>
          {scheduleLater && (
            <div className="grid gap-1.5">
              <Label htmlFor="join-at">Join at</Label>
              <Input
                id="join-at"
                type="datetime-local"
                value={joinAt}
                onChange={(e) => setJoinAt(e.target.value)}
                required={scheduleLater}
              />
            </div>
          )}
          <DialogFooter className="pt-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={createMeeting.isPending}>
              {createMeeting.isPending
                ? "Sending..."
                : scheduleLater
                  ? "Schedule Bot"
                  : "Send Bot"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
