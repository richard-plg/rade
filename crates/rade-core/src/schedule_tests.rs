use super::*;

fn berlin() -> TimeZone {
    TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap()
}

/// `YYYY-MM-DD HH:MM` local in `tz` → Unix s.
fn t(s: &str, tz: &TimeZone) -> i64 {
    parse_at(s, tz).unwrap()
}

/// Unix s → `YYYY-MM-DD HH:MM +HH:MM` in `tz`.
fn show(secs: i64, tz: &TimeZone) -> String {
    let z = Timestamp::from_second(secs).unwrap().to_zoned(tz.clone());
    format!(
        "{} {:02}:{:02} {}",
        z.date(),
        z.hour(),
        z.minute(),
        z.offset()
    )
}

fn next(expr: &str, from: &str) -> Option<String> {
    let tz = berlin();
    Cron::parse(expr)
        .unwrap()
        .next_after(t(from, &tz), &tz)
        .map(|x| show(x, &tz))
}

#[test]
fn r136_a_next_after_cron_days_and_dst() {
    assert_eq!(
        next("*/15 9-17 * * mon-fri", "2026-10-30 17:50").as_deref(),
        Some("2026-11-02 09:00 +01")
    );
    assert_eq!(
        next("0 9 1 * mon", "2026-11-29 12:00").as_deref(),
        Some("2026-11-30 09:00 +01")
    );
    assert_eq!(
        next("0 9 1 * mon", "2026-11-30 10:00").as_deref(),
        Some("2026-12-01 09:00 +01")
    );
    assert_eq!(
        next("@daily", "2026-10-30 23:59").as_deref(),
        Some("2026-10-31 00:00 +01")
    );
    // Spring: 02:30 does not exist → the gap's end, once.
    let tz = berlin();
    let c = Cron::parse("30 2 * * *").unwrap();
    let first = c.next_after(t("2027-03-28 01:00", &tz), &tz).unwrap();
    assert_eq!(show(first, &tz), "2027-03-28 03:00 +02");
    let second = c.next_after(first, &tz).unwrap();
    assert_eq!(show(second, &tz), "2027-03-29 02:30 +02");
    // Autumn: 02:30 happens twice → the earlier, once.
    let from = t("2026-10-25 00:00", &tz);
    let first = c.next_after(from, &tz).unwrap();
    assert_eq!(show(first, &tz), "2026-10-25 02:30 +02");
    let second = c.next_after(first, &tz).unwrap();
    assert_eq!(show(second, &tz), "2026-10-26 02:30 +01");
    // Never.
    assert_eq!(next("0 0 30 2 *", "2026-10-30 00:00"), None);
}

#[test]
fn r136_a_cron_parse_forms_and_errors() {
    assert!(Cron::parse("0 0 * * 7").unwrap().dow & 1 == 1, "7 = Sunday");
    assert!(Cron::parse("5,10-20/5 */2 1 jan-mar sun").is_ok());
    assert!(Cron::parse("@weekly").is_ok());
    let e = Cron::parse("61 * * * *").unwrap_err();
    assert!(e.contains("minute") && e.contains("61"), "{e}");
    let e = Cron::parse("* * * *").unwrap_err();
    assert!(e.contains("5 fields"), "{e}");
    let e = Cron::parse("* * * * fun").unwrap_err();
    assert!(e.contains("day-of-week"), "{e}");
    assert!(Cron::parse("@sometimes").is_err());
    assert!(Cron::parse("*/0 * * * *").is_err());
}

fn sched(tz: &TimeZone) -> Schedule {
    Schedule {
        name: "s".into(),
        workspace: "default".into(),
        project: Some("api".into()),
        link: Some("api".into()),
        dir: None,
        cron: Some("0 * * * *".into()),
        at: None,
        agent: "claude".into(),
        account: None,
        prompt: "go".into(),
        enabled: true,
        done: false,
        catch_up: CatchUp::Skip,
        keep_runs: 3,
        created: 0,
        checked: t("2026-10-30 10:59", tz),
        last_n: 0,
        runs: vec![],
    }
}

#[test]
fn r136_b_due_start_miss_and_once() {
    let tz = berlin();
    let s = sched(&tz);
    let at11 = t("2026-10-30 11:00", &tz);
    assert_eq!(
        s.due(at11 + 20, &tz),
        Due::Start {
            at: at11,
            missed: 0
        }
    );
    let late = t("2026-10-30 14:05", &tz);
    assert_eq!(
        s.due(late, &tz),
        Due::Missed {
            count: 4,
            latest: t("2026-10-30 14:00", &tz)
        }
    );
    let mut once = s.clone();
    once.catch_up = CatchUp::Once;
    assert_eq!(
        once.due(late, &tz),
        Due::Start {
            at: late,
            missed: 4
        }
    );
    // Off, or nothing in the window.
    let mut off = s.clone();
    off.enabled = false;
    assert_eq!(off.due(late, &tz), Due::Nothing);
    assert_eq!(s.due(at11 - 30, &tz), Due::Nothing);

    let mut a = s.clone();
    a.cron = None;
    a.at = Some(t("2026-10-30 11:30", &tz));
    let now = t("2026-10-30 11:31", &tz);
    assert_eq!(
        a.due(now, &tz),
        Due::Start {
            at: a.at.unwrap(),
            missed: 0
        }
    );
    let n = a.next_n();
    a.record(Run::new(n, now, Outcome::Started, None));
    assert!(a.done && !a.enabled);
    assert_eq!(a.due(now + 3600, &tz), Due::Nothing);
    assert_eq!(a.next(now, &tz), None);
}

#[test]
fn r136_b_runs_kept_and_panes_closed() {
    let tz = berlin();
    let mut s = sched(&tz);
    s.keep_runs = 2;
    for i in 0..60 {
        let n = s.next_n();
        let mut r = Run::new(n, i, Outcome::Started, None);
        r.pane = Some(format!("p_{n}"));
        s.record(r);
    }
    assert_eq!(s.runs.len(), MAX_RUNS);
    assert_eq!(s.runs[0].n, 11);
    // p_60, p_59 kept; p_58 busy kept; p_57 finished closed; p_56 closed
    // already; the rest open and finished.
    let status = |p: &str| -> Option<Option<AgentStatus>> {
        match p {
            "p_58" => Some(Some(AgentStatus::Waiting)),
            "p_56" => None,
            "p_57" => Some(Some(AgentStatus::Done)),
            _ => Some(None),
        }
    };
    let close = s.panes_to_close(status);
    assert!(!close.contains(&"p_60".to_string()) && !close.contains(&"p_59".to_string()));
    assert!(
        !close.contains(&"p_58".to_string()),
        "a waiting Agent is busy"
    );
    assert!(close.contains(&"p_57".to_string()) && !close.contains(&"p_56".to_string()));
    assert_eq!(
        s.busy_pane(|_| Some(Some(AgentStatus::Interrupted)))
            .as_deref(),
        Some("p_60")
    );
    assert_eq!(s.busy_pane(|_| Some(Some(AgentStatus::Done))), None);
}

fn ctx_with<'a>(
    tz: &'a TimeZone,
    projects: &'a [(String, Vec<String>)],
    presets: &'a [String],
    names: &'a [String],
    account: &'a dyn Fn(&str, &str) -> Result<()>,
) -> Ctx<'a> {
    Ctx {
        workspace: "acme",
        projects,
        presets,
        names,
        now: t("2026-10-30 12:00", tz),
        tz,
        account,
    }
}

#[test]
fn r136_c_validation_refusals() {
    let tz = berlin();
    let projects = vec![
        (
            "shop".to_string(),
            vec!["api".to_string(), "web".to_string()],
        ),
        ("solo".to_string(), vec!["one".to_string()]),
    ];
    let presets = vec!["claude".to_string(), "codex".to_string()];
    let names = vec!["used".to_string()];
    let account = |_agent: &str, name: &str| -> Result<()> {
        match name {
            "acme-work" => Ok(()),
            "personal" => Err(Error::forbidden(
                "account 'personal' belongs to workspace 'personal'",
            )),
            o => Err(Error::usage(format!("no claude account '{o}'"))),
        }
    };
    let ctx = ctx_with(&tz, &projects, &presets, &names, &account);
    let good = Spec {
        name: Some("nightly".into()),
        project: Some("solo".into()),
        cron: Some("0 2 * * *".into()),
        prompt: Some("check deps".into()),
        ..Default::default()
    };
    let s = create(&good, &ctx).unwrap();
    assert_eq!(s.link.as_deref(), Some("one"), "a Project's only link");
    assert_eq!(s.checked, ctx.now);

    use crate::rpc::ErrorKind::*;
    let dir = crate::paths::outside_temp_dir().join(format!("rade-sched-{}", std::process::id()));
    let cases: Vec<(Spec, crate::rpc::ErrorKind, &str)> = vec![
        (
            Spec {
                project: Some("shop".into()),
                ..good.clone()
            },
            Usage,
            "api, web",
        ),
        (
            Spec {
                project: Some("nope".into()),
                ..good.clone()
            },
            NotFound,
            "nope",
        ),
        (
            Spec {
                project: None,
                dir: Some(dir.join("missing").display().to_string()),
                ..good.clone()
            },
            Usage,
            "--dir",
        ),
        (
            Spec {
                project: None,
                dir: Some("rel/path".into()),
                ..good.clone()
            },
            Usage,
            "absolute",
        ),
        (
            Spec {
                agent: Some("gemini".into()),
                ..good.clone()
            },
            Usage,
            "gemini",
        ),
        (
            Spec {
                cron: Some("61 * * * *".into()),
                ..good.clone()
            },
            Usage,
            "minute",
        ),
        (
            Spec {
                cron: None,
                at: Some("2026-10-29 09:00".into()),
                ..good.clone()
            },
            Usage,
            "past",
        ),
        (
            Spec {
                name: Some("used".into()),
                ..good.clone()
            },
            Usage,
            "already exists",
        ),
        (
            Spec {
                name: Some("Bad Name".into()),
                ..good.clone()
            },
            Usage,
            "name",
        ),
        (
            Spec {
                account: Some(Some("ghost".into())),
                ..good.clone()
            },
            Usage,
            "ghost",
        ),
        (
            Spec {
                account: Some(Some("personal".into())),
                ..good.clone()
            },
            Forbidden,
            "personal",
        ),
        (
            Spec {
                prompt: Some("  ".into()),
                ..good.clone()
            },
            Usage,
            "prompt",
        ),
        (
            Spec {
                keep_runs: Some(0),
                ..good.clone()
            },
            Usage,
            "keep-runs",
        ),
        (
            Spec {
                catch_up: Some("always".into()),
                ..good.clone()
            },
            Usage,
            "catch-up",
        ),
    ];
    for (spec, kind, word) in cases {
        let e = create(&spec, &ctx).unwrap_err();
        assert_eq!(e.kind, kind, "{spec:?}: {}", e.message);
        assert!(e.message.contains(word), "{}: wants '{word}'", e.message);
    }
    // A `done` Schedule's prompt can still be edited; its past `at` is not
    // checked again.
    let mut done = s.clone();
    done.cron = None;
    done.at = Some(ctx.now - 3600);
    done.done = true;
    done.enabled = false;
    let e = edit(
        &done,
        &Spec {
            prompt: Some("new".into()),
            ..Default::default()
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(e.prompt, "new");
    assert!(e.done);
    // A future `at` clears done and switches it on.
    let e = edit(
        &done,
        &Spec {
            at: Some("2026-10-31 09:00".into()),
            ..Default::default()
        },
        &ctx,
    )
    .unwrap();
    assert!(!e.done && e.enabled);
    // A dir target clears the Project.
    std::fs::create_dir_all(&dir).unwrap();
    let e = edit(
        &s,
        &Spec {
            dir: Some(dir.display().to_string()),
            ..Default::default()
        },
        &ctx,
    )
    .unwrap();
    assert!(e.project.is_none() && e.link.is_none() && e.in_scratch());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn r136_d_on_again_or_edited_has_nothing_due() {
    let tz = berlin();
    let projects = vec![("solo".to_string(), vec!["one".to_string()])];
    let presets = vec!["claude".to_string()];
    let account = |_: &str, _: &str| -> Result<()> { Ok(()) };
    let mut s = sched(&tz);
    s.project = Some("solo".into());
    s.link = Some("one".into());
    s.checked = t("2026-10-30 10:30", &tz);
    s.enabled = false;
    let at = t("2026-10-30 14:10", &tz);
    let mut ctx = ctx_with(&tz, &projects, &presets, &[], &account);
    ctx.now = at;
    let on = edit(
        &s,
        &Spec {
            enabled: Some(true),
            ..Default::default()
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(on.due(at + 15, &tz), Due::Nothing);
    let mut live = s.clone();
    live.enabled = true;
    let edited = edit(
        &live,
        &Spec {
            cron: Some("5 * * * *".into()),
            ..Default::default()
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(edited.due(at + 15, &tz), Due::Nothing);
    assert_eq!(
        live.due(at + 15, &tz),
        Due::Missed {
            count: 4,
            latest: t("2026-10-30 14:00", &tz)
        }
    );
}

#[test]
fn r136_texts() {
    let tz = berlin();
    assert_eq!(cron_text("0 9 * * 1-5"), "Weekdays 09:00");
    assert_eq!(cron_text("*/15 * * * *"), "Every 15 min");
    assert_eq!(cron_text("@daily"), "Daily 00:00");
    assert_eq!(cron_text("0 * * * *"), "Hourly");
    assert_eq!(cron_text("0 */2 * * *"), "Every 2 h");
    assert_eq!(cron_text("30 8 * * mon"), "Mondays 08:30");
    assert_eq!(cron_text("0 9 1 * *"), "Monthly, day 1 09:00");
    assert_eq!(cron_text("0 9 1 * mon"), "0 9 1 * mon");
    let now = t("2026-10-30 12:00", &tz);
    assert_eq!(
        fmt_time(t("2026-11-02 14:00", &tz), now, &tz),
        "Nov 2 14:00"
    );
    assert_eq!(
        fmt_time(t("2027-01-02 14:00", &tz), now, &tz),
        "Jan 2 2027 14:00"
    );
}
