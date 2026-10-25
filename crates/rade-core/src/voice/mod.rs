//! Voice prompts (PRD R124, ADR-0046): GTK-free parts. `context` gathers what
//! the voice agent is told, `agent` builds its command and reads its stream,
//! `stt` builds the speech-to-text request, `words` marks changed words,
//! `composer` is the Voice Composer's state machine.

pub mod agent;
pub mod context;
pub mod stt;
pub mod words;
