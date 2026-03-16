"use client"

import * as React from "react"
import { HugeiconsIcon } from "@hugeicons/react"
import { ArrowLeft01Icon, ArrowRight01Icon, Calendar01Icon } from "@hugeicons/core-free-icons"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { useMeetingsQuery } from "@/lib/hooks/use-meetings-query"
import { useGoogleCalendarConnect } from "@/lib/hooks/use-google-calendar"
import { NewMeetingDialog } from "@/components/new-meeting"
import type { MeetingListItem } from "@/lib/types"

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

function getMonthGrid(year: number, month: number) {
  const firstDay = new Date(year, month, 1)
  const lastDay = new Date(year, month + 1, 0)

  // Monday = 0, Sunday = 6
  let startDow = firstDay.getDay() - 1
  if (startDow < 0) startDow = 6

  const days: (Date | null)[] = []

  // Fill leading empty days
  for (let i = 0; i < startDow; i++) {
    const d = new Date(year, month, -(startDow - 1 - i))
    days.push(d)
  }

  // Fill month days
  for (let d = 1; d <= lastDay.getDate(); d++) {
    days.push(new Date(year, month, d))
  }

  // Fill trailing days to complete grid (6 rows)
  while (days.length < 42) {
    const next = days.length - startDow - lastDay.getDate() + 1
    days.push(new Date(year, month + 1, next))
  }

  return days
}

function formatTime(dateStr: string) {
  const d = new Date(dateStr)
  return d.toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    hour12: false,
  })
}

function platformColor(platform: string): string {
  switch (platform) {
    case "google_meet":
      return "bg-blue-500"
    case "zoom":
      return "bg-indigo-500"
    case "microsoft_teams":
      return "bg-violet-500"
    default:
      return "bg-muted-foreground"
  }
}

function statusColor(status: string): string {
  switch (status) {
    case "recording":
      return "text-red-600"
    case "completed":
      return "text-emerald-600"
    case "processing":
      return "text-amber-600"
    default:
      return "text-muted-foreground"
  }
}

function MeetingDotInline({ meeting }: { meeting: MeetingListItem }) {
  const time = meeting.scheduled_start_at
    ? formatTime(meeting.scheduled_start_at)
    : meeting.actual_start_at
      ? formatTime(meeting.actual_start_at)
      : null

  return (
    <div className="flex items-center gap-1 rounded px-1 py-0.5 text-[0.6rem] leading-tight truncate">
      <span className={`size-1.5 shrink-0 rounded-full ${platformColor(meeting.platform)}`} />
      <span className="text-muted-foreground tabular-nums">{time}</span>
      <span className="truncate">{meeting.title}</span>
    </div>
  )
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

function formatTimeRange(meeting: MeetingListItem) {
  const start = meeting.scheduled_start_at ?? meeting.actual_start_at
  const end = meeting.actual_end_at
  if (!start) return "No time"
  const startTime = new Date(start).toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  })
  if (!end) return startTime
  const endTime = new Date(end).toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  })
  return `${startTime} – ${endTime}`
}

function DayDetailSheet({
  date,
  meetings,
  open,
  onOpenChange,
}: {
  date: Date
  meetings: MeetingListItem[]
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const dateLabel = date.toLocaleDateString("en-US", {
    weekday: "long",
    month: "long",
    day: "numeric",
    year: "numeric",
  })

  const sorted = [...meetings].sort((a, b) => {
    const aTime = a.scheduled_start_at ?? a.actual_start_at ?? a.created_at
    const bTime = b.scheduled_start_at ?? b.actual_start_at ?? b.created_at
    return new Date(aTime).getTime() - new Date(bTime).getTime()
  })

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>{dateLabel}</SheetTitle>
          <SheetDescription>
            {meetings.length === 0
              ? "No meetings scheduled"
              : `${meetings.length} meeting${meetings.length > 1 ? "s" : ""}`}
          </SheetDescription>
        </SheetHeader>
        {sorted.length > 0 && (
          <div className="flex flex-col gap-1 mt-4">
            {sorted.map((meeting) => (
              <div
                key={meeting.id}
                className="flex gap-3 rounded-lg border border-border/50 p-3 hover:bg-muted/50 transition-colors"
              >
                {/* Time column */}
                <div className="flex flex-col items-center shrink-0 w-16">
                  <span className="text-xs font-medium tabular-nums">
                    {formatTimeRange(meeting).split("–")[0]?.trim()}
                  </span>
                  {meeting.actual_end_at && (
                    <span className="text-[0.65rem] text-muted-foreground tabular-nums">
                      {formatTimeRange(meeting).split("–")[1]?.trim()}
                    </span>
                  )}
                </div>
                {/* Color bar */}
                <div className={`w-1 shrink-0 rounded-full ${platformColor(meeting.platform)}`} />
                {/* Details */}
                <div className="flex flex-col gap-0.5 min-w-0 flex-1">
                  <span className="text-sm font-medium truncate">{meeting.title}</span>
                  <span className="text-xs text-muted-foreground">
                    {platformLabel(meeting.platform)}
                  </span>
                  <Badge
                    variant="outline"
                    className={`w-fit text-[0.6rem] mt-1 ${statusColor(meeting.status)}`}
                  >
                    {meeting.status}
                  </Badge>
                </div>
              </div>
            ))}
          </div>
        )}
      </SheetContent>
    </Sheet>
  )
}

function DayCell({
  date,
  isCurrentMonth,
  isToday,
  meetings,
  onSelect,
}: {
  date: Date
  isCurrentMonth: boolean
  isToday: boolean
  meetings: MeetingListItem[]
  onSelect: () => void
}) {
  const maxVisible = 3
  const visible = meetings.slice(0, maxVisible)
  const overflow = meetings.length - maxVisible

  return (
    <div
      onClick={onSelect}
      className={`min-h-[100px] border-t border-border/50 p-1 cursor-pointer transition-colors hover:bg-muted/40 ${
        isCurrentMonth ? "" : "opacity-40"
      }`}
    >
      <div className="flex items-center justify-between mb-0.5">
        <span
          className={`text-xs tabular-nums px-1 py-0.5 rounded ${
            isToday
              ? "bg-primary text-primary-foreground font-medium"
              : "text-muted-foreground"
          }`}
        >
          {date.getDate()}
        </span>
        {meetings.length > 0 && (
          <span className="text-[0.55rem] text-muted-foreground/50">
            {meetings.length}
          </span>
        )}
      </div>
      <div className="flex flex-col gap-0.5">
        {visible.map((m) => (
          <MeetingDotInline key={m.id} meeting={m} />
        ))}
        {overflow > 0 && (
          <span className="text-[0.6rem] text-muted-foreground/60 px-1">
            + {overflow} more
          </span>
        )}
      </div>
    </div>
  )
}

export function CalendarView() {
  const [currentDate, setCurrentDate] = React.useState(() => new Date())
  const year = currentDate.getFullYear()
  const month = currentDate.getMonth()
  const today = new Date()

  const [newMeetingOpen, setNewMeetingOpen] = React.useState(false)
  const [selectedDay, setSelectedDay] = React.useState<Date | null>(null)
  const { data, isLoading } = useMeetingsQuery(100, 0)
  const meetings = data?.items ?? []
  const googleConnect = useGoogleCalendarConnect()

  const monthName = currentDate.toLocaleDateString("en-US", {
    month: "long",
    year: "numeric",
  })

  const grid = getMonthGrid(year, month)

  // Group meetings by date string
  const meetingsByDate = React.useMemo(() => {
    const map = new Map<string, MeetingListItem[]>()
    for (const m of meetings) {
      const dateStr =
        m.scheduled_start_at ?? m.actual_start_at ?? m.created_at
      if (!dateStr) continue
      const key = new Date(dateStr).toDateString()
      if (!map.has(key)) map.set(key, [])
      map.get(key)!.push(m)
    }
    return map
  }, [meetings])

  function prevMonth() {
    setCurrentDate(new Date(year, month - 1, 1))
  }

  function nextMonth() {
    setCurrentDate(new Date(year, month + 1, 1))
  }

  function goToday() {
    setCurrentDate(new Date())
  }

  return (
    <div className="flex gap-6">
      {/* Sidebar */}
      <div className="hidden lg:flex flex-col gap-4 w-48 shrink-0">
        {/* Full date display */}
        <div className="text-sm font-medium text-foreground">
          {today.toLocaleDateString("en-US", {
            weekday: "long",
            day: "numeric",
            month: "long",
            year: "numeric",
          })}
        </div>

        {/* Mini calendar */}
        <MiniCalendar
          year={year}
          month={month}
          today={today}
          onSelect={(d) => setCurrentDate(d)}
          onPrevMonth={prevMonth}
          onNextMonth={nextMonth}
        />

        {/* Connect calendar */}
        <div className="flex flex-col gap-2 mt-auto pt-4">
          <span className="text-[0.65rem] font-medium text-muted-foreground uppercase tracking-wider">
            Connect a Calendar
          </span>
          <Button
            variant="outline"
            size="sm"
            className="justify-start"
            onClick={() => googleConnect.mutate()}
            disabled={googleConnect.isPending}
          >
            <HugeiconsIcon icon={Calendar01Icon} strokeWidth={2} className="size-4 mr-2" />
            Google
          </Button>
        </div>
      </div>

      {/* Main calendar */}
      <div className="flex-1 min-w-0">
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">{monthName}</h2>
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => setNewMeetingOpen(true)}>
              New Meeting
            </Button>
            <Button variant="outline" size="sm" onClick={goToday}>
              Today
            </Button>
            <div className="flex items-center">
              <Button variant="ghost" size="icon-sm" onClick={prevMonth}>
                <HugeiconsIcon icon={ArrowLeft01Icon} strokeWidth={2} className="size-4" />
              </Button>
              <Button variant="ghost" size="icon-sm" onClick={nextMonth}>
                <HugeiconsIcon icon={ArrowRight01Icon} strokeWidth={2} className="size-4" />
              </Button>
            </div>
          </div>
        </div>

        {isLoading ? (
          <div className="grid grid-cols-7 gap-px">
            {Array.from({ length: 35 }).map((_, i) => (
              <Skeleton key={i} className="h-[100px]" />
            ))}
          </div>
        ) : (
          <>
            {/* Day headers */}
            <div className="grid grid-cols-7 border-b border-border/50">
              {DAYS.map((day) => (
                <div
                  key={day}
                  className="py-2 text-center text-xs font-medium text-muted-foreground"
                >
                  {day}
                </div>
              ))}
            </div>

            {/* Grid */}
            <div className="grid grid-cols-7">
              {grid.map((date, i) => {
                if (!date) return <div key={i} />
                const isCurrentMonth = date.getMonth() === month
                const isToday = date.toDateString() === today.toDateString()
                const dayMeetings = meetingsByDate.get(date.toDateString()) ?? []

                return (
                  <DayCell
                    key={i}
                    date={date}
                    isCurrentMonth={isCurrentMonth}
                    isToday={isToday}
                    meetings={dayMeetings}
                    onSelect={() => setSelectedDay(date)}
                  />
                )
              })}
            </div>
          </>
        )}
      </div>
      <NewMeetingDialog open={newMeetingOpen} onOpenChange={setNewMeetingOpen} />
      {selectedDay && (
        <DayDetailSheet
          date={selectedDay}
          meetings={meetingsByDate.get(selectedDay.toDateString()) ?? []}
          open={!!selectedDay}
          onOpenChange={(open) => { if (!open) setSelectedDay(null) }}
        />
      )}
    </div>
  )
}

function MiniCalendar({
  year,
  month,
  today,
  onSelect,
  onPrevMonth,
  onNextMonth,
}: {
  year: number
  month: number
  today: Date
  onSelect: (date: Date) => void
  onPrevMonth: () => void
  onNextMonth: () => void
}) {
  const monthName = new Date(year, month).toLocaleDateString("en-US", {
    month: "long",
    year: "numeric",
  })
  const grid = getMonthGrid(year, month)

  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-medium">{monthName}</span>
        <div className="flex">
          <Button variant="ghost" size="icon-sm" onClick={onPrevMonth} className="size-5">
            <HugeiconsIcon icon={ArrowLeft01Icon} strokeWidth={2} className="size-3" />
          </Button>
          <Button variant="ghost" size="icon-sm" onClick={onNextMonth} className="size-5">
            <HugeiconsIcon icon={ArrowRight01Icon} strokeWidth={2} className="size-3" />
          </Button>
        </div>
      </div>
      <div className="grid grid-cols-7 gap-0">
        {["M", "T", "W", "T", "F", "S", "S"].map((d, i) => (
          <div key={i} className="text-center text-xs text-muted-foreground py-1">
            {d}
          </div>
        ))}
        {grid.slice(0, 42).map((date, i) => {
          if (!date) return <div key={i} />
          const isCurrentMonth = date.getMonth() === month
          const isToday = date.toDateString() === today.toDateString()
          return (
            <button
              key={i}
              onClick={() => onSelect(date)}
              className={`text-center text-xs py-1.5 rounded hover:bg-muted ${
                isCurrentMonth ? "" : "opacity-30"
              } ${isToday ? "bg-primary text-primary-foreground font-bold" : ""}`}
            >
              {date.getDate()}
            </button>
          )
        })}
      </div>
    </div>
  )
}
