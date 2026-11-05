use super::*;
use crate::pr::{status_from_json, Check};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d =
        crate::paths::outside_temp_dir().join(format!("rade-cilog-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fake_gh(dir: &Path, body: &str) -> String {
    let p = dir.join("gh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

/// A PR at `head` whose failing checks are `(workflow, name, url)`.
fn failing_status(head: &str, local: &str, checks: &[(&str, &str, &str)]) -> PrStatus {
    let items: Vec<String> = checks
        .iter()
        .map(|(w, n, u)| {
            format!(
                r#"{{"__typename":"CheckRun","name":"{n}","workflowName":"{w}","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"{u}"}}"#
            )
        })
        .collect();
    let json = format!(
        r#"[{{"number":482,"title":"Add the thing","url":"u","state":"OPEN","headRefOid":"{head}","statusCheckRollup":[{}]}}]"#,
        items.join(",")
    );
    status_from_json(&json, local, Some(1)).unwrap().unwrap()
}

const JOB: &str = "https://github.com/o/r/actions/runs/123/job/456";

fn check(url: &str) -> Check {
    failing_status("h", "h", &[("CI", "test", url)])
        .failed
        .remove(0)
}

#[test]
fn r147_a_jobs_are_planned_from_details_urls() {
    assert_eq!(actions_job(JOB), Some(("123".into(), "456".into())));
    assert_eq!(
        actions_job("https://github.com/o/r/actions/runs/123/job/456?pr=7#step:3:1"),
        Some(("123".into(), "456".into()))
    );
    assert_eq!(
        plan(&check(JOB)).unwrap(),
        ["run", "view", "123", "--job", "456", "--log-failed"]
    );
    // A run without a job, other hosts, a StatusContext URL, nothing.
    for u in [
        "https://github.com/o/r/actions/runs/123",
        "https://circleci.com/gh/o/r/9",
        "https://github.com/o/r/pull/7/checks",
        "https://github.com/o/r/actions/runs/abc/job/1",
        "",
    ] {
        assert!(plan(&check(u)).is_none(), "{u}");
    }
}

const RAW: &str = "test\tRun tests\t2026-11-05T10:00:00.1234567Z ##[group]Run cargo test\n\
test\tRun tests\t2026-11-05T10:00:00.2000000Z \u{1b}[36;1mcargo test\u{1b}[0m\n\
test\tRun tests\t2026-11-05T10:00:01.0000000Z ##[endgroup]\n\
test\tRun tests\t2026-11-05T10:00:02.0000000Z thread 'main' panicked at caf\u{e9} \u{1f980}\n\
test\tRun tests\t2026-11-05T10:00:03.0000000Z \n\
test\tRun tests\t2026-11-05T10:00:04.0000000Z ##[error]Process completed with exit code 101.\n\
test\tPost\t2026-11-05T10:00:05Z \n\n";

#[test]
fn r147_b_logs_are_cleaned() {
    assert_eq!(
        clean(RAW),
        "cargo test\nthread 'main' panicked at caf\u{e9} \u{1f980}\n\nProcess completed with exit code 101."
    );
    // Lines of other shapes survive.
    assert_eq!(clean("plain\n\ttab\n"), "plain\n\ttab");
    assert_eq!(clean("\u{1b}[31mred\u{1b}[0m"), "red");
}

fn big_log(lines: usize) -> String {
    (1..=lines)
        .map(|i| format!("job\tstep\t2026-11-05T10:00:00.0000000Z line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn r147_b_cuts_at_150_lines_6_jobs_and_24_kb_with_each_cut_stated() {
    let t = tail_lines(&clean(&big_log(962)), MAX_LINES);
    let ls: Vec<&str> = t.lines().collect();
    assert_eq!(ls[0], "(first 812 lines cut)");
    assert_eq!(ls.len(), 151);
    assert_eq!(*ls.last().unwrap(), "line 962");
    assert_eq!(tail_lines("a\nb", 150), "a\nb");

    // 8 failed jobs: 6 shown, the other 2 named.
    let names: Vec<(String, String)> = (1..=8)
        .map(|i| ("CI".to_string(), format!("job{i}")))
        .collect();
    let checks: Vec<(&str, &str, &str)> = names
        .iter()
        .map(|(w, n)| (w.as_str(), n.as_str(), JOB))
        .collect();
    let st = failing_status("abcdef0123", "abcdef0123", &checks);
    let logs: Vec<Option<String>> = (0..8).map(|_| Some(big_log(1000))).collect();
    let f = failures(&st, &logs);
    assert_eq!(f.jobs.len(), 6);
    let p = f.prompt.as_deref().unwrap();
    assert!(
        p.contains("(2 more failed jobs not shown: CI / job7, CI / job8)"),
        "{p}"
    );
    assert!(p.contains("(first 850 lines cut)"));
    assert!(p.starts_with("CI failed on PR #482 \"Add the thing\" at abcdef0.\n"));
    assert!(p.ends_with(&format!("\n{CLOSING}\n")));

    // 24 KB in all: later jobs are cut to what is left, and say so.
    let long: String = (0..150)
        .map(|i| format!("{i} {}\n", "x".repeat(80)))
        .collect();
    let logs: Vec<Option<String>> = (0..6).map(|_| Some(long.clone())).collect();
    let f = failures(&st, &logs);
    let total: usize = f
        .jobs
        .iter()
        .filter_map(|j| j.log.as_ref())
        .map(|l| l.len())
        .sum();
    assert!(total <= MAX_BYTES, "{total}");
    // The note gives the budget that job had, not always 24 KB.
    assert!(f
        .jobs
        .iter()
        .filter_map(|j| j.log.as_deref())
        .any(|l| l.starts_with("(cut to ") && l.contains(" KB)")));
    assert!(!f.jobs[0].log.as_deref().unwrap().starts_with("(cut"));

    // Multi-byte text is cut on char boundaries, whatever the limit.
    let text = "\u{e9}\u{1f980}\u{4e2d}\n".repeat(400);
    for max in [0, 1, 15, 16, 17, 100, 101, 102, 103, 1000] {
        let c = cut_to_bytes(&text, max);
        assert!(std::str::from_utf8(c.as_bytes()).is_ok());
        assert!(
            c.len() <= max.max(text.len().min(20)) || c.len() == text.len(),
            "{max} {}",
            c.len()
        );
    }
    assert_eq!(cut_to_bytes("short", 100), "short");
}

#[test]
fn r147_b_prompt_text() {
    let st = failing_status(
        "abcdef0123",
        "0123456789",
        &[("CI", "test", JOB), ("", "lint", "https://ci.example/lint")],
    );
    let f = failures(
        &st,
        &[
            Some("t\ts\t2026-11-05T10:00:00Z boom with ``` inside\n".into()),
            None,
        ],
    );
    assert!(f.stale);
    assert_eq!((f.number, f.head.as_str()), (482, "abcdef0123"));
    let p = f.prompt.unwrap();
    let want = "These checks ran on an older push (abcdef0); your branch is at 0123456.\n\n\
CI failed on PR #482 \"Add the thing\" at abcdef0.\n\n\
## CI / test\n````\nboom with ``` inside\n````\n\n\
## lint\nfailure: https://ci.example/lint\n\n\
Find the cause, fix it, run the failing step locally if you can, then commit and push.\n";
    assert_eq!(p, want);
    // Not stale: no first line. Job fields as the answer.
    let st = failing_status("abcdef0123", "abcdef0123", &[("CI", "test", JOB)]);
    let f = failures(&st, &[Some("x".into())]);
    assert!(f
        .prompt
        .as_ref()
        .unwrap()
        .starts_with("CI failed on PR #482"));
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["jobs"][0]["workflow"], "CI");
    assert_eq!(v["jobs"][0]["job"], "test");
    assert_eq!(v["jobs"][0]["conclusion"], "failure");
    assert_eq!(v["jobs"][0]["log"], "x");
    assert_eq!(v["stale"], false);
    assert!(v["jobs"][0]["url"].is_string() && v["prompt"].is_string());
}

#[test]
fn r147_c_auto_send() {
    use AgentState::*;
    let one = |a: &mut AutoSend, mode, head, st| a.step(mode, Read::Failing { head }, 1, Some(st));

    // ask never sends.
    let mut a = AutoSend::default();
    assert_eq!(one(&mut a, Mode::Ask, "h1", Waiting), Action::Nothing);
    // Running: wait; then once the Agent waits, one send; same head: nothing.
    let mut a = AutoSend::default();
    assert_eq!(one(&mut a, Mode::Send, "h1", Running), Action::Wait);
    assert_eq!(one(&mut a, Mode::Send, "h1", Running), Action::Wait);
    assert_eq!(one(&mut a, Mode::Send, "h1", Waiting), Action::Send);
    assert_eq!(one(&mut a, Mode::Send, "h1", Done), Action::Nothing);
    // A new failing head: a second send; the 3rd; no 4th until a pass.
    assert_eq!(one(&mut a, Mode::Send, "h2", Done), Action::Send);
    assert_eq!(one(&mut a, Mode::Send, "h3", Waiting), Action::Send);
    assert_eq!(
        a.step(Mode::Send, Read::Other, 1, Some(Waiting)),
        Action::Nothing
    );
    assert_eq!(
        one(&mut a, Mode::Send, "h4", Waiting),
        Action::AskStillFailing
    );
    // The notice once; nothing more.
    assert_eq!(one(&mut a, Mode::Send, "h4", Waiting), Action::Nothing);
    assert_eq!(one(&mut a, Mode::Send, "h5", Waiting), Action::Nothing);
    assert_eq!(still_failing(7), "CI still failing on #7 after 3 tries");
    // A passing read in between resets the count.
    assert_eq!(
        a.step(Mode::Send, Read::Passing, 1, Some(Waiting)),
        Action::Nothing
    );
    assert_eq!(one(&mut a, Mode::Send, "h6", Waiting), Action::Send);
    // Passing between sends keeps the run from reaching 3.
    let mut a = AutoSend::default();
    for (i, h) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        assert_eq!(one(&mut a, Mode::Send, h, Waiting), Action::Send, "{i}");
        a.step(Mode::Send, Read::Passing, 1, Some(Waiting));
    }
    // None or several Agents: ask.
    let mut a = AutoSend::default();
    assert_eq!(
        a.step(Mode::Send, Read::Failing { head: "h" }, 0, None),
        Action::Nothing
    );
    assert_eq!(
        a.step(Mode::Send, Read::Failing { head: "h" }, 2, Some(Waiting)),
        Action::Nothing
    );
    assert_eq!(Mode::parse("send"), Mode::Send);
    assert_eq!(Mode::parse("ask"), Mode::Ask);
    assert_eq!(Mode::parse("nonsense"), Mode::Ask);
}

#[test]
fn r147_runner_fetches_logs_per_job_and_falls_back_to_name_and_url() {
    let d = tmp("fetch");
    // Job 456 has a log, job 789 fails to read, the lint check is no Actions job.
    let gh = fake_gh(
        &d,
        "case \"$*\" in\n\
 'run view 123 --job 456 --log-failed') printf 'test\\tRun\\t2026-11-05T10:00:00Z boom\\n';;\n\
 *) echo 'log not found' >&2; exit 1;;\nesac",
    );
    let st = failing_status(
        "abcdef0123",
        "abcdef0123",
        &[
            ("CI", "test", JOB),
            (
                "CI",
                "slow",
                "https://github.com/o/r/actions/runs/123/job/789",
            ),
            ("", "lint", "https://ci.example/lint"),
        ],
    );
    let f = fetch_failures(&d, &gh, &st, &[]).unwrap();
    assert_eq!(f.jobs[0].log.as_deref(), Some("boom"));
    assert_eq!(f.jobs[1].log, None);
    assert_eq!(f.jobs[2].log, None);
    let p = f.prompt.unwrap();
    assert!(p.contains("## CI / test\n```\nboom\n```"));
    assert!(p.contains("## CI / slow\nfailure: https://github.com/o/r/actions/runs/123/job/789"));
    assert!(p.contains("## lint\nfailure: https://ci.example/lint"));
    // Missing gh: every job falls back, the prompt still exists.
    let f = fetch_failures(&d, "/nonexistent/gh", &st, &[]).unwrap();
    assert!(f.jobs.iter().all(|j| j.log.is_none()) && f.prompt.is_some());
    // Not failing: empty jobs, no prompt.
    let ok = status_from_json(
        r#"[{"number":1,"state":"OPEN","statusCheckRollup":[{"name":"a","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#,
        "",
        None,
    )
    .unwrap()
    .unwrap();
    let f = fetch_failures(&d, &gh, &ok, &[]).unwrap();
    assert!(f.jobs.is_empty() && f.prompt.is_none());
}

#[test]
fn r148_3_log_reading_is_capped_at_90_s_in_all_and_states_it() {
    assert_eq!(log_limit(Duration::ZERO), Some(LOG_LIMIT));
    assert_eq!(
        log_limit(Duration::from_secs(70)),
        Some(Duration::from_secs(20))
    );
    assert_eq!(log_limit(Duration::from_secs(90)), None);
    assert_eq!(log_limit(Duration::from_secs(120)), None);
    let st = failing_status(
        "abcdef0123",
        "abcdef0123",
        &[("CI", "a", JOB), ("CI", "b", JOB), ("CI", "c", JOB)],
    );
    let logs = vec![Some("boom".to_string()), None, None];
    let f = failures_with(&st, &logs, &[false, true, true]);
    let p = f.prompt.unwrap();
    assert!(
        p.contains("(logs not read, the 90 s limit for reading logs was reached, see the URLs: CI / b, CI / c)"),
        "{p}"
    );
    assert!(p.contains("## CI / b\nfailure: "));
    assert!(p.ends_with(&format!("\n{CLOSING}\n")));
}

#[test]
fn r147_b_a_job_without_room_and_the_real_budget_are_stated() {
    assert_eq!(size_text(24 * 1024), "24 KB");
    assert_eq!(size_text(10_000), "9.8 KB");
    assert_eq!(size_text(40), "40 bytes");
    assert!(cut_to_bytes(&"x\n".repeat(500), 300).starts_with("(cut to 300 bytes)\n"));
    let checks: Vec<(&str, String)> = (1..=6).map(|i| ("CI", format!("j{i}"))).collect();
    let refs: Vec<(&str, &str, &str)> = checks.iter().map(|(w, n)| (*w, n.as_str(), JOB)).collect();
    let st = failing_status("abcdef0123", "abcdef0123", &refs);
    // 150 lines of 59 bytes = 8850 bytes a job: the 3rd is cut to what is
    // left of 24 KB, the 4th has < 64 bytes left.
    let log: String = (0..150).map(|_| format!("{}\n", "x".repeat(58))).collect();
    let logs: Vec<Option<String>> = vec![Some(log); 6];
    let f = failures(&st, &logs);
    assert!(f.jobs[2].log.as_deref().unwrap().starts_with("(cut to "));
    assert!(f.jobs[3..].iter().all(|j| j.log.is_none()));
    let p = f.prompt.unwrap();
    assert!(
        p.contains(
            "(logs left out, the 24 KB limit for all logs was reached: CI / j4, CI / j5, CI / j6)"
        ),
        "{p}"
    );
}

#[test]
fn r147_c_a_restored_auto_send_does_not_send_a_head_again() {
    use AgentState::*;
    let fail =
        |a: &mut AutoSend, h| a.step(Mode::Send, Read::Failing { head: h }, 1, Some(Waiting));
    // Relaunch: the last sent head and the count come back from state.json.
    let mut a = AutoSend::restore(Some("h1".into()), 2);
    assert_eq!(fail(&mut a, "h1"), Action::Nothing);
    assert_eq!(fail(&mut a, "h2"), Action::Send);
    assert_eq!((a.sent_head(), a.in_a_row()), (Some("h2"), 3));
    // The cap survives a relaunch: no send, the notice once.
    let mut a = AutoSend::restore(Some("h2".into()), 3);
    assert_eq!(fail(&mut a, "h2"), Action::Nothing);
    assert_eq!(fail(&mut a, "h3"), Action::AskStillFailing);
    assert_eq!(fail(&mut a, "h4"), Action::Nothing);
    a.step(Mode::Send, Read::Passing, 1, Some(Waiting));
    assert_eq!((a.sent_head(), a.in_a_row()), (Some("h2"), 0));
}
