//! Voice prompts, the controller (PRD R124, PLAN-voice.md T7): the recorder,
//! speech-to-text requests and the voice agent process, driven through
//! `rade_core::voice::composer`. Stub: the interface the view builds on.
#![allow(dead_code)]

use rade_core::voice::composer::Event;

/// An event from the Voice Composer's widget (keys inside the card, the
/// close, Send and Paste only buttons, edits of the draft).
pub fn on_ui(_event: Event) {}
