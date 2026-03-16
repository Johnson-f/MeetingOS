"use client"

import * as React from "react"
import { HugeiconsIcon } from "@hugeicons/react"
import { SentIcon } from "@hugeicons/core-free-icons"
import { useAuth } from "@clerk/nextjs"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import { ScrollArea } from "@/components/ui/scroll-area"
import type { SearchSource } from "@/lib/types"

const BACKEND_URL =
  process.env.NEXT_PUBLIC_BACKEND_URL?.replace(/\/$/, "") ?? ""

function formatTimestamp(ms: number) {
  const totalSeconds = Math.floor(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes}:${seconds.toString().padStart(2, "0")}`
}

function SourceCard({ source, index }: { source: SearchSource; index: number }) {
  return (
    <div className="rounded-lg border border-border/50 p-3 text-xs">
      <div className="flex items-center justify-between mb-1.5">
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="text-[0.6rem] px-1.5">
            Source {index + 1}
          </Badge>
          {source.meeting_title && (
            <span className="text-muted-foreground font-medium">{source.meeting_title}</span>
          )}
          {source.speaker_label && (
            <span className="text-muted-foreground">{source.speaker_label}</span>
          )}
        </div>
        <span className="text-muted-foreground/60 tabular-nums text-[0.65rem]">
          {formatTimestamp(source.start_ms)}
        </span>
      </div>
      <p className="text-muted-foreground leading-relaxed line-clamp-3">
        {source.text}
      </p>
    </div>
  )
}

interface ChatMessage {
  role: "user" | "assistant"
  content: string
  sources?: SearchSource[]
  streaming?: boolean
}

export function MeetingChat() {
  const { getToken } = useAuth()
  const [input, setInput] = React.useState("")
  const [messages, setMessages] = React.useState<ChatMessage[]>([])
  const [isStreaming, setIsStreaming] = React.useState(false)
  const scrollRef = React.useRef<HTMLDivElement>(null)

  React.useEffect(() => {
    scrollRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [messages])

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!input.trim() || isStreaming) return

    const query = input.trim()
    setInput("")
    setMessages((prev) => [...prev, { role: "user", content: query }])
    setIsStreaming(true)

    // Add empty assistant message that we'll stream into
    setMessages((prev) => [
      ...prev,
      { role: "assistant", content: "", streaming: true },
    ])

    try {
      const token = await getToken()
      const response = await fetch(`${BACKEND_URL}/api/v1/chat`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({ query }),
      })

      if (!response.ok) {
        throw new Error(`Chat request failed: ${response.status}`)
      }

      const reader = response.body?.getReader()
      if (!reader) throw new Error("No response stream")

      const decoder = new TextDecoder()
      let buffer = ""
      let sources: SearchSource[] = []

      while (true) {
        const { done, value } = await reader.read()
        if (done) break

        buffer += decoder.decode(value, { stream: true })

        // Process complete SSE lines
        while (buffer.includes("\n")) {
          const newlineIndex = buffer.indexOf("\n")
          const line = buffer.slice(0, newlineIndex).trim()
          buffer = buffer.slice(newlineIndex + 1)

          if (!line.startsWith("data: ")) continue
          const data = line.slice(6)

          try {
            const parsed = JSON.parse(data)

            if (parsed.type === "answer_chunk") {
              setMessages((prev) => {
                const updated = [...prev]
                const last = updated[updated.length - 1]
                if (last?.role === "assistant") {
                  updated[updated.length - 1] = {
                    ...last,
                    content: last.content + parsed.content,
                  }
                }
                return updated
              })
            } else if (parsed.type === "done") {
              sources = parsed.sources ?? []
            } else if (parsed.type === "error") {
              setMessages((prev) => {
                const updated = [...prev]
                const last = updated[updated.length - 1]
                if (last?.role === "assistant") {
                  updated[updated.length - 1] = {
                    ...last,
                    content: `Error: ${parsed.content}`,
                    streaming: false,
                  }
                }
                return updated
              })
            }
          } catch {
            // ignore malformed JSON
          }
        }
      }

      // Finalize the message with sources
      setMessages((prev) => {
        const updated = [...prev]
        const last = updated[updated.length - 1]
        if (last?.role === "assistant") {
          updated[updated.length - 1] = {
            ...last,
            sources,
            streaming: false,
          }
        }
        return updated
      })
    } catch (err) {
      setMessages((prev) => {
        const updated = [...prev]
        const last = updated[updated.length - 1]
        if (last?.role === "assistant") {
          updated[updated.length - 1] = {
            ...last,
            content:
              (err as Error).message || "Something went wrong. Please try again.",
            streaming: false,
          }
        }
        return updated
      })
    } finally {
      setIsStreaming(false)
    }
  }

  return (
    <div className="flex flex-col h-[600px]">
      <ScrollArea className="flex-1 pr-4">
        <div className="flex flex-col gap-4 pb-4">
          {messages.length === 0 && (
            <div className="flex items-center justify-center h-[400px]">
              <p className="text-sm text-muted-foreground">
                Ask anything about your meetings...
              </p>
            </div>
          )}
          {messages.map((msg, i) => (
            <div key={i} className="flex flex-col gap-2">
              {msg.role === "user" ? (
                <div className="flex justify-end">
                  <div className="max-w-[80%] rounded-lg bg-primary px-3 py-2 text-xs text-primary-foreground">
                    {msg.content}
                  </div>
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  <div className="max-w-[90%] text-xs text-foreground leading-relaxed whitespace-pre-wrap">
                    {msg.content}
                    {msg.streaming && (
                      <span className="inline-block w-1.5 h-3.5 bg-foreground/60 animate-pulse ml-0.5" />
                    )}
                  </div>
                  {msg.sources && msg.sources.length > 0 && (
                    <>
                      <Separator className="my-1" />
                      <div className="flex flex-col gap-1.5">
                        <span className="text-[0.65rem] text-muted-foreground font-medium">
                          Sources ({msg.sources.length})
                        </span>
                        {msg.sources.map((source, j) => (
                          <SourceCard key={j} source={source} index={j} />
                        ))}
                      </div>
                    </>
                  )}
                </div>
              )}
            </div>
          ))}
          <div ref={scrollRef} />
        </div>
      </ScrollArea>
      <form onSubmit={handleSubmit} className="flex items-center gap-2 pt-3 border-t">
        <Input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="Ask about your meetings..."
          disabled={isStreaming}
          className="flex-1"
        />
        <Button
          type="submit"
          size="icon"
          disabled={isStreaming || !input.trim()}
        >
          <HugeiconsIcon icon={SentIcon} strokeWidth={2} className="size-4" />
          <span className="sr-only">Send</span>
        </Button>
      </form>
    </div>
  )
}
