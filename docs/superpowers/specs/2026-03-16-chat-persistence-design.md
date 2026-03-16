# Chat Persistence with Thread History & AI Memory

**Date:** 2026-03-16
**Status:** Approved

## Overview

Add persistent chat threads with message history, AI-generated titles, and cross-thread memory via vector search. Users can create chat threads, have multi-turn conversations with the AI, and the AI remembers relevant context from past conversations through semantic search.

## Database Schema

### `chat_threads`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT PK | UUIDv4 |
| user_id | TEXT NOT NULL | FK to users |
| workspace_id | TEXT NOT NULL | FK to workspaces |
| title | TEXT | Auto-generated from first message, user-editable. Fallback: first 50 chars of first message if generation fails |
| created_at | TEXT NOT NULL | RFC3339 |
| updated_at | TEXT NOT NULL | RFC3339 |
| deleted_at | TEXT | Soft delete |

**Indexes:** `(user_id, deleted_at)`

### `chat_messages`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT PK | UUIDv4 |
| thread_id | TEXT NOT NULL | FK to chat_threads |
| role | TEXT NOT NULL | "user" or "assistant" |
| content | TEXT NOT NULL | Message text |
| sources_json | TEXT | JSON array of SearchSource, only set for assistant messages |
| created_at | TEXT NOT NULL | RFC3339 |

**Indexes:** `(thread_id, created_at)`

## API Endpoints

### Thread Management

All endpoints require Clerk JWT. Thread ownership is verified by joining `chat_threads.user_id` against the authenticated user before returning data or allowing mutations.

| Method | Route | Description |
|--------|-------|-------------|
| GET | `/api/v1/chat/threads` | List threads. `?limit=3` for recent view, omit for all. Sorted by `updated_at DESC`. Excludes soft-deleted. |
| GET | `/api/v1/chat/threads/:id/messages` | Messages for a thread, ordered by `created_at ASC`. Supports `?limit=50&before=<message_id>` cursor pagination. |
| PATCH | `/api/v1/chat/threads/:id` | Update thread title. Body: `{ title: string }` |
| DELETE | `/api/v1/chat/threads/:id` | Soft delete (sets `deleted_at`) |

### Chat (replaces current stateless `/api/v1/chat`)

**POST `/api/v1/chat`**

Request body:
```json
{
  "query": "string",
  "thread_id": "uuid | null",
  "meeting_id": "uuid | null"
}
```

`meeting_id` is kept for backward compatibility — when provided, Qdrant search scopes to that meeting instead of the full user corpus. This preserves the existing meeting-scoped chat behavior.

Flow:
1. If no `thread_id` → create new thread row, fire-and-forget title generation
2. Save user message to `chat_messages`
3. Load last 20 messages from thread as conversation history
4. Embed query → Qdrant hybrid search (transcripts + past chat) → Jina rerank top 5
5. Deduplicate: if a search result's text already appears in the conversation history, drop it to avoid wasting context tokens
6. Build Groq prompt: system + conversation history + search sources + user query
7. Stream response via SSE
8. On stream completion → save assistant message + sources_json to `chat_messages` (even if partial — save what was streamed)
9. Enqueue `vectorize_chat_qa` job via the jobs system to embed the Q&A pair into Qdrant (provides retry semantics, consistent with transcript vectorization pattern)

SSE event types:
```json
{"type": "thread_created", "thread_id": "uuid", "title": null}
{"type": "answer_chunk", "content": "text"}
{"type": "thread_title", "title": "Generated title"}
{"type": "done", "sources": [...]}
{"type": "error", "content": "error message"}
```

The `thread_title` event is sent on the same SSE stream, before the `done` event. Title generation runs concurrently with the main answer stream. If the title Groq call hasn't completed by the time `done` is ready, `done` is sent without waiting — the frontend fetches the title on next thread list load.

### Title Generation

Async, concurrent with the answer stream:
- Prompt: "Generate a concise 5-8 word title for a conversation that starts with this question: {query}. Return only the title, nothing else."
- Model: same as notes_model
- Result stored in `chat_threads.title`
- Sent to frontend via `thread_title` SSE event if ready before stream ends
- **Fallback:** if Groq call fails, set title to first 50 characters of the user's query + "..."

## Qdrant Schema Changes

### Add `source_type` field to all points

**Existing transcript points** are stored without a `source_type` field. The following changes are needed:

1. **New transcript upserts** include `source_type: "transcript"` in the payload going forward
2. **Search result parsing** treats missing `source_type` as `"transcript"` (backward compatible — all existing points are transcripts)
3. **Add `source_type` keyword index** to the Qdrant collection via `ensure_indexes`
4. **No backfill migration needed** — the "missing = transcript" default handles existing data

### Refactor `SearchResult` to support polymorphic sources

The existing `SearchResult` struct requires `meeting_id`, `start_ms`, `end_ms` which don't exist on chat points. Refactor to:

```rust
pub struct SearchResult {
    pub source_type: String,       // "transcript" or "chat"
    pub text: String,
    // Transcript-specific (None for chat sources)
    pub meeting_id: Option<String>,
    pub meeting_title: Option<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub speaker_label: Option<String>,
    // Chat-specific (None for transcript sources)
    pub thread_id: Option<String>,
    pub created_at: Option<String>,
}
```

The `hybrid_search` result parsing changes from `filter_map` (which drops points missing required fields) to always extracting `text` and `source_type`, with optional fields parsed based on source type.

### Chat Q&A Payload

```json
{
  "source_type": "chat",
  "thread_id": "uuid",
  "user_id": "uuid",
  "text": "Q: ...\nA: ...",
  "created_at": "RFC3339"
}
```

### Transcript Payload (updated)

```json
{
  "source_type": "transcript",
  "meeting_id": "uuid",
  "meeting_title": "string",
  "user_id": "uuid",
  "text": "...",
  "start_ms": 0,
  "end_ms": 0,
  "speaker_label": "string",
  "chunk_index": 0
}
```

## Vector Memory

### What Gets Vectorized

After each assistant response completes, a `vectorize_chat_qa` job is enqueued. The job:
1. Combines the Q&A pair: `"Q: {user question}\nA: {assistant answer}"`
2. Skips vectorization if the answer is too short (< 50 chars) or is an error message
3. Embeds via Jina with task `"retrieval.passage"`
4. Upserts to Qdrant with the chat payload schema above

Only Q&A pairs are vectorized — not individual messages. This captures knowledge, not conversation noise.

### Search Behavior

- Chat search queries both source types (transcript + chat) — no filter on `source_type`
- User filter (`user_id`) ensures per-user isolation
- Context formatting distinguishes sources:
  - Transcript: `[Source N] (Meeting: Title, Speaker: Name, Time: M:S)`
  - Chat: `[Source N] (Past conversation, date)`
- Meeting-only search (when `meeting_id` is provided) filters to `source_type: "transcript"` OR `meeting_id` match
- Deduplication: search results whose text overlaps with conversation history messages are dropped before building the prompt

### Soft Delete and Qdrant Cleanup

When a thread is soft-deleted, its vectorized Q&A pairs remain in Qdrant. This is intentional — the knowledge from past conversations is still valid and useful. If a user explicitly wants to "forget" a conversation, a future enhancement could add a hard-delete option that also removes Qdrant points filtered by `thread_id`.

## Conversation Context Window

Last 20 messages from the current thread are included in the Groq prompt. Older messages are in the DB (scrollable in UI via cursor pagination) and discoverable via vector search, but not sent directly to the LLM.

The 20-message limit is a constant. With typical Q&A lengths (~100-200 tokens per message), 20 messages ≈ 2K-4K tokens, leaving ample room for search sources (~1K-2K tokens) and the system prompt within standard context windows (8K-128K depending on Groq model).

### Prompt Structure

```
System: You are a meeting Q&A assistant. Answer based on meeting transcript
excerpts and relevant past conversations. [rules...]

[Conversation History]
User: previous question 1
Assistant: previous answer 1
...

[Meeting & Chat Sources]
[Source 1] (Meeting: Q3 Planning, Speaker: John, Time: 2:31)
excerpt text...

[Source 2] (Past conversation, Mar 14)
Q: what was the budget?
A: The budget was set at...

User: current question
```

Three layers of context:
1. **Conversation history** — thread continuity for follow-ups
2. **Meeting sources** — factual grounding from transcripts
3. **Chat sources** — accumulated knowledge from past conversations

## Frontend — Chat Panel

### Three Views

**Thread list** (default when no active thread):
- "New Chat" button at top
- 3 most recent threads: title + relative timestamp
- "View all" link → scrollable list of all threads within the panel
- Click thread → conversation view

**Conversation view** (active thread):
- Back arrow → thread list
- Thread title in header, click to edit inline
- Messages loaded from API (paginated, most recent first)
- Input bar, streaming behavior same as current

**New chat mode** (no thread yet):
- Empty state with AI icon + placeholder
- First message send → backend creates thread
- `thread_created` SSE event provides thread_id
- `thread_title` SSE event updates title when generated

### Thread Creation

Lazy — thread only created when user sends first message. No empty/orphan threads.

## Source Pollution Prevention

Chat Q&A pairs in Qdrant are tagged `source_type: "chat"`. Any future meeting-transcript-only search can filter to `source_type: "transcript"` to exclude chat-derived content. The unified chat search includes both, with clear labeling so the AI distinguishes factual meeting content from prior AI-generated answers. Existing Qdrant points without `source_type` are treated as `"transcript"` by default.
