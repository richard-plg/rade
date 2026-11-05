//! The pure rules behind R142 (ADR-0052): the UI thread only builds widgets.
//! Everything that reads git, `/proc`, config files or a Terminal's text runs
//! off it or is cached, by the rules here:
//!
//! - [`Reader`]: one background read in flight per kind, a result for a key
//!   that is no longer wanted is dropped (R142.2, R142-c, R142-d).
//! - [`StampCache`]: a value kept while the files it was read from have the
//!   same modification time and size (layered config R142-e, a Worktree's
//!   branch, a Source's main branch).
//! - [`Tail`]: a pattern wait reads only the Terminal rows added since its
//!   last look (R142.3, R142-f).
//! - [`Settle`]: the "type when settled" timer, with a limit (R140.5).

use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// One read in flight, stale results dropped
// ---------------------------------------------------------------------------

/// The bookkeeping of one kind of background read (git facts, ports, …):
/// at most one in flight, and a finished read applies only if its key is
/// still the wanted one.
#[derive(Debug)]
pub struct Reader<K> {
    in_flight: Option<K>,
    started: u32,
}

impl<K> Default for Reader<K> {
    fn default() -> Self {
        Self {
            in_flight: None,
            started: 0,
        }
    }
}

/// What to do with a finished read.
#[derive(Debug, PartialEq)]
pub struct Done<K> {
    /// The result is for the key still wanted: show it.
    pub apply: bool,
    /// A read for the wanted key was started in its place (the caller
    /// spawns it); `None` when the result was the wanted one.
    pub next: Option<K>,
}

impl<K: PartialEq + Clone> Reader<K> {
    /// A read for `key` is wanted: `true` = start it now (and call
    /// [`Reader::done`] when it ends); `false` = one is already in flight,
    /// start nothing (its `done` starts the wanted key if it changed).
    pub fn request(&mut self, key: &K) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        self.in_flight = Some(key.clone());
        self.started += 1;
        true
    }

    /// The read of `read` ended and `wanted` is what is wanted now.
    pub fn done(&mut self, read: &K, wanted: &K) -> Done<K> {
        self.in_flight = None;
        if read == wanted {
            Done {
                apply: true,
                next: None,
            }
        } else {
            self.in_flight = Some(wanted.clone());
            self.started += 1;
            Done {
                apply: false,
                next: Some(wanted.clone()),
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Reads started so far (tests, `debug.ui perf`).
    pub fn started(&self) -> u32 {
        self.started
    }
}

// ---------------------------------------------------------------------------
// What a part of the window shows (R142.1)
// ---------------------------------------------------------------------------

use crate::model::State;
use std::hash::Hasher;

/// A hash of what the *hot* model changes — an Agent's hook report, a
/// Notification, a terminal title, a detected Preset, the focus — can show
/// in the window: every Pane's name / status / title / Agent / account, the
/// Tabs and their focus, the active place, the Notifications and the
/// Fan-outs. It leaves out what a report changes without showing it (`hook_at`,
/// `hook_statuses`), so a repeated `running` is no change. The invocation's
/// argv is in: the Pane header's model text comes from it.
/// Updates that change anything else (a spawn, a split, a Review) do not
/// use it: they sync every part.
pub fn hot_key(m: &State) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m.active_workspace().hash(&mut h);
    let (s, w) = m.active();
    (&s.id, &w.id, s.paused).hash(&mut h);
    m.file_tree.hash(&mut h);
    for (src, wt) in m.worktrees() {
        (&src.id, src.paused, &wt.id, &wt.active_tab).hash(&mut h);
        for tab in &wt.tabs {
            (
                &tab.id,
                &tab.name,
                tab.renamed,
                &tab.focused,
                &tab.zoomed,
                &tab.grid,
            )
                .hash(&mut h);
            for p in tab.panes() {
                (
                    &p.id,
                    &p.name,
                    &p.agent,
                    &p.detected,
                    p.status.map(|s| s.as_str()),
                    p.status_since,
                    p.status_heuristic,
                    &p.title,
                    &p.account,
                    &p.session_id,
                    p.invocation.as_ref().map(|i| &i.argv),
                )
                    .hash(&mut h);
            }
        }
    }
    for n in &m.notifications {
        (n.id, n.read, n.text.len()).hash(&mut h);
    }
    h.write(format!("{:?}", m.fanouts).as_bytes());
    h.finish()
}

/// A hash of what the Inspector and the pages that follow the focused
/// checkout read: the Workspace, the checkout's folder, the Inspector flag.
pub fn inspector_key(m: &State) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m.active_workspace().hash(&mut h);
    let (s, w) = m.active();
    (&s.id, &w.id, &w.path, &w.base).hash(&mut h);
    m.file_tree.hash(&mut h);
    h.finish()
}

/// The last key each part synced under.
#[derive(Debug, Default)]
pub struct Gate {
    last: [Option<u64>; 2],
}

/// The parts a deferred sync tells apart.
#[derive(Clone, Copy)]
pub enum Part {
    /// Pane widgets, Tabs, Layouts, Navigator, titlebar, Statusline, focus.
    Window = 0,
    /// Files root, Changes, Search follow, Inspector visibility.
    Inspector = 1,
}

impl Gate {
    /// Whether `part` must sync under `key`.
    pub fn would_change(&self, part: Part, key: u64) -> bool {
        self.last[part as usize] != Some(key)
    }

    /// The widgets now show the model under these keys.
    pub fn synced(&mut self, window: u64, inspector: u64) {
        self.last = [Some(window), Some(inspector)];
    }
}

// ---------------------------------------------------------------------------
// Values cached by file stamps
// ---------------------------------------------------------------------------

/// `(file, mtime ns, size)`; a missing file is `(file, -1, 0)`.
pub type Stamp = Vec<(PathBuf, i128, u64)>;

/// The stamp of `files` now (stat only, nothing is read).
pub fn stamp(files: &[PathBuf]) -> Stamp {
    files.iter().map(|f| stamp_of(f)).collect()
}

/// One file's stamp.
pub fn stamp_of(f: &Path) -> (PathBuf, i128, u64) {
    match std::fs::metadata(f) {
        Ok(m) => {
            let ns = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i128);
            (f.to_path_buf(), ns, m.len())
        }
        Err(_) => (f.to_path_buf(), -1, 0),
    }
}

/// Values kept while the stamp of the files they came from is unchanged.
#[derive(Debug)]
pub struct StampCache<K, V> {
    entries: HashMap<K, (Stamp, V)>,
    loads: u32,
}

impl<K, V> Default for StampCache<K, V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            loads: 0,
        }
    }
}

impl<K: Hash + Eq, V: Clone> StampCache<K, V> {
    /// The value for `key`, loaded with `load` only when there is none or
    /// `now` (the files' stamp now) differs from the one it was loaded under.
    pub fn get(&mut self, key: K, now: Stamp, load: impl FnOnce() -> V) -> V {
        if let Some((s, v)) = self.entries.get(&key) {
            if *s == now {
                return v.clone();
            }
        }
        self.loads += 1;
        let v = load();
        self.entries.insert(key, (now, v.clone()));
        v
    }

    /// How many times `load` ran.
    pub fn loads(&self) -> u32 {
        self.loads
    }

    pub fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        self.entries.retain(|k, _| keep(k));
    }
}

/// The git directory of a checkout: `<path>/.git`, or what a linked
/// Worktree's `.git` file points to.
fn git_dir(path: &Path) -> Option<PathBuf> {
    let dot = path.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let dir = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let dir = Path::new(dir);
    Some(if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        path.join(dir)
    })
}

/// The files whose change moves a checkout's current branch: `.git`
/// (a Worktree's pointer) and its `HEAD`.
pub fn head_files(path: &Path) -> Vec<PathBuf> {
    let mut v = vec![path.join(".git")];
    if let Some(d) = git_dir(path) {
        v.push(d.join("HEAD"));
    }
    v
}

/// The files whose change moves a Source's main branch (`origin/HEAD`):
/// the common git directory's `refs/remotes/origin/HEAD`, `packed-refs`
/// and `config`.
pub fn main_branch_files(path: &Path) -> Vec<PathBuf> {
    let mut v = vec![path.join(".git")];
    if let Some(d) = git_dir(path) {
        // A linked Worktree's git dir names the common one.
        let common = std::fs::read_to_string(d.join("commondir"))
            .ok()
            .map(|c| d.join(c.trim()))
            .unwrap_or(d);
        v.push(common.join("refs/remotes/origin/HEAD"));
        v.push(common.join("packed-refs"));
        v.push(common.join("config"));
        v.push(common.join("HEAD"));
    }
    v
}

// ---------------------------------------------------------------------------
// Reading only what a Terminal added
// ---------------------------------------------------------------------------

/// How much of the rows that scrolled out of the active area is kept, so a
/// pattern over a few lines still matches across two looks.
pub const CARRY_MAX: usize = 8 * 1024;

/// A waiter's view of a Terminal's screen text. The rows above the active
/// area never change while Ghostty's row numbering is the same (its
/// `row_space_revision`: it moves when scrollback is pruned at the cap, on a
/// resize reflow, a clear), so each look reads only the rows that scrolled
/// out since the last one plus the active area (the last `rows` rows). The
/// caller passes the counts straight from the terminal
/// (`ghostty_surface_scrollbar`), never the ones the last draw published: a
/// Pane nobody sees does not draw.
#[derive(Debug, Default)]
pub struct Tail {
    committed: usize,
    rows: usize,
    revision: u64,
    started: bool,
    carry: String,
}

/// What a look reads: the rows `scrolled.0 .. scrolled.1` of the screen
/// (empty when equal) and the active area.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Plan {
    pub scrolled: (usize, usize),
}

impl Tail {
    /// The rows to read now: the Terminal has `total` rows (scrollback +
    /// active area) of which the last `rows` are the active area, numbered
    /// under `revision`. A first look, a resize, a new numbering (rows pruned
    /// at the scrollback cap or erased) or a Terminal that lost rows starts
    /// over from row 0, so no row is ever read under a stale number.
    pub fn plan(&mut self, total: usize, rows: usize, revision: u64) -> Plan {
        let committed = total.saturating_sub(rows);
        if !self.started
            || rows != self.rows
            || revision != self.revision
            || committed < self.committed
        {
            self.carry.clear();
            self.committed = 0;
        }
        self.started = true;
        self.rows = rows;
        self.revision = revision;
        Plan {
            scrolled: (self.committed, committed),
        }
    }

    /// The text to search after reading `plan`: what scrolled out
    /// (`scrolled`) and the active area now (`active`). The scrolled text
    /// joins the carry; the result is carry + active.
    pub fn text(&mut self, plan: Plan, scrolled: &str, active: &str) -> String {
        if plan.scrolled.1 > plan.scrolled.0 && !scrolled.is_empty() {
            if !self.carry.is_empty() && !self.carry.ends_with('\n') {
                self.carry.push('\n');
            }
            self.carry.push_str(scrolled.trim_end_matches('\n'));
        }
        self.committed = plan.scrolled.1;
        // The first look searches everything (a pattern printed before the
        // wait began still matches); only then is the carry cut.
        let text = if self.carry.is_empty() {
            active.to_string()
        } else if active.is_empty() {
            self.carry.clone()
        } else {
            format!("{}\n{active}", self.carry)
        };
        if self.carry.len() > CARRY_MAX {
            let mut cut = self.carry.len() - CARRY_MAX;
            while !self.carry.is_char_boundary(cut) {
                cut += 1;
            }
            self.carry.drain(..cut);
        }
        text
    }
}

// ---------------------------------------------------------------------------
// Type when settled
// ---------------------------------------------------------------------------

/// The tick of the settle timer.
pub const SETTLE_TICK_MS: u64 = 250;
/// Output unchanged for this many ticks = settled (0.75 s).
pub const SETTLE_STABLE_TICKS: u32 = 3;
/// A Terminal that never settles is typed into after this many ticks (15 s).
pub const SETTLE_MAX_TICKS: u32 = 60;
/// A Worktree `setup` that has not ended after this long is given up on:
/// nothing is typed (R140.5).
pub const SETUP_LIMIT_MS: u64 = 10 * 60 * 1000;

/// What the settle timer does after a tick.
#[derive(Debug, PartialEq, Eq)]
pub enum Settled {
    Wait,
    /// Paste the prompt and press Enter.
    Type,
    /// Stop watching, type nothing.
    GiveUp,
}

/// Prompt mode `type` (R16.1): once the new Terminal shows output that has
/// not changed for 0.75 s (or after 15 s), type the prompt; with a `setup`
/// first, only after it has ended, and never after [`SETUP_LIMIT_MS`].
#[derive(Debug)]
pub struct Settle {
    setup: bool,
    last: String,
    stable: u32,
    ticks: u32,
    waited_ms: u64,
}

impl Settle {
    pub fn new(setup: bool) -> Self {
        Self {
            setup,
            last: String::new(),
            stable: 0,
            ticks: 0,
            waited_ms: 0,
        }
    }

    /// The setup has not ended yet: the caller looks for it in the screen
    /// text and passes `setup_ended` to [`Settle::tick`].
    pub fn awaiting_setup(&self) -> bool {
        self.setup
    }

    /// One tick (every [`SETTLE_TICK_MS`]). `setup_ended` = the setup's end
    /// line is on screen (only looked at while awaiting it); `viewport` =
    /// the visible text (only needed once the setup is over).
    pub fn tick(&mut self, setup_ended: bool, viewport: impl FnOnce() -> String) -> Settled {
        self.waited_ms += SETTLE_TICK_MS;
        if self.setup {
            if !setup_ended {
                return if self.waited_ms >= SETUP_LIMIT_MS {
                    Settled::GiveUp
                } else {
                    Settled::Wait
                };
            }
            self.setup = false;
        }
        let screen = viewport();
        self.ticks += 1;
        if !screen.trim().is_empty() && screen == self.last {
            self.stable += 1;
        } else {
            self.stable = 0;
            self.last = screen;
        }
        if self.stable < SETTLE_STABLE_TICKS && self.ticks < SETTLE_MAX_TICKS {
            Settled::Wait
        } else {
            Settled::Type
        }
    }
}

#[cfg(test)]
#[path = "ui_work_tests.rs"]
mod tests;
