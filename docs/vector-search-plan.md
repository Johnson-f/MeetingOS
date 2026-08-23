# Vector Search & AI Q&A Implementation Plan

## Overview

After a meeting transcript is generated, chunk it, embed with Jina, store in Qdrant. Users can ask questions → embed query → hybrid search (semantic + keyword) → rerank with Jina → send to LLM for answer.

---

## Architecture

```
Transcript Ready
    ↓
Chunk transcript (sentence-based, 400-512 tokens, 10% overlap)
    ↓
Embed chunks via Jina (jina-embeddings-v3, 1024 dims)
    ↓
Store in Qdrant (dense + sparse vectors per chunk)
    ↓
User asks a question
    ↓
Embed query via Jina (task: retrieval.query)
    ↓
Hybrid search in Qdrant:
  - Dense semantic search (top 25)
  - Sparse BM25 keyword search (top 25)
  - Reciprocal Rank Fusion
    ↓
Rerank top results via Jina Reranker v3
    ↓
Send top 5 chunks + question to Groq LLM
    ↓
Return grounded answer with citations
```

---

## Tech Stack

| Component | Tool | Model/Version |
|---|---|---|
| Embeddings | Jina API | `jina-embeddings-v3` (1024 dims) |
| Reranker | Jina API | `jina-reranker-v3` |
| Vector DB | Qdrant | v1.17+ with hybrid search |
| LLM | Groq | Same model as notes (Llama 4 Scout) |
| Chunking | Custom Rust | Sentence-based with overlap |

---

## Implementation Steps

### Step 1: Add dependencies and config

**Cargo.toml:**
```toml
qdrant-client = "1.17"
```

**.env:**
```
JINA_API_KEY=your_key_here
QDRANT_URL=http://localhost:6334
QDRANT_API_KEY=optional_for_cloud
```

**config.rs:** Add `JinaConfig` and `QdrantConfig` structs.

**Files:**
- `src/config.rs` — add JinaConfig, QdrantConfig
- `Cargo.toml` — add qdrant-client

---

### Step 2: Create Jina client

HTTP client for Jina's API with two methods:

**`embed(texts, task)`** — POST to `https://api.jina.ai/v1/embeddings`
```json
{
  "model": "jina-embeddings-v3",
  "input": ["chunk1", "chunk2"],
  "task": "retrieval.passage"  // or "retrieval.query" for queries
}
```
Returns: `Vec<Vec<f32>>` (1024-dim vectors)

**`rerank(query, documents, top_n)`** — POST to `https://api.jina.ai/v1/rerank`
```json
{
  "model": "jina-reranker-v3",
  "query": "user question",
  "documents": ["chunk1", "chunk2"],
  "top_n": 5,
  "return_documents": true
}
```
Returns: ranked documents with relevance scores

**Files:**
- `src/service/jina/mod.rs`
- `src/service/jina/client.rs`

---

### Step 3: Create Qdrant client wrapper

Wrapper around `qdrant-client` crate for our specific use case.

**Collection setup:**
- Collection name: `meeting_transcripts`
- Dense vectors: `"dense"` — 1024 dims, Cosine distance (from Jina)
- Sparse vectors: `"sparse"` — BM25 keyword vectors (built by Qdrant)
- Payload fields: `meeting_id`, `user_id`, `chunk_index`, `text`, `start_ms`, `end_ms`, `speaker_label`

**Methods:**
- `ensure_collection()` — create collection if not exists
- `upsert_chunks(meeting_id, chunks_with_vectors)` — store embedded chunks
- `hybrid_search(query_vector, sparse_vector, user_id, limit)` — prefetch dense + sparse, fuse with RRF
- `delete_meeting_chunks(meeting_id)` — cleanup on meeting delete

**Files:**
- `src/service/qdrant/mod.rs`
- `src/service/qdrant/client.rs`

---

### Step 4: Implement transcript chunker

Strategy: **Sentence-based chunking with overlap** (research-backed best practice for transcripts).

1. Split transcript into sentences (by `.`, `?`, `!` boundaries)
2. Group sentences into chunks of ~400-512 tokens
3. Overlap: include the last 1-2 sentences of the previous chunk at the start of the next
4. Each chunk carries metadata: `meeting_id`, `chunk_index`, `start_ms`, `end_ms` (from transcript segments), `speaker_label`
5. Prepend context to each chunk: `"Meeting: {title} | Speaker: {speaker}"` — makes chunks self-contained for retrieval

**Why this approach:**
- Sentence boundaries preserve meaning (no mid-sentence cuts)
- 400-512 tokens is the sweet spot for embedding quality
- 10% overlap prevents losing context at boundaries
- Contextual prefix improves retrieval accuracy by ~20% (per 2026 research)
- Speaker labels help disambiguate who said what

**Files:**
- `src/service/vector/chunker.rs`

---

### Step 5: Vectorization job (runs after transcription)

New job type: `vectorize_transcript`

**Triggered by:** `transcribe_recording_job` — after transcription is stored, enqueue vectorization.

**Flow:**
1. Load transcript (full text + segments) from Turso
2. Chunk the transcript using the chunker
3. Embed all chunks via Jina (`task: retrieval.passage`)
4. Generate sparse vectors (BM25) — Qdrant can do this server-side since v1.15
5. Upsert all chunks + vectors to Qdrant
6. Log completion

**Files:**
- `src/service/jobs/handlers.rs` — add `vectorize_transcript_job`
- `src/service/jobs/constants.rs` — add `JOB_VECTORIZE_TRANSCRIPT`

---

### Step 6: Search API endpoint

New endpoint: `POST /api/v1/meetings/search`

**Request:**
```json
{
  "query": "What did we decide about the API deadline?",
  "meeting_id": "optional—scope to one meeting"
}
```

**Flow:**
1. Embed the query via Jina (`task: retrieval.query`)
2. Hybrid search in Qdrant:
   - Dense: semantic similarity (top 25)
   - Sparse: BM25 keyword match (top 25)
   - Fusion: Reciprocal Rank Fusion
3. Extract text from top 15 results
4. Rerank via Jina Reranker v3 (query + 15 documents → top 5)
5. Send top 5 chunks + user's question to Groq LLM with a system prompt:
   ```
   Answer the question based ONLY on the meeting transcript excerpts provided.
   Cite which meeting and approximate timestamp when possible.
   If the answer isn't in the excerpts, say so.
   ```
6. Return the LLM's answer + the source chunks (for frontend to display citations)

**Response:**
```json
{
  "answer": "The team decided to set the API deadline for March 28th...",
  "sources": [
    {
      "meeting_id": "...",
      "meeting_title": "Q2 Planning",
      "text": "...",
      "start_ms": 145000,
      "speaker_label": "Sarah"
    }
  ]
}
```

**Files:**
- `src/routes/search.rs` — new route handler
- `src/routes/router.rs` — register route
- `src/service/vector/search.rs` — search orchestration logic

---

### Step 7: Frontend search UI

- Search bar component (can live in the header or as a dedicated page)
- Displays the LLM answer
- Shows source citations with meeting title, speaker, and timestamp
- Clicking a citation navigates to that meeting's transcript at that timestamp

**Files:**
- `frontend/src/components/meetings/meeting-search.tsx`
- `frontend/src/lib/hooks/use-meeting-search.ts`
- `frontend/src/lib/backend_connection/client.ts` — add search method

---

### Step 8: Wire into ServiceRegistry

- Add `JinaClient` and `QdrantClient` to `ServiceRegistry`
- Initialize on startup (optional, like Redis)
- Log status: `jina=true/false qdrant=true/false`
- Add `vector_search_ready()` helper

---

## File Structure

```
src/service/
├── jina/
│   ├── mod.rs
│   └── client.rs          # Jina embed + rerank HTTP client
├── qdrant/
│   ├── mod.rs
│   └── client.rs          # Qdrant collection + search wrapper
└── vector/
    ├── mod.rs
    ├── chunker.rs          # Transcript chunking logic
    └── search.rs           # Search orchestration (embed → search → rerank → LLM)
```

---

## Implementation Order

1. Config + dependencies (Step 1)
2. Jina client (Step 2)
3. Qdrant client (Step 3)
4. Chunker (Step 4)
5. Vectorization job (Step 5)
6. Search endpoint + orchestration (Step 6)
7. Frontend (Step 7)
8. Wire into ServiceRegistry (Step 8 — do alongside Step 2-3)

Steps 2-4 are independent and can be built in parallel.
Step 5 depends on 2-4.
Step 6 depends on 2-3 + 5.
Step 7 depends on 6.
