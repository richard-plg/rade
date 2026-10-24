//! Terminal titles (OSC 0/2) → Tab labels and Pane captions (R1, see
//! docs/internals/checks/M12-titles.md): sanitizing what programs send, which title
//! a Tab shows, and how often the UI takes a new one.

use crate::{Pane, Tab};
use std::time::{Duration, Instant};

/// Longest title kept (a program can send kilobytes).
const KEEP: usize = 256;
/// Longest title shown in a Tab label or caption; the tooltip has it all.
pub const SHOWN: usize = 60;
/// At most one UI update per Pane per this (≤ 4/s): spinners animate at ~10/s.
pub const MIN_INTERVAL: Duration = Duration::from_millis(250);

/// Leading glyphs Agents animate or use as status: Claude Code's ✳ and
/// ◐◑◒◓, Codex's braille spinner ⠋⠙⠹…, bullets, stars, emoji.
fn is_status_glyph(c: char) -> bool {
    matches!(c, '·' | '•' | '*' | '\u{2190}'..='\u{2BFF}' | '\u{1F000}'..='\u{1FAFF}')
        || c.is_whitespace()
        || c.is_control()
}

/// What a raw OSC title becomes: status glyphs stripped from the front,
/// whitespace (and control characters) collapsed to single spaces, at most
/// [`KEEP`] characters. `None` = nothing left (ignored: keep the old title).
pub fn sanitize(raw: &str) -> Option<String> {
    let s = raw.trim_start_matches(is_status_glyph);
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    let s: String = words.join(" ").chars().take(KEEP).collect();
    (!s.is_empty()).then_some(s)
}

/// `s` cut to `max` characters, the last one an ellipsis.
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.truncate(out.trim_end().len());
    out + "…"
}

/// How long to wait before showing a Pane's new title, given when it last
/// changed the UI: zero if [`MIN_INTERVAL`] has passed.
pub fn delay(last: Option<Instant>, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |l| {
        MIN_INTERVAL.saturating_sub(now.saturating_duration_since(l))
    })
}

impl Tab {
    /// The title the Tab follows: its Agent Grid Parent's, else its focused
    /// Pane's, else the first Pane's that has one (a Viewer opened beside an
    /// Agent has none, and shouldn't blank the label).
    pub fn title(&self) -> Option<&str> {
        let panes = self.panes();
        let of = |id: &str| {
            panes
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| p.title.as_deref())
        };
        if let Some(g) = &self.grid {
            if let Some(t) = of(g) {
                return Some(t);
            }
        }
        of(&self.focused).or_else(|| panes.iter().find_map(|p| p.title.as_deref()))
    }

    /// The Tab's label: the user's name if they renamed it (or a Fan-out's),
    /// else its title (shortened; a path shows its last folder), else its
    /// Agent Grid Parent's or first Agent's name, else `Review` for a Tab
    /// holding only a Review, else its default `Tab <n>`; with two or more
    /// Agents, their count (`Refunds · 4`). Never a raw path (R1.8, M14).
    pub fn label(&self) -> String {
        let panes = self.panes();
        let agents: Vec<_> = panes.iter().filter(|p| p.agent.is_some()).collect();
        let lead = self
            .grid
            .as_deref()
            .and_then(|g| agents.iter().find(|p| p.id == g))
            .or(agents.first())
            .and_then(|p| p.name.clone());
        let base = match self.title() {
            _ if self.renamed => self.name.clone(),
            Some(t) => ellipsize(short_title(t), SHOWN),
            None => lead.unwrap_or_else(|| {
                if !panes.is_empty() && panes.iter().all(|p| p.is_review()) {
                    "Review".into()
                } else {
                    self.name.clone()
                }
            }),
        };
        match agents.len() {
            0 | 1 => base,
            n => format!("{base} · {n}"),
        }
    }
}

/// A title that is a path (`/a/b`, `~/a`, `user@host:~/a`; no spaces)
/// shortened to its last folder; any other title as it is.
pub fn short_title(t: &str) -> &str {
    if t.contains(char::is_whitespace) {
        return t;
    }
    let path = match t.split_once(':') {
        Some((who, p)) if who.contains('@') => p,
        _ => t,
    };
    if !(path.starts_with('/') || path.starts_with('~')) {
        return t;
    }
    match path.trim_end_matches('/').rsplit('/').next() {
        Some("") | None => &path[..1],
        Some(last) => last,
    }
}

impl Pane {
    /// The title shown in a caption, shortened (a path to its last folder).
    pub fn shown_title(&self) -> Option<String> {
        self.title
            .as_deref()
            .map(|t| ellipsize(short_title(t), SHOWN))
    }
    /// The Agent's session title for its Navigator row (R104.7): the title
    /// it set (Claude Code / Codex name the session), without Codex's
    /// ` | <folder>` suffix; none while it is a product name, a shell's
    /// `user@host:path`, a path, or the Preset's name.
    pub fn session_title(&self) -> Option<String> {
        let t = self.title.as_deref()?.trim();
        let folder = self.cwd().file_name().and_then(|f| f.to_str());
        let t = match t.rsplit_once(" | ") {
            Some((head, tail)) if Some(tail) == folder => head.trim(),
            _ => t,
        };
        let generic = [
            "claude code",
            "claude",
            "codex",
            "openai codex",
            "antigravity",
            "agy",
            "opencode",
        ];
        let lower = t.to_lowercase();
        let shell = t
            .split_once(':')
            .is_some_and(|(who, _)| who.contains('@') && !who.contains(' '));
        let path = t.starts_with('/') || t.starts_with('~');
        let preset = self.agent_name().is_some_and(|a| a.eq_ignore_ascii_case(t));
        (!t.is_empty() && !generic.contains(&lower.as_str()) && !shell && !path && !preset)
            .then(|| ellipsize(t, SHOWN))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layout, State};
    use std::path::Path;

    #[test]
    fn r104_p_session_title_drops_generic_shell_and_folder_suffix() {
        let mut p = Pane::new("p_1".into(), "/w/rade".into(), None, None);
        p.agent = Some("codex".into());
        let t = |p: &mut Pane, t: &str| {
            p.title = Some(t.into());
            p.session_title()
        };
        assert_eq!(
            t(&mut p, "Đánh giá UI/UX | rade").as_deref(),
            Some("Đánh giá UI/UX")
        );
        assert_eq!(
            t(&mut p, "Fix login | other").as_deref(),
            Some("Fix login | other")
        );
        assert_eq!(
            t(&mut p, "Icon agent theo handoff").as_deref(),
            Some("Icon agent theo handoff")
        );
        for generic in [
            "Claude Code",
            "Codex",
            "codex",
            "richard@host:~/w/rade",
            "~/w/rade",
            "/w/rade",
        ] {
            assert_eq!(t(&mut p, generic), None, "{generic}");
        }
        p.title = None;
        assert_eq!(p.session_title(), None);
    }

    #[test]
    fn sanitize_strips_agent_status_glyphs() {
        // Measured: Claude Code idle / working, Codex spinner, zsh.
        assert_eq!(sanitize("✳ Claude Code").as_deref(), Some("Claude Code"));
        assert_eq!(
            sanitize("◐ Just the word hi").as_deref(),
            Some("Just the word hi")
        );
        assert_eq!(sanitize("⠋ RADE").as_deref(), Some("RADE"));
        for g in ["✢", "✶", "✻", "✽", "·", "⠹", "◑", "•", "*", "🤖"] {
            assert_eq!(
                sanitize(&format!("{g} Refactor auth")).as_deref(),
                Some("Refactor auth")
            );
        }
        assert_eq!(
            sanitize("richard@host:~/code").as_deref(),
            Some("richard@host:~/code")
        );
        // Only leading glyphs go; paths and inner symbols stay.
        assert_eq!(sanitize("/tmp/x → y ✳").as_deref(), Some("/tmp/x → y ✳"));
    }

    #[test]
    fn sanitize_collapses_whitespace_and_ignores_empty() {
        assert_eq!(sanitize("  a \t b\n\x07c  ").as_deref(), Some("a b c"));
        assert_eq!(sanitize(""), None);
        assert_eq!(sanitize("✳ "), None);
        assert_eq!(sanitize("⠋"), None);
        assert_eq!(sanitize(&"x".repeat(1000)).unwrap().len(), KEEP);
    }

    #[test]
    fn ellipsize_caps_with_ellipsis() {
        assert_eq!(ellipsize("short", 60), "short");
        let long = "word ".repeat(20);
        let e = ellipsize(&long, 60);
        assert_eq!(e.chars().count(), 60);
        assert!(e.ends_with("word…"), "{e}");
        // No space before the ellipsis.
        assert_eq!(ellipsize("abc def", 5), "abc…");
        assert_eq!(ellipsize("ééééé", 3), "éé…");
    }

    #[test]
    fn delay_throttles_to_four_per_second() {
        let t0 = Instant::now();
        assert_eq!(delay(None, t0), Duration::ZERO);
        assert_eq!(delay(Some(t0), t0), MIN_INTERVAL);
        assert_eq!(
            delay(Some(t0), t0 + Duration::from_millis(100)),
            Duration::from_millis(150)
        );
        assert_eq!(delay(Some(t0), t0 + Duration::from_secs(1)), Duration::ZERO);
    }

    fn tab_of(w: &State, pane: &str) -> crate::Tab {
        w.tab(&w.locate(pane).unwrap()).unwrap().clone()
    }

    fn first_tab(w: &mut State) -> (String, String) {
        let (p, wt) = w.active();
        let (p, wt) = (p.id.clone(), wt.id.clone());
        w.new_tab(&p, &wt, None, None).unwrap()
    }

    #[test]
    fn label_follows_title_unless_renamed() {
        let mut w = State::new(Path::new("/home/u"));
        let (tab, a) = first_tab(&mut w);
        assert_eq!(tab_of(&w, &a).label(), "Tab 1");
        assert!(w.set_pane_title(&a, "Refactor auth"));
        assert!(!w.set_pane_title(&a, "Refactor auth"), "unchanged");
        assert_eq!(tab_of(&w, &a).label(), "Refactor auth");
        w.set_pane_title(&a, &"long ".repeat(30));
        let l = tab_of(&w, &a).label();
        assert!(l.chars().count() <= SHOWN && l.ends_with('…'), "{l}");
        // Renamed: the name sticks through new titles.
        w.rename_tab(&tab, "auth").unwrap();
        w.set_pane_title(&a, "Something else");
        assert_eq!(tab_of(&w, &a).label(), "auth");
        assert_eq!(tab_of(&w, &a).title(), Some("Something else"));
        // Clearing the name: back to following the title.
        w.rename_tab(&tab, "  ").unwrap();
        assert_eq!(tab_of(&w, &a).label(), "Something else");
        assert_eq!(tab_of(&w, &a).name, "Tab 1");
    }

    #[test]
    fn paths_show_their_last_folder() {
        assert_eq!(short_title("~"), "~");
        assert_eq!(short_title("/"), "/");
        assert_eq!(short_title("~/code/foo"), "foo");
        assert_eq!(short_title("/tmp/x/feat-refunds/"), "feat-refunds");
        assert_eq!(short_title("richard@host:~/code"), "code");
        assert_eq!(short_title("richard@host:~"), "~");
        assert_eq!(short_title("Refactor auth"), "Refactor auth");
        assert_eq!(short_title("/tmp/x → y ✳"), "/tmp/x → y ✳");
        assert_eq!(short_title("a@b"), "a@b");
    }

    #[test]
    fn label_names_agents_counts_them_and_review() {
        let mut w = State::new(Path::new("/home/u"));
        let (tab, a) = first_tab(&mut w);
        w.set_pane_title(&a, "/tmp/rade/data/worktrees/payments-api/feat-refunds");
        assert_eq!(tab_of(&w, &a).label(), "feat-refunds");
        // No title: the lead Agent's name; two Agents: their count.
        let mut t = tab_of(&w, &a);
        let mut lead = crate::Pane::new("p_7".into(), "/".into(), None, Some("lead".into()));
        lead.agent = Some("claude".into());
        let mut kid = crate::Pane::new("p_8".into(), "/".into(), None, Some("kid".into()));
        kid.agent = Some("codex".into());
        t.layout = Layout::Split {
            dir: crate::SplitDir::Right,
            ratio: 0.5,
            a: Box::new(Layout::leaf(lead)),
            b: Box::new(Layout::leaf(kid)),
        };
        t.focused = "p_8".into();
        assert_eq!(t.label(), "lead · 2");
        t.grid = Some("p_8".into());
        assert_eq!(t.label(), "kid · 2");
        t.renamed = true;
        t.name = "Refunds".into();
        assert_eq!(t.label(), "Refunds · 2");
        // A Tab of only a Review.
        let r = crate::Pane::review("p_9".into(), "/".into());
        t.layout = Layout::leaf(r);
        t.renamed = false;
        t.grid = None;
        assert_eq!(t.label(), "Review");
        let _ = tab;
    }

    #[test]
    fn label_prefers_grid_parent_then_focused_then_any() {
        let mut w = State::new(Path::new("/home/u"));
        let (_, a) = first_tab(&mut w);
        let b = w.split(&a, crate::SplitDir::Right, None, None).unwrap();
        w.set_pane_title(&a, "left");
        w.set_pane_title(&b, "right");
        w.focus_pane(&b).unwrap();
        assert_eq!(tab_of(&w, &a).label(), "right");
        w.focus_pane(&a).unwrap();
        assert_eq!(tab_of(&w, &a).label(), "left");
        // The focused Pane has no title: another Pane's.
        let c = w.split(&a, crate::SplitDir::Down, None, None).unwrap();
        w.focus_pane(&c).unwrap();
        assert_eq!(tab_of(&w, &a).label(), "left");
        // An Agent Grid Parent drives the Tab whatever is focused.
        let mut t = tab_of(&w, &a);
        t.grid = Some(b.clone());
        assert_eq!(t.label(), "right");
        // A Pane without a title anywhere: the default name.
        t.layout = Layout::leaf(crate::Pane::new("p_9".into(), "/".into(), None, None));
        t.grid = None;
        t.focused = "p_9".into();
        assert_eq!(t.label(), "Tab 1");
    }
}
