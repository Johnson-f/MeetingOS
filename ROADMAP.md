# Meeting bot — roadmap

## Phase 1 — Core pipeline (MVP)
> Get the basic record → transcribe → summarize loop working end to end

- [ ] Init Rust/Axum backend project
- [ ] Set up Turso with LibSQL + migrations
- [ ] `POST /meetings` — accept meeting link, call Recall.ai to send bot
- [ ] `POST /webhook` — receive Recall.ai callback when meeting ends
- [ ] Download mp3 from Recall recording URL
- [ ] Send mp3 to Groq Whisper large-v3 for transcription
- [ ] Send transcript to Claude for summary + action item extraction
- [ ] Save transcript, summary, action items to DB
- [ ] `GET /notes/:id` — return meeting notes to frontend
- [ ] Init Next.js frontend with shadcn/ui
- [ ] Basic auth (Clerk or Supabase)
- [ ] Paste meeting link form → triggers bot
- [ ] Meeting detail page — show summary + action items

---

## Phase 2 — Playback & transcript view
> Let users actually read and listen to their meetings

- [ ] Serve mp3 from backend (or store in S3/Cloudflare R2)
- [ ] Audio player on meeting detail page (play/pause, scrub)
- [ ] Full transcript view with speaker labels + timestamps
- [ ] Click timestamp in transcript → seek audio to that point
- [ ] Summary tab / transcript tab switcher
- [ ] Search across transcripts

---

## Phase 3 — Dashboard & meeting management
> Give users visibility into all their meetings

- [ ] Dashboard with stats (total meetings, hours recorded, this week)
- [ ] Upcoming meetings list
- [ ] Past meetings list with status badges (pending, recording, done, failed)
- [ ] Meeting status polling (show "bot is recording..." live)
- [ ] Cancel bot before/during meeting
- [ ] Delete meeting + recording

---

## Phase 4 — Integrations
> Let users push notes to their tools automatically

- [ ] Integrations settings page
- [ ] Notion — push summary + action items as a new page
- [ ] Confluence — push notes to a chosen space
- [ ] Slack — post summary to a chosen channel
- [ ] Linear/Jira — create tickets from action items
- [ ] Per-meeting toggle (enable/disable integration per meeting)
- [ ] Global defaults (always push to Notion unless disabled)

---

## Phase 5 — Share with participants
> Send transcript and summary to everyone on the call

- [ ] Recall.ai returns participant emails — store them
- [ ] Per-meeting participant list
- [ ] Select which participants to send to
- [ ] Email template — summary + action items + link to full transcript
- [ ] Send via Resend or Postmark
- [ ] Option to auto-send after every meeting

---

## Phase 6 — Calendar integration
> Remove the manual "paste a link" step entirely

- [ ] Google Calendar OAuth
- [ ] Outlook Calendar OAuth
- [ ] Pull upcoming meetings automatically
- [ ] Auto-schedule bot for detected meetings
- [ ] User preference — auto-join all / ask each time / blocklist certain meetings
- [ ] Show calendar view in dashboard

---

## Phase 7 — Polish & scale
> Make it production-ready

- [ ] ngrok → deploy backend to Fly.io
- [ ] Deploy frontend to Vercel
- [ ] Environment config (dev / staging / prod)
- [ ] Error handling — failed transcription, bot rejected from meeting
- [ ] Retry logic for Groq + Claude failures
- [ ] Audio chunking for meetings > 25MB
- [ ] Usage limits per user (free tier vs paid)
- [ ] Billing with Stripe
- [ ] Rate limiting on API routes
- [ ] Webhook signature verification (Recall.ai HMAC)
- [ ] Mobile responsive frontend