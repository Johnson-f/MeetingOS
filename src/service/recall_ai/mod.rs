mod client;
mod groq;
mod types;
mod utils;

pub use client::RecallAiClient;
pub use groq::GroqClient;
#[allow(unused_imports)]
pub use types::{
    GeneratedActionItem, GeneratedNote, GroqSegment, GroqTranscriptionResponse,
    RecallCreateBotRequest, RecallCreatedBot, RecallRecordingMedia, SpeakerSpan,
};
#[allow(unused_imports)]
pub use utils::{
    ResolvedMeetingTiming, build_dedup_key, normalize_meeting_url, platform_from_url,
    resolve_meeting_timing,
};
