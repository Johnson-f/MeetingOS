"use client"

import * as React from "react"
import { useParams } from "next/navigation"
import { useQuery } from "@tanstack/react-query"
import ReactMarkdown from "react-markdown"
import { createBackendClient } from "@/lib/backend_connection"
import { queryKeys } from "@/lib/service"

function formatTimestamp(ms: number) {
  const totalSeconds = Math.floor(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes}:${seconds.toString().padStart(2, "0")}`
}

function formatDate(dateStr: string | null) {
  if (!dateStr) return null
  try {
    return new Date(dateStr).toLocaleString(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
    })
  } catch {
    return dateStr
  }
}

// A public-only client without auth
const publicClient = createBackendClient()

export default function SharedMeetingPage() {
  const params = useParams()
  const token = typeof params.token === "string" ? params.token : ""

  const { data, isLoading, isError, error } = useQuery({
    queryKey: queryKeys.sharedMeeting(token),
    queryFn: () => publicClient.getSharedMeeting(token),
    enabled: Boolean(token),
    retry: false,
  })

  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <p className="text-muted-foreground text-sm">Loading shared meeting…</p>
      </div>
    )
  }

  if (isError || !data) {
    const message = (error as Error)?.message ?? ""
    const isExpired =
      message.toLowerCase().includes("expired") ||
      message.toLowerCase().includes("not found") ||
      message.toLowerCase().includes("404")
    return (
      <div className="min-h-screen flex flex-col items-center justify-center gap-3 px-4">
        <h1 className="text-xl font-semibold">
          {isExpired ? "Link Expired or Invalid" : "Could Not Load Meeting"}
        </h1>
        <p className="text-sm text-muted-foreground text-center max-w-sm">
          {isExpired
            ? "This sharing link has expired or is no longer valid."
            : message || "An unexpected error occurred."}
        </p>
      </div>
    )
  }

  const { meeting, participants, audio_url } = data
  const note = meeting.note ?? null
  const transcription = meeting.transcription ?? null
  const meetingDate =
    formatDate(meeting.actual_start_at) ??
    formatDate(meeting.scheduled_start_at)

  return (
    <div className="min-h-screen flex flex-col">
      <main className="flex-1 max-w-3xl mx-auto w-full px-4 py-8 flex flex-col gap-8">
        {/* Header */}
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-bold">{meeting.title}</h1>
          <div className="flex flex-wrap gap-3 text-sm text-muted-foreground">
            {meetingDate && <span>{meetingDate}</span>}
            {meeting.actual_end_at && meeting.actual_start_at && (
              <span>
                Duration:{" "}
                {Math.round(
                  (new Date(meeting.actual_end_at).getTime() -
                    new Date(meeting.actual_start_at).getTime()) /
                    60000
                )}{" "}
                min
              </span>
            )}
            <span className="capitalize">{meeting.platform}</span>
          </div>
        </div>

        {/* Audio player */}
        {audio_url && (
          <div>
            <h2 className="text-base font-semibold mb-2">Recording</h2>
            <audio controls className="w-full h-10" preload="metadata">
              <source src={audio_url} type="audio/mpeg" />
              Your browser does not support the audio element.
            </audio>
          </div>
        )}

        {/* Notes */}
        {note && (
          <div className="flex flex-col gap-6">
            {note.summary_markdown && (
              <div>
                <h2 className="text-base font-semibold mb-2">Summary</h2>
                <div className="prose prose-sm max-w-none text-muted-foreground">
                  <ReactMarkdown>{note.summary_markdown}</ReactMarkdown>
                </div>
              </div>
            )}

            {note.key_points && note.key_points.length > 0 && (
              <div>
                <h2 className="text-base font-semibold mb-2">Key Points</h2>
                <ul className="list-disc list-inside text-sm text-muted-foreground space-y-1">
                  {note.key_points.map((point, i) => (
                    <li key={i}>{point}</li>
                  ))}
                </ul>
              </div>
            )}

            {note.action_items && note.action_items.length > 0 && (
              <div>
                <h2 className="text-base font-semibold mb-2">Action Items</h2>
                <ul className="text-sm text-muted-foreground space-y-2">
                  {note.action_items.map((item) => (
                    <li key={item.id} className="flex items-start gap-2">
                      <span className="shrink-0 mt-1.5 size-1.5 rounded-full bg-foreground/40" />
                      <span>
                        {item.description}
                        {item.assignee_name && (
                          <span className="text-foreground/60">
                            {" "}— {item.assignee_name}
                          </span>
                        )}
                        {item.due_date && (
                          <span className="text-foreground/60">
                            {" "}(due {item.due_date})
                          </span>
                        )}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        )}

        {/* Participants */}
        {participants && participants.length > 0 && (
          <div>
            <h2 className="text-base font-semibold mb-2">
              Participants ({participants.length})
            </h2>
            <div className="flex flex-col gap-1.5">
              {participants.map((p) => {
                const name = p.display_name ?? p.email ?? "Unknown"
                return (
                  <div key={p.id} className="flex items-center gap-2 text-sm">
                    <div className="flex size-6 shrink-0 items-center justify-center rounded-full bg-secondary text-xs font-semibold uppercase text-secondary-foreground">
                      {name.charAt(0)}
                    </div>
                    <span className="font-medium">{name}</span>
                    {p.is_host && (
                      <span className="text-[0.6rem] font-semibold uppercase tracking-wide text-primary bg-primary/10 px-1.5 py-0.5 rounded-full">
                        Host
                      </span>
                    )}
                    {p.email && p.display_name && (
                      <span className="text-muted-foreground">{p.email}</span>
                    )}
                  </div>
                )
              })}
            </div>
          </div>
        )}

        {/* Transcript */}
        {transcription &&
          transcription.status === "ready" &&
          (transcription.segments?.length > 0 || transcription.full_text) && (
            <div>
              <h2 className="text-base font-semibold mb-2">Transcript</h2>
              {transcription.segments && transcription.segments.length > 0 ? (
                <div className="flex flex-col gap-2 max-h-[500px] overflow-y-auto pr-1">
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
              ) : (
                <p className="text-sm text-muted-foreground leading-relaxed whitespace-pre-wrap">
                  {transcription.full_text}
                </p>
              )}
            </div>
          )}
      </main>

      {/* Footer */}
      <footer className="border-t py-4 px-4 text-center text-xs text-muted-foreground">
        Powered by Meeting Bot
      </footer>
    </div>
  )
}
