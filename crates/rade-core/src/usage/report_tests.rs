use super::*;
use crate::usage::TokenRow;

const NOW: i64 = 1_793_170_000; // 2026-10-23 ~10:46 UTC

fn lim(id: &str, group: &str, name: &str, used: f64, reset: i64, t: i64) -> Limit {
    Limit {
        limit_id: id.into(),
        group: group.into(),
        name: name.into(),
        used_percent: Some(used),
        resets_at: Some(reset),
        source: "rollout".into(),
        t,
        ..Limit::default()
    }
}

fn spark(used: f64, reset: i64, t: i64) -> Limit {
    Limit {
        model: Some("GPT-5.3-Codex-Spark".into()),
        window_minutes: Some(300),
        ..lim("codex_bengalfox", "Spark", "5-hour", used, reset, t)
    }
}

fn rec(l: Limit) -> Record {
    Record::Limit {
        account: Some("acme".into()),
        limit: l,
    }
}

fn tok(day: &str, project: &str, model: &str, input: u64, cache_read: u64) -> Record {
    Record::Tokens {
        t: NOW,
        row: TokenRow {
            day: day.into(),
            project: project.into(),
            model: model.into(),
            input,
            output: 10,
            cache_creation: 0,
            cache_read,
        },
    }
}

#[test]
fn one_statusline_batch_is_recorded_once() {
    // One batch of three limits with one `t`: all new → all kept.
    let batch = vec![
        lim("five_hour", "Session", "5-hour", 62.0, NOW + 4000, NOW),
        lim("seven_day", "Weekly", "All models", 41.0, NOW + 90_000, NOW),
    ];
    let (recs, ev) = new_samples(&[], "acme-work", batch.clone());
    assert_eq!(recs.len(), 2);
    assert!(ev.is_empty());
    assert!(recs
        .iter()
        .all(|r| matches!(r, Record::Limit { account: Some(a), .. } if a == "acme-work")));
    // The same values 10 s later: nothing worth a line.
    let again: Vec<Limit> = batch
        .into_iter()
        .map(|l| Limit { t: NOW + 10, ..l })
        .collect();
    let (recs2, _) = new_samples(&recs, "acme-work", again);
    assert!(recs2.is_empty());
}

#[test]
fn limit_events_over_80_blocked_reset_once_each() {
    let r = NOW + 9000;
    let mut store: Vec<Record> = Vec::new();
    let mut lines = Vec::new();
    for l in [
        spark(79.0, r, NOW),
        spark(81.0, r, NOW + 60),
        spark(81.0, r, NOW + 70), // unchanged: no line, no event
        Limit {
            blocked: true,
            ..spark(81.0, r, NOW + 120)
        },
        spark(3.0, r + 18_000, NOW + 200),
    ] {
        let (recs, ev) = new_samples(&store, "acme", vec![l]);
        store.extend(recs);
        lines.extend(ev);
    }
    assert_eq!(
        lines,
        [
            "account acme: Spark 5-hour over 80%",
            "account acme: Spark 5-hour blocked",
            "account acme: Spark 5-hour reset",
        ]
    );
}

#[test]
fn project_for_takes_the_longest_checkout() {
    let c = vec![
        (PathBuf::from("/w/acme/shop"), "Shop".to_string()),
        (PathBuf::from("/w/acme/shop/sub"), "Sub".to_string()),
    ];
    assert_eq!(project_for("/w/acme/shop/src", &c).as_deref(), Some("Shop"));
    assert_eq!(
        project_for("/w/acme/shop/sub/x", &c).as_deref(),
        Some("Sub")
    );
    assert_eq!(project_for("/w/acme/shopping", &c), None);
    assert_eq!(project_for("/tmp", &c), None);
}

#[test]
fn tokens_follow_the_period_and_exclude_cache_reads() {
    let today = local_day(NOW, 0);
    let d3 = local_day(NOW - 3 * 86_400, 0);
    let d20 = local_day(NOW - 20 * 86_400, 0);
    let recs = vec![
        tok(&today, "Shop", "opus", 100, 99_999),
        tok(&d3, "Shop", "sonnet", 200, 0),
        tok(&d20, "Scratch", "opus", 1000, 0),
    ];
    let s = |p| summary(&recs, None, p, NOW, 0).tokens;
    assert_eq!(s(Period::Today), 110);
    assert_eq!(s(Period::Week), 110 + 210);
    assert_eq!(s(Period::Month), 110 + 210 + 1010);
    let t = tokens(&recs, Period::Week, NOW, 0);
    assert_eq!(t.by_day.len(), 7);
    assert_eq!(
        t.by_day[6],
        DayTokens {
            day: today,
            tokens: 110
        }
    );
    assert_eq!(t.by_day[3].tokens, 210);
    assert_eq!(
        t.by_project,
        [ProjectTokens {
            project: "Shop".into(),
            tokens: 320
        }]
    );
    assert_eq!(t.by_model[0].model, "sonnet");
    assert_eq!(Period::parse(None), Ok(Period::Week));
    assert_eq!(Period::parse(Some("30d")), Ok(Period::Month));
    assert!(Period::parse(Some("1y")).is_err());
}

#[test]
fn summary_state_and_tightest() {
    assert_eq!(
        summary(&[], None, Period::Week, NOW, 0).state,
        UsageState::NoData
    );
    let recs = vec![
        rec(lim(
            "five_hour",
            "Session",
            "5-hour",
            62.0,
            NOW + 100,
            NOW - 50,
        )),
        rec(lim(
            "seven_day",
            "Weekly",
            "All models",
            41.0,
            NOW + 900,
            NOW - 40,
        )),
    ];
    let s = summary(&recs, Some(UsageState::Ok), Period::Week, NOW, 0);
    assert_eq!(s.state, UsageState::Ok);
    assert_eq!(s.tightest.unwrap().limit_id, "five_hour");
    assert_eq!(s.updated, Some(NOW - 40));
    // A scan that read lines but found no sample: no number at all.
    let f = summary(&recs, Some(UsageState::FormatChanged), Period::Week, NOW, 0);
    assert_eq!((f.state, f.tightest), (UsageState::FormatChanged, None));
}

#[test]
fn limits_grouped_as_spec_e30() {
    let mon = NOW + 200_000;
    let l = vec![
        lim("seven_day_sonnet", "Weekly", "Sonnet", 22.0, mon, NOW),
        lim("spend_limit", "Extra usage", "Spend limit", 24.0, mon, NOW),
        lim("seven_day", "Weekly", "All models", 41.0, mon, NOW),
        lim("five_hour", "Session", "5-hour", 62.0, NOW + 4320, NOW),
        lim(
            "seven_day_oauth_apps",
            "Weekly",
            "OAuth apps",
            3.0,
            mon,
            NOW,
        ),
        lim("seven_day_opus", "Weekly", "Opus", 58.0, mon, NOW),
    ];
    let g: Vec<(String, Vec<String>)> = groups(&l)
        .into_iter()
        .map(|(g, v)| (g, v.into_iter().map(|x| x.name).collect()))
        .collect();
    let s = |x: &str| x.to_string();
    assert_eq!(
        g,
        vec![
            (s("Session"), vec![s("5-hour")]),
            (
                s("Weekly"),
                vec![s("All models"), s("Opus"), s("Sonnet"), s("OAuth apps")]
            ),
            (s("Extra usage"), vec![s("Spend limit")]),
        ]
    );
    // Codex: by limit_id in first-seen order, windows shortest first, credits last.
    let c = vec![
        Limit {
            limit_id: "credits".into(),
            group: "Credits".into(),
            name: "Credits".into(),
            ..Limit::default()
        },
        Limit {
            window_minutes: Some(10_080),
            ..lim("codex", "Codex", "weekly", 41.0, mon, NOW)
        },
        Limit {
            window_minutes: Some(10_080),
            ..lim("codex_bengalfox", "Spark", "weekly", 33.0, mon, NOW)
        },
        spark(81.0, NOW + 9000, NOW),
    ];
    let g: Vec<(String, Vec<String>)> = groups(&c)
        .into_iter()
        .map(|(g, v)| (g, v.into_iter().map(|x| x.name).collect()))
        .collect();
    assert_eq!(
        g,
        vec![
            (s("Codex"), vec![s("weekly")]),
            (s("Spark"), vec![s("5-hour"), s("weekly")]),
            (s("Credits"), vec![s("Credits")]),
        ]
    );
}

#[test]
fn account_line_text() {
    assert_eq!(
        account_line(
            "acme",
            "codex",
            Some(("Spark 5-hour", 81.0, Some(NOW + 9000))),
            "ok",
            NOW,
            0
        ),
        "acme  codex  81% Spark 5-hour  resets in 2h 30m"
    );
    assert_eq!(
        account_line("oss", "codex", None, "no-data", NOW, 0),
        "oss  codex  no usage data"
    );
    assert_eq!(
        account_line("x", "claude", None, "format-changed", NOW, 0),
        "x  claude  no usage data (format changed?)"
    );
}

#[test]
fn summary_and_account_limits_drop_a_passed_window() {
    let recs = vec![
        rec(lim(
            "five_hour",
            "Session",
            "5-hour",
            95.0,
            NOW - 3600,
            NOW - 7200,
        )),
        rec(lim(
            "seven_day",
            "Weekly",
            "All models",
            41.0,
            NOW + 9000,
            NOW - 7200,
        )),
    ];
    let s = summary(&recs, Some(UsageState::Ok), Period::Week, NOW, 0);
    assert_eq!(s.tightest.unwrap().limit_id, "seven_day");
    let a = account_limits("acme-work", &recs, NOW);
    let five = &a
        .limits
        .iter()
        .find(|(l, _)| l.limit_id == "five_hour")
        .unwrap()
        .0;
    assert_eq!((five.used_percent, five.resets_at), (Some(0.0), None));
}

/// 90 days of a 5-hour and a weekly sample every 15 minutes (R111-d's
/// store): the Usage page's answers from the whole store equal those from
/// the current windows alone, and the latest sample per limit is the newest.
#[test]
fn r111_d_account_limits_over_90_days_match_the_window() {
    let mut recs = Vec::new();
    let mut t = NOW - 90 * 86_400;
    while t <= NOW - 60 {
        for (id, group, nm, w) in [
            ("five_hour", "Session", "5-hour", 300i64),
            ("seven_day", "Weekly", "weekly", 10_080),
        ] {
            let p = w * 60;
            let used = ((t % p) * 100 / p) as f64;
            recs.push(rec(Limit {
                window_minutes: Some(w as u64),
                ..lim(id, group, nm, used, t - t % p + p, t)
            }));
        }
        t += 900;
    }
    recs.push(tok("2026-10-23", "Shop", "opus", 1_000, 0));
    assert!(recs.len() > 17_000);
    let latest = store::latest(&recs);
    assert_eq!(latest.len(), 2);
    let newest = |key: &str| {
        recs.iter()
            .rev()
            .find_map(|r| match r {
                Record::Limit { limit, .. } if limit.key() == key => Some(limit.clone()),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(latest[0], newest("five_hour/5-hour"));
    assert_eq!(latest[1], newest("seven_day/weekly"));
    let s = store::series(&recs, "seven_day/weekly");
    assert_eq!(s.len(), (recs.len() - 1) / 2);
    assert!(s.windows(2).all(|w| w[0].t <= w[1].t));
    assert!(store::series(&recs, "seven_day/week").is_empty());
    assert!(store::series(&recs, "seven_da/yweekly").is_empty());

    // Only the current windows' samples (+ the token row).
    let windowed: Vec<Record> = recs
        .iter()
        .filter(|r| match r {
            Record::Limit { limit, .. } => {
                limit.resets_at
                    == latest
                        .iter()
                        .find(|l| l.key() == limit.key())
                        .unwrap()
                        .resets_at
            }
            _ => true,
        })
        .cloned()
        .collect();
    assert!(windowed.len() < recs.len() / 10);
    // The same limits and forecasts (order = first seen in the store).
    let sorted = |r: &[Record]| {
        let mut v: Vec<String> = account_limits("a", r, NOW)
            .limits
            .iter()
            .map(|x| format!("{x:?}"))
            .collect();
        v.sort();
        v
    };
    assert_eq!(sorted(&recs), sorted(&windowed));
    assert!(account_limits("a", &recs, NOW)
        .limits
        .iter()
        .all(|(_, f)| f.is_some()));
}

/// R111-d: the records the Usage page keeps per store ([`store::for_view`])
/// give the same answers as the whole 90-day store at any later `now`,
/// including limits without a percent, a reset mid-way and a sample stamped
/// in the future (clock skew), with a small fraction of the records.
#[test]
fn r111_d_view_records_answer_like_the_whole_store() {
    let mut recs = Vec::new();
    let built = NOW - 600;
    let mut t = built - 90 * 86_400;
    while t <= built - 60 {
        for (id, group, nm, w) in [
            ("five_hour", "Session", "5-hour", 300i64),
            ("seven_day", "Weekly", "weekly", 10_080),
        ] {
            let p = w * 60;
            recs.push(rec(Limit {
                window_minutes: Some(w as u64),
                ..lim(id, group, nm, ((t % p) * 100 / p) as f64, t - t % p + p, t)
            }));
        }
        if t % 86_400 < 900 {
            recs.push(Record::Tokens {
                t,
                row: TokenRow {
                    day: crate::usage::local_day(t, 0),
                    project: "Shop".into(),
                    model: "opus".into(),
                    input: 500,
                    output: 5,
                    cache_creation: 0,
                    cache_read: 9,
                },
            });
            // A limit without a percent (Codex Premium) now and then.
            recs.push(rec(Limit {
                used_percent: None,
                resets_at: None,
                ..lim("premium", "Premium", "Premium", 0.0, 0, t)
            }));
        }
        t += 900;
    }
    // Spark: a sample stamped 2 hours ahead (a skewed clock).
    recs.push(rec(spark(10.0, built + 9_000, built - 3_000)));
    recs.push(rec(spark(30.0, built + 9_000, built + 7_200)));
    let view = store::for_view(&recs, built);
    assert!(
        view.len() * 10 < recs.len(),
        "{} of {}",
        view.len(),
        recs.len()
    );
    for now in [
        built,
        built + 60,
        built + 3_600,
        built + 7_200,
        built + 86_400,
    ] {
        assert_eq!(
            store::latest(&view),
            store::latest(&recs),
            "latest at {now}"
        );
        for per in ["today", "7d", "30d"] {
            let per = Period::parse(Some(per)).unwrap();
            assert_eq!(
                summary(&view, None, per, now, 0),
                summary(&recs, None, per, now, 0)
            );
            assert_eq!(tokens(&view, per, now, 0), tokens(&recs, per, now, 0));
        }
        assert_eq!(
            format!("{:?}", account_limits("a", &view, now)),
            format!("{:?}", account_limits("a", &recs, now)),
            "forecasts at {now}"
        );
    }
    assert!(store::for_view(&[], built).is_empty());
}
