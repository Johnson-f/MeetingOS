"use client"

import { Badge } from "@/components/ui/badge"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { useMeetingsQuery } from "@/lib/hooks/use-meetings-query"
import { MeetingsList, LoadingSkeleton } from "./meetings-list"
import type { MeetingListItem } from "@/lib/types"

const UPCOMING_STATUSES = new Set(["draft", "scheduled", "joining"])
const PROCESSING_STATUSES = new Set(["recording", "processing"])
const FAILED_STATUSES = new Set(["failed", "cancelled"])

function sortByDate(meetings: MeetingListItem[], direction: "asc" | "desc" = "desc") {
  return [...meetings].sort((a, b) => {
    const dateA = a.scheduled_start_at ?? a.created_at
    const dateB = b.scheduled_start_at ?? b.created_at
    return direction === "asc" ? dateA.localeCompare(dateB) : dateB.localeCompare(dateA)
  })
}

function TabBadge({ count }: { count: number }) {
  if (count === 0) return null
  return (
    <Badge variant="secondary" className="ml-1.5 px-1.5 text-[0.6rem]">
      {count}
    </Badge>
  )
}

export function MeetingsView() {
  const { data, isLoading } = useMeetingsQuery(100, 0)
  const allMeetings = data?.items ?? []

  const upcomingMeetings = sortByDate(allMeetings.filter((m) => UPCOMING_STATUSES.has(m.status)), "asc")
  const recentMeetings = sortByDate(allMeetings.filter((m) => m.status === "completed"))
  const processingMeetings = sortByDate(allMeetings.filter((m) => PROCESSING_STATUSES.has(m.status)))
  const failedMeetings = sortByDate(allMeetings.filter((m) => FAILED_STATUSES.has(m.status)))

  return (
    <div>
      <Tabs defaultValue="upcoming">
        <TabsList>
          <TabsTrigger value="upcoming">
            Upcoming
            <TabBadge count={upcomingMeetings.length} />
          </TabsTrigger>
          <TabsTrigger value="recent">
            Recent
            <TabBadge count={recentMeetings.length} />
          </TabsTrigger>
          <TabsTrigger value="processing">
            Processing
            <TabBadge count={processingMeetings.length} />
          </TabsTrigger>
          <TabsTrigger value="failed">
            Failed
            <TabBadge count={failedMeetings.length} />
          </TabsTrigger>
        </TabsList>
        <TabsContent value="upcoming">
          {isLoading ? <LoadingSkeleton /> : <MeetingsList meetings={upcomingMeetings} />}
        </TabsContent>
        <TabsContent value="recent">
          {isLoading ? <LoadingSkeleton /> : <MeetingsList meetings={recentMeetings} />}
        </TabsContent>
        <TabsContent value="processing">
          {isLoading ? <LoadingSkeleton /> : <MeetingsList meetings={processingMeetings} />}
        </TabsContent>
        <TabsContent value="failed">
          {isLoading ? <LoadingSkeleton /> : <MeetingsList meetings={failedMeetings} />}
        </TabsContent>
      </Tabs>
    </div>
  )
}
