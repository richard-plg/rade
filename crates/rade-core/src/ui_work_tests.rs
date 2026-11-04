use super::*;
use std::cell::Cell;

fn tmp(tag: &str) -> PathBuf {
    let d =
        crate::paths::outside_temp_dir().join(format!("rade-ui-work-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn r142_c_a_result_for_a_checkout_no_longer_focused_is_dropped() {
    let mut r = Reader::<&str>::default();
    assert!(r.request(&"A"), "A's read starts");
    // The focus moves to B while A is read: nothing else starts.
    assert!(!r.request(&"B"));
    assert_eq!(r.started(), 1);
    // A's result arrives: dropped, and B's read starts in its place.
    let d = r.done(&"A", &"B");
    assert_eq!(
        d,
        Done {
            apply: false,
            next: Some("B")
        }
    );
    assert!(r.busy());
    // B's result is shown.
    let d = r.done(&"B", &"B");
    assert_eq!(
        d,
        Done {
            apply: true,
            next: None
        }
    );
    assert!(!r.busy());
}

#[test]
fn r142_d_never_two_reads_in_flight() {
    let mut r = Reader::<u8>::default();
    let mut running = 0;
    let mut most = 0;
    // A 3 s timer firing 4 times while the first read (5 s) is running.
    for _ in 0..4 {
        if r.request(&1) {
            running += 1;
            most = most.max(running);
        }
    }
    assert_eq!((running, most, r.started()), (1, 1, 1));
    r.done(&1, &1);
    running -= 1;
    assert!(r.request(&1), "the next timer tick starts a new read");
    assert_eq!(running, 0);
}

#[test]
fn r142_e_config_is_not_read_again_while_its_files_are_unchanged() {
    let d = tmp("config");
    let user = d.join("config.toml");
    let ws = d.join("workspace.toml");
    std::fs::write(&user, "idle_seconds = 5\n").unwrap();
    let files = vec![user.clone(), ws.clone()];
    let mut c = StampCache::<(String, Option<String>), String>::default();
    let key = ("src".to_string(), None);
    let reads = Cell::new(0);
    let read = || {
        reads.set(reads.get() + 1);
        std::fs::read_to_string(&user).unwrap()
    };
    let a = c.get(key.clone(), stamp(&files), read);
    // Five status hooks later: still one read.
    for _ in 0..5 {
        assert_eq!(c.get(key.clone(), stamp(&files), read), a);
    }
    assert_eq!(reads.get(), 1);
    // A file changed (here: the size): read again.
    std::fs::write(&user, "idle_seconds = 500\n").unwrap();
    let b = c.get(key.clone(), stamp(&files), read);
    assert_eq!((b.as_str(), reads.get()), ("idle_seconds = 500\n", 2));
    // A layer file that appears is a change too.
    std::fs::write(&ws, "x = 1\n").unwrap();
    c.get(key, stamp(&files), read);
    assert_eq!(reads.get(), 3);
}

fn run_git(d: &Path, args: &[&str]) {
    let o = std::process::Command::new("git")
        .args(args)
        .current_dir(d)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "{args:?}");
}

#[test]
fn r142_branch_cache_follows_head_and_origin_head() {
    let d = tmp("branches");
    run_git(&d, &["init", "-q", "-b", "main"]);
    run_git(
        &d,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "i",
        ],
    );
    let mut c = StampCache::<PathBuf, Option<String>>::default();
    let branch = || crate::git::branch(&d);
    let first = c.get(d.clone(), stamp(&head_files(&d)), branch);
    assert_eq!(first.as_deref(), Some("main"));
    c.get(d.clone(), stamp(&head_files(&d)), branch);
    assert_eq!(c.loads(), 1, "HEAD unchanged: cached");
    run_git(&d, &["checkout", "-q", "-b", "topic"]);
    let now = c.get(d.clone(), stamp(&head_files(&d)), branch);
    assert_eq!(now.as_deref(), Some("topic"));
    assert_eq!(c.loads(), 2, "HEAD moved: read again");
    // origin/HEAD appearing moves the main branch.
    let mut m = StampCache::<PathBuf, u8>::default();
    m.get(d.clone(), stamp(&main_branch_files(&d)), || 0);
    m.get(d.clone(), stamp(&main_branch_files(&d)), || 0);
    assert_eq!(m.loads(), 1);
    let o = d.join(".git/refs/remotes/origin");
    std::fs::create_dir_all(&o).unwrap();
    std::fs::write(o.join("HEAD"), "ref: refs/remotes/origin/main\n").unwrap();
    m.get(d.clone(), stamp(&main_branch_files(&d)), || 0);
    assert_eq!(m.loads(), 2, "origin/HEAD created: read again");
}

/// A Terminal's rows as Ghostty gives them: the last `rows` are the active
/// area. `read` counts the rows asked for.
struct Screen {
    lines: Vec<String>,
    rows: usize,
    read: Cell<usize>,
}

impl Screen {
    fn total(&self) -> usize {
        self.lines.len().max(self.rows)
    }
    fn range(&self, from: usize, to: usize) -> String {
        self.read.set(self.read.get() + to.saturating_sub(from));
        (from..to)
            .map(|i| self.lines.get(i).map_or("", String::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    }
    /// One look of `tail`: (the text to search, rows read).
    fn look(&self, tail: &mut Tail) -> (String, usize) {
        self.read.set(0);
        let plan = tail.plan(self.total(), self.rows);
        let scrolled = self.range(plan.scrolled.0, plan.scrolled.1);
        let active = self.range(self.total() - self.rows, self.total());
        (tail.text(plan, &scrolled, &active), self.read.get())
    }
}

#[test]
fn r142_f_a_look_reads_only_the_rows_added_since_the_last_one() {
    let mut s = Screen {
        lines: (0..5000).map(|i| format!("line {i}")).collect(),
        rows: 40,
        read: Cell::new(0),
    };
    let mut tail = Tail::default();
    let re = regex_lite::Regex::new("BUILD (OK|FAILED)").unwrap();
    // First look reads everything once.
    let (text, read) = s.look(&mut tail);
    assert!(!re.is_match(&text));
    assert_eq!(read, 5000 - 40 + 40);
    // Nothing added: only the active area.
    let (_, read) = s.look(&mut tail);
    assert_eq!(read, 40);
    // 30 rows added, one of them the pattern.
    for i in 0..30 {
        s.lines.push(if i == 17 {
            "BUILD OK".into()
        } else {
            format!("more {i}")
        });
    }
    let (text, read) = s.look(&mut tail);
    assert!(re.is_match(&text), "found");
    assert_eq!(read, 30 + 40, "30 new rows + the active area, not 5030");
}

#[test]
fn r142_f_a_pattern_over_two_looks_and_a_resize() {
    let mut s = Screen {
        lines: (0..100).map(|i| format!("l{i}")).collect(),
        rows: 10,
        read: Cell::new(0),
    };
    let mut tail = Tail::default();
    s.look(&mut tail);
    s.lines.push("first".into());
    s.lines.extend((0..15).map(|i| format!("pad{i}")));
    let (t1, _) = s.look(&mut tail);
    assert!(t1.contains("first"));
    s.lines.push("second".into());
    let (t2, read) = s.look(&mut tail);
    assert!(t2.contains("first") && t2.contains("second"), "carry kept");
    assert_eq!(read, 1 + 10);
    // A resize starts over.
    s.rows = 12;
    let (_, read) = s.look(&mut tail);
    assert_eq!(read, s.total());
}

#[test]
fn r142_f_carry_is_bounded() {
    let mut tail = Tail::default();
    let plan = tail.plan(100_000, 10);
    let huge = "x".repeat(100_000);
    // The first look searches everything.
    let out = tail.text(plan, &huge, "tail");
    assert!(out.len() > 100_000 && out.ends_with("\ntail"));
    // Later looks carry only the last CARRY_MAX bytes.
    let plan = tail.plan(100_001, 10);
    let out = tail.text(plan, "x", "tail");
    assert!(out.len() <= CARRY_MAX + 8, "{}", out.len());
    assert!(out.ends_with("\ntail"));
}

#[test]
fn r140_5_settle_types_when_quiet_and_gives_up_on_a_hung_setup() {
    // No setup: three identical non-empty screens.
    let mut s = Settle::new(false);
    assert_eq!(s.tick(false, || "$ ".into()), Settled::Wait);
    assert_eq!(s.tick(false, || "$ ".into()), Settled::Wait);
    assert_eq!(s.tick(false, || "$ ".into()), Settled::Wait);
    assert_eq!(s.tick(false, || "$ ".into()), Settled::Type);
    // A screen that keeps changing is typed into after 60 ticks.
    let mut s = Settle::new(false);
    let mut n = 0;
    let verdict = loop {
        n += 1;
        match s.tick(false, || format!("{n}")) {
            Settled::Wait => {}
            v => break v,
        }
    };
    assert_eq!((verdict, n), (Settled::Type, SETTLE_MAX_TICKS));
    // A setup that ends: then the screen settles as usual.
    let mut s = Settle::new(true);
    assert!(s.awaiting_setup());
    assert_eq!(s.tick(false, || unreachable!()), Settled::Wait);
    assert_eq!(s.tick(true, || "ok".into()), Settled::Wait);
    assert!(!s.awaiting_setup());
    // A setup that never ends: given up at the limit, nothing typed.
    let mut s = Settle::new(true);
    let mut ticks = 0u64;
    let verdict = loop {
        ticks += 1;
        match s.tick(false, || unreachable!()) {
            Settled::Wait => {}
            v => break v,
        }
    };
    assert_eq!(verdict, Settled::GiveUp);
    assert_eq!(ticks * SETTLE_TICK_MS, SETUP_LIMIT_MS);
}

fn two_panes() -> (State, String, String) {
    let mut w = State::new(Path::new("/home/u"));
    let x = w.add_source(Path::new("/tmp/x"), None).unwrap();
    let wt = w.source(&x).unwrap().active_worktree.clone();
    let (t1, _) = w.new_tab(&x, &wt, None, None).unwrap();
    let (t2, _) = w.new_tab(&x, &wt, None, None).unwrap();
    let focused = |w: &State, tab: &str| {
        w.worktrees()
            .flat_map(|(_, wt)| wt.tabs.iter())
            .find(|t| t.id == tab)
            .unwrap()
            .focused
            .clone()
    };
    let (a, b) = (focused(&w, &t1), focused(&w, &t2));
    (w, a, b)
}

#[test]
fn r142_1_a_repeated_report_changes_no_part_and_a_new_status_changes_the_window() {
    use crate::agents::AgentStatus::{Done, Running};
    let (mut w, a, b) = two_panes();
    w.set_status(&a, Some(Running), None, Some("claude".into()), 100)
        .unwrap();
    w.record_hook(&a, Some(Running), 100).unwrap();
    let (hot, ins) = (hot_key(&w), inspector_key(&w));
    // The same report again, a second later: the hook time moves, nothing
    // shown does.
    w.set_status(&a, Some(Running), None, Some("claude".into()), 101)
        .unwrap();
    w.record_hook(&a, Some(Running), 101).unwrap();
    assert_eq!(hot_key(&w), hot);
    let mut gate = Gate::default();
    gate.synced(hot, ins);
    assert!(!gate.would_change(Part::Window, hot_key(&w)));
    assert!(!gate.would_change(Part::Inspector, inspector_key(&w)));
    // `done`: the window shows it, the Inspector does not care.
    w.set_status(&a, Some(Done), None, None, 102).unwrap();
    assert!(gate.would_change(Part::Window, hot_key(&w)));
    assert!(!gate.would_change(Part::Inspector, inspector_key(&w)));
    // Another Pane's focus is the window's too; the Inspector flag its own.
    let mut w2 = w.clone();
    w2.focus_pane(&b).unwrap();
    assert_ne!(hot_key(&w2), hot_key(&w));
    w2.file_tree = !w2.file_tree;
    assert_ne!(inspector_key(&w2), inspector_key(&w));
}
