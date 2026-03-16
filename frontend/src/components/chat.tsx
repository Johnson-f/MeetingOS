"use client"

import * as React from "react"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  SentIcon,
  AiChat02Icon,
  Cancel01Icon,
  ArrowLeft01Icon,
  PlusSignCircleIcon,
  PencilEdit01Icon,
  Delete02Icon,
  Clock01Icon,
} from "@hugeicons/core-free-icons"
import { useAuth } from "@clerk/nextjs"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import { ScrollArea } from "@/components/ui/scroll-area"
import type { SearchSource, ChatThread } from "@/lib/types"

const BACKEND_URL =
  process.env.NEXT_PUBLIC_BACKEND_URL?.replace(/\/$/, "") ?? ""

const CHAT_WIDTH = 380

// ── Helpers ──

function relativeTime(dateStr: string): string {
  const now = Date.now()
  const then = new Date(dateStr).getTime()
  const diffMs = now - then
  const minutes = Math.floor(diffMs / 60000)
  if (minutes < 1) return "just now"
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  return `${days}d ago`
}

function formatTimestamp(ms: number) {
  const totalSeconds = Math.floor(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes}:${seconds.toString().padStart(2, "0")}`
}

// ── Context so any component can know if the chat panel is open ──

type ViewState = "threads" | "conversation" | "new"

const ChatContext = React.createContext<{
  open: boolean
  toggle: () => void
}>({ open: false, toggle: () => {} })

export function useChatPanel() {
  return React.useContext(ChatContext)
}

export function ChatProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = React.useState(false)
  const toggle = React.useCallback(() => setOpen((o) => !o), [])

  return (
    <ChatContext.Provider value={{ open, toggle }}>
      <div className="flex h-full w-full">
        <div
          className="flex-1 min-w-0 transition-[margin] duration-200 ease-in-out"
          style={{ marginRight: open ? CHAT_WIDTH : 0 }}
        >
          {children}
        </div>
        {open && (
          <div
            className="fixed top-0 right-0 z-40 h-full border-l bg-background shadow-lg flex flex-col animate-in slide-in-from-right duration-200"
            style={{ width: CHAT_WIDTH }}
          >
            <ChatPanelRoot />
          </div>
        )}
      </div>
    </ChatContext.Provider>
  )
}

// ── Header trigger button ──

export function ChatTrigger() {
  const { toggle } = useChatPanel()
  return (
    <Button variant="outline" size="sm" onClick={toggle}>
      <HugeiconsIcon icon={AiChat02Icon} strokeWidth={2} className="size-4" />
      Chat AI
    </Button>
  )
}

// ── Source card (shared) ──

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

// ── Chat message type ──

interface ChatMessage {
  role: "user" | "assistant"
  content: string
  sources?: SearchSource[]
  streaming?: boolean
}

// ── Root panel with view management ──

function ChatPanelRoot() {
  const { toggle } = useChatPanel()
  const { getToken } = useAuth()
  const [view, setView] = React.useState<ViewState>("threads")
  const [activeThreadId, setActiveThreadId] = React.useState<string | null>(null)
  const [activeThreadTitle, setActiveThreadTitle] = React.useState<string | null>(null)
  const [threads, setThreads] = React.useState<ChatThread[]>([])
  const [loadingThreads, setLoadingThreads] = React.useState(false)
  const [showAllThreads, setShowAllThreads] = React.useState(false)

  const fetchThreads = React.useCallback(async (limit?: number) => {
    setLoadingThreads(true)
    try {
      const token = await getToken()
      const params = limit ? `?limit=${limit}` : ""
      const res = await fetch(`${BACKEND_URL}/api/v1/chat/threads${params}`, {
        headers: { Authorization: `Bearer ${token}` },
      })
      if (res.ok) {
        const data = await res.json()
        setThreads(data.threads ?? [])
      }
    } catch {
      // ignore
    } finally {
      setLoadingThreads(false)
    }
  }, [getToken])

  // Load recent threads on mount
  React.useEffect(() => {
    if (view === "threads") {
      fetchThreads(showAllThreads ? undefined : 3)
    }
  }, [view, showAllThreads, fetchThreads])

  function openThread(thread: ChatThread) {
    setActiveThreadId(thread.id)
    setActiveThreadTitle(thread.title)
    setView("conversation")
  }

  function startNewChat() {
    setActiveThreadId(null)
    setActiveThreadTitle(null)
    setView("new")
  }

  function goBackToThreads() {
    setActiveThreadId(null)
    setActiveThreadTitle(null)
    setShowAllThreads(false)
    setView("threads")
  }

  async function handleDeleteThread(threadId: string) {
    try {
      const token = await getToken()
      await fetch(`${BACKEND_URL}/api/v1/chat/threads/${threadId}`, {
        method: "DELETE",
        headers: { Authorization: `Bearer ${token}` },
      })
      setThreads((prev) => prev.filter((t) => t.id !== threadId))
    } catch {
      // ignore
    }
  }

  async function handleUpdateTitle(threadId: string, title: string) {
    try {
      const token = await getToken()
      await fetch(`${BACKEND_URL}/api/v1/chat/threads/${threadId}`, {
        method: "PATCH",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({ title }),
      })
      setActiveThreadTitle(title)
    } catch {
      // ignore
    }
  }

  // If threads loaded and empty, jump straight to new chat
  const showNewChatDirectly = view === "threads" && !loadingThreads && threads.length === 0

  if (showNewChatDirectly) {
    return (
      <>
        <div className="flex items-center justify-between px-4 py-3 border-b">
          <div className="flex flex-col gap-0.5">
            <h2 className="text-sm font-medium">Chat AI</h2>
            <p className="text-xs text-muted-foreground">Ask questions about your meetings</p>
          </div>
          <Button variant="ghost" size="icon-sm" onClick={toggle}>
            <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
            <span className="sr-only">Close</span>
          </Button>
        </div>
        <ConversationView
          threadId={null}
          threadTitle={null}
          onThreadCreated={(id, title) => {
            setActiveThreadId(id)
            setActiveThreadTitle(title)
            setView("conversation")
          }}
          onTitleUpdate={setActiveThreadTitle}
        />
      </>
    )
  }

  if (view === "threads") {
    return (
      <>
        <div className="flex items-center justify-between px-4 py-3 border-b">
          <div className="flex flex-col gap-0.5">
            <h2 className="text-sm font-medium">Chat AI</h2>
            <p className="text-xs text-muted-foreground">Ask questions about your meetings</p>
          </div>
          <Button variant="ghost" size="icon-sm" onClick={toggle}>
            <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
            <span className="sr-only">Close</span>
          </Button>
        </div>
        <div className="flex flex-col flex-1 min-h-0 overflow-hidden">
          <div className="px-4 py-3">
            <Button variant="outline" size="sm" className="w-full justify-start gap-2" onClick={startNewChat}>
              <HugeiconsIcon icon={PlusSignCircleIcon} strokeWidth={2} className="size-4" />
              New Chat
            </Button>
          </div>
          <ScrollArea className="flex-1 overflow-hidden px-4">
            {loadingThreads ? (
              <div className="flex flex-col gap-2 py-4">
                {[1, 2, 3].map((i) => (
                  <div key={i} className="h-14 rounded-lg bg-muted/50 animate-pulse" />
                ))}
              </div>
            ) : (
              <div className="flex flex-col gap-1 pb-4">
                {threads.map((thread) => (
                  <div
                    key={thread.id}
                    className="group flex items-center justify-between rounded-lg px-3 py-2.5 hover:bg-muted/50 cursor-pointer transition-colors"
                    onClick={() => openThread(thread)}
                  >
                    <div className="flex flex-col gap-0.5 min-w-0 flex-1">
                      <span className="text-sm font-medium truncate">
                        {thread.title || "Untitled chat"}
                      </span>
                      <div className="flex items-center gap-1 text-xs text-muted-foreground">
                        <HugeiconsIcon icon={Clock01Icon} strokeWidth={2} className="size-3" />
                        {relativeTime(thread.updated_at)}
                      </div>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="opacity-0 group-hover:opacity-100 transition-opacity"
                      onClick={(e) => {
                        e.stopPropagation()
                        handleDeleteThread(thread.id)
                      }}
                    >
                      <HugeiconsIcon icon={Delete02Icon} strokeWidth={2} className="size-3.5 text-muted-foreground" />
                      <span className="sr-only">Delete</span>
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </ScrollArea>
          {!showAllThreads && threads.length >= 3 && (
            <div className="px-4 py-2 border-t">
              <button
                className="text-xs text-muted-foreground hover:text-foreground transition-colors w-full text-center"
                onClick={() => setShowAllThreads(true)}
              >
                View all
              </button>
            </div>
          )}
        </div>
      </>
    )
  }

  if (view === "conversation") {
    return (
      <>
        <ConversationHeader
          title={activeThreadTitle}
          threadId={activeThreadId}
          onBack={goBackToThreads}
          onClose={toggle}
          onTitleSave={handleUpdateTitle}
        />
        <ConversationView
          threadId={activeThreadId}
          threadTitle={activeThreadTitle}
          onThreadCreated={(id, title) => {
            setActiveThreadId(id)
            setActiveThreadTitle(title)
          }}
          onTitleUpdate={(title) => setActiveThreadTitle(title)}
        />
      </>
    )
  }

  // view === "new"
  return (
    <>
      <ConversationHeader
        title={null}
        threadId={null}
        onBack={goBackToThreads}
        onClose={toggle}
        onTitleSave={handleUpdateTitle}
      />
      <ConversationView
        threadId={null}
        threadTitle={null}
        onThreadCreated={(id, title) => {
          setActiveThreadId(id)
          setActiveThreadTitle(title)
          setView("conversation")
        }}
        onTitleUpdate={(title) => setActiveThreadTitle(title)}
      />
    </>
  )
}

// ── Conversation header with editable title ──

function ConversationHeader({
  title,
  threadId,
  onBack,
  onClose,
  onTitleSave,
}: {
  title: string | null
  threadId: string | null
  onBack: () => void
  onClose: () => void
  onTitleSave: (threadId: string, title: string) => void
}) {
  const [editing, setEditing] = React.useState(false)
  const [editValue, setEditValue] = React.useState(title ?? "")
  const inputRef = React.useRef<HTMLInputElement>(null)

  React.useEffect(() => {
    setEditValue(title ?? "")
  }, [title])

  React.useEffect(() => {
    if (editing) {
      inputRef.current?.focus()
    }
  }, [editing])

  function handleSave() {
    setEditing(false)
    if (threadId && editValue.trim() && editValue.trim() !== (title ?? "")) {
      onTitleSave(threadId, editValue.trim())
    }
  }

  return (
    <div className="flex items-center gap-2 px-4 py-3 border-b">
      <Button variant="ghost" size="icon-sm" onClick={onBack}>
        <HugeiconsIcon icon={ArrowLeft01Icon} strokeWidth={2} />
        <span className="sr-only">Back</span>
      </Button>
      <div className="flex-1 min-w-0">
        {editing ? (
          <input
            ref={inputRef}
            className="w-full text-sm font-medium bg-transparent border-b border-foreground/20 outline-none py-0.5"
            value={editValue}
            onChange={(e) => setEditValue(e.target.value)}
            onBlur={handleSave}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSave()
              if (e.key === "Escape") setEditing(false)
            }}
          />
        ) : (
          <div className="flex items-center gap-1.5">
            <h2 className="text-sm font-medium truncate">
              {title || "New Chat"}
            </h2>
            {threadId && (
              <button
                className="text-muted-foreground hover:text-foreground transition-colors"
                onClick={() => setEditing(true)}
              >
                <HugeiconsIcon icon={PencilEdit01Icon} strokeWidth={2} className="size-3.5" />
              </button>
            )}
          </div>
        )}
      </div>
      <Button variant="ghost" size="icon-sm" onClick={onClose}>
        <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
        <span className="sr-only">Close</span>
      </Button>
    </div>
  )
}

// ── Conversation view (used for both existing threads and new chats) ──

function ConversationView({
  threadId,
  threadTitle,
  onThreadCreated,
  onTitleUpdate,
}: {
  threadId: string | null
  threadTitle: string | null
  onThreadCreated: (id: string, title: string | null) => void
  onTitleUpdate: (title: string) => void
}) {
  const { getToken } = useAuth()
  const [input, setInput] = React.useState("")
  const [messages, setMessages] = React.useState<ChatMessage[]>([])
  const [isStreaming, setIsStreaming] = React.useState(false)
  const [loadingMessages, setLoadingMessages] = React.useState(false)
  const scrollRef = React.useRef<HTMLDivElement>(null)
  const currentThreadIdRef = React.useRef<string | null>(threadId)

  // Keep ref in sync
  React.useEffect(() => {
    currentThreadIdRef.current = threadId
  }, [threadId])

  // Scroll on message changes
  React.useEffect(() => {
    scrollRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [messages])

  // Load existing messages when threadId is set
  React.useEffect(() => {
    if (!threadId) {
      setMessages([])
      return
    }

    let cancelled = false
    async function loadMessages() {
      setLoadingMessages(true)
      try {
        const token = await getToken()
        const res = await fetch(`${BACKEND_URL}/api/v1/chat/threads/${threadId}/messages`, {
          headers: { Authorization: `Bearer ${token}` },
        })
        if (res.ok && !cancelled) {
          const data = await res.json()
          const loaded: ChatMessage[] = (data.messages ?? []).map(
            (m: { role: "user" | "assistant"; content: string; sources_json: string | null }) => ({
              role: m.role,
              content: m.content,
              sources: m.sources_json ? JSON.parse(m.sources_json) : undefined,
            })
          )
          setMessages(loaded)
        }
      } catch {
        // ignore
      } finally {
        if (!cancelled) setLoadingMessages(false)
      }
    }

    loadMessages()
    return () => { cancelled = true }
  }, [threadId, getToken])

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!input.trim() || isStreaming) return

    const query = input.trim()
    setInput("")
    setMessages((prev) => [...prev, { role: "user", content: query }])
    setIsStreaming(true)

    setMessages((prev) => [
      ...prev,
      { role: "assistant", content: "", streaming: true },
    ])

    try {
      const token = await getToken()
      const body: Record<string, string> = { query }
      if (currentThreadIdRef.current) {
        body.thread_id = currentThreadIdRef.current
      }

      const response = await fetch(`${BACKEND_URL}/api/v1/chat`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify(body),
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
            } else if (parsed.type === "thread_created") {
              currentThreadIdRef.current = parsed.thread_id
              onThreadCreated(parsed.thread_id, null)
            } else if (parsed.type === "thread_title") {
              onTitleUpdate(parsed.title)
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
    <div className="flex flex-col flex-1 min-h-0 overflow-hidden">
      <ScrollArea className="flex-1 overflow-hidden px-4">
        <div className="flex flex-col gap-4 pb-4">
          {loadingMessages ? (
            <div className="flex flex-col gap-2 py-4">
              {[1, 2, 3].map((i) => (
                <div key={i} className="h-8 rounded-lg bg-muted/50 animate-pulse" />
              ))}
            </div>
          ) : messages.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-[400px] gap-2">
              <HugeiconsIcon icon={AiChat02Icon} strokeWidth={1.5} className="size-8 text-muted-foreground/40" />
              <p className="text-sm text-muted-foreground">
                Ask anything about your meetings...
              </p>
            </div>
          ) : (
            messages.map((msg, i) => (
              <div key={i} className="flex flex-col gap-2">
                {msg.role === "user" ? (
                  <div className="flex justify-end">
                    <div className="max-w-[85%] rounded-lg bg-primary px-3 py-2 text-xs text-primary-foreground">
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
            ))
          )}
          <div ref={scrollRef} />
        </div>
      </ScrollArea>
      <form onSubmit={handleSubmit} className="flex items-center gap-2 px-4 py-3 border-t">
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
