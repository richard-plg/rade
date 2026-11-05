use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn s(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn tmp(name: &str) -> PathBuf {
    let d = crate::paths::outside_temp_dir().join(format!("rade-pr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An executable `gh` that runs `body` (sh).
fn fake_gh(dir: &Path, body: &str) -> String {
    let p = dir.join("gh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

const OPEN: &str = r#"[
 {"number":480,"title":"old","url":"https://github.com/o/r/pull/480","state":"OPEN","isDraft":false,
  "baseRefName":"main","headRefOid":"aaaaaaa1111","reviewDecision":"","mergeStateStatus":"CLEAN","statusCheckRollup":[]},
 {"number":482,"title":"Add the thing","url":"https://github.com/o/r/pull/482","state":"OPEN","isDraft":false,
  "baseRefName":"main","headRefOid":"bbbbbbb2222","reviewDecision":"CHANGES_REQUESTED","mergeStateStatus":"BLOCKED",
  "statusCheckRollup":[
   {"__typename":"CheckRun","name":"build","workflowName":"CI","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://github.com/o/r/actions/runs/1/job/10"},
   {"__typename":"CheckRun","name":"test","workflowName":"CI","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://github.com/o/r/actions/runs/1/job/11"},
   {"__typename":"StatusContext","context":"lint","state":"ERROR","targetUrl":"https://ci.example/lint"},
   {"__typename":"CheckRun","name":"slow","workflowName":"CI","status":"IN_PROGRESS","conclusion":"","detailsUrl":"https://github.com/o/r/actions/runs/1/job/12"}
  ]},
 {"number":470,"title":"merged one","url":"u","state":"MERGED","isDraft":false,"baseRefName":"main","headRefOid":"c","reviewDecision":null,"statusCheckRollup":null}
]"#;

fn status(json: &str, local: &str) -> PrStatus {
    status_from_json(json, local, Some(1000)).unwrap().unwrap()
}

fn pr_json(
    number: u64,
    state: &str,
    head: &str,
    review: &str,
    checks: &str,
    draft: bool,
) -> String {
    format!(
        r#"[{{"number":{number},"title":"t","url":"u","state":"{state}","isDraft":{draft},"baseRefName":"main","headRefOid":"{head}","reviewDecision":"{review}","statusCheckRollup":[{checks}]}}]"#
    )
}

fn run(name: &str, conclusion: &str) -> String {
    format!(
        r#"{{"__typename":"CheckRun","name":"{name}","workflowName":"CI","status":"COMPLETED","conclusion":"{conclusion}","detailsUrl":"d"}}"#
    )
}

fn st(checks: &str, head: &str) -> PrStatus {
    status(&pr_json(7, "OPEN", head, "", checks, false), head)
}

#[test]
fn r146_a_args_and_choice() {
    let a = gh_list_args("feat/x");
    assert_eq!(&a[..4], ["pr", "list", "--head", "feat/x"]);
    assert!(a.contains(&"all".to_string()) && a.contains(&"5".to_string()));
    assert!(a.last().unwrap().ends_with("statusCheckRollup"));
    // The newest open (highest number), whatever the order.
    let prs = parse(OPEN).unwrap();
    assert_eq!(choose(&prs).unwrap().number, 482);
    // Else the newest merged, else the newest closed.
    let j = r#"[{"number":3,"state":"CLOSED"},{"number":5,"state":"CLOSED"},{"number":1,"state":"MERGED"},{"number":2,"state":"MERGED"}]"#;
    assert_eq!(choose(&parse(j).unwrap()).unwrap().number, 2);
    let j = r#"[{"number":3,"state":"CLOSED"},{"number":5,"state":"CLOSED"}]"#;
    assert_eq!(choose(&parse(j).unwrap()).unwrap().number, 5);
    assert!(choose(&parse("[]").unwrap()).is_none());
    assert!(parse("").unwrap().is_empty());
    assert!(parse("not json").is_err());
}

#[test]
fn r146_a_checks_rollup_and_status() {
    let p = status(OPEN, "bbbbbbb2222");
    assert_eq!(p.number, 482);
    assert_eq!(p.checks, Checks::Failing);
    assert_eq!(p.failing, vec!["test", "lint"]);
    assert_eq!(p.failed[0].workflow, "CI");
    assert_eq!(p.failed[1].url, "https://ci.example/lint");
    assert_eq!(p.review, Some(Review::ChangesRequested));
    assert!(!p.stale);
    assert_eq!(p.head, "bbbbbbb2222");
    // Stale: the PR's head is not the Worktree's HEAD; unknown HEAD is not.
    assert!(status(OPEN, "ccccccc3333").stale);
    assert!(!status(OPEN, "").stale);
    // Rollup order: failing > pending > passing > none.
    assert_eq!(st(&run("a", "SUCCESS"), "h").checks, Checks::Passing);
    assert_eq!(
        st(
            &format!("{},{}", run("a", "SUCCESS"), run("b", "SKIPPED")),
            "h"
        )
        .checks,
        Checks::Passing
    );
    assert_eq!(st("", "h").checks, Checks::None);
    let pending = r#"{"__typename":"CheckRun","name":"p","status":"QUEUED","conclusion":""}"#;
    assert_eq!(st(pending, "h").checks, Checks::Pending);
    assert_eq!(
        st(&format!("{pending},{}", run("b", "SUCCESS")), "h").checks,
        Checks::Pending
    );
    assert_eq!(
        st(&format!("{pending},{}", run("b", "CANCELLED")), "h").checks,
        Checks::Failing
    );
    for c in [
        "FAILURE",
        "TIMED_OUT",
        "CANCELLED",
        "ACTION_REQUIRED",
        "STARTUP_FAILURE",
    ] {
        assert_eq!(st(&run("x", c), "h").checks, Checks::Failing, "{c}");
    }
    for c in ["NEUTRAL", "SKIPPED", "STALE", ""] {
        assert_eq!(st(&run("x", c), "h").checks, Checks::Passing, "{c}");
    }
    // StatusContext states.
    let ctx = |state: &str| {
        format!(r#"{{"__typename":"StatusContext","context":"c","state":"{state}"}}"#)
    };
    assert_eq!(st(&ctx("SUCCESS"), "h").checks, Checks::Passing);
    assert_eq!(st(&ctx("FAILURE"), "h").checks, Checks::Failing);
    assert_eq!(st(&ctx("PENDING"), "h").checks, Checks::Pending);
    assert_eq!(st(&ctx("EXPECTED"), "h").checks, Checks::Pending);
    // Lenient about missing fields.
    let p = status("[{\"number\":9}]", "x");
    assert_eq!(
        (p.state, p.checks, p.review),
        (PrState::Open, Checks::None, None)
    );
    // A merged PR with a null rollup.
    let m = status(&pr_json(5, "MERGED", "h", "APPROVED", "", false), "h");
    assert_eq!(
        (m.state, m.review),
        (PrState::Merged, Some(Review::Approved))
    );
}

#[test]
fn r146_a_chip_tooltip_cli_and_kpi_texts() {
    let ok = st(&run("a", "SUCCESS"), "h");
    let bad = st(&run("test", "FAILURE"), "h");
    let wait = st(
        r#"{"__typename":"CheckRun","name":"p","status":"IN_PROGRESS"}"#,
        "h",
    );
    let none = st("", "h");
    let c = |p: &PrStatus| (chip(p).text, chip(p).tone);
    assert_eq!(c(&ok), ("#7 \u{2713}".into(), Tone::Ok));
    assert_eq!(c(&bad), ("#7 \u{2717}".into(), Tone::Error));
    assert_eq!(c(&wait), ("#7 \u{25cf}".into(), Tone::Warn));
    assert_eq!(c(&none), ("#7".into(), Tone::Fg3));
    let merged = status(&pr_json(7, "MERGED", "h", "", "", false), "h");
    let closed = status(&pr_json(7, "CLOSED", "h", "", "", false), "h");
    assert_eq!(c(&merged), ("#7 merged".into(), Tone::Fg3));
    assert_eq!(c(&closed), ("#7 closed".into(), Tone::Fg3));
    let draft = status(
        &pr_json(7, "OPEN", "h", "", &run("a", "FAILURE"), true),
        "h",
    );
    assert_eq!(chip(&draft).text, "draft #7 \u{2717}");
    assert_eq!(
        chip_recorded(482),
        Chip {
            text: "#482".into(),
            tone: Tone::Fg3
        }
    );
    assert_eq!(meta(&ok), "PR #7 \u{2713}");

    let p = status(OPEN, "bbbbbbb2222");
    assert_eq!(
        cli_line(&p),
        "#482 open \u{b7} checks failing (test, lint) \u{b7} changes requested"
    );
    assert_eq!(cli_line_none("feat/x"), "no pull request for feat/x");
    let stale = status(OPEN, "other");
    assert!(cli_line(&stale).ends_with("checks for an older push"));
    let tip = tooltip(&stale);
    assert!(tip.starts_with("#482 Add the thing\nopen"));
    assert!(tip.contains("review: changes requested"));
    assert!(tip.contains("checks for an older push"));
    assert!(!tooltip(&p).contains("older push"));

    // Project home.
    assert_eq!(
        kpi([Some(&ok), None, Some(&merged)]),
        Kpi {
            value: "1".into(),
            sub: "#7 checks passing".into()
        }
    );
    assert_eq!(kpi([Some(&bad)]).sub, "#7 checks failing");
    assert_eq!(kpi([Some(&ok), Some(&bad), Some(&wait)]).sub, "1 failing");
    assert_eq!(kpi([Some(&ok), Some(&wait), Some(&ok)]).sub, "1 pending");
    assert_eq!(kpi([Some(&ok), Some(&ok)]).sub, "2 passing");
    assert_eq!(
        kpi([None, Some(&merged)]),
        Kpi {
            value: "0".into(),
            sub: String::new()
        }
    );
}

fn cand(shown: bool, pending: bool, last: Option<u64>) -> Candidate {
    Candidate {
        eligible: true,
        shown,
        pending,
        last_read: last.map(s),
        trigger: false,
    }
}

#[test]
fn r146_b_due_times() {
    let now = s(1000);
    assert_eq!(due_at(&cand(true, false, Some(1000)), now), Some(s(1060)));
    assert_eq!(due_at(&cand(true, true, Some(1000)), now), Some(s(1020)));
    assert_eq!(due_at(&cand(false, false, Some(1000)), now), Some(s(1600)));
    assert_eq!(due_at(&cand(false, true, Some(1000)), now), Some(s(1600)));
    // Never read: now. A trigger (just pushed): now.
    assert_eq!(due_at(&cand(true, false, None), now), Some(now));
    let mut pushed = cand(false, false, Some(999));
    pushed.trigger = true;
    assert_eq!(due_at(&pushed, now), Some(now));
    // No upstream and no recorded PR: never.
    let mut c = cand(true, false, None);
    c.eligible = false;
    c.trigger = true;
    assert_eq!(due_at(&c, now), None);
    // The most overdue due one is picked.
    let items = [
        ("a", cand(true, false, Some(990))),
        ("b", cand(true, false, Some(900))),
        ("c", cand(true, true, Some(500))),
        ("d", c),
    ];
    assert_eq!(next_due(&items, now), Some("c"));
    assert_eq!(next_due(&items[..1], now), None);
}

#[test]
fn r146_b_a_failure_marks_only_that_workspace_down_for_10_minutes() {
    let mut a = Gate::default();
    let mut b = Gate::default();
    assert!(a.start(s(0)));
    // At most one in flight per Workspace.
    assert!(!a.start(s(1)));
    assert!(b.start(s(1)));
    assert!(a.finish_err(s(20), "gh: not logged in\nrun gh auth login"));
    assert!(!a.can_start(s(20)));
    assert!(!a.start(s(619)));
    assert_eq!(a.message(s(100)), Some("gh: not logged in"));
    assert!(!b.can_start(s(20)));
    b.finish_ok();
    assert!(b.can_start(s(21)));
    // After 10 minutes it may run again; a failure then is a new row.
    assert!(a.start(s(620)));
    assert!(a.message(s(620)).is_none());
    assert!(a.finish_err(s(630), "again"));
    // A failure while already down (a forced refresh) is not a second row.
    assert!(!a.finish_err(s(640), "still"));
    a.finish_ok();
    assert!(a.can_start(s(641)));
}

fn failing(head: &str) -> PrStatus {
    status(
        &pr_json(7, "OPEN", head, "", &run("test", "FAILURE"), false),
        head,
    )
}
fn passing(head: &str) -> PrStatus {
    status(
        &pr_json(7, "OPEN", head, "", &run("test", "SUCCESS"), false),
        head,
    )
}

#[test]
fn r146_c_transitions() {
    let mut t = Tracker::default();
    // The first read is silent, even failing.
    assert!(t.feed("w", Some(&failing("h1"))).is_empty());
    // Failing again on the same head: nothing.
    assert!(t.feed("w", Some(&failing("h1"))).is_empty());
    // A new failing head: again.
    let ev = t.feed("w", Some(&failing("h2")));
    assert_eq!(
        ev,
        vec![Event::ChecksFailed {
            number: 7,
            head: "h2".into(),
            jobs: vec!["test".into()]
        }]
    );
    assert_eq!(ev[0].activity(), "#7 checks failed (1 job)");
    let (title, body) = ev[0].notification().unwrap();
    assert_eq!(
        (title.as_str(), body.as_str()),
        ("#7 checks failed", "test")
    );
    assert!(ev[0].offers_send_to_agent());
    assert!(t.feed("w", Some(&failing("h2"))).is_empty());
    // Passing, then failing on the same head: still once per head.
    let ev = t.feed("w", Some(&passing("h2")));
    assert_eq!(ev, vec![Event::ChecksPassed { number: 7 }]);
    assert_eq!(ev[0].activity(), "#7 checks passed");
    assert!(ev[0].notification().is_none());
    assert!(t.feed("w", Some(&passing("h2"))).is_empty());
    // pending -> failing counts as to failing.
    let pend = status(
        &pr_json(
            7,
            "OPEN",
            "h3",
            "",
            r#"{"__typename":"CheckRun","name":"p","status":"QUEUED"}"#,
            false,
        ),
        "h3",
    );
    assert!(t.feed("w", Some(&pend)).is_empty());
    assert_eq!(t.feed("w", Some(&failing("h3"))).len(), 1);
    // Merged: an Activity row and the Toast, nothing else.
    let merged = status(
        &pr_json(7, "MERGED", "h3", "", &run("test", "FAILURE"), false),
        "h3",
    );
    let ev = t.feed("w", Some(&merged));
    assert_eq!(ev, vec![Event::Merged { number: 7 }]);
    assert_eq!(ev[0].toast().as_deref(), Some("#7 merged"));
    assert_eq!(ev[0].activity(), "#7 merged");
    // Review -> changes requested: an Activity row and a Notification.
    let mut t = Tracker::default();
    let ok = |r: &str| {
        status(
            &pr_json(7, "OPEN", "h", r, &run("a", "SUCCESS"), false),
            "h",
        )
    };
    assert!(t.feed("w", Some(&ok("REVIEW_REQUIRED"))).is_empty());
    let ev = t.feed("w", Some(&ok("CHANGES_REQUESTED")));
    assert_eq!(ev, vec![Event::ChangesRequested { number: 7 }]);
    assert_eq!(ev[0].activity(), "#7 changes requested");
    assert!(ev[0].notification().is_some());
    assert!(t.feed("w", Some(&ok("CHANGES_REQUESTED"))).is_empty());
    // Another Worktree and another PR number start silent.
    assert!(t.feed("x", Some(&failing("h"))).is_empty());
    assert!(t.feed("w", None).is_empty());
    assert!(t.feed("w", Some(&failing("h"))).is_empty());
    let two = status(
        &pr_json(
            7,
            "OPEN",
            "h",
            "",
            &format!("{},{}", run("a", "FAILURE"), run("b", "FAILURE")),
            false,
        ),
        "h",
    );
    assert_eq!(
        Event::ChecksFailed {
            number: 7,
            head: "h".into(),
            jobs: two.failing.clone()
        }
        .activity(),
        "#7 checks failed (2 jobs)"
    );
}

#[test]
fn r146_runner_reads_through_gh_and_maps_errors() {
    let d = tmp("runner");
    let env: Vec<(String, String)> = vec![("RADE_TEST_ENV".into(), "yes".into())];
    // Args, cwd, env and the non-interactive switches reach gh.
    let gh = fake_gh(
        &d,
        &format!(
            "[ \"$1 $2 $3 $4\" = 'pr list --head feat' ] && [ \"$RADE_TEST_ENV\" = yes ] && [ \"$GH_PROMPT_DISABLED\" = 1 ] && [ \"$NO_COLOR\" = 1 ] || exit 9\ncat <<'EOF'\n{OPEN}\nEOF"
        ),
    );
    let prs = read(&d, &gh, "feat", &env).unwrap();
    assert_eq!(choose(&prs).unwrap().number, 482);
    // A non-zero exit: gh's first stderr line.
    let gh = fake_gh(
        &d,
        "echo 'HTTP 401: bad credentials' >&2; echo more >&2; exit 1",
    );
    let e = read(&d, &gh, "feat", &[]).unwrap_err();
    assert_eq!(
        (e.kind, e.message.as_str()),
        (ErrorKind::Usage, "HTTP 401: bad credentials")
    );
    // gh missing: the install hint.
    let e = read(&d, "/nonexistent/gh", "feat", &[]).unwrap_err();
    assert!(e.message.contains("not installed"));
    // Over the limit.
    let gh = fake_gh(&d, "sleep 5");
    let e = run_gh(&d, &gh, &gh_list_args("b"), &[], Duration::from_millis(300)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Timeout);
}
