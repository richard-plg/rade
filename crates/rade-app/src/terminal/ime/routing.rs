// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae

//! Routing decisions for the per-pane IME — pure logic, no GTK or
//! ghostty FFI.
//!
//! Two GTK input-method contexts run side-by-side per pane: the
//! `IMMulticontext` (primary, owns ibus / fcitx5 / wayland slaves)
//! and a `GtkIMContextSimple` (fallback, drives libxkbcommon's
//! compose tables in-process). For every keypress we have to decide
//! which one sees the event first, and how the "compose is in
//! flight" latch transitions afterwards.
//!
//! The bug this exists to avoid: on Plasma 6 Wayland without
//! ibus/fcitx5, GTK's `wayland` slave claims AZERTY dead keys over
//! text-input-v3 (`filter_keypress` returns `true` and a preedit
//! fires with the bare dead-key glyph), but KWin never commits the
//! composed glyph. If we route a dead-key initiator through the
//! multicontext first, the fallback never sees it and the compose
//! silently aborts. So we bypass the multicontext for compose
//! initiators and for the follow-up keystrokes that complete the
//! sequence.

use gtk::glib::translate::IntoGlib;
use gtk4 as gtk;

/// X11 keysym for the Compose / `Multi_key` initiator.
const MULTI_KEY_KEYSYM: u32 = 0xFF20;

/// Returns true for keysyms that initiate a compose sequence — X11
/// dead keys (`XK_dead_*`, 0xFE50–0xFEFF) and the Compose key
/// (`XK_Multi_key`, 0xFF20).
pub fn is_compose_initiator(keyval: gtk::gdk::Key) -> bool {
    let raw: u32 = keyval.into_glib();
    matches!(raw, 0xFE50..=0xFEFF | MULTI_KEY_KEYSYM)
}

/// Which IM context should see a keypress first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposeRouting {
    /// Run the simple-compose fallback before the multicontext.
    /// Used when the keysym opens a compose sequence (dead key,
    /// Multi_key) or when one is already in flight — running the
    /// multicontext for these on Wayland would let KWin grab the
    /// event over text-input-v3 and swallow the compose.
    FallbackFirst,
    /// Run the multicontext (ibus / fcitx5 / wayland slave) first.
    /// The fallback only sees the event as a safety net if the
    /// multicontext doesn't claim it.
    PrimaryFirst,
}

/// Decide which context sees a keypress first given the current
/// compose latch state and the incoming keysym. Pure function; the
/// caller applies the result by calling `filter_keypress` on the
/// chosen context.
pub fn decide_routing(fallback_composing: bool, is_initiator: bool) -> ComposeRouting {
    if fallback_composing || is_initiator {
        ComposeRouting::FallbackFirst
    } else {
        ComposeRouting::PrimaryFirst
    }
}

/// What to do with the `fallback_composing` latch after the fallback
/// context has filtered a keypress that we routed to it first.
///
/// * `Some(true)`  — the fallback just consumed a compose initiator,
///   so the next keypress must also be routed through it.
/// * `Some(false)` — the fallback declined the event; the compose
///   (if any) is abandoned and routing returns to the multicontext.
/// * `None`        — fallback consumed a non-initiator; the result
///   (compose completed or still pending) is communicated through
///   the fallback's `commit` signal, which clears the latch from
///   there.
pub fn update_latch_after_fallback_first(
    is_initiator: bool,
    fallback_handled: bool,
) -> Option<bool> {
    match (is_initiator, fallback_handled) {
        (true, true) => Some(true),
        (_, false) => Some(false),
        (false, true) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_initiator_detection_covers_dead_keys_and_multi_key() {
        // A few representative dead keys plus Multi_key.
        assert!(is_compose_initiator(gtk::gdk::Key::dead_grave));
        assert!(is_compose_initiator(gtk::gdk::Key::dead_acute));
        assert!(is_compose_initiator(gtk::gdk::Key::dead_circumflex));
        assert!(is_compose_initiator(gtk::gdk::Key::dead_tilde));
        assert!(is_compose_initiator(gtk::gdk::Key::dead_diaeresis));
        assert!(is_compose_initiator(gtk::gdk::Key::Multi_key));

        // Plain printable + control keys must not be misclassified.
        assert!(!is_compose_initiator(gtk::gdk::Key::a));
        assert!(!is_compose_initiator(gtk::gdk::Key::e));
        assert!(!is_compose_initiator(gtk::gdk::Key::space));
        assert!(!is_compose_initiator(gtk::gdk::Key::Return));
        assert!(!is_compose_initiator(gtk::gdk::Key::BackSpace));
    }

    // -----------------------------------------------------------------
    // Routing-decision regression tests.
    //
    // The bug fixed here was that GTK4's IMMulticontext on Plasma 6
    // Wayland (without ibus / fcitx5) claims dead-key events over
    // text-input-v3 without ever delivering a commit, masking the
    // compose. These tests pin the routing rules so a refactor can't
    // silently reintroduce that behavior.
    // -----------------------------------------------------------------

    #[test]
    fn routing_sends_compose_initiator_to_fallback_first() {
        assert_eq!(
            decide_routing(false, true),
            ComposeRouting::FallbackFirst,
            "dead-key / Multi_key must bypass the multicontext so KWin's \
             text-input-v3 cannot grab and swallow it"
        );
    }

    #[test]
    fn routing_stays_on_fallback_while_compose_pending() {
        assert_eq!(
            decide_routing(true, false),
            ComposeRouting::FallbackFirst,
            "the compose follow-up keystroke (e.g. `e` after `^`) is not \
             a compose initiator, but it must still go through the \
             fallback so libxkbcommon's compose tables can complete the \
             sequence"
        );
    }

    #[test]
    fn routing_uses_multicontext_first_for_plain_keys() {
        assert_eq!(
            decide_routing(false, false),
            ComposeRouting::PrimaryFirst,
            "ASCII typing and modifier shortcuts must hit the multicontext \
             first so ibus / fcitx5 / CJK IMEs keep working — only when \
             the multicontext declines does the simple-compose fallback \
             see the key"
        );
    }

    #[test]
    fn routing_does_not_resurrect_compose_after_completion() {
        // After commit, fallback_composing is cleared (by the commit
        // signal). The next plain key must return to multicontext-first.
        assert_eq!(decide_routing(false, false), ComposeRouting::PrimaryFirst);
    }

    #[test]
    fn latch_arms_when_initiator_is_consumed() {
        assert_eq!(
            update_latch_after_fallback_first(true, true),
            Some(true),
            "after the fallback consumes a dead-key press, the latch \
             must arm so the follow-up keystroke is also routed through \
             the fallback"
        );
    }

    #[test]
    fn latch_disarms_when_fallback_declines() {
        // Initiator declined — e.g. an obscure dead key with no compose
        // entry in libxkbcommon's tables. We must fall back to the
        // multicontext for this key and for the next one.
        assert_eq!(update_latch_after_fallback_first(true, false), Some(false));
        // Follow-up key declined — compose abandoned.
        assert_eq!(update_latch_after_fallback_first(false, false), Some(false));
    }

    #[test]
    fn latch_left_alone_on_successful_follow_up() {
        // The fallback consumed a non-initiator; whether it completed
        // the compose or kept it open is signalled via its `commit`
        // signal, which clears the latch from elsewhere. The routing
        // helper must not touch the latch here.
        assert_eq!(update_latch_after_fallback_first(false, true), None);
    }
}
