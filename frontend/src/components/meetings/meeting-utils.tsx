import { Badge } from "@/components/ui/badge"
import type { MeetingListItem } from "@/lib/types"

export function formatTime(dateStr: string | null) {
  if (!dateStr) return null
  const date = new Date(dateStr)
  return date.toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  })
}

export function formatRelativeDate(dateStr: string | null) {
  if (!dateStr) return "No date"
  const date = new Date(dateStr)
  const now = new Date()
  const isToday = date.toDateString() === now.toDateString()
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  const isYesterday = date.toDateString() === yesterday.toDateString()
  const tomorrow = new Date(now)
  tomorrow.setDate(tomorrow.getDate() + 1)
  const isTomorrow = date.toDateString() === tomorrow.toDateString()

  if (isToday) return "Today"
  if (isYesterday) return "Yesterday"
  if (isTomorrow) return "Tomorrow"
  return date.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
  })
}

export function platformLabel(platform: string) {
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

export function durationLabel(meeting: MeetingListItem) {
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

export function toLocalDatetimeValue(dateStr: string | null) {
  if (!dateStr) return ""
  const d = new Date(dateStr)
  const offset = d.getTimezoneOffset()
  const local = new Date(d.getTime() - offset * 60000)
  return local.toISOString().slice(0, 16)
}

export function statusBadge(status: string) {
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

const COMPLETED_STATUSES = new Set(["completed", "failed", "cancelled"])

export function isEditable(meeting: MeetingListItem) {
  return !COMPLETED_STATUSES.has(meeting.status)
}
