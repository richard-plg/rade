//! Notifications (PRD R3): storage with burst de-duplication, Unread counts
//! per Pane / Tab / Worktree / Source, what activating one does, and
//! whether it also goes to the desktop (`desktop_notify`). GTK-free; the
//! app renders the panel and talks to the notification daemon.

use crate::config::DesktopNotify;
use crate::model::State;
use crate::rpc::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// Unique, growing (state.json from M2 has none: repaired on load).
    #[serde(default)]
    pub id: u64,
    pub pane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub text: String,
    /// Unix seconds.
    pub time: u64,
    #[serde(default)]
    pub read: bool,
    /// "Source / Worktree / Tab / Pane" when it arrived, shown once the
    /// Pane is gone.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// What raised it, for coalescing one Agent event into one entry.
    #[serde(default, skip_serializing_if = "Origin::is_message")]
    pub source: Origin,
    /// RADE's own notice (PR / CI, R146.4): never coalesced with an Agent
    /// event, so it is never absorbed into one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub own: bool,
    /// What its Inbox row offers besides Focus (R146.4, R147.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<NoteAction>,
    /// The Worktree `(source id, worktree id)` the action is about when it
    /// is not the Pane's own: a set's Worktree with no Pane of its own
    /// puts its notice on the set's Agent (R151.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<(String, String)>,
}

/// A Notification's own button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteAction {
    /// Send the failing CI log of its Pane's Worktree to the Agent (R147.3).
    SendCiFailure,
}

impl NoteAction {
    pub fn label(self) -> &'static str {
        match self {
            NoteAction::SendCiFailure => "Send to Agent",
        }
    }
}

/// Where a Notification came from (R3.1 coalescing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// OSC 9 / 777 from the Terminal, or `rade notify`.
    #[default]
    Message,
    Bell,
    /// An Agent Status transition (hook or fallback, R10.3).
    Status,
    /// A Status plus the Message or Bell that reported the same event.
    Merged,
}

impl Origin {
    fn is_message(&self) -> bool {
        *self == Origin::Message
    }
}

/// Oldest Notifications are dropped beyond this.
pub const MAX_NOTIFICATIONS: usize = 500;
/// The same text on the same Pane within this many seconds is one entry.
pub const DEDUPE_SECS: u64 = 2;
/// A Status Notification and a Message/Bell on the same Pane within this
/// many seconds (either order) are one Agent event: one entry (R3.1).
pub const COALESCE_SECS: u64 = 5;

/// Unread Notifications of live Panes, rolled up; `total` counts all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnreadCounts {
    by_id: HashMap<String, u32>,
    pub total: u32,
}

impl UnreadCounts {
    fn get(&self, id: &str) -> u32 {
        self.by_id.get(id).copied().unwrap_or(0)
    }
    pub fn pane(&self, id: &str) -> u32 {
        self.get(id)
    }
    pub fn tab(&self, id: &str) -> u32 {
        self.get(id)
    }
    pub fn worktree(&self, id: &str) -> u32 {
        self.get(id)
    }
    /// Source ids are slugs, never `p_`/`t_`/`w_` ids, so one map holds all.
    pub fn source(&self, id: &str) -> u32 {
        self.get(id)
    }
}

impl State {
    /// Store a Notification for `pane` (R3.1): OSC 9 / 777 or `rade
    /// notify`. Returns its id; `None` when it repeats the Pane's last one
    /// (same title and text) within [`DEDUPE_SECS`] (that one's time is
    /// refreshed instead). Coalesces with an Agent Status Notification of the
    /// same Pane within [`COALESCE_SECS`], returning that entry's id.
    pub fn notify(
        &mut self,
        pane: &str,
        title: Option<String>,
        text: &str,
        now: u64,
    ) -> Option<u64> {
        self.post(pane, title, text, Origin::Message, now, false)
    }

    /// [`State::notify`] with a button on its Inbox row (R146.4: a failing
    /// PR's Send to Agent). A repeat within [`DEDUPE_SECS`] or an entry it
    /// coalesces into gets the action too.
    pub fn notify_action(
        &mut self,
        pane: &str,
        title: Option<String>,
        text: &str,
        now: u64,
        action: NoteAction,
        about: Option<(String, String)>,
    ) -> Option<u64> {
        // RADE's own notice: never folded into an Agent event (R146.4).
        let id = self.post(pane, title, text, Origin::Message, now, true);
        let target = id.or_else(|| {
            self.notifications
                .iter()
                .rev()
                .find(|n| n.pane == pane)
                .map(|n| n.id)
        });
        if let Some(n) = target.and_then(|t| self.notifications.iter_mut().find(|n| n.id == t)) {
            n.action = Some(action);
            n.target = about;
        }
        id
    }

    /// BEL (R3.1): "Bell", or nothing new when it reports an Agent event
    /// that already has an entry.
    pub fn notify_bell(&mut self, pane: &str, now: u64) -> Option<u64> {
        self.post(pane, None, "Bell", Origin::Bell, now, false)
    }

    /// RADE's own notice on `pane` (R146.4: checks failed, changes
    /// requested, CI still failing): de-duplicated, never coalesced.
    pub fn notify_own(
        &mut self,
        pane: &str,
        title: Option<String>,
        text: &str,
        now: u64,
    ) -> Option<u64> {
        self.post(pane, title, text, Origin::Message, now, true)
    }

    /// An Agent Status transition ("<name> finished", R10.3).
    pub(crate) fn notify_status(&mut self, pane: &str, text: &str, now: u64) -> Option<u64> {
        self.post(pane, None, text, Origin::Status, now, false)
    }

    fn post(
        &mut self,
        pane: &str,
        title: Option<String>,
        text: &str,
        source: Origin,
        now: u64,
        own: bool,
    ) -> Option<u64> {
        if let Some(last) = self.notifications.iter_mut().rev().find(|n| n.pane == pane) {
            let age = now.abs_diff(last.time);
            if last.text == text && last.title == title && age <= DEDUPE_SECS {
                last.time = now;
                return None;
            }
            // The window counts from the entry's own time, never slides.
            if !own
                && !last.own
                && age <= COALESCE_SECS
                && coalesce(last, title.as_deref(), text, source)
            {
                return Some(last.id);
            }
        }
        let id = self.notifications.last().map_or(0, |n| n.id) + 1;
        let path = self.pane_path(pane).unwrap_or_default();
        self.notifications.push(Notification {
            id,
            pane: pane.into(),
            title,
            text: text.into(),
            time: now,
            read: false,
            path,
            source,
            own,
            action: None,
            target: None,
        });
        let extra = self.notifications.len().saturating_sub(MAX_NOTIFICATIONS);
        self.notifications.drain(..extra);
        Some(id)
    }

    pub fn notification(&self, id: u64) -> Option<&Notification> {
        self.notifications.iter().find(|n| n.id == id)
    }

    pub fn unread(&self, pane: &str) -> u32 {
        self.notifications
            .iter()
            .filter(|n| n.pane == pane && !n.read)
            .count() as u32
    }

    /// Focusing a Pane reads its Notifications (R3.4).
    pub fn mark_read(&mut self, pane: &str) {
        for n in self.notifications.iter_mut().filter(|n| n.pane == pane) {
            n.read = true;
        }
    }

    pub fn mark_all_read(&mut self) {
        self.notifications.iter_mut().for_each(|n| n.read = true);
    }

    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
    }

    pub fn unread_counts(&self) -> UnreadCounts {
        let mut u = UnreadCounts::default();
        let mut per_pane: HashMap<&str, u32> = HashMap::new();
        for n in self.notifications.iter().filter(|n| !n.read) {
            *per_pane.entry(&n.pane).or_default() += 1;
            u.total += 1;
        }
        for (p, w) in self.worktrees() {
            for t in &w.tabs {
                for x in t.panes() {
                    let Some(&k) = per_pane.get(x.id.as_str()) else {
                        continue;
                    };
                    for id in [&x.id, &t.id, &w.id, &p.id] {
                        *u.by_id.entry(id.clone()).or_default() += k;
                    }
                }
            }
        }
        u
    }

    /// Click on a Notification (R3.3): focus its Pane (the `rade focus`
    /// path: shows its Source, Worktree, Tab, Zooms a Title Bar, reads it).
    /// A gone Pane's Notification is only marked read. Returns the Pane
    /// focused.
    pub fn activate_notification(&mut self, id: u64) -> Result<Option<String>> {
        let pane = self
            .notification(id)
            .ok_or_else(|| Error::not_found(format!("no notification {id}")))?
            .pane
            .clone();
        if self.pane_exists(&pane) {
            self.focus_pane(&pane)?;
            return Ok(Some(pane));
        }
        self.notifications
            .iter_mut()
            .filter(|n| n.id == id)
            .for_each(|n| n.read = true);
        Ok(None)
    }

    /// Newest unread Notification whose Pane still exists (Ctrl+Shift+G).
    pub fn newest_unread(&self) -> Option<u64> {
        self.notifications
            .iter()
            .rev()
            .find(|n| !n.read && self.pane_exists(&n.pane))
            .map(|n| n.id)
    }

    pub fn pane_exists(&self, pane: &str) -> bool {
        self.locate(pane).is_some()
    }

    /// "Source / Worktree / Tab / Pane" (Pane name, else id).
    pub fn pane_path(&self, pane: &str) -> Option<String> {
        let loc = self.locate(pane)?;
        let p = self.source(&loc.source)?;
        let w = p.worktree(&loc.worktree)?;
        let t = w.tab(&loc.tab)?;
        let x = t.pane(pane)?;
        let name = x.name.as_deref().unwrap_or(&x.id);
        Some(format!("{} / {} / {} / {name}", p.name, w.name, t.name))
    }

    /// Whether `pane` is drawn now: in the Tab on screen, not hidden by
    /// another Pane's Zoom, not collapsed to a Title Bar (R3.5 `smart`).
    pub fn pane_on_screen(&self, pane: &str) -> bool {
        let Some(tab) = self.active_tab() else {
            return false;
        };
        match &tab.zoomed {
            Some(z) => z == pane,
            None => tab.layout.pane(pane).is_some(),
        }
    }

    /// Give Notifications from before M3 (id 0) unique ids.
    pub(crate) fn repair_notification_ids(&mut self) {
        let n = &self.notifications;
        let ok = n.windows(2).all(|w| w[0].id < w[1].id) && n.first().is_none_or(|n| n.id > 0);
        if !ok {
            for (i, n) in self.notifications.iter_mut().enumerate() {
                n.id = i as u64 + 1;
            }
        }
    }
}

/// Fold a new Notification into `last` if they report one Agent event: a
/// Status and a Message/Bell, in either order, once per entry. The Status
/// text leads ("rev finished — PONG"); a Bell adds no text; a Message
/// arriving after the merge is absorbed. Keeps `last`'s read state; a
/// Status never merges into a read entry.
fn coalesce(last: &mut Notification, title: Option<&str>, text: &str, source: Origin) -> bool {
    let message = |title: Option<&str>, text: &str| match title {
        Some(t) => format!("{t}: {text}"),
        None => text.to_string(),
    };
    // A status after a message the user already read is news: own entry.
    if source == Origin::Status && last.read {
        return false;
    }
    let merged = match (last.source, source) {
        (Origin::Merged, Origin::Message | Origin::Bell) => return true,
        (Origin::Status, Origin::Bell) => last.text.clone(),
        (Origin::Status, Origin::Message) => {
            format!("{} — {}", last.text, message(title, text))
        }
        (Origin::Bell, Origin::Status) => text.to_string(),
        (Origin::Message, Origin::Status) => {
            format!("{text} — {}", message(last.title.as_deref(), &last.text))
        }
        _ => return false,
    };
    last.text = merged;
    last.title = None;
    last.source = Origin::Merged;
    true
}

/// Whether a new Notification also goes to the desktop (R3.5), and why.
/// `smart`: only when the RADE window is not active or the Pane is not on
/// screen ([`State::pane_on_screen`]).
pub fn desktop_decision(
    mode: DesktopNotify,
    window_active: bool,
    on_screen: bool,
) -> (bool, &'static str) {
    match mode {
        DesktopNotify::Off => (false, "desktop_notify = off"),
        DesktopNotify::Always => (true, "desktop_notify = always"),
        DesktopNotify::Smart if !on_screen => (true, "pane not visible"),
        DesktopNotify::Smart if !window_active => (true, "window not active"),
        DesktopNotify::Smart => (false, "pane visible in the active window"),
    }
}

/// "now", "5 min", "2 h", "3 d" — the panel's time column.
pub fn relative_time(then: u64, now: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{} min", s / 60),
        3600..86400 => format!("{} h", s / 3600),
        _ => format!("{} d", s / 86400),
    }
}

/// One call to `org.freedesktop.Notifications` that [`DesktopSlots`] wants made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopCall<C> {
    /// `Notify` with this `replaces_id` (0 = a new banner).
    Notify { replaces_id: u32, content: C },
    /// `CloseNotification` of the server's id.
    Close(u32),
}

enum Slot<C> {
    /// `Notify` sent, the server's id not known yet: a newer text waits here
    /// (only the latest is kept), a withdraw is remembered.
    Pending {
        queued: Option<C>,
        withdrawn: bool,
    },
    Shown(u32),
}

/// R3.1: one Agent event is one desktop banner, replaced in place, never sent
/// twice. A replace needs the server's id from the first `Notify` reply, so
/// while that reply is outstanding the newest text is queued and sent as the
/// replace when the reply arrives. Keys are Notification ids, `C` the content.
pub struct DesktopSlots<C> {
    slots: std::collections::HashMap<u64, Slot<C>>,
}

impl<C> Default for DesktopSlots<C> {
    fn default() -> Self {
        Self {
            slots: Default::default(),
        }
    }
}

impl<C> DesktopSlots<C> {
    /// Show `content` for `key`: a new banner, a replace, or queued.
    pub fn send(&mut self, key: u64, content: C) -> Option<DesktopCall<C>> {
        match self.slots.get_mut(&key) {
            None => {
                self.slots.insert(
                    key,
                    Slot::Pending {
                        queued: None,
                        withdrawn: false,
                    },
                );
                Some(DesktopCall::Notify {
                    replaces_id: 0,
                    content,
                })
            }
            Some(Slot::Pending { queued, .. }) => {
                *queued = Some(content);
                None
            }
            Some(Slot::Shown(id)) => Some(DesktopCall::Notify {
                replaces_id: *id,
                content,
            }),
        }
    }

    /// The server answered the first `Notify` with its id.
    pub fn replied(&mut self, key: u64, server_id: u32) -> Option<DesktopCall<C>> {
        let Some(Slot::Pending { queued, withdrawn }) = self.slots.remove(&key) else {
            return None;
        };
        if withdrawn {
            return Some(DesktopCall::Close(server_id));
        }
        self.slots.insert(key, Slot::Shown(server_id));
        queued.map(|content| DesktopCall::Notify {
            replaces_id: server_id,
            content,
        })
    }

    /// The first `Notify` failed: a queued text becomes a new first `Notify`.
    pub fn failed(&mut self, key: u64) -> Option<DesktopCall<C>> {
        match self.slots.remove(&key) {
            Some(Slot::Pending {
                queued: Some(content),
                withdrawn: false,
            }) => {
                self.slots.insert(
                    key,
                    Slot::Pending {
                        queued: None,
                        withdrawn: false,
                    },
                );
                Some(DesktopCall::Notify {
                    replaces_id: 0,
                    content,
                })
            }
            _ => None,
        }
    }

    /// Take the banner down (read in the app).
    pub fn withdraw(&mut self, key: u64) -> Option<DesktopCall<C>> {
        match self.slots.get_mut(&key) {
            Some(Slot::Pending { queued, withdrawn }) => {
                *queued = None;
                *withdrawn = true;
                None
            }
            Some(Slot::Shown(id)) => {
                let id = *id;
                self.slots.remove(&key);
                Some(DesktopCall::Close(id))
            }
            None => None,
        }
    }

    /// The Notification a server id belongs to (for the click actions).
    pub fn key_of(&self, server_id: u32) -> Option<u64> {
        self.slots.iter().find_map(|(k, s)| match s {
            Slot::Shown(id) if *id == server_id => Some(*k),
            _ => None,
        })
    }
}

#[cfg(test)]
#[path = "notify_tests.rs"]
mod tests;
