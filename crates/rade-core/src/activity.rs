//! Activity (PRD R107.2): what happened since RADE started — an Agent
//! waiting or done, a PR opened, a commit, an account limit event (R110)
//! — newest first, at most [`CAP`], in memory only. Nothing is ever
//! written to disk.
//! The Inspector's Activity tab and Project home's Recent activity read it.

use serde::Serialize;
use std::collections::VecDeque;

/// How many events are kept.
pub const CAP: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Waiting,
    Done,
    PrOpened,
    Commit,
    Limit,
    /// An Update (R127): available, installed, failed.
    Update,
    /// A hook report dropped because another Agent sent it (R128).
    Ignored,
}

/// One event. A row reads `<actor> <text>` (`tests asked which
/// fixtures`) with its age on the right.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Event {
    pub kind: Kind,
    /// Unix seconds.
    pub at: u64,
    pub actor: String,
    pub text: String,
    /// The Project it belongs to (Recent activity filters on it).
    pub project: Option<String>,
    /// The Pane a click opens.
    pub pane: Option<String>,
}

/// `Which fixtures?` → `which fixtures`: the question as running text.
fn running(msg: &str) -> String {
    let m = msg.trim().trim_end_matches(['?', '.', '!']).trim();
    let mut c = m.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

impl Event {
    fn new(kind: Kind, at: u64, actor: &str, text: String) -> Event {
        Event {
            kind,
            at,
            actor: actor.into(),
            text,
            project: None,
            pane: None,
        }
    }

    /// `tests asked which fixtures`; `tests needs input` without a message.
    pub fn waiting(agent: &str, message: Option<&str>, at: u64) -> Event {
        let text = match message.map(running).filter(|m| !m.is_empty()) {
            Some(m) => format!("asked {m}"),
            None => "needs input".into(),
        };
        Event::new(Kind::Waiting, at, agent, text)
    }

    /// `planner finished · 24 tests passed`; `planner finished` alone.
    pub fn done(agent: &str, message: Option<&str>, at: u64) -> Event {
        let text = match message.map(str::trim).filter(|m| !m.is_empty()) {
            Some(m) => format!("finished · {m}"),
            None => "finished".into(),
        };
        Event::new(Kind::Done, at, agent, text)
    }

    /// `#482 opened from perf-img`.
    pub fn pr(number: u64, worktree: &str, at: u64) -> Event {
        Event::new(
            Kind::PrOpened,
            at,
            &format!("#{number}"),
            format!("opened from {worktree}"),
        )
    }

    /// `shop-web committed 1a2b3c4 Split the form`.
    pub fn commit(source: &str, short: &str, subject: &str, at: u64) -> Event {
        Event::new(
            Kind::Commit,
            at,
            source,
            format!("committed {short} {}", subject.trim()),
        )
    }

    /// An account limit event (R110): `acme-work five_hour passed 80 %`.
    /// `RADE 1.2.0 is available` (R127).
    pub fn update(text: &str, at: u64) -> Event {
        Event::new(Kind::Update, at, "RADE", text.into())
    }

    /// `b1-k2 ignored a codex session (started by its Agent)` (R128).
    pub fn ignored(pane: &str, agent: &str, at: u64) -> Event {
        Event::new(
            Kind::Ignored,
            at,
            pane,
            format!("ignored a {agent} session (started by its Agent)"),
        )
    }

    pub fn limit(account: &str, text: &str, at: u64) -> Event {
        Event::new(Kind::Limit, at, account, text.into())
    }

    /// A full limit line of [`crate::usage::limits::limit_events`]
    /// (`account acme: Spark 5-hour over 80%`): the row reads exactly it,
    /// `account acme:` as the actor.
    pub fn limit_line(line: &str, at: u64) -> Event {
        match line.split_once(": ") {
            Some((actor, text)) => Event::new(Kind::Limit, at, &format!("{actor}:"), text.into()),
            None => Event::new(Kind::Limit, at, "account", line.into()),
        }
    }

    pub fn in_project(mut self, project: Option<String>) -> Event {
        self.project = project;
        self
    }

    pub fn on_pane(mut self, pane: Option<String>) -> Event {
        self.pane = pane;
        self
    }

    /// The row text without the age.
    pub fn line(&self) -> String {
        format!("{} {}", self.actor, self.text)
    }
}

/// The in-memory list, newest first.
#[derive(Clone, Debug, Default)]
pub struct Activity {
    events: VecDeque<Event>,
}

impl Activity {
    pub fn push(&mut self, e: Event) {
        self.events.push_front(e);
        self.events.truncate(CAP);
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Newest first.
    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events.iter()
    }

    /// A Project's newest `n` events (Recent activity, R105.4).
    pub fn of_project(&self, project: &str, n: usize) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| e.project.as_deref() == Some(project))
            .take(n)
            .collect()
    }
}

/// An event's age (spec B.35): `now` under a minute, else `1m`, `2h`, `3d`.
pub fn ago(secs: u64) -> String {
    if secs < 60 {
        "now".into()
    } else {
        crate::design::age(secs)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn limit_line_reads_as_written() {
        let e = Event::limit_line("account acme: Spark 5-hour over 80%", 7);
        assert_eq!(e.line(), "account acme: Spark 5-hour over 80%");
        assert_eq!((e.kind, e.actor.as_str()), (Kind::Limit, "account acme:"));
        assert_eq!(e.text, "Spark 5-hour over 80%");
    }

    use super::*;

    #[test]
    fn r127_update_event_is_rade_s() {
        let e = Event::update("1.2.0 is available", 9);
        assert_eq!((e.kind, e.actor.as_str(), e.at), (Kind::Update, "RADE", 9));
        assert_eq!(e.line(), "RADE 1.2.0 is available");
        assert_eq!(serde_json::to_string(&e.kind).unwrap(), "\"update\"");
    }

    #[test]
    fn r128_ignored_event_names_the_pane() {
        let e = Event::ignored("b1-k2", "codex", 3);
        assert_eq!(e.kind, Kind::Ignored);
        assert_eq!(
            e.line(),
            "b1-k2 ignored a codex session (started by its Agent)"
        );
        assert_eq!(serde_json::to_string(&e.kind).unwrap(), "\"ignored\"");
    }

    #[test]
    fn r107_c_activity_keeps_newest_500_in_memory_only() {
        let dir = std::env::temp_dir().join(format!("rade-activity-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".rade")).unwrap();
        let mut a = Activity::default();
        assert!(a.is_empty());
        for i in 0..501u64 {
            let e = match i % 5 {
                0 => Event::waiting("tests", Some("which fixtures?"), i),
                1 => Event::done("planner", None, i),
                2 => Event::pr(i, "perf-img", i),
                3 => Event::commit("shop-web", "1a2b3c4", "Split", i),
                _ => Event::limit("acme-work", "five_hour passed 80 %", i),
            };
            a.push(e.in_project(Some("shop".into())));
        }
        assert_eq!(a.len(), CAP);
        let ats: Vec<u64> = a.iter().map(|e| e.at).collect();
        assert_eq!(ats.first(), Some(&500));
        assert_eq!(ats.last(), Some(&1), "the oldest (0) dropped");
        assert!(ats.windows(2).all(|w| w[0] > w[1]), "newest first");
        let kinds: std::collections::BTreeSet<String> =
            a.iter().map(|e| format!("{:?}", e.kind)).collect();
        assert_eq!(kinds.len(), 5);
        // A restart is a new list: empty, and nothing was written anywhere.
        let restarted = Activity::default();
        assert!(restarted.is_empty());
        assert_eq!(std::fs::read_dir(dir.join(".rade")).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn activity_rows_text_and_age() {
        let mut a = Activity::default();
        a.push(Event::pr(482, "perf-img", 100).in_project(Some("shop".into())));
        a.push(
            Event::done("planner", Some("24 tests passed"), 7100).in_project(Some("shop".into())),
        );
        a.push(Event::done("ui", None, 7150).in_project(Some("billing".into())));
        a.push(
            Event::waiting("tests", Some("Which fixtures?"), 7190).in_project(Some("shop".into())),
        );
        let now = 7200;
        let rows: Vec<(String, String)> = a
            .of_project("shop", 3)
            .iter()
            .map(|e| (e.line(), ago(now - e.at)))
            .collect();
        assert_eq!(
            rows,
            [
                ("tests asked which fixtures".to_string(), "now".to_string()),
                ("planner finished · 24 tests passed".into(), "1m".into()),
                ("#482 opened from perf-img".into(), "1h".into()),
            ]
        );
        assert!(a.of_project("docs", 3).is_empty());
        assert_eq!(Event::waiting("tests", None, 0).line(), "tests needs input");
        assert_eq!(Event::done("ui", Some(" "), 0).line(), "ui finished");
        assert_eq!(ago(7200), "2h");
    }
}
