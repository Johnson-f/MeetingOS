"use client";

import {
  Calendar03Icon,
  ChartUpIcon,
  Clock01Icon,
  LinkSquare01Icon,
  TaskDone01Icon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardAction,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { useAnalyticsOverviewQuery } from "@/lib/hooks";

export function SectionCards() {
  const { data, isLoading } = useAnalyticsOverviewQuery();

  const totalMeetings = data?.total_meetings ?? 0;
  const previousThisWeek = data?.meetings_this_week_previous ?? 0;
  const upcomingThisWeek = data?.meetings_this_week_upcoming ?? 0;
  const recordedHours = data?.recorded_hours ?? 0;
  const integrationLabel = data?.integrations.label ?? "Coming soon";

  return (
    <div className="grid grid-cols-1 gap-4 px-4 *:data-[slot=card]:bg-linear-to-t *:data-[slot=card]:from-primary/5 *:data-[slot=card]:to-card *:data-[slot=card]:shadow-xs lg:px-6 @xl/main:grid-cols-2 @5xl/main:grid-cols-4 dark:*:data-[slot=card]:bg-card">
      <Card size="sm" className="@container/card">
        <CardHeader>
          <CardDescription>Total Meetings</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {isLoading ? "--" : totalMeetings.toLocaleString()}
          </CardTitle>
          <CardAction>
            <Badge variant="outline">
              <HugeiconsIcon icon={TaskDone01Icon} strokeWidth={2} />
              all time
            </Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="items-start text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            Meetings tracked across your workspace{" "}
            <HugeiconsIcon
              icon={ChartUpIcon}
              strokeWidth={2}
              className="size-4"
            />
          </div>
        </CardFooter>
      </Card>
      <Card size="sm" className="@container/card">
        <CardHeader>
          <CardDescription>This Week</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {isLoading
              ? "--"
              : `${previousThisWeek.toLocaleString()} / ${upcomingThisWeek.toLocaleString()}`}
          </CardTitle>
          <CardAction>
            <Badge variant="outline">
              <HugeiconsIcon icon={Calendar03Icon} strokeWidth={2} />
              prev / next
            </Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="items-start text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            {isLoading
              ? "Loading this week's meetings"
              : `${previousThisWeek} completed and ${upcomingThisWeek} upcoming`}{" "}
            <HugeiconsIcon
              icon={Calendar03Icon}
              strokeWidth={2}
              className="size-4"
            />
          </div>
        </CardFooter>
      </Card>
      <Card size="sm" className="@container/card">
        <CardHeader>
          <CardDescription>Hours Recorded</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {isLoading ? "--" : recordedHours.toFixed(1)}
          </CardTitle>
          <CardAction>
            <Badge variant="outline">
              <HugeiconsIcon icon={Clock01Icon} strokeWidth={2} />
              audio total
            </Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="items-start text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            Total audio duration across recordings{" "}
            <HugeiconsIcon
              icon={Clock01Icon}
              strokeWidth={2}
              className="size-4"
            />
          </div>
        </CardFooter>
      </Card>
      <Card size="sm" className="@container/card">
        <CardHeader>
          <CardDescription>Integrations</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {integrationLabel}
          </CardTitle>
          <CardAction>
            <Badge variant="outline">
              <HugeiconsIcon icon={LinkSquare01Icon} strokeWidth={2} />
              placeholder
            </Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="items-start text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            Integration analytics are reserved for a later pass{" "}
            <HugeiconsIcon
              icon={LinkSquare01Icon}
              strokeWidth={2}
              className="size-4"
            />
          </div>
        </CardFooter>
      </Card>
    </div>
  );
}
