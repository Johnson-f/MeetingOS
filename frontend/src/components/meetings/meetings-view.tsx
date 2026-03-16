"use client"

import { Badge } from "@/components/ui/badge"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { useMeetingsQuery } from "@/lib/hooks/use-meetings-query"
import { MeetingsList, LoadingSkeleton } from "./meetings-list"

const ACTIVE_STATUSES = new Set(["draft", "scheduled", "joining", "recording", "processing"])

export function MeetingsView() {
  const { data, isLoading } = useMeetingsQuery(100, 0)
  const allMeetings = data?.items ?? []
  const upcomingMeetings = allMeetings.filter((m) => ACTIVE_STATUSES.has(m.status))
  const completedMeetings = allMeetings.filter((m) => m.status === "completed")

  return (
    <div>
      <Tabs defaultValue="all">
        <TabsList>
          <TabsTrigger value="all">
            All
            {allMeetings.length > 0 && (
              <Badge variant="secondary" className="ml-1.5 px-1.5 text-[0.6rem]">
                {allMeetings.length}
              </Badge>
            )}
          </TabsTrigger>
          <TabsTrigger value="upcoming">
            Upcoming
            {upcomingMeetings.length > 0 && (
              <Badge variant="secondary" className="ml-1.5 px-1.5 text-[0.6rem]">
                {upcomingMeetings.length}
              </Badge>
            )}
          </TabsTrigger>
          <TabsTrigger value="past">
            Past
            {completedMeetings.length > 0 && (
              <Badge variant="secondary" className="ml-1.5 px-1.5 text-[0.6rem]">
                {completedMeetings.length}
              </Badge>
            )}
          </TabsTrigger>
        </TabsList>
        <TabsContent value="all">
          {isLoading ? <LoadingSkeleton /> : <MeetingsList meetings={allMeetings} />}
        </TabsContent>
        <TabsContent value="upcoming">
          {isLoading ? <LoadingSkeleton /> : <MeetingsList meetings={upcomingMeetings} />}
        </TabsContent>
        <TabsContent value="past">
          {isLoading ? <LoadingSkeleton /> : <MeetingsList meetings={completedMeetings} />}
        </TabsContent>
      </Tabs>
    </div>
  )
}
