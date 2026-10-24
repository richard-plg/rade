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
