//! The Voice Composer widget (PRD R124, PLAN-voice.md T8): the card over an
//! Agent Pane, drawn only from `rade_core::voice::composer::ComposerView`.
//! Stub: the interface the controller builds on.
#![allow(dead_code)]

use rade_core::voice::composer::ComposerView;
use std::ops::Range;

/// Byte ranges of the draft text that get a `code`, `changed` or `unsure`
/// tag.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftTags {
    pub code: Vec<Range<usize>>,
    pub changed: Vec<Range<usize>>,
    pub unsure: Vec<Range<usize>>,
}

/// Show the composer on the Pane `pane_id`.
pub fn open(_pane_id: &str) {}

/// Remove the composer.
pub fn close() {}

/// Draw `view`.
pub fn render(_view: &ComposerView) {}

/// The newest meter levels, 0..1, oldest first.
pub fn set_levels(_levels: &[f32]) {}

/// Streamed draft text after what is shown.
pub fn append_draft(_text: &str) {}

/// Replace the draft text and its tags.
pub fn set_draft(_text: &str, _tags: &DraftTags) {}

/// The draft box's text, edits included.
pub fn draft_text() -> String {
    String::new()
}
