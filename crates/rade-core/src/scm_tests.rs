use super::*;
use crate::git::tests::{repo, sh};
use std::path::PathBuf;

fn status_of(d: &Path) -> Status {
    status(d).unwrap()
}

fn letters(st: &Status, s: Section) -> String {
    st.section(s)
        .iter()
        .map(|c| format!("{}{}", c.letter(s), c.path))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A repo whose commits work without the user's identity.
fn repo_with_id(name: &str) -> PathBuf {
    let d = repo(name);
    sh(&d, &["config", "user.name", "t"]);
    sh(&d, &["config", "user.email", "t@t"]);
    d
}

#[test]
fn r21_1_parses_porcelain_v2_headers_entries_renames_conflicts() {
    let out = b"# branch.oid 1234\0# branch.head feat x\0# branch.upstream origin/feat x\0# branch.ab +2 -1\0\
1 M. N... 100644 100644 100644 aaa bbb src/a b.rs\0\
1 .M N... 100644 100644 100644 aaa bbb lib.rs\0\
2 R. N... 100644 100644 100644 aaa bbb R100 new.rs\0old.rs\0\
u UU N... 100644 100644 100644 100644 a b c conflict.rs\0\
? notes/todo.txt\0";
    let st = parse_status(out);
    assert_eq!(st.branch.as_deref(), Some("feat x"));
    assert_eq!(st.upstream.as_deref(), Some("origin/feat x"));
    assert_eq!((st.ahead, st.behind, st.initial), (2, 1, false));
    assert_eq!(st.files.len(), 5);
    assert_eq!(st.files[2].orig.as_deref(), Some("old.rs"));
    assert_eq!(letters(&st, Section::Staged), "Msrc/a b.rs Rnew.rs");
    assert_eq!(letters(&st, Section::Unstaged), "Mlib.rs !conflict.rs");
    assert_eq!(letters(&st, Section::Untracked), "Unotes/todo.txt");
    assert_eq!(st.own_upstream(), Some(("origin", "feat x")));

    let st = parse_status(b"# branch.oid (initial)\0# branch.head (detached)\0");
    assert!(st.initial);
    assert_eq!(st.branch, None);
    // A Worktree branched from origin/main tracks it: not its own upstream.
    let st = parse_status(b"# branch.head feat\0# branch.upstream origin/main\0");
    assert_eq!(st.own_upstream(), None);
}

#[test]
fn r21_2_stage_unstage_discard_files() {
    let d = repo_with_id("scm-files");
    std::fs::write(d.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    std::fs::write(d.join("new.txt"), "n\n").unwrap();
    std::fs::write(d.join(":odd*"), "x\n").unwrap();
    let st = status_of(&d);
    assert_eq!(letters(&st, Section::Unstaged), "Ma.txt");
    assert_eq!(letters(&st, Section::Untracked), "U:odd* Unew.txt");

    stage(&d, &["a.txt".into(), ":odd*".into()]).unwrap();
    let st = status_of(&d);
    assert_eq!(letters(&st, Section::Staged), "A:odd* Ma.txt");
    assert_eq!(letters(&st, Section::Unstaged), "");

    unstage(&d, &["a.txt".into()], false).unwrap();
    let st = status_of(&d);
    assert_eq!(letters(&st, Section::Staged), "A:odd*");
    assert_eq!(letters(&st, Section::Unstaged), "Ma.txt");

    let a = st.files.iter().find(|c| c.path == "a.txt").unwrap().clone();
    discard(&d, &a).unwrap();
    assert_eq!(
        std::fs::read_to_string(d.join("a.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
    let n = st
        .files
        .iter()
        .find(|c| c.path == "new.txt")
        .unwrap()
        .clone();
    discard(&d, &n).unwrap();
    assert!(!d.join("new.txt").exists());
    let bad = Change {
        path: "../x".into(),
        orig: None,
        x: '?',
        y: '?',
    };
    assert!(discard(&d, &bad).is_err());
}

#[test]
fn r21_2_unstage_before_the_first_commit() {
    let d = std::env::temp_dir().join(format!("rade-scm-initial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    sh(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("f"), "x\n").unwrap();
    stage(&d, &["f".into()]).unwrap();
    let st = status_of(&d);
    assert!(st.initial);
    assert_eq!(letters(&st, Section::Staged), "Af");
    unstage(&d, &["f".into()], true).unwrap();
    assert_eq!(letters(&status_of(&d), Section::Untracked), "Uf");
}

fn ten_lines(edit: &[usize]) -> String {
    (1..=20)
        .map(|i| {
            if edit.contains(&i) {
                format!("CHANGED {i}\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect()
}

#[test]
fn r21_3_one_hunk_staged_unstaged_and_discarded() {
    let d = repo_with_id("scm-hunks");
    std::fs::write(d.join("f.txt"), ten_lines(&[])).unwrap();
    sh(&d, &["add", "f.txt"]);
    sh(&d, &["commit", "-qm", "f"]);
    // Two hunks: line 2 and line 18.
    std::fs::write(d.join("f.txt"), ten_lines(&[2, 18])).unwrap();
    let st = status_of(&d);
    let c = st.files[0].clone();
    let raw = file_diff(&d, &c, Section::Unstaged).unwrap();
    let parsed = crate::diff::parse(&raw, &[]);
    assert!(hunks_apply(&parsed[0]));
    let fp = split_patch(&raw);
    assert_eq!(fp.len(), 1);
    assert_eq!(fp[0].hunks.len(), parsed[0].hunks.len());
    assert_eq!(fp[0].hunks.len(), 2);

    // Stage the second hunk only.
    apply_hunk(&d, &hunk_patch(&fp[0], 1).unwrap(), HunkOp::Stage).unwrap();
    let cached = git::run(&d, &["diff", "--cached"]).unwrap();
    assert!(cached.contains("+CHANGED 18") && !cached.contains("CHANGED 2\n"));
    let rest = git::run(&d, &["diff"]).unwrap();
    assert!(rest.contains("+CHANGED 2") && !rest.contains("CHANGED 18"));

    // Unstage it again from the staged diff.
    let staged = split_patch(&file_diff(&d, &c, Section::Staged).unwrap());
    apply_hunk(&d, &hunk_patch(&staged[0], 0).unwrap(), HunkOp::Unstage).unwrap();
    assert_eq!(git::run(&d, &["diff", "--cached"]).unwrap(), "");

    // Discard the first hunk from the working tree; the second stays.
    let fp = split_patch(&file_diff(&d, &c, Section::Unstaged).unwrap());
    apply_hunk(&d, &hunk_patch(&fp[0], 0).unwrap(), HunkOp::Discard).unwrap();
    assert_eq!(
        std::fs::read_to_string(d.join("f.txt")).unwrap(),
        ten_lines(&[18])
    );
    assert_eq!(hunk_patch(&fp[0], 9), None);
}

#[test]
fn r21_3_split_keeps_no_newline_markers_and_file_headers() {
    let raw = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\ndiff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-c\n+d\n";
    let f = split_patch(raw);
    assert_eq!(f.len(), 2);
    assert_eq!(
        f[0].header,
        "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n"
    );
    assert!(f[0].hunks[0].ends_with("+b\n\\ No newline at end of file\n"));
    assert_eq!(
        hunk_patch(&f[1], 0).unwrap(),
        "diff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-c\n+d\n"
    );
}

#[test]
fn r21_2_untracked_file_diff_shows_its_lines() {
    let d = repo_with_id("scm-untracked-diff");
    std::fs::write(d.join("n.txt"), "hello\n").unwrap();
    let c = status_of(&d).files[0].clone();
    let raw = file_diff(&d, &c, Section::Untracked).unwrap();
    let parsed = crate::diff::parse(&raw, &[]);
    assert_eq!(parsed[0].added, 1);
    assert!(!hunks_apply(&parsed[0]));
}

#[test]
fn r21_4_r21_5_commit_push_pull_with_a_bare_remote() {
    let d = repo_with_id("scm-push");
    let bare = d.with_extension("bare");
    let _ = std::fs::remove_dir_all(&bare);
    sh(
        &d,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    sh(&d, &["remote", "add", "origin", bare.to_str().unwrap()]);
    assert!(commit(&d, "  ").is_err());
    std::fs::write(d.join("a.txt"), "changed\n").unwrap();
    std::fs::write(d.join("b.txt"), "not staged\n").unwrap();
    stage(&d, &["a.txt".into()]).unwrap();
    let done = commit(&d, "feat: change a\n\nbody").unwrap();
    assert!(done.ends_with(" feat: change a"), "{done}");
    // Only the staged file was committed.
    let st = status_of(&d);
    assert_eq!(letters(&st, Section::Untracked), "Ub.txt");
    assert_eq!(letters(&st, Section::Staged), "");

    // First push: no upstream → -u origin main.
    let plan = push_plan(&d, &st).unwrap();
    assert_eq!(
        plan,
        PushPlan {
            remote: "origin".into(),
            branch: "main".into(),
            set_upstream: true,
            commits: 2
        }
    );
    push(&d, &plan, &[]).unwrap();
    let st = status_of(&d);
    assert_eq!(st.upstream.as_deref(), Some("origin/main"));
    assert_eq!((st.ahead, st.behind), (0, 0));

    // Another clone pushes; pull fast-forwards.
    let other = d.with_extension("other");
    let _ = std::fs::remove_dir_all(&other);
    sh(
        &d,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("c.txt"), "c\n").unwrap();
    sh(&other, &["add", "c.txt"]);
    sh(&other, &["commit", "-qm", "c"]);
    sh(&other, &["push", "-q"]);
    sh(&d, &["fetch", "-q"]);
    assert_eq!(status_of(&d).behind, 1);
    pull(&d, &[]).unwrap();
    assert!(d.join("c.txt").exists());

    // A later commit: plan without -u, one commit.
    std::fs::write(d.join("a.txt"), "again\n").unwrap();
    stage(&d, &["a.txt".into()]).unwrap();
    commit(&d, "fix: again").unwrap();
    let plan = push_plan(&d, &status_of(&d)).unwrap();
    assert!(!plan.set_upstream);
    assert_eq!(plan.commits, 1);
}

#[test]
fn r21_5_remote_choice_and_pr_base() {
    let v = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        choose_remote(&v(&["fork", "origin"])).as_deref(),
        Some("origin")
    );
    assert_eq!(choose_remote(&v(&["fork"])).as_deref(), Some("fork"));
    assert_eq!(choose_remote(&v(&["a", "b"])), None);
    assert_eq!(choose_remote(&[]), None);
    assert_eq!(pr_base(Some("origin/main")), "main");
    assert_eq!(pr_base(Some("main")), "main");
    assert_eq!(pr_base(Some("feat/x")), "feat/x");
    assert_eq!(pr_base(None), "main");
}

#[test]
fn r21_6_create_pr_runs_gh_and_reports_a_missing_gh() {
    let d = repo_with_id("scm-gh");
    let log = d.join("gh.log");
    let gh = d.join("fake-gh");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" \"cfg=$GH_CONFIG_DIR\" > {}\necho https://example.test/pr/7\n",
            log.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A fork in another test thread can hold the script's write fd until
    // its exec: ETXTBSY for a moment. Retry.
    let url = (0..100)
        .find_map(|_| {
            match create_pr(
                &d,
                gh.to_str().unwrap(),
                "feat",
                "main",
                "Title",
                "Body\nmore",
                &[("GH_CONFIG_DIR".into(), "/g/a".into())],
            ) {
                Err(e) if e.message.contains("Text file busy") => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    None
                }
                r => Some(r),
            }
        })
        .unwrap()
        .unwrap();
    assert_eq!(url, "https://example.test/pr/7");
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "pr\ncreate\n--head\nfeat\n--base\nmain\n--title\nTitle\n--body\nBody\nmore\ncfg=/g/a\n",
        "R23.3: gh runs with the Workspace env"
    );
    let e = create_pr(&d, "/nonexistent/gh", "feat", "main", "T", "", &[]).unwrap_err();
    assert!(e.message.contains("not installed"), "{}", e.message);
    assert!(create_pr(&d, gh.to_str().unwrap(), "feat", "main", " ", "", &[]).is_err());
}

#[test]
fn r21_7_draft_prompt_holds_rules_log_and_diff() {
    let i = DraftInput {
        diff: "diff --git a/x b/x\n+new\n".into(),
        rules: vec![("CLAUDE.md".into(), "Use Conventional Commits.".into())],
        log: vec!["feat(R1): one".into()],
        ..Default::default()
    };
    let p = draft_prompt(DraftKind::Commit, &i);
    assert!(p.starts_with("Write a git commit message"));
    assert!(p.contains("## CLAUDE.md\n\nUse Conventional Commits.\n"));
    assert!(p.contains("- feat(R1): one\n"));
    assert!(p.contains("```diff\ndiff --git a/x b/x\n+new\n```"));
    assert!(!p.contains("was cut"));

    let big = DraftInput {
        diff: "é".repeat(40_000),
        branch: Some("feat".into()),
        base: Some("main".into()),
        commits: vec!["wip".into()],
        ..Default::default()
    };
    let p = draft_prompt(DraftKind::Pr, &big);
    assert!(p.contains("merging branch `feat` into `main`"));
    assert!(p.contains("## Commits on this branch\n\n- wip\n"));
    assert!(p.contains("(The diff was cut at 60000 bytes of 80000.)"));
}

#[test]
fn r21_7_parse_draft_strips_fences_and_splits_pr() {
    assert_eq!(
        parse_draft(DraftKind::Commit, "\n```\nfeat: x\n\nbody\n```\n"),
        ("feat: x\n\nbody".into(), String::new())
    );
    assert_eq!(
        parse_draft(DraftKind::Commit, "```text\nfix: y\n```"),
        ("fix: y".into(), String::new())
    );
    assert_eq!(
        parse_draft(DraftKind::Pr, "# Add refunds\n\n## Summary\n- a\n"),
        ("Add refunds".into(), "## Summary\n- a".into())
    );
    assert_eq!(
        parse_draft(DraftKind::Pr, "Title: Only a title"),
        ("Only a title".into(), String::new())
    );
}

#[test]
fn r21_7_gather_reads_rules_and_staged_diff() {
    let d = repo_with_id("scm-gather");
    std::fs::write(d.join("CLAUDE.md"), "rules here\n").unwrap();
    assert!(gather_draft(&d, DraftKind::Commit, None)
        .unwrap_err()
        .message
        .contains("nothing is staged"));
    std::fs::write(d.join("a.txt"), "x\n").unwrap();
    stage(&d, &["a.txt".into()]).unwrap();
    let i = gather_draft(&d, DraftKind::Commit, None).unwrap();
    assert_eq!(i.rules, vec![("CLAUDE.md".into(), "rules here\n".into())]);
    assert_eq!(i.log, vec!["init".to_string()]);
    assert!(i.diff.contains("+x"));

    sh(&d, &["commit", "-qm", "feat: x"]);
    sh(&d, &["checkout", "-qb", "feat"]);
    std::fs::write(d.join("b.txt"), "b\n").unwrap();
    sh(&d, &["add", "b.txt"]);
    sh(&d, &["commit", "-qm", "feat: b"]);
    let i = gather_draft(&d, DraftKind::Pr, Some("main")).unwrap();
    assert_eq!(i.commits, vec!["feat: b".to_string()]);
    assert!(i.diff.contains("+b") && !i.diff.contains("+x"));
    assert_eq!(i.branch.as_deref(), Some("feat"));
}

#[test]
fn r21_7_run_draft_pipes_the_prompt_and_stops_on_timeout_or_cancel() {
    let d = std::env::temp_dir();
    let no = AtomicBool::new(false);
    let out = run_draft(
        "tr a-z A-Z",
        &d,
        "feat: hi",
        Duration::from_secs(5),
        &no,
        &[],
    )
    .unwrap();
    let env = [("GH_CONFIG_DIR".to_string(), "/g/a".to_string())];
    let cfg = run_draft(
        "printf %s \"$GH_CONFIG_DIR\"",
        &d,
        "",
        Duration::from_secs(5),
        &no,
        &env,
    )
    .unwrap();
    assert_eq!(
        cfg, "/g/a",
        "R23.3: the Draft command gets the Workspace env"
    );
    assert_eq!(out, "FEAT: HI");
    let e = run_draft("sleep 5", &d, "", Duration::from_millis(200), &no, &[]).unwrap_err();
    assert!(e.message.contains("timed out"));
    let yes = AtomicBool::new(true);
    let e = run_draft("sleep 5", &d, "", Duration::from_secs(5), &yes, &[]).unwrap_err();
    assert!(e.message.contains("cancelled"));
    let e = run_draft(
        "echo oops >&2; exit 3",
        &d,
        "",
        Duration::from_secs(5),
        &no,
        &[],
    )
    .unwrap_err();
    assert!(e.message.contains("oops"), "{}", e.message);
    let e = run_draft("cat >/dev/null", &d, "x", Duration::from_secs(5), &no, &[]).unwrap_err();
    assert!(e.message.contains("printed nothing"));
}

fn bare_remote(d: &Path) -> PathBuf {
    let bare = d.with_extension("bare");
    let _ = std::fs::remove_dir_all(&bare);
    sh(
        d,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    sh(d, &["remote", "add", "origin", bare.to_str().unwrap()]);
    bare
}

fn head_subject(d: &Path) -> String {
    crate::git::run(d, &["log", "-1", "--format=%s"])
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn r121_a_amend_keeps_subject_commit_all_takes_untracked_undo_restores_staged() {
    let d = repo_with_id("scm-kinds");
    std::fs::write(d.join("a.txt"), "changed\n").unwrap();
    stage(&d, &["a.txt".into()]).unwrap();
    commit(&d, "second\n\nbody").unwrap();
    // Amend with an empty message keeps the subject and takes the staged file.
    std::fs::write(d.join("b.txt"), "b\n").unwrap();
    stage(&d, &["b.txt".into()]).unwrap();
    let done = commit_kind(&d, "  ", CommitKind::Amend).unwrap();
    assert!(done.ends_with(" second"), "{done}");
    assert!(
        crate::git::run(&d, &["show", "--stat", "--format=", "HEAD"])
            .unwrap()
            .contains("b.txt")
    );
    // Amend with a message replaces it.
    commit_kind(&d, "renamed", CommitKind::Amend).unwrap();
    assert_eq!(head_subject(&d), "renamed");
    // Commit All stages the untracked file too.
    std::fs::write(d.join("a.txt"), "again\n").unwrap();
    std::fs::write(d.join("new.txt"), "new\n").unwrap();
    commit_kind(&d, "all", CommitKind::All).unwrap();
    let shown = crate::git::run(&d, &["show", "--stat", "--format=", "HEAD"]).unwrap();
    assert!(
        shown.contains("new.txt") && shown.contains("a.txt"),
        "{shown}"
    );
    assert!(status_of(&d).files.is_empty());
    assert!(commit_kind(&d, "", CommitKind::All).is_err());
    // Undo: HEAD back at "renamed", the files staged, the message returned.
    let msg = undo_last_commit(&d).unwrap();
    assert_eq!(msg, "all");
    assert_eq!(head_subject(&d), "renamed");
    let st = status_of(&d);
    assert_eq!(letters(&st, Section::Staged), "Ma.txt Anew.txt");
}

#[test]
fn r121_a_undo_root_commit_empties_head() {
    let d = repo_with_id("scm-root");
    let msg = undo_last_commit(&d).unwrap();
    assert_eq!(msg, "init");
    let st = status_of(&d);
    assert!(st.initial);
    assert_eq!(letters(&st, Section::Staged), "Aa.txt");
    assert!(undo_last_commit(&d).is_err(), "nothing left to undo");
}

#[test]
fn r121_a_amend_and_undo_refused_when_pushed() {
    let d = repo_with_id("scm-pushed");
    let _bare = bare_remote(&d);
    assert!(!head_pushed(&d));
    sh(&d, &["push", "-q", "-u", "origin", "main"]);
    assert!(head_pushed(&d));
    let e = commit_kind(&d, "x", CommitKind::Amend).unwrap_err();
    assert!(e.message.contains("already pushed"), "{}", e.message);
    assert!(undo_last_commit(&d).is_err());
    assert_eq!(head_subject(&d), "init");
}

#[test]
fn r121_f_sync_pulls_then_pushes_and_stops_on_a_failed_pull() {
    let d = repo_with_id("scm-sync");
    let bare = bare_remote(&d);
    sh(&d, &["push", "-q", "-u", "origin", "main"]);
    let other = d.with_extension("other");
    let _ = std::fs::remove_dir_all(&other);
    sh(
        &d,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    // The remote gains c.txt; we gain a local commit.
    std::fs::write(other.join("c.txt"), "c\n").unwrap();
    sh(&other, &["add", "c.txt"]);
    sh(&other, &["commit", "-qm", "c"]);
    sh(&other, &["push", "-q"]);
    std::fs::write(d.join("a.txt"), "local\n").unwrap();
    sh(&d, &["commit", "-qam", "local"]);
    let st = status_of(&d);
    assert_eq!(fetch_remote(&d, &st).unwrap(), "origin");
    fetch(&d, "origin", &[]).unwrap();
    let st = status_of(&d);
    assert_eq!((st.ahead, st.behind), (1, 1));
    // Diverged: the pull fails, nothing is pushed.
    let before = crate::git::run(&bare, &["rev-parse", "main"]).unwrap();
    let e = sync(&d, &[]).unwrap_err();
    assert!(e.message.starts_with("git pull"), "{}", e.message);
    assert_eq!(
        crate::git::run(&bare, &["rev-parse", "main"]).unwrap(),
        before
    );

    // Behind only: sync fast-forwards and pushes nothing.
    sh(&d, &["reset", "-q", "--hard", "origin/main"]);
    std::fs::write(other.join("c.txt"), "c2\n").unwrap();
    sh(&other, &["commit", "-qam", "c2"]);
    sh(&other, &["push", "-q"]);
    fetch(&d, "origin", &[]).unwrap();
    assert_eq!((status_of(&d).ahead, status_of(&d).behind), (0, 1));
    let plan = sync(&d, &[]).unwrap();
    assert_eq!(plan.commits, 0);
    assert_eq!(std::fs::read_to_string(d.join("c.txt")).unwrap(), "c2\n");
    // Behind and ahead through a clean fast-forward: push after the pull.
    std::fs::write(d.join("a.txt"), "ahead\n").unwrap();
    sh(&d, &["commit", "-qam", "ahead"]);
    let plan = sync(&d, &[]).unwrap();
    assert_eq!(plan.commits, 1);
    assert_eq!(
        crate::git::run(&bare, &["log", "-1", "--format=%s", "main"])
            .unwrap()
            .trim(),
        "ahead"
    );
}

#[test]
fn r120_d_discard_many_trashes_untracked_and_reports_failures() {
    let d = repo_with_id("scm-discard");
    std::fs::write(d.join("a.txt"), "changed\n").unwrap();
    std::fs::write(d.join("u1.txt"), "1\n").unwrap();
    std::fs::write(d.join("u2.txt"), "2\n").unwrap();
    let st = status_of(&d);
    let tracked: Vec<Change> = st
        .files
        .iter()
        .filter(|c| !c.untracked())
        .cloned()
        .collect();
    let trash_dir = d.with_extension("trash");
    let _ = std::fs::remove_dir_all(&trash_dir);
    std::fs::create_dir_all(&trash_dir).unwrap();
    let td = trash_dir.clone();
    let trash = move |p: &Path| -> std::io::Result<()> {
        if p.ends_with("u2.txt") {
            return Err(std::io::Error::other("no trash here"));
        }
        std::fs::rename(p, td.join(p.file_name().unwrap()))
    };
    let failed = discard_many(
        &d,
        &tracked,
        &["u1.txt".to_string(), "u2.txt".to_string()],
        &trash,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(d.join("a.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
    assert!(trash_dir.join("u1.txt").exists() && !d.join("u1.txt").exists());
    assert_eq!(
        failed,
        vec![("u2.txt".to_string(), "no trash here".to_string())]
    );
    assert!(discard_many(&d, &[], &["../x".to_string()], &trash).is_err());
    delete_untracked(&d, &["u2.txt".to_string()]).unwrap();
    assert!(!d.join("u2.txt").exists());
    assert!(delete_untracked(&d, &["../x".to_string()]).is_err());
}

#[test]
fn r120_e_gitignore_add_appends_rooted_path() {
    let d = repo_with_id("scm-ignore");
    gitignore_add(&d, "app.log").unwrap();
    assert_eq!(
        std::fs::read_to_string(d.join(".gitignore")).unwrap(),
        "/app.log\n"
    );
    std::fs::write(d.join(".gitignore"), "target").unwrap();
    gitignore_add(&d, "src/x.log").unwrap();
    assert_eq!(
        std::fs::read_to_string(d.join(".gitignore")).unwrap(),
        "target\n/src/x.log\n"
    );
    assert!(gitignore_add(&d, "../x").is_err());
}

/// ten numbered lines
fn ten(changed: &[(usize, &str)]) -> String {
    (1..=10)
        .map(|i| {
            changed
                .iter()
                .find(|(n, _)| *n == i)
                .map_or(format!("l{i}"), |(_, t)| t.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn r122_b_lines_patch_stage_unstage_discard() {
    let d = repo_with_id("scm-lines");
    std::fs::write(d.join("t.txt"), ten(&[])).unwrap();
    sh(&d, &["add", "t.txt"]);
    sh(&d, &["commit", "-qm", "ten"]);
    // Hunk 1: l1, l2 → A1, A2; hunk 2: l10 → Z.
    let edited = ten(&[(1, "A1"), (2, "A2"), (10, "Z")]);
    std::fs::write(d.join("t.txt"), &edited).unwrap();
    let change = status_of(&d)
        .files
        .iter()
        .find(|c| c.path == "t.txt")
        .unwrap()
        .clone();
    let raw = file_diff(&d, &change, Section::Unstaged).unwrap();
    let f = split_patch(&raw).remove(0);
    assert_eq!(f.hunks.len(), 2);
    let parsed = crate::diff::parse(&raw, &[]);
    // The second added line of hunk 1.
    let idx = parsed[0].hunks[0]
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == crate::diff::LineKind::Added)
        .nth(1)
        .unwrap()
        .0;
    assert_eq!(parsed[0].hunks[0].lines[idx].text, "A2");
    assert!(lines_patch(&f, &[], false).is_none());

    // Stage: the index holds only A2 (the removals stay unstaged).
    let patch = lines_patch(&f, &[(0, idx)], false).unwrap();
    apply_hunk(&d, &patch, HunkOp::Stage).unwrap();
    let cached = crate::git::run(&d, &["diff", "--cached", "-U0"]).unwrap();
    let added: Vec<&str> = cached
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .collect();
    let removed: Vec<&str> = cached
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .collect();
    assert_eq!(added, vec!["+A2"], "{cached}");
    assert!(removed.is_empty(), "{cached}");

    // Unstage the same line from the staged diff.
    let change = status_of(&d)
        .files
        .iter()
        .find(|c| c.path == "t.txt")
        .unwrap()
        .clone();
    let raw = file_diff(&d, &change, Section::Staged).unwrap();
    let f = split_patch(&raw).remove(0);
    let parsed = crate::diff::parse(&raw, &[]);
    let idx = parsed[0].hunks[0]
        .lines
        .iter()
        .position(|l| l.kind == crate::diff::LineKind::Added)
        .unwrap();
    let patch = lines_patch(&f, &[(0, idx)], true).unwrap();
    apply_hunk(&d, &patch, HunkOp::Unstage).unwrap();
    assert_eq!(crate::git::run(&d, &["diff", "--cached"]).unwrap(), "");

    // Discard a selected removed line: only l1 comes back.
    let change = status_of(&d)
        .files
        .iter()
        .find(|c| c.path == "t.txt")
        .unwrap()
        .clone();
    let raw = file_diff(&d, &change, Section::Unstaged).unwrap();
    let f = split_patch(&raw).remove(0);
    let parsed = crate::diff::parse(&raw, &[]);
    let idx = parsed[0].hunks[0]
        .lines
        .iter()
        .position(|l| l.kind == crate::diff::LineKind::Removed)
        .unwrap();
    assert_eq!(parsed[0].hunks[0].lines[idx].text, "l1");
    let patch = lines_patch(&f, &[(0, idx)], true).unwrap();
    apply_hunk(&d, &patch, HunkOp::Discard).unwrap();
    assert_eq!(
        std::fs::read_to_string(d.join("t.txt")).unwrap(),
        "l1\nA1\nA2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nZ\n"
    );
}

#[test]
fn r123_b_quick_diff_none_for_untracked_and_outside_a_repo() {
    let d = repo_with_id("scm-quick");
    std::fs::write(d.join("new.txt"), "x\n").unwrap();
    assert_eq!(quick_diff(&d.join("new.txt")).unwrap(), None);
    assert_eq!(quick_diff(&d.join("a.txt")).unwrap().as_deref(), Some(""));
    std::fs::write(d.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    let u0 = quick_diff(&d.join("a.txt")).unwrap().unwrap();
    assert!(u0.contains("@@ -2 +2 @@"), "{u0}");
    let (f, i) = hunk_at_line(&d.join("a.txt"), 2).unwrap().unwrap();
    assert_eq!(i, 0);
    assert!(f.hunks[0].contains("+TWO"));
    assert!(hunk_at_line(&d.join("a.txt"), 40).unwrap().is_none());
    let out = std::env::temp_dir().join(format!("rade-scm-outside-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("f.txt"), "x\n").unwrap();
    assert_eq!(quick_diff(&out.join("f.txt")).unwrap(), None);
    assert_eq!(quick_diff(&out.join("missing/f.txt")).unwrap(), None);
}

#[test]
fn snapshot_counts_per_side_and_head_pushed() {
    let d = repo_with_id("scm-snap");
    std::fs::write(d.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    std::fs::write(d.join("b.txt"), "b1\nb2\n").unwrap();
    sh(&d, &["add", "b.txt"]);
    std::fs::write(d.join("b.txt"), "b1\nb2\nb3\n").unwrap();
    std::fs::write(d.join("u.txt"), "u1\nu2\nu3").unwrap();
    let s = snapshot(&d, "main").unwrap();
    assert_eq!(s.staged.get("b.txt"), Some(&(2, 0)));
    assert_eq!(s.unstaged.get("a.txt"), Some(&(1, 0)));
    assert_eq!(s.unstaged.get("b.txt"), Some(&(1, 0)));
    assert_eq!(s.untracked_lines.get("u.txt"), Some(&3));
    assert_eq!(s.head.as_ref().map(|h| h.1.as_str()), Some("init"));
    assert!(!s.head_pushed);
    assert_eq!(s.ahead_base, 0);
    let _bare = bare_remote(&d);
    sh(&d, &["push", "-q", "-u", "origin", "main"]);
    assert!(snapshot(&d, "main").unwrap().head_pushed);
}

#[test]
fn recent_messages_newest_first() {
    let d = repo_with_id("scm-recent");
    std::fs::write(d.join("a.txt"), "2\n").unwrap();
    sh(&d, &["commit", "-qam", "second\n\nwith body"]);
    let m = recent_messages(&d, 20);
    assert_eq!(
        m,
        vec!["second\n\nwith body".to_string(), "init".to_string()]
    );
    assert_eq!(recent_messages(&d, 1).len(), 1);
}
