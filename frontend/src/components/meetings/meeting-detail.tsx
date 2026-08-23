"use client"

import { Skeleton } from "@/components/ui/skeleton"
import { Separator } from "@/components/ui/separator"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ScrollArea } from "@/components/ui/scroll-area"
import { useBackendClient } from "@/lib/hooks/use-backend-client"
import { useMeetingQuery, useMeetingNoteQuery } from "@/lib/hooks/use-meeting-query"
import { useQuery } from "@tanstack/react-query"
import { queryKeys } from "@/lib/service"
import { ShareDialog } from "./share-dialog"
import { ParticipantsList } from "./participants-list"

function useAudioUrl(meetingId: string) {
  const client = useBackendClient()
  return useQuery({
    queryKey: queryKeys.audio(meetingId),
    queryFn: async () => {
      const res = await client.getAudioUrl(meetingId)
      return res.url
    },
    enabled: Boolean(meetingId),
    retry: false,
  })
}

function AudioPlayer({ meetingId }: { meetingId: string }) {
  const { data: audioUrl, isLoading, isError, error } = useAudioUrl(meetingId)

  if (isLoading) {
    return (
      <div className="flex items-center gap-3">
        <Skeleton className="size-10 rounded-full" />
        <div className="flex-1">
          <Skeleton className="h-3 w-40 mb-2" />
          <Skeleton className="h-2 w-full" />
        </div>
      </div>
    )
  }

  if (isError || !audioUrl) {
    const message = (error as Error)?.message ?? ""
    const isProcessing = message.includes("not ready") || message.includes("CONFLICT")
    return (
      <p className="text-xs text-muted-foreground">
        {isProcessing ? "Audio is still processing..." : "Audio not available."}
      </p>
    )
  }

  return (
    <audio controls className="w-full h-10" preload="metadata">
      <source src={audioUrl} type="audio/mpeg" />
      Your browser does not support the audio element.
    </audio>
  )
}

function formatTimestamp(ms: number) {
  const totalSeconds = Math.floor(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes}:${seconds.toString().padStart(2, "0")}`
}

function TranscriptTab({ meetingId }: { meetingId: string }) {
  const { data, isLoading } = useMeetingQuery(meetingId)
  const transcription = data?.meeting?.transcription

  if (isLoading) {
    return (
      <div className="flex flex-col gap-2">
        <Skeleton className="h-3 w-full" />
        <Skeleton className="h-3 w-5/6" />
        <Skeleton className="h-3 w-full" />
        <Skeleton className="h-3 w-4/6" />
      </div>
    )
  }

  if (!transcription || transcription.status !== "ready") {
    return (
      <p className="text-xs text-muted-foreground">
        {transcription?.status === "processing"
          ? "Transcript is still processing..."
          : "No transcript available."}
      </p>
    )
  }

  if (transcription.segments && transcription.segments.length > 0) {
    return (
      <ScrollArea className="h-[400px]">
        <div className="flex flex-col gap-2 pr-4">
          {transcription.segments.map((segment) => (
            <div key={segment.id} className="flex gap-3">
              <span className="shrink-0 text-[0.65rem] text-muted-foreground/60 tabular-nums pt-0.5 w-10 text-right">
                {formatTimestamp(segment.start_ms)}
              </span>
              <div className="flex-1 min-w-0">
                {segment.speaker_label && (
                  <span className="text-xs font-medium text-foreground/80">
                    {segment.speaker_label}:{" "}
                  </span>
                )}
                <span className="text-xs text-muted-foreground leading-relaxed">
                  {segment.text}
                </span>
              </div>
            </div>
          ))}
        </div>
      </ScrollArea>
    )
  }

  if (transcription.full_text) {
    return (
      <ScrollArea className="h-[400px]">
        <p className="text-xs text-muted-foreground leading-relaxed whitespace-pre-wrap pr-4">
          {transcription.full_text}
        </p>
      </ScrollArea>
    )
  }

  return (
    <p className="text-xs text-muted-foreground">No transcript available.</p>
  )
}

function SummaryTab({ meetingId }: { meetingId: string }) {
  const { data, isLoading } = useMeetingNoteQuery(meetingId)
  const note = data?.note

  if (isLoading) {
    return (
      <div className="flex flex-col gap-2">
        <Skeleton className="h-4 w-20" />
        <Skeleton className="h-3 w-full" />
        <Skeleton className="h-3 w-3/4" />
      </div>
    )
  }

  if (!note) {
    return (
      <p className="text-xs text-muted-foreground">No notes available yet.</p>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      {note.summary_markdown && (
        <div>
          <h4 className="text-sm font-medium mb-1">Summary</h4>
          <p className="text-xs text-muted-foreground leading-relaxed whitespace-pre-wrap">
            {note.summary_markdown}
          </p>
        </div>
      )}
      {note.key_points && note.key_points.length > 0 && (
        <div>
          <h4 className="text-sm font-medium mb-1">Key Points</h4>
          <ul className="list-disc list-inside text-xs text-muted-foreground space-y-0.5">
            {note.key_points.map((point, i) => (
              <li key={i}>{point}</li>
            ))}
          </ul>
        </div>
      )}
      {note.decisions && note.decisions.length > 0 && (
        <div>
          <h4 className="text-sm font-medium mb-1">Decisions</h4>
          <ul className="list-disc list-inside text-xs text-muted-foreground space-y-0.5">
            {note.decisions.map((d, i) => (
              <li key={i}>{d}</li>
            ))}
          </ul>
        </div>
      )}
      {note.action_items && note.action_items.length > 0 && (
        <div>
          <h4 className="text-sm font-medium mb-1">Action Items</h4>
          <ul className="text-xs text-muted-foreground space-y-1">
            {note.action_items.map((item) => (
              <li key={item.id} className="flex items-start gap-2">
                <span className="shrink-0 mt-0.5 size-1.5 rounded-full bg-foreground/40" />
                <span>
                  {item.description}
                  {item.assignee_name && (
                    <span className="text-foreground/60"> — {item.assignee_name}</span>
                  )}
                  {item.due_date && (
                    <span className="text-foreground/60"> (due {item.due_date})</span>
                  )}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {note.risks && note.risks.length > 0 && (
        <div>
          <h4 className="text-sm font-medium mb-1">Risks</h4>
          <ul className="list-disc list-inside text-xs text-muted-foreground space-y-0.5">
            {note.risks.map((r, i) => (
              <li key={i}>{r}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}

export function MeetingDetail({ meetingId }: { meetingId: string }) {
  return (
    <div className="flex flex-col gap-4 px-4 py-3">
      <div className="flex items-center justify-between gap-2">
        <div className="flex-1">
          <AudioPlayer meetingId={meetingId} />
        </div>
        <ShareDialog meetingId={meetingId} />
      </div>
      <Separator />
      <Tabs defaultValue="summary">
        <TabsList>
          <TabsTrigger value="summary">Summary</TabsTrigger>
          <TabsTrigger value="transcript">Transcript</TabsTrigger>
          <TabsTrigger value="participants">Participants</TabsTrigger>
        </TabsList>
        <TabsContent value="summary">
          <SummaryTab meetingId={meetingId} />
        </TabsContent>
        <TabsContent value="transcript">
          <TranscriptTab meetingId={meetingId} />
        </TabsContent>
        <TabsContent value="participants">
          <ParticipantsList meetingId={meetingId} />
        </TabsContent>
      </Tabs>
    </div>
  )
}
