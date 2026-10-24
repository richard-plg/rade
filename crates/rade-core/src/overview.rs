//! Agent Overview (R117): every Agent of every Workspace in bands, most
//! urgent first — Needs you (waiting → error → interrupted, newest first
//! within each), Finished, not seen, Running, Quiet — or in one column per
//! Workspace; the filter, the frozen order while open, keyboard steps and
//! the hold-to-flip gesture. GTK-free: the app adds the screen facts (last
//! change, tail lines, git) and draws it.

use std::collections::{BTreeMap, HashSet};

use crate::design::age;
use crate::layout::Pane;
use crate::model::State;
use crate::palette::fold;
use crate::AgentStatus;

/// A running Agent whose screen has not changed for this long is `quiet`.
pub const QUIET_SECS: u64 = 600;
/// Running shows this many cards before `+n more ▾` (R117.7).
pub const RUNNING_SHOWN: usize = 8;

/// What the Overview knows about one Agent Pane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fact {
    pub pane: String,
    pub name: String,
    /// Agent Preset (`claude`, `codex`).
    pub agent: String,
    pub model: Option<String>,
    /// `None`: no status reported yet (shown as idle, in Quiet).
    pub status: Option<AgentStatus>,
    /// When `status` last changed (Unix seconds).
    pub since: u64,
    /// It has an Unread Notification.
    pub unread: bool,
    /// The text of its newest Notification.
    pub note: Option<String>,
    pub workspace: String,
    pub workspace_name: String,
    /// `Project / link / Worktree`, or `Scratch / <folder>`.
    pub place: String,
    /// The Parent's name, for a Subagent.
    pub parent: Option<String>,
    /// The Fan-out it belongs to.
    pub fanout: Option<String>,
    pub account: Option<String>,
    /// When its screen last changed (Unix seconds; the app's idle watcher).
    pub changed_at: Option<u64>,
    /// `↑3 · 4 changed`.
    pub git: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Band {
    NeedsYou,
    Finished,
    Running,
    Quiet,
}

impl Band {
    pub const ALL: [Band; 4] = [Band::NeedsYou, Band::Finished, Band::Running, Band::Quiet];

    pub fn label(self) -> &'static str {
        match self {
            Band::NeedsYou => "Needs you",
            Band::Finished => "Finished, not seen",
            Band::Running => "Running",
            Band::Quiet => "Quiet",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Band::NeedsYou => "needs_you",
            Band::Finished => "finished",
            Band::Running => "running",
            Band::Quiet => "quiet",
        }
    }

    pub fn parse(s: &str) -> Option<Band> {
        Band::ALL.into_iter().find(|b| b.id() == s)
    }

    /// How many cards show before the fold (`None`: never folds). Quiet
    /// folds only past the room the app says is left (`quiet_cap`).
    fn shown(self, q: &Query) -> Option<usize> {
        match self {
            Band::Running => Some(RUNNING_SHOWN),
            Band::Quiet => q.quiet_cap,
            _ => None,
        }
    }
}

pub fn band_of(f: &Fact) -> Band {
    use AgentStatus::*;
    match f.status {
        Some(Waiting | Error | Interrupted) => Band::NeedsYou,
        Some(Done) if f.unread => Band::Finished,
        Some(Running) => Band::Running,
        _ => Band::Quiet,
    }
}

/// Order inside Needs you: waiting, error, interrupted.
fn need_rank(s: Option<AgentStatus>) -> u8 {
    match s {
        Some(AgentStatus::Waiting) => 0,
        Some(AgentStatus::Error) => 1,
        _ => 2,
    }
}

/// The status word (`needs input`, `failed`, …; `idle` without a status).
pub fn word(s: Option<AgentStatus>) -> &'static str {
    s.map_or("idle", |s| s.words())
}

/// One card: an Agent, or the Agents of one Fan-out.
#[derive(Clone, Debug, PartialEq)]
pub enum Card {
    Agent(Box<Fact>),
    Fanout { id: String, members: Vec<Fact> },
}

impl Card {
    /// Stable key: the Pane id, or `fanout:<id>`.
    pub fn key(&self) -> String {
        match self {
            Card::Agent(f) => f.pane.clone(),
            Card::Fanout { id, .. } => format!("fanout:{id}"),
        }
    }

    pub fn facts(&self) -> Vec<&Fact> {
        match self {
            Card::Agent(f) => vec![f],
            Card::Fanout { members, .. } => members.iter().collect(),
        }
    }

    /// A Fan-out sits in its most urgent member's band.
    pub fn band(&self) -> Band {
        self.facts()
            .into_iter()
            .map(band_of)
            .min()
            .unwrap_or(Band::Quiet)
    }

    fn since(&self) -> u64 {
        self.facts().iter().map(|f| f.since).max().unwrap_or(0)
    }

    fn need_rank(&self) -> u8 {
        self.facts()
            .iter()
            .map(|f| need_rank(f.status))
            .min()
            .unwrap_or(2)
    }

    pub fn workspace(&self) -> &str {
        &self.facts()[0].workspace
    }

    /// The Pane a jump goes to: the Agent's, or a Fan-out's first member's
    /// (its Fan-out Tab shows them all).
    pub fn pane(&self) -> &str {
        &self.facts()[0].pane
    }

    pub fn panes(&self) -> Vec<&str> {
        self.facts().into_iter().map(|f| f.pane.as_str()).collect()
    }

    /// `needs input · 4m`, `running · quiet 14m`; a Fan-out's counts
    /// (`2 running · 1 done`).
    pub fn status_text(&self, now: u64) -> String {
        match self {
            Card::Agent(f) => {
                if f.status == Some(AgentStatus::Running) {
                    if let Some(t) = f.changed_at {
                        let still = now.saturating_sub(t);
                        if still >= QUIET_SECS {
                            return format!("running · quiet {}", age(still));
                        }
                    }
                }
                // No status, or no time for it (a restored Agent): no age.
                if f.status.is_none() || f.since == 0 {
                    return word(f.status).into();
                }
                format!("{} · {}", word(f.status), age(now.saturating_sub(f.since)))
            }
            Card::Fanout { members, .. } => {
                let mut n: BTreeMap<(u8, &str), usize> = BTreeMap::new();
                for m in members {
                    *n.entry((rank(m.status), word(m.status))).or_default() += 1;
                }
                n.into_iter()
                    .map(|((_, w), c)| format!("{c} {w}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            }
        }
    }

    fn matches(&self, words: &[String]) -> bool {
        let hay = match self {
            Card::Agent(f) => hay(f),
            Card::Fanout { id, members } => {
                let mut h = fold(&id.to_lowercase());
                for m in members {
                    h.push(' ');
                    h.push_str(&hay(m));
                }
                h
            }
        };
        words.iter().all(|w| hay.contains(w.as_str()))
    }
}

/// Worst first, as R10.3 orders them (for a Fan-out's counts).
fn rank(s: Option<AgentStatus>) -> u8 {
    use AgentStatus::*;
    match s {
        Some(Waiting) => 0,
        Some(Error) => 1,
        Some(Interrupted) => 2,
        Some(Running) => 3,
        Some(Done) => 4,
        Some(Exited) => 5,
        None => 6,
    }
}

fn hay(f: &Fact) -> String {
    let parts = [
        f.name.as_str(),
        &f.agent,
        f.model.as_deref().unwrap_or(""),
        &f.workspace_name,
        &f.place,
        word(f.status),
        f.status.map_or("", |s| s.as_str()),
        f.note.as_deref().unwrap_or(""),
        f.account.as_deref().unwrap_or(""),
        f.fanout.as_deref().unwrap_or(""),
    ];
    fold(&parts.join(" ").to_lowercase())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Group {
    #[default]
    Status,
    Workspace,
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Group::Status => "status",
            Group::Workspace => "workspace",
        }
    }
}

/// What to show: the filter text, one Workspace, one band (a Statusline
/// count or the switcher's `!`), the unfolded bands.
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: String,
    pub workspace: Option<String>,
    pub band: Option<Band>,
    pub group: Group,
    pub unfolded: HashSet<Band>,
    /// Quiet cards that fit under the other bands (whole rows, at least
    /// one); `None` = room for all of them (R117.2).
    pub quiet_cap: Option<usize>,
}

impl Section {
    /// Its cards, shown and folded: the count by its title.
    pub fn total(&self) -> usize {
        self.cards.len() + self.more
    }
}

/// A band (by status) or a Workspace column.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub band: Option<Band>,
    pub workspace: Option<String>,
    pub title: String,
    /// Cards shown, in order.
    pub cards: Vec<Card>,
    /// Cards folded away (`+n more ▾`).
    pub more: usize,
}

/// Agent counts per status for the header chips.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counts {
    pub waiting: usize,
    pub error: usize,
    pub interrupted: usize,
    pub finished: usize,
    pub running: usize,
    pub quiet: usize,
}

impl Counts {
    /// `2 need input`, `1 failed`, …; zero counts left out.
    pub fn chips(&self) -> Vec<(Band, &'static str, String)> {
        [
            (Band::NeedsYou, "wait", self.waiting, "need input"),
            (Band::NeedsYou, "err", self.error, "failed"),
            (Band::NeedsYou, "intr", self.interrupted, "interrupted"),
            (Band::Finished, "done", self.finished, "finished, unseen"),
            (Band::Running, "run", self.running, "running"),
            (Band::Quiet, "idle", self.quiet, "quiet"),
        ]
        .into_iter()
        .filter(|(.., n, _)| *n > 0)
        .map(|(b, led, n, w)| (b, led, format!("{n} {w}")))
        .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overview {
    pub sections: Vec<Section>,
    /// Agents in every Workspace (before the filter).
    pub total: usize,
    /// Workspaces with at least one Agent.
    pub workspaces: usize,
    pub counts: Counts,
    /// Agents per Workspace id (before the filter).
    pub per_workspace: BTreeMap<String, usize>,
    /// The order cards were placed in when it opened (R117.2: frozen).
    order: Vec<(usize, String)>,
}

/// Cards from facts: one per Agent, one per Fan-out.
fn cards(facts: &[Fact]) -> Vec<Card> {
    let mut out: Vec<Card> = Vec::new();
    for f in facts {
        match &f.fanout {
            Some(id) => match out
                .iter_mut()
                .find(|c| matches!(c, Card::Fanout { id: i, .. } if i == id))
            {
                Some(Card::Fanout { members, .. }) => members.push(f.clone()),
                _ => out.push(Card::Fanout {
                    id: id.clone(),
                    members: vec![f.clone()],
                }),
            },
            None => out.push(Card::Agent(Box::new(f.clone()))),
        }
    }
    out
}

fn sort_band(cards: &mut [Card]) {
    cards.sort_by(|a, b| {
        a.band()
            .cmp(&b.band())
            .then_with(|| match a.band() {
                Band::NeedsYou => a.need_rank().cmp(&b.need_rank()),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| b.since().cmp(&a.since()))
            .then_with(|| a.key().cmp(&b.key()))
    });
}

impl Overview {
    /// Build it fresh (on open). `workspaces` = (id, name) in switcher
    /// order, for the by-Workspace columns.
    pub fn build(facts: &[Fact], q: &Query, workspaces: &[(String, String)]) -> Overview {
        let mut all = cards(facts);
        sort_band(&mut all);
        let mut ov = Overview::header(facts);
        ov.order = all.iter().enumerate().map(|(i, c)| (i, c.key())).collect();
        ov.sections = Self::sections(all, q, workspaces);
        ov
    }

    /// The same Overview with new facts (statuses update in place, R117.2):
    /// cards keep the order they had when it opened; a new Agent goes
    /// after them in its band; a gone one drops out.
    pub fn refresh(&self, facts: &[Fact], q: &Query, workspaces: &[(String, String)]) -> Overview {
        let mut all = cards(facts);
        let pos = |c: &Card| {
            self.order
                .iter()
                .find(|(_, k)| *k == c.key())
                .map(|(i, _)| *i)
        };
        let known: Vec<Card> = {
            let mut k: Vec<(usize, Card)> = all
                .iter()
                .filter_map(|c| pos(c).map(|i| (i, c.clone())))
                .collect();
            k.sort_by_key(|(i, _)| *i);
            k.into_iter().map(|(_, c)| c).collect()
        };
        all.retain(|c| pos(c).is_none());
        sort_band(&mut all);
        let mut ov = Overview::header(facts);
        let mut order = self.order.clone();
        let start = order.len();
        order.extend(all.iter().enumerate().map(|(i, c)| (start + i, c.key())));
        ov.order = order;
        // Frozen: a known card keeps the band it was placed in.
        let band_at: BTreeMap<String, Band> = self
            .sections
            .iter()
            .flat_map(|s| s.cards.iter().map(move |c| (c.key(), s.band)))
            .filter_map(|(k, b)| b.map(|b| (k, b)))
            .collect();
        ov.sections = Self::sections_frozen(known, all, q, workspaces, &band_at);
        ov
    }

    fn header(facts: &[Fact]) -> Overview {
        let mut c = Counts::default();
        let mut per: BTreeMap<String, usize> = BTreeMap::new();
        for f in facts {
            *per.entry(f.workspace.clone()).or_default() += 1;
            match (band_of(f), f.status) {
                (Band::NeedsYou, Some(AgentStatus::Waiting)) => c.waiting += 1,
                (Band::NeedsYou, Some(AgentStatus::Error)) => c.error += 1,
                (Band::NeedsYou, _) => c.interrupted += 1,
                (Band::Finished, _) => c.finished += 1,
                (Band::Running, _) => c.running += 1,
                (Band::Quiet, _) => c.quiet += 1,
            }
        }
        Overview {
            total: facts.len(),
            workspaces: per.len(),
            per_workspace: per,
            counts: c,
            ..Default::default()
        }
    }

    fn keep(c: &Card, q: &Query, words: &[String]) -> bool {
        q.workspace.as_deref().is_none_or(|w| c.workspace() == w)
            && q.band.is_none_or(|b| c.band() == b)
            && c.matches(words)
    }

    fn words(q: &Query) -> Vec<String> {
        fold(&q.text.to_lowercase())
            .split_whitespace()
            .map(String::from)
            .collect()
    }

    fn sections(all: Vec<Card>, q: &Query, workspaces: &[(String, String)]) -> Vec<Section> {
        let words = Self::words(q);
        let all: Vec<Card> = all
            .into_iter()
            .filter(|c| Self::keep(c, q, &words))
            .collect();
        match q.group {
            Group::Status => Band::ALL
                .into_iter()
                .filter_map(|b| {
                    let cards: Vec<Card> = all.iter().filter(|c| c.band() == b).cloned().collect();
                    Self::band_section(b, cards, q)
                })
                .collect(),
            Group::Workspace => workspaces
                .iter()
                .filter_map(|(id, name)| {
                    let cards: Vec<Card> = all
                        .iter()
                        .filter(|c| c.workspace() == id)
                        .cloned()
                        .collect();
                    (!cards.is_empty()).then(|| Section {
                        band: None,
                        workspace: Some(id.clone()),
                        title: name.clone(),
                        cards,
                        more: 0,
                    })
                })
                .collect(),
        }
    }

    fn band_section(b: Band, mut cards: Vec<Card>, q: &Query) -> Option<Section> {
        if cards.is_empty() {
            return None;
        }
        let mut more = 0;
        if let Some(n) = b.shown(q).filter(|_| !q.unfolded.contains(&b)) {
            if cards.len() > n {
                more = cards.len() - n;
                cards.truncate(n);
            }
        }
        Some(Section {
            band: Some(b),
            workspace: None,
            title: b.label().into(),
            cards,
            more,
        })
    }

    fn sections_frozen(
        known: Vec<Card>,
        new: Vec<Card>,
        q: &Query,
        workspaces: &[(String, String)],
        band_at: &BTreeMap<String, Band>,
    ) -> Vec<Section> {
        let words = Self::words(q);
        match q.group {
            Group::Workspace => {
                let mut all = known;
                all.extend(new);
                Self::sections_by_ws(all, q, &words, workspaces)
            }
            Group::Status => Band::ALL
                .into_iter()
                .filter_map(|b| {
                    let mut cards: Vec<Card> = known
                        .iter()
                        .filter(|c| band_at.get(&c.key()).copied().unwrap_or(c.band()) == b)
                        .filter(|c| Self::keep_text(c, q, &words))
                        .cloned()
                        .collect();
                    cards.extend(
                        new.iter()
                            .filter(|c| c.band() == b && Self::keep(c, q, &words))
                            .cloned(),
                    );
                    if q.band.is_some_and(|x| x != b) {
                        return None;
                    }
                    Self::band_section(b, cards, q)
                })
                .collect(),
        }
    }

    /// The filter without the band test (a frozen card stays in its band).
    fn keep_text(c: &Card, q: &Query, words: &[String]) -> bool {
        q.workspace.as_deref().is_none_or(|w| c.workspace() == w) && c.matches(words)
    }

    fn sections_by_ws(
        all: Vec<Card>,
        q: &Query,
        words: &[String],
        workspaces: &[(String, String)],
    ) -> Vec<Section> {
        workspaces
            .iter()
            .filter_map(|(id, name)| {
                let cards: Vec<Card> = all
                    .iter()
                    .filter(|c| c.workspace() == id && Self::keep(c, q, words))
                    .cloned()
                    .collect();
                (!cards.is_empty()).then(|| Section {
                    band: None,
                    workspace: Some(id.clone()),
                    title: name.clone(),
                    cards,
                    more: 0,
                })
            })
            .collect()
    }

    /// Re-group or re-filter the cards placed at open, keeping their order.
    pub fn requery(&self, facts: &[Fact], q: &Query, workspaces: &[(String, String)]) -> Overview {
        self.refresh(facts, q, workspaces)
    }

    pub fn shown(&self) -> usize {
        self.sections.iter().map(|s| s.cards.len()).sum()
    }

    pub fn card(&self, at: Pos) -> Option<&Card> {
        self.sections.get(at.0)?.cards.get(at.1)
    }

    /// Where a Pane's card is.
    pub fn find(&self, pane: &str) -> Option<Pos> {
        self.sections.iter().enumerate().find_map(|(i, s)| {
            s.cards
                .iter()
                .position(|c| c.panes().contains(&pane))
                .map(|j| (i, j))
        })
    }

    /// The one line shown instead of cards (R117.7), if any.
    pub fn empty_line(&self, q: &Query) -> Option<String> {
        if self.total == 0 {
            Some("No Agents running · Ctrl+Shift+P > Spawn Agent".into())
        } else if self.shown() == 0 {
            Some(format!(
                "No Agent matches \"{}\" · Esc clears",
                q.text.trim()
            ))
        } else {
            None
        }
    }
}

/// (section, card) index.
pub type Pos = (usize, usize);

/// The card it opens on: the first Needs-you card, else the Agent focused
/// before, else the first (R117.4).
pub fn initial(ov: &Overview, previous: Option<&str>) -> Option<Pos> {
    for (i, s) in ov.sections.iter().enumerate() {
        if let Some(j) = s.cards.iter().position(|c| c.band() == Band::NeedsYou) {
            return Some((i, j));
        }
    }
    previous
        .and_then(|p| ov.find(p))
        .or_else(|| (ov.shown() > 0).then(|| first(ov)))
}

fn first(ov: &Overview) -> Pos {
    (
        ov.sections
            .iter()
            .position(|s| !s.cards.is_empty())
            .unwrap_or(0),
        0,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    NextSection,
    PrevSection,
    /// The next card in reading order (a flip press).
    Next,
}

/// One keyboard step. By status the sections are bands of a `cols`-wide
/// grid (arrows wrap across bands); by Workspace they are columns.
pub fn step(ov: &Overview, cols: usize, group: Group, at: Pos, m: Move) -> Pos {
    let n = ov.sections.len();
    if n == 0 || ov.shown() == 0 {
        return at;
    }
    let len = |i: usize| ov.sections[i].cards.len();
    let cols = cols.max(1);
    let (s, c) = at;
    let flat_next = |s: usize, c: usize| -> Pos {
        if c + 1 < len(s) {
            (s, c + 1)
        } else {
            let mut t = (s + 1) % n;
            while len(t) == 0 {
                t = (t + 1) % n;
            }
            (t, 0)
        }
    };
    let flat_prev = |s: usize, c: usize| -> Pos {
        if c > 0 {
            (s, c - 1)
        } else {
            let mut t = (s + n - 1) % n;
            while len(t) == 0 {
                t = (t + n - 1) % n;
            }
            (t, len(t) - 1)
        }
    };
    let next_sec = |s: usize| (s + 1) % n;
    let prev_sec = |s: usize| (s + n - 1) % n;
    match (group, m) {
        (_, Move::Next) => flat_next(s, c),
        (_, Move::NextSection) => (next_sec(s), 0),
        (_, Move::PrevSection) => (prev_sec(s), 0),
        (Group::Status, Move::Right) => flat_next(s, c),
        (Group::Status, Move::Left) => flat_prev(s, c),
        (Group::Status, Move::Down) => {
            if c + cols < len(s) {
                (s, c + cols)
            } else if c / cols < (len(s) - 1) / cols {
                (s, len(s) - 1)
            } else {
                let t = next_sec(s);
                (t, (c % cols).min(len(t) - 1))
            }
        }
        (Group::Status, Move::Up) => {
            if c >= cols {
                (s, c - cols)
            } else {
                let t = prev_sec(s);
                let last_row = (len(t) - 1) / cols * cols;
                (t, (last_row + c % cols).min(len(t) - 1))
            }
        }
        (Group::Workspace, Move::Down) => ((s), (c + 1) % len(s)),
        (Group::Workspace, Move::Up) => (s, (c + len(s) - 1) % len(s)),
        (Group::Workspace, Move::Right) => {
            let t = next_sec(s);
            (t, c.min(len(t) - 1))
        }
        (Group::Workspace, Move::Left) => {
            let t = prev_sec(s);
            (t, c.min(len(t) - 1))
        }
    }
}

/// Hold `Ctrl+Shift`, tap `Space` (R117.4): the presses since it opened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flip {
    pub presses: u32,
    /// The modifiers were released once: no more flip.
    pub over: bool,
}

impl Flip {
    pub fn opened() -> Flip {
        Flip {
            presses: 1,
            over: false,
        }
    }

    /// Another `Space` while held: `true` = move to the next card.
    pub fn press(&mut self) -> bool {
        if self.over {
            return false;
        }
        self.presses += 1;
        true
    }

    /// `Ctrl` released: `true` = jump to the selection now.
    pub fn release(&mut self) -> bool {
        if self.over {
            return false;
        }
        self.over = true;
        self.presses >= 2
    }
}

/// The last `n` lines of an Agent's screen that are its output (R117.3).
/// The input box and footer an agent CLI keeps at the bottom are cut off
/// first (`output`). Blank lines are skipped; lines are right-trimmed,
/// tabs become 4 spaces, at most 120 chars.
pub fn tail_of(screen: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = output(screen)
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.replace('\t', "    ").chars().take(120).collect())
        .collect();
    let k = out.len();
    out.drain(..k.saturating_sub(n));
    out
}

/// A Preview's body (R117.9): the last `n` lines of the output as on
/// screen — a run of blank lines kept as one (paragraphs), none at either
/// end; tabs become 4 spaces, at most 240 chars (the view ellipsizes).
pub fn preview_of(screen: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in output(screen) {
        let blank = l.trim().is_empty();
        if blank && out.last().is_none_or(|p: &String| p.is_empty()) {
            continue;
        }
        out.push(if blank {
            String::new()
        } else {
            l.replace('\t', "    ").chars().take(240).collect()
        });
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    let k = out.len();
    out.drain(..k.saturating_sub(n));
    while out.first().is_some_and(String::is_empty) {
        out.remove(0);
    }
    out
}

/// A screen's lines above the input box and footer an agent CLI keeps at
/// the bottom — Claude Code's `────` rules around its `❯` box and the
/// status lines under it, Codex's `›` prompt line and the footer under it
/// — or a card would show `? for shortcuts` instead of the answer.
/// Right-trimmed.
fn output(screen: &str) -> Vec<&str> {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    // The lowest chrome line near the bottom: an older `› hello` of the
    // user's higher up in the history must not cut the answer below it.
    let near = lines.len().saturating_sub(12);
    let cut = match (near..lines.len()).rev().find(|&i| chrome(lines[i])) {
        // The bottom rule of an input box: cut from the box's top rule.
        Some(m) if rule(lines[m]) => (m.saturating_sub(10)..m)
            .rev()
            .find(|&i| rule(lines[i]))
            .unwrap_or(m),
        Some(m) => m,
        None => lines.len(),
    };
    // Right-aligned notices just over the box (`Not logged in · Run
    // /login`) belong to it.
    let mut cut = cut;
    if cut < lines.len() {
        while cut > 0 && (lines[cut - 1].trim().is_empty() || notice(lines[cut - 1])) {
            cut -= 1;
        }
    }
    lines[..cut].to_vec()
}

/// Lines of screen a Preview shows at most (R117.9).
pub const PREVIEW_LINES: usize = 24;
/// Lines of each Fan-out member a Fan-out card's Preview shows.
pub const PREVIEW_MEMBER_LINES: usize = 6;
/// Space between a card and its Preview (px).
pub const PREVIEW_GAP: i32 = 12;
/// The narrowest a Preview beside its card may get (px).
pub const PREVIEW_MIN_W: i32 = 300;

/// A rectangle in the Overview's coordinates (px).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn right(self) -> i32 {
        self.x + self.w
    }

    fn bottom(self) -> i32 {
        self.y + self.h
    }
}

/// Where a Preview sits against its card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
    Below,
    Above,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Right => "right",
            Side::Left => "left",
            Side::Below => "below",
            Side::Above => "above",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub rect: Rect,
    pub side: Side,
}

/// A Preview's width for a window `window` px wide: min(600, 46 %).
pub fn preview_width(window: i32) -> i32 {
    (window * 46 / 100).min(600)
}

/// Where a card's Preview goes (R117.9): beside the card, top-aligned with
/// it — right when it fits, else left —; when neither side fits, below the
/// card's row, else above it; when its content fits neither, beside after
/// all on the roomier side, narrower (at least `PREVIEW_MIN_W`); else the
/// larger of below / above, shortened. Always inside `body`, never over
/// its own card; it may cover other cards. `size` = the width and the
/// height its content wants.
pub fn preview_place(card: Rect, body: Rect, size: (i32, i32)) -> Placement {
    let w = size.0.min(body.w).max(0);
    let clamp = |v: i32, lo: i32, hi: i32| v.min(hi).max(lo);
    let beside = |x: i32, w: i32, side: Side| {
        let h = size.1.min(body.h).max(0);
        Placement {
            rect: Rect {
                x,
                y: clamp(card.y, body.y, body.bottom() - h),
                w,
                h,
            },
            side,
        }
    };
    let right = body.right() - (card.right() + PREVIEW_GAP);
    let left = card.x - PREVIEW_GAP - body.x;
    if w <= right {
        return beside(card.right() + PREVIEW_GAP, w, Side::Right);
    }
    if w <= left {
        return beside(card.x - PREVIEW_GAP - w, w, Side::Left);
    }
    let below = body.bottom() - (card.bottom() + PREVIEW_GAP);
    let above = card.y - PREVIEW_GAP - body.y;
    if size.1 > below && size.1 > above && right.max(left) >= PREVIEW_MIN_W {
        return if right >= left {
            beside(card.right() + PREVIEW_GAP, right, Side::Right)
        } else {
            beside(body.x, left, Side::Left)
        };
    }
    let side = if size.1 <= below || (size.1 > above && below >= above) {
        Side::Below
    } else {
        Side::Above
    };
    let room = if side == Side::Below { below } else { above };
    let h = size.1.min(room).max(0);
    let y = match side {
        Side::Below => card.bottom() + PREVIEW_GAP,
        _ => card.y - PREVIEW_GAP - h,
    };
    Placement {
        rect: Rect {
            x: clamp(card.x, body.x, body.right() - w),
            y,
            w,
            h,
        },
        side,
    }
}

/// A line an agent CLI draws around its input: a rule or a prompt line.
fn chrome(line: &str) -> bool {
    let t = line.trim_start();
    rule(line) || t == "❯" || t == "›" || t.starts_with("❯ ") || t.starts_with("› ")
}

/// A line pushed far right (20+ leading spaces): a CLI's status notice.
fn notice(line: &str) -> bool {
    line.len() - line.trim_start().len() >= 20 && !line.trim().is_empty()
}

/// A horizontal rule (`────`, a box edge `╭───╮`): 20+ chars, ≥ 90 % of
/// them box-drawing.
fn rule(line: &str) -> bool {
    let t = line.trim();
    let all = t.chars().count();
    let drawn = t.chars().filter(|c| "─━═╭╮╰╯┌┐└┘".contains(*c)).count();
    all >= 20 && drawn * 10 >= all * 9
}

/// Facts the model holds about every Agent Pane of every Workspace.
/// `place` gives a Pane's `Project / link / Worktree` (the app knows the
/// links); the app fills `changed_at` and `git` itself.
pub fn facts(ws: &State, place: &dyn Fn(&str) -> String) -> Vec<Fact> {
    let looks = ws.workspace_looks();
    let ws_name = |id: &str| {
        looks
            .iter()
            .find(|l| l.id == id)
            .map_or_else(|| id.to_string(), |l| l.name.clone())
    };
    let mut out = Vec::new();
    for (src, wt) in ws.worktrees() {
        for tab in &wt.tabs {
            for p in tab.panes() {
                // A Preset, or the agent found running in it (R115.4).
                let Some(agent) = p.agent_name().map(String::from) else {
                    continue;
                };
                out.push(Fact {
                    pane: p.id.clone(),
                    name: name_of(p),
                    agent,
                    model: crate::chrome::pane_model(p),
                    status: p.status,
                    since: p.status_since.unwrap_or(0),
                    unread: ws.unread(&p.id) > 0,
                    note: ws
                        .notifications
                        .iter()
                        .rev()
                        .find(|n| n.pane == p.id)
                        .map(|n| n.text.clone()),
                    workspace: src.workspace.clone(),
                    workspace_name: ws_name(&src.workspace),
                    place: place(&p.id),
                    parent: p.parent.as_deref().and_then(|id| ws.pane(id)).map(name_of),
                    fanout: ws
                        .fanouts
                        .iter()
                        .find(|f| f.members.iter().any(|m| m.pane == p.id))
                        .map(|f| f.id.clone()),
                    account: p.account.clone(),
                    changed_at: None,
                    git: None,
                });
            }
        }
    }
    out
}

/// As the Navigator's Agent row: its name, else its Preset.
fn name_of(p: &Pane) -> String {
    p.name
        .clone()
        .or_else(|| p.agent_name().map(String::from))
        .unwrap_or_else(|| "Terminal".into())
}

#[cfg(test)]
#[path = "overview_tests.rs"]
mod tests;
