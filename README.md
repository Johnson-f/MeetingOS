# MeetingOS

MeetingOS is an AI-powered meeting intelligence system that joins online calls, captures the conversation, produces speaker-aware transcripts and structured notes, and makes past meetings searchable through an AI assistant.

It supports Google Meet, Zoom, and Microsoft Teams links. Users can send a bot to a meeting immediately, schedule it for later, or connect Google Calendar so upcoming calls are discovered automatically. After the call, MeetingOS turns the recording into a durable meeting record containing audio, participants, a transcript, a summary, decisions, topics, and action items.

MeetingOS is more than a meeting bot. The bot is only the capture layer; the rest of the system is an operating layer for collecting, processing, organizing, searching, and sharing meeting knowledge.

## What the system provides

- Immediate and scheduled meeting capture
- Google Calendar discovery and synchronization
- Speaker-aware transcription
- Structured summaries, decisions, topics, and action items
- Searchable meeting history and analytics
- AI chat grounded in meeting transcripts and previous conversations
- Participant tracking and meeting sharing
- Real-time processing updates in the dashboard
- Durable background processing with retries and recovery

## Architecture at a glance

MeetingOS is split into a browser application, an HTTP API, a background worker, and a set of external services.

The frontend presents the dashboard, meeting history, schedules, meeting details, analytics, sharing controls, and the meeting Q&A assistant. It communicates with the backend through authenticated HTTP requests and receives processing updates through Server-Sent Events.

The Rust backend owns the application rules. It authenticates users, isolates data by user and workspace, creates meeting records, schedules capture bots, accepts provider webhooks, runs the processing pipeline, exposes meeting data, and coordinates search and sharing.

Long-running work does not happen inside webhook or API requests. It is written to a durable job queue in the primary database and processed asynchronously by a worker. The API and worker can run together in one process or independently for deployment and scaling.

## Technology stack

### Web application

- **Next.js 16 and React 19** provide the browser application and route structure.
- **TypeScript** defines the frontend API and meeting data contracts.
- **Tailwind CSS 4**, Base UI, and shadcn-style components provide the visual system.
- **TanStack Query** manages server state, caching, refetching, and invalidation in the browser.
- **TanStack Table** and **Recharts** support data-heavy meeting and analytics views.
- **Clerk for Next.js** manages browser authentication and supplies tokens to the backend.

### Application backend

- **Rust** provides the core service and background worker.
- **Axum** exposes the HTTP API, webhooks, public share pages, and event stream.
- **Tokio** runs asynchronous network, database, storage, and background tasks.
- **Serde** defines and validates JSON exchanged between the application and external providers.
- **Reqwest with rustls** handles outbound HTTPS calls without depending on the host's TLS stack.
- **Tower HTTP** provides request tracing and CORS controls.

### Data and infrastructure

- **Turso through libSQL** is the system of record for users, workspaces, meetings, provider events, jobs, recordings, transcripts, notes, participants, calendar state, chat history, share links, and delivery records.
- **Cloudflare R2**, accessed through its S3-compatible API, stores meeting audio separately from relational data.
- **Redis** is an optional derived cache for meeting lists, analytics, completed transcripts, notes, audio URLs, and chat history.
- **Qdrant** stores searchable vectors in separate collections for transcript knowledge and previous chat exchanges.
- **Docker** packages the Rust service, while **Caddy** terminates HTTPS and reverse-proxies traffic to the backend.
- **GitHub Actions** checks and builds the Rust service on changes and publishes tagged backend images to Docker Hub.

### AI and integrations

- **Recall.ai** supplies the meeting participant that joins calls and exposes recording, participant, and speaker-timeline data.
- **Groq** runs speech-to-text and structured note generation. The default configuration uses Whisper Large V3 for transcription and Llama 3.3 70B Versatile for notes and chat, but both models are configurable.
- **Jina AI** creates retrieval embeddings and reranks search candidates against the user's question.
- **Qdrant** performs semantic and hybrid retrieval over transcript chunks and prior chat exchanges.
- **Google Calendar** supplies upcoming meeting events through OAuth, incremental synchronization, and push notifications.
- **Resend** delivers meeting-share emails and records delivery outcomes.

## How a meeting moves through MeetingOS

### 1. Authentication and workspace resolution

Clerk authenticates the user in the frontend. Protected API calls include the Clerk token, which the Axum middleware validates against Clerk's signing keys. The backend then resolves the authenticated subject into MeetingOS user and workspace records.

Meeting reads and mutations are performed through that user context. This keeps application data associated with the workspace that created it instead of trusting user or workspace identifiers supplied by the browser.

### 2. Meeting discovery or creation

A meeting can enter the system in two ways.

For a manual meeting, the user pastes a supported meeting URL and chooses whether the bot should join now or later. The backend normalizes the URL, detects its platform, resolves the requested time, and creates a workspace-scoped deduplication key. Repeating the same request therefore resolves to the existing meeting instead of creating multiple bots.

For a calendar meeting, the user connects Google Calendar through OAuth. MeetingOS stores the connection, discovers calendars, registers Google watch channels, and synchronizes events. Events containing supported meeting links become scheduled MeetingOS meetings. Sync cursors make later updates incremental, while expired cursors trigger a safe full refresh. Changed and cancelled calendar events update the linked meeting record.

### 3. Bot scheduling and call capture

When a meeting starts immediately, the backend asks Recall.ai to create a bot and attaches the MeetingOS meeting, workspace, and user identifiers as provider metadata. For future meetings, a recurring scheduler finds meetings approaching their start time and creates the bot at the appropriate point.

Recall.ai handles joining the conference and capturing the call. MeetingOS tracks the provider bot and translates provider status events into its own meeting lifecycle, including scheduled, joining, recording, completed, cancelled, and failed states.

### 4. Verified webhook ingestion

Recall.ai reports bot and recording changes to the public webhook endpoint. MeetingOS verifies the webhook signature before accepting the payload.

Each provider event is stored with its provider event ID before processing begins. The unique event record and job deduplication keys make webhook delivery idempotent: repeated delivery can be acknowledged without processing the same event twice.

The webhook responds after persisting and enqueueing the event. Downloading media, transcription, note generation, indexing, and email delivery happen later in the worker, keeping provider callbacks fast and recoverable.

### 5. Recording and participant ingestion

When Recall.ai reports that a recording is ready, the worker fetches the final recording metadata. It extracts participants, join and leave events, and the speaker diarization timeline, then links them to the internal meeting and recording.

The mixed audio file is downloaded from Recall.ai and uploaded to Cloudflare R2. Turso stores the recording metadata and object key; the audio bytes themselves remain in object storage. If audio has already been stored, the idempotent job can continue directly to transcription.

### 6. Transcription and speaker attribution

The worker retrieves the audio from R2 and sends it to Groq's speech-to-text API. The returned transcript includes timed segments.

MeetingOS combines those segments with Recall.ai's diarization timeline to replace generic speaker positions with the best available participant names. It stores both the full transcript and the timed, speaker-aware segments in Turso.

### 7. Structured meeting intelligence

Once transcription succeeds, note generation and vector indexing are queued independently.

Groq receives the transcript with a constrained structured-output schema. The resulting meeting record can contain:

- A concise Markdown summary
- Explicit decisions
- Main discussion topics
- Action items, including the named assignee and due date when they were actually stated

The prompt is designed to use only information present in the transcript. Missing or low-quality meeting content is represented explicitly rather than filled with invented details.

### 8. Search indexing

MeetingOS divides the transcript into useful chunks while preserving meeting, timing, and speaker metadata. Jina converts the chunks into retrieval embeddings, and Qdrant stores them in the transcript collection.

Indexing is derived work: Turso remains authoritative, while a failed or rebuilt vector index does not destroy the underlying transcript.

### 9. Grounded meeting chat

When a user asks a question, MeetingOS saves the message to a persistent thread and embeds the query with Jina. It searches transcript knowledge and previous chat exchanges in parallel, optionally limiting retrieval to selected meetings.

The combined candidates are reranked by Jina. Only the strongest excerpts are sent to Groq as context, together with recent messages from the current thread. The assistant is instructed to answer from those sources, cite the supporting source numbers, and say when the requested information is absent.

The response streams to the browser as it is generated. The completed answer, its source metadata, and the conversation title are stored in Turso. The question-and-answer pair is then indexed into the separate chat collection so later conversations can build on earlier work without mixing chat content into the transcript index.

### 10. Real-time UI updates

Worker milestones publish lightweight events through an in-process Tokio broadcast channel. The frontend listens through Server-Sent Events and invalidates the relevant TanStack Query entries when a meeting, note, audio asset, analytics result, or share delivery changes.

The browser then fetches current state from the API. The event stream is a refresh signal, not the source of truth.

### 11. Sharing and playback

Authenticated users can request a temporary, presigned R2 URL for meeting audio. This allows playback without making the storage bucket public.

Meeting sharing creates an expiring random token linked to the meeting. The public share view resolves that token and can present the summary, transcript, participants, and a temporary audio URL. When email sharing is requested, Resend sends the link and MeetingOS records each delivery attempt so already successful recipients are not sent the same share repeatedly.

## Data ownership and flow

MeetingOS deliberately gives each storage system one job:

- **Turso is authoritative.** It owns durable application state and the job queue.
- **R2 owns large audio objects.** Turso stores references to those objects.
- **Redis accelerates reads.** Its entries are invalidated after writes and can be recreated from Turso.
- **Qdrant accelerates semantic retrieval.** Its transcript and chat indexes are derived from durable source data.
- **Server-Sent Events carry ephemeral refresh signals.** Clients always return to the API for current state.

This separation keeps large media, transactional state, low-latency caching, and vector retrieval from competing inside one system.

## Reliability model

Meeting processing is a chain of small, idempotent jobs rather than one long request. The queue covers provider-event processing, recording discovery, audio storage, transcription, note generation, transcript indexing, calendar sync, scheduled bot creation, chat indexing, and share-email delivery.

Workers lease due jobs from Turso so only one worker owns a job at a time. Failed jobs are retried with a delay until their configured attempt limit is reached. Expired leases are requeued after a worker crash, and jobs that exhaust their attempts move to a dead state instead of retrying forever.

Long-running worker loops are supervised with bounded backoff. Separate maintenance loops recover stale leases, report stuck work, schedule upcoming bots, maintain calendar watches, and purge old dead jobs. The service also supports graceful shutdown and can run in API-only, worker-only, or combined mode.

Provider event IDs, meeting deduplication keys, recording asset state, chat persistence, and email-delivery records make repeated requests safe at the important external boundaries.

## Security and privacy boundaries

- Protected application routes validate Clerk JWTs and resolve access through MeetingOS user and workspace records.
- Meeting detail, audio, participant, chat, and mutation queries verify ownership instead of accepting arbitrary record access.
- Recall.ai webhook payloads are rejected unless their signatures verify.
- Audio remains in private object storage and is exposed through short-lived presigned URLs.
- Public meeting pages require an unexpired random share token.
- CORS origins are explicitly configured, and Caddy provides HTTPS for the containerized backend.
- Provider credentials and OAuth tokens are sensitive deployment data and must be supplied and stored outside client code.

There are two important current boundaries to understand. The Server-Sent Events endpoint uses a process-wide broadcast channel and is not tenant-scoped, so it must not carry sensitive content and should be hardened before an untrusted multi-tenant deployment. Google OAuth tokens are persisted in the primary database, so production database access, backups, credential rotation, and log redaction must be treated as security-critical operations.

## Deployment model

The frontend is an independent Next.js application and can be deployed separately from the API. Its backend origin is configured at deployment time, and its current Vercel configuration keeps automatic Git deployments disabled.

The Rust backend uses a multi-stage Docker build. Cargo Chef caches dependency compilation, the final binary runs as an unprivileged user in a minimal Debian image, and the container exposes a health endpoint.

Docker Compose runs the backend alongside Caddy. Caddy terminates TLS and forwards traffic to the healthy backend service. Tagged releases run formatting and compile checks, build a versioned container image, and publish it to Docker Hub. Runtime roles allow the API and worker to stay together for a small deployment or split into separate processes as load grows.

## Current integration scope

Google Calendar is the implemented calendar integration. Microsoft Graph routes currently exist as placeholders and are not yet connected to the processing pipeline.

Recall.ai is the call-capture boundary for Google Meet, Zoom, and Microsoft Teams. MeetingOS does not implement a conference client or record calls directly; it orchestrates the provider and owns everything that happens around and after capture.
