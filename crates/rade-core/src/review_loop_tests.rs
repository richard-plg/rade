use super::*;
use crate::diff::parse;
use crate::git::tests::{repo, sh};
use crate::SplitDir;

fn ws() -> State {
    let mut w = State::new(Path::new("/home/u"));
    w.add_source(Path::new("/r/x"), None).unwrap();
    for name in ["one", "title", "two", "none"] {
        w.add_worktree("x", name, &Path::new("/d").join(name), None)
            .unwrap();
    }
    w
}

/// A shell Tab in Worktree `wt` with an Agent of Preset `agent` split off
/// it; returns the Agent's Pane id.
fn agent(w: &mut State, wt: &str, agent: &str, since: u64) -> String {
    let (p, wid) = w.find_worktree("x", Some(wt)).unwrap();
    let (_, shell) = w.new_tab(&p, &wid, None, None).unwrap();
    let a = w.split(&shell, SplitDir::Right, None, None).unwrap();
    w.set_status(
        &a,
        Some(AgentStatus::Running),
        None,
        Some(agent.into()),
        since,
    )
    .unwrap();
    a
}

fn head(w: &State, wt: &str) -> Head {
    let (p, wid) = w.find_worktree("x", Some(wt)).unwrap();
    w.review_head(&p, &wid)
}

#[test]
fn r118_a_head_names_the_agent_and_its_task() {
    let mut w = ws();
    let a = agent(&mut w, "one", "claude", 10);
    w.set_task(&a, "add  OTP\nlogin").unwrap();
    let h = head(&w, "one");
    let ag = h.agent.as_ref().unwrap();
    assert_eq!(
        (ag.name.as_str(), ag.mark, ag.more),
        ("claude", AgentMark::Claude, 0)
    );
    assert_eq!(h.task.as_deref(), Some("add OTP login"));

    // No prompt: its session title.
    let t = agent(&mut w, "title", "codex", 10);
    let loc = w.locate_or_err(&t).unwrap();
    w.tab_mut(&loc).unwrap().pane_mut(&t).unwrap().title = Some("Fix flaky test".into());
    assert_eq!(head(&w, "title").task.as_deref(), Some("Fix flaky test"));

    // Two Agents: the one whose status changed last, plus the other.
    agent(&mut w, "two", "claude", 10);
    let late = agent(&mut w, "two", "codex", 20);
    let h = head(&w, "two");
    assert_eq!(
        h.agent.as_ref().map(|a| (a.pane.clone(), a.more)),
        Some((late, 1))
    );

    assert_eq!(head(&w, "none"), Head::default());
    // An exited Agent is not the head's.
    w.set_status(&a, Some(AgentStatus::Exited), None, None, 30)
        .unwrap();
    assert_eq!(head(&w, "one").agent, None);
}

fn patch(paths: &[&str]) -> Vec<FileDiff> {
    let text: String = paths
        .iter()
        .map(|p| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-a\n+b\n"))
        .collect();
    parse(&text, &[])
}

#[test]
fn r118_b_rail_is_a_tree_in_diff_order_with_generated_last() {
    let r = repo("rail");
    std::fs::write(
        r.join(".gitattributes"),
        "gen/** linguist-generated\nkeep.rs -linguist-generated\n",
    )
    .unwrap();
    let paths = [
        "Cargo.lock",
        "README.md",
        "gen/x.rs",
        "keep.rs",
        "src/main.rs",
        "src/ui/a.rs",
        "src/ui/b.rs",
        "web/app.min.js",
    ];
    let attrs = generated_attrs(&r, &paths);
    assert_eq!(attrs, HashSet::from(["gen/x.rs".to_string()]));
    let generated = |p: &str| generated_by_name(p) || attrs.contains(p);
    let files = patch(&paths);
    let order = file_order(&files, &generated);
    let names: Vec<&str> = order.iter().map(|&i| files[i].path.as_str()).collect();
    assert_eq!(
        names,
        [
            "README.md",
            "keep.rs",
            "src/main.rs",
            "src/ui/a.rs",
            "src/ui/b.rs",
            "Cargo.lock",
            "gen/x.rs",
            "web/app.min.js"
        ]
    );
    let folded = HashSet::from([GENERATED.to_string()]);
    let rows = rail(&files, &order, &generated, &folded, "", &|_| false);
    let text: Vec<String> = rows
        .iter()
        .map(|r| match r {
            RailRow::Folder { name, depth, .. } => format!("{depth}{name}"),
            RailRow::File { name, depth, .. } => format!("{depth}{name}"),
            RailRow::Generated { count, folded } => format!("gen {count} {folded}"),
        })
        .collect();
    assert_eq!(
        text,
        [
            "0README.md",
            "0keep.rs",
            "0src/",
            "1main.rs",
            "1ui/",
            "2a.rs",
            "2b.rs",
            "gen 3 true"
        ]
    );

    // Single-child folders join; a folded folder hides its files.
    let files = patch(&["a/b/c/one.rs", "a/b/c/two.rs", "z.rs"]);
    let order: Vec<usize> = (0..3).collect();
    let none = |_: &str| false;
    let rows = rail(&files, &order, &none, &HashSet::new(), "", &|_| false);
    assert_eq!(
        rows[0],
        RailRow::Folder {
            path: "a/b/c/".into(),
            name: "a/b/c/".into(),
            depth: 0,
            folded: false
        }
    );
    assert_eq!(rows.len(), 4);
    let folded = HashSet::from(["a/b/c/".to_string()]);
    assert_eq!(
        rail(&files, &order, &none, &folded, "", &|_| false).len(),
        2
    );

    // A filter lists matches flat; hide drops files.
    let rows = rail(&files, &order, &none, &HashSet::new(), "TWO", &|_| false);
    assert_eq!(
        rows,
        [RailRow::File {
            index: 1,
            name: "a/b/c/two.rs".into(),
            depth: 0
        }]
    );
    let rows = rail(&files, &order, &none, &HashSet::new(), "", &|i| i != 2);
    assert_eq!(
        rows,
        [RailRow::File {
            index: 2,
            name: "z.rs".into(),
            depth: 0
        }]
    );
    let _ = std::fs::remove_dir_all(&r);
}

fn pairs(v: &[(String, u64)]) -> Vec<(&str, u64)> {
    v.iter().map(|(p, h)| (p.as_str(), *h)).collect()
}

#[test]
fn r118_c_viewed_resets_when_the_file_changes() {
    let r = repo("viewed");
    for p in ["a.rs", "b.rs", "c.rs"] {
        std::fs::write(r.join(p), "one\n").unwrap();
    }
    let files = patch(&["a.rs", "b.rs", "c.rs"]);
    let hashes = |files: &[FileDiff]| -> Vec<(String, u64)> {
        files
            .iter()
            .map(|f| (f.path.clone(), file_hash(&r, f)))
            .collect()
    };
    let h = hashes(&files);
    let mut l = Loop::default();
    l.set_viewed("a.rs", h[0].1, true);
    l.set_viewed("b.rs", h[1].1, true);
    assert_eq!(l.clone().progress(&pairs(&h)), (2, 3));
    std::fs::write(r.join("b.rs"), "two\n").unwrap();
    let h2 = hashes(&files);
    assert!(l.is_viewed("a.rs", h2[0].1) && !l.is_viewed("b.rs", h2[1].1));
    assert_eq!(l.progress(&pairs(&h2)), (1, 3));
    // A file no longer changed loses its mark.
    l.set_viewed("c.rs", h2[2].1, true);
    l.progress(&pairs(&h2[..2]));
    assert!(!l.viewed.contains_key("c.rs"));
    l.set_viewed("a.rs", 0, false);
    assert_eq!(l.progress(&pairs(&h2)), (0, 3));
    // Whitespace hidden or not, the same file has the same hash.
    let mut ws_only = files[0].clone();
    ws_only.hunks.clear();
    assert_eq!(file_hash(&r, &ws_only), h2[0].1);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn r118_d_moves_over_hunks_files_and_comments_without_wrapping() {
    let text = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-x\n+y\n@@ -9 +9 @@\n-x\n+y\n\
                diff --git a/b b/b\n--- a/b\n+++ b/b\n@@ -1 +1 @@\n-x\n+y\n\
                diff --git a/c b/c\n--- a/c\n+++ b/c\n@@ -1 +1 @@\n-x\n+y\n";
    let files = parse(text, &[]);
    let folded = HashSet::from(["b".to_string()]);
    let rows = crate::diff::rows(&files, &folded, &HashSet::new());
    let cards = [
        Card {
            id: 1,
            path: "a".into(),
            line: 9,
            outdated: false,
        },
        Card {
            id: 2,
            path: "c".into(),
            line: 1,
            outdated: false,
        },
        Card {
            id: 3,
            path: "c".into(),
            line: 50,
            outdated: false,
        },
        Card {
            id: 4,
            path: "b".into(),
            line: 1,
            outdated: false,
        },
    ];
    let rows = place_cards(&rows, &files, &cards, Some(2));
    use Row::*;
    assert_eq!(
        rows,
        [
            File(0),
            Hunk(0, 0),
            Line(0, 0, 0),
            Composer,
            Line(0, 0, 1),
            Hunk(0, 1),
            Line(0, 1, 0),
            Line(0, 1, 1),
            Comment(1),
            File(1),
            File(2),
            Comment(3),
            Hunk(2, 0),
            Line(2, 0, 0),
            Line(2, 0, 1),
            Comment(2),
        ]
    );
    assert_eq!(step(&rows, None, Move::NextHunk), Some(1));
    assert_eq!(step(&rows, Some(1), Move::NextHunk), Some(5));
    assert_eq!(
        step(&rows, Some(5), Move::NextHunk),
        Some(12),
        "folded b has no hunk"
    );
    assert_eq!(step(&rows, Some(12), Move::NextHunk), None, "no wrap");
    assert_eq!(step(&rows, Some(12), Move::PrevHunk), Some(5));
    assert_eq!(step(&rows, Some(3), Move::NextFile), Some(9));
    assert_eq!(step(&rows, Some(9), Move::PrevFile), Some(0));
    assert_eq!(step(&rows, Some(0), Move::PrevFile), None);
    assert_eq!(step(&rows, Some(0), Move::NextComment), Some(8));
    assert_eq!(step(&rows, Some(8), Move::NextComment), Some(11));
    assert_eq!(step(&rows, Some(15), Move::PrevComment), Some(11));
    assert_eq!(step(&rows, None, Move::PrevComment), None);
}

#[test]
fn r118_loop_state_is_omitted_when_empty() {
    let mut w = ws();
    let json = serde_json::to_string(&w).unwrap();
    assert!(!json.contains("\"review\""), "{json}");
    let (p, wid) = w.find_worktree("x", Some("one")).unwrap();
    w.worktree_mut(&p, &wid)
        .unwrap()
        .review
        .set_viewed("a.rs", 7, true);
    let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
    assert!(back.worktree(&p, &wid).unwrap().review.is_viewed("a.rs", 7));
    let _ = sh;
}

#[test]
fn r118_a_commits_since_the_base_and_whitespace_hidden() {
    let r = repo("commits");
    sh(&r, &["checkout", "-qb", "feat"]);
    for (i, msg) in ["first", "second: with colon"].iter().enumerate() {
        std::fs::write(r.join(format!("f{i}")), "x\n").unwrap();
        sh(&r, &["add", "-A"]);
        sh(&r, &["commit", "-qm", msg]);
    }
    let c = crate::diff::changes(&r, Some("main")).unwrap();
    let got: Vec<String> = commits(&r, &c.base)
        .into_iter()
        .map(|c| c.subject)
        .collect();
    assert_eq!(got, ["second: with colon", "first"]);
    assert!(commits(&r, "nope").is_empty());
    // A whitespace-only edit disappears with -w.
    std::fs::write(r.join("a.txt"), "one\n  two\nthree\n").unwrap();
    let all = crate::diff::changes_with(&r, Some("main"), false).unwrap();
    let ws = crate::diff::changes_with(&r, Some("main"), true).unwrap();
    assert!(all.files.iter().any(|f| f.path == "a.txt"));
    assert!(
        !ws.files
            .iter()
            .any(|f| f.path == "a.txt" && !f.hunks.is_empty()),
        "{:?}",
        ws.files
    );
    let _ = std::fs::remove_dir_all(&r);
}

// ---- R119: Rounds and conclusion

fn draft(path: &str, line: u32, text: &str) -> crate::review::ReviewComment {
    crate::review::ReviewComment {
        path: path.into(),
        line,
        excerpt: vec![format!("+line {line}")],
        text: text.into(),
        ..Default::default()
    }
}

#[test]
fn r119_a_rounds_keep_twenty_snapshots_and_number_the_drafts() {
    let r = repo("rounds");
    std::fs::write(r.join("new.txt"), "untracked\n").unwrap();
    let mut w = ws();
    let (p, wid) = w.find_worktree("x", Some("one")).unwrap();
    w.add_comment(&p, &wid, draft("a.txt", 1, "one")).unwrap();
    w.add_comment(&p, &wid, draft("a.txt", 2, "two")).unwrap();
    for i in 0..21u32 {
        let n = w.next_round(&p, &wid);
        assert_eq!(n, i + 1);
        if i > 0 {
            w.add_comment(&p, &wid, draft("a.txt", 3, "more")).unwrap();
        }
        let sha = snapshot(&r, "one", n).unwrap();
        let dropped = w
            .record_round(
                &p,
                &wid,
                Round {
                    n,
                    at: 100 + n as u64,
                    snapshot: sha,
                    approved: false,
                },
            )
            .unwrap();
        drop_refs(&r, "one", Some(&dropped));
        assert_eq!(dropped, if n == 21 { vec![1] } else { vec![] });
    }
    let wt = w.worktree(&p, &wid).unwrap();
    assert_eq!(wt.review.rounds.first().map(|r| r.n), Some(2));
    assert_eq!(wt.review.rounds.len(), ROUNDS_KEPT);
    assert_eq!(
        wt.comments
            .iter()
            .filter_map(|c| c.round)
            .take(3)
            .collect::<Vec<_>>(),
        [1, 1, 2]
    );
    assert!(
        wt.comments.iter().all(|c| c.round.is_some()),
        "no drafts left"
    );
    let refs = crate::git::run(
        &r,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/rade/review/one/",
        ],
    )
    .unwrap();
    assert!(
        !refs.lines().any(|l| l.ends_with("/1")) && refs.lines().count() == 20,
        "{refs}"
    );
    let last = &wt.review.rounds.last().unwrap().snapshot;
    let tree = crate::git::run(&r, &["ls-tree", "--name-only", last]).unwrap();
    assert!(
        tree.lines().any(|l| l == "new.txt"),
        "untracked file in the snapshot: {tree}"
    );
    drop_refs(&r, "one", None);
    assert_eq!(
        crate::git::run(&r, &["for-each-ref", "refs/rade/"])
            .unwrap()
            .trim(),
        ""
    );
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn r119_b_since_a_round_lists_only_what_changed_after_it() {
    let r = repo("since");
    for f in ["b", "c", "d", "e"] {
        std::fs::write(r.join(format!("{f}.txt")), "x\n").unwrap();
    }
    sh(&r, &["add", "-A"]);
    sh(&r, &["commit", "-qm", "five files"]);
    sh(&r, &["checkout", "-qb", "feat"]);
    for f in ["a", "b", "c", "d", "e"] {
        std::fs::write(r.join(format!("{f}.txt")), "edited\n").unwrap();
    }
    let snap = snapshot(&r, "feat", 1).unwrap();
    std::fs::write(r.join("b.txt"), "again\n").unwrap();
    std::fs::write(r.join("d.txt"), "again\n").unwrap();
    std::fs::write(r.join("f.txt"), "new\n").unwrap();
    let c = crate::diff::since(&r, None, &snap, false).unwrap();
    let paths: Vec<&str> = c.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["b.txt", "d.txt", "f.txt"]);
    // Their Viewed marks no longer match (the content changed).
    let all = crate::diff::changes(&r, Some("main")).unwrap();
    let mut l = Loop::default();
    let f = |p: &str| all.files.iter().find(|f| f.path == p).unwrap().clone();
    l.set_viewed("a.txt", file_hash(&r, &f("a.txt")), true);
    assert!(l.is_viewed("a.txt", file_hash(&r, &f("a.txt"))));
    drop_refs(&r, "feat", None);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn r119_c_sent_comments_read_code_changed_or_untouched_after_the_turn() {
    let file = "fn a() {\n    one();\n}\nfn b() {\n    two();\n}\n";
    let mut first = draft("x.rs", 2, "Rename one.");
    first.excerpt = vec!["+    one();".into()];
    first.capture(file, None);
    let mut second = draft("x.rs", 5, "Why two?\nExplain.");
    second.excerpt = vec!["+    two();".into()];
    second.capture(file, None);
    assert_eq!(comment_state(&first, true, Some(file)), CommentState::Draft);
    first.round = Some(1);
    second.round = Some(1);
    second.id = 2;
    assert_eq!(comment_state(&first, false, Some(file)), CommentState::Sent);
    let after = file.replace("one();", "uno();");
    assert_eq!(
        comment_state(&first, true, Some(&after)),
        CommentState::Changed
    );
    assert_eq!(
        comment_state(&second, true, Some(&after)),
        CommentState::Untouched
    );
    assert_eq!(
        comment_state(&second, true, None),
        CommentState::Changed,
        "file gone"
    );

    // A follow-up on the second quotes it under its Round.
    let mut follow = draft("x.rs", 5, "Still unclear.");
    follow.reply_to = Some(2);
    let p = crate::review::compose_round("feat", Some(2), &[follow], &[first, second]);
    assert!(
        p.starts_with("Review round 2: comments on the changes in this worktree (feat)."),
        "{p}"
    );
    assert!(
        p.contains("Re: round 1, x.rs:5: \"Why two?\"\n   Still unclear."),
        "{p}"
    );

    // An Agent's turn ending after the Round's time counts.
    let mut w = ws();
    let a = agent(&mut w, "one", "claude", 10);
    let (p, wid) = w.find_worktree("x", Some("one")).unwrap();
    assert!(!w.turn_ended_since(&p, &wid, 50));
    w.set_status(&a, Some(AgentStatus::Done), None, None, 60)
        .unwrap();
    assert!(w.turn_ended_since(&p, &wid, 50));
    assert!(w.turn_ended_since(&p, &wid, 60), "the same second");
    assert!(!w.turn_ended_since(&p, &wid, 61));
}

#[test]
fn r119_d_approve_menu_by_context_and_the_ask_prompts() {
    use Approve::*;
    assert_eq!(
        Approve::items(true, false),
        [AskCommit, AskCommitPr, CommitMyself, CreatePr]
    );
    assert_eq!(Approve::items(false, false), [CommitMyself, CreatePr]);
    assert_eq!(
        Approve::items(true, true),
        [AskCommit, AskCommitPr, CommitMyself, CreatePr, ChooseWinner]
    );
    assert_eq!(
        AskCommit.prompt(2, "main").unwrap(),
        "Approved (review round 2). Commit these changes following this repository's conventions. Do not push."
    );
    assert!(AskCommitPr
        .prompt(2, "develop")
        .unwrap()
        .ends_with("open a pull request against develop."));
    assert_eq!(CreatePr.prompt(2, "main"), None);
    let r = Round {
        n: 1,
        at: 0,
        snapshot: String::new(),
        approved: true,
    };
    assert_eq!(RoundState::of(&r, false).words(), "approved");
    assert_eq!(RoundState::of(&r, true).words(), "changed after approval");
    assert_eq!(
        RoundState::of(
            &Round {
                approved: false,
                ..r
            },
            true
        )
        .words(),
        "changes requested"
    );
}

#[test]
fn r119_e_a_finished_agent_offers_its_worktrees_review() {
    let mut w = ws();
    let a = agent(&mut w, "one", "claude", 10);
    let (p, wid) = w.find_worktree("x", Some("one")).unwrap();
    assert_eq!(w.review_target(&a), Some((p.clone(), wid.clone())));
    assert!(w.finished_for_review().is_empty(), "still running");
    w.set_status(&a, Some(AgentStatus::Done), None, None, 20)
        .unwrap();
    assert_eq!(w.finished_for_review(), std::slice::from_ref(&a));
    // An Agent in the Source's main checkout offers none until a review starts.
    let (mp, mw) = w.find_worktree("x", None).unwrap();
    let (_, shell) = w.new_tab(&mp, &mw, None, None).unwrap();
    let m = w.split(&shell, SplitDir::Right, None, None).unwrap();
    w.set_status(&m, Some(AgentStatus::Done), None, Some("codex".into()), 20)
        .unwrap();
    assert_eq!(w.review_target(&m), None);
    w.add_comment(&mp, &mw, draft("a", 1, "x")).unwrap();
    assert_eq!(w.review_target(&m), Some((mp, mw)));
    assert_eq!(w.review_target(&shell), None, "a shell");
}

#[test]
fn r119_e_navigator_chip_says_open_comments_or_approved() {
    let mut w = ws();
    let (p, wid) = w.find_worktree("x", Some("one")).unwrap();
    let chip = |w: &State| chip(w.worktree(&p, &wid).unwrap());
    assert_eq!(chip(&w), None);
    let a = w.add_comment(&p, &wid, draft("a", 1, "x")).unwrap();
    w.add_comment(&p, &wid, draft("a", 2, "y")).unwrap();
    w.record_round(
        &p,
        &wid,
        Round {
            n: 1,
            at: 1,
            snapshot: "s".into(),
            approved: false,
        },
    )
    .unwrap();
    assert_eq!(chip(&w).as_deref(), Some("R1 · 2 open"));
    w.resolve_comment(&p, &wid, a, true).unwrap();
    assert_eq!(chip(&w).as_deref(), Some("R1 · 1 open"));
    w.record_round(
        &p,
        &wid,
        Round {
            n: 2,
            at: 2,
            snapshot: "s".into(),
            approved: true,
        },
    )
    .unwrap();
    assert_eq!(chip(&w).as_deref(), Some("approved"));
    assert!(w.set_changed_since(&p, &wid, true));
    assert_eq!(chip(&w).as_deref(), Some("R2 · changed"));
    assert!(!w.set_changed_since(&p, &wid, true), "no change");
}
