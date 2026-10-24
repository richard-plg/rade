use super::*;

/// Scene E's time: Fri 2026-10-23 14:00 (UTC, offset 0 in these tests).
const NOW: i64 = 1_792_764_000;
const MIN: i64 = 60;
const HOUR: i64 = 3600;

fn lim(id: &str, group: &str, name: &str, used: f64, reset: i64, t: i64) -> Limit {
    Limit {
        limit_id: id.into(),
        group: group.into(),
        name: name.into(),
        window_minutes: match name {
            "5-hour" => Some(300),
            _ if group == "Weekly" || name == "weekly" => Some(10_080),
            _ => None,
        },
        used_percent: Some(used),
        resets_at: Some(reset),
        source: "statusline".into(),
        t,
        ..Limit::default()
    }
}

fn five(used: f64, reset: i64, t: i64) -> Limit {
    lim("five_hour", "Session", "5-hour", used, reset, t)
}

/// `five_hour` rising 1 % per minute from 22 % to 62 %, now = the last
/// sample, reset in 72 minutes.
fn rising() -> Vec<Limit> {
    (0..=40)
        .map(|k| five(22.0 + k as f64, NOW + 72 * MIN, NOW - (40 - k) * MIN))
        .collect()
}

#[test]
fn r110_a_tightest_highest_then_earlier_reset() {
    let acme_work = vec![
        lim(
            "seven_day",
            "Weekly",
            "All models",
            41.0,
            NOW + 3 * 86_400,
            NOW,
        ),
        five(62.0, NOW + 72 * MIN, NOW),
        lim(
            "seven_day_opus",
            "Weekly",
            "Opus",
            58.0,
            NOW + 3 * 86_400,
            NOW,
        ),
    ];
    let t = tightest(&acme_work).unwrap();
    assert_eq!(
        (t.limit_id.as_str(), t.used_percent),
        ("five_hour", Some(62.0))
    );
    assert_eq!(t.label(), "5-hour");

    let tie = vec![
        lim(
            "seven_day",
            "Weekly",
            "All models",
            62.0,
            NOW + 3 * 86_400,
            NOW,
        ),
        five(62.0, NOW + 72 * MIN, NOW),
    ];
    assert_eq!(tightest(&tie).unwrap().limit_id, "five_hour");
    let tie_rev: Vec<Limit> = tie.into_iter().rev().collect();
    assert_eq!(tightest(&tie_rev).unwrap().limit_id, "five_hour");

    assert!(tightest(&[]).is_none());
    // Credits (no percentage) never count.
    let credits = Limit {
        limit_id: "credits".into(),
        ..Limit::default()
    };
    assert!(tightest(&[credits]).is_none());
}

#[test]
fn r110_b_forecast_rising_flat_and_reset_midway() {
    let f = forecast(&rising(), NOW).unwrap();
    assert_eq!(f.text(), "hits 100% in ~40m");
    assert_eq!(f.hits_100_in_s, Some(38 * 60));

    let flat: Vec<Limit> = (0..=40)
        .map(|k| five(40.0, NOW + 72 * MIN, NOW - (40 - k) * MIN))
        .collect();
    assert_eq!(forecast(&flat, NOW), None);

    // 90 → 99 % in the old window, reset, then 0 → 20 % over 40 minutes.
    let mut midway: Vec<Limit> = (0..10)
        .map(|k| five(90.0 + k as f64, NOW - 50 * MIN, NOW - (100 - k) * MIN))
        .collect();
    midway.extend((0..=40).map(|k| five(k as f64 * 0.5, NOW + 4 * HOUR, NOW - (40 - k) * MIN)));
    let w = current_window(&midway, NOW);
    assert_eq!(w.len(), 41);
    assert!(w.iter().all(|l| l.resets_at == Some(NOW + 4 * HOUR)));
    // 20 % now at 0.5 %/min: 80 % more takes 160 min, before the 4 h reset.
    let f = forecast(&midway, NOW).unwrap();
    assert_eq!(f.hits_100_in_s, Some(160 * 60));
    assert_eq!(f.text(), "hits 100% in ~2h 40m");
}

#[test]
fn r110_k_forecast_out_of_order_skew_and_past_reset() {
    let mut shuffled = rising();
    shuffled.swap(3, 30);
    shuffled.swap(0, 40);
    assert_eq!(
        forecast(&shuffled, NOW).unwrap().text(),
        "hits 100% in ~40m"
    );

    let mut skewed = rising();
    skewed.push(five(99.0, NOW + 72 * MIN, NOW + 10 * MIN));
    assert_eq!(forecast(&skewed, NOW).unwrap().text(), "hits 100% in ~40m");

    let past: Vec<Limit> = rising()
        .into_iter()
        .map(|mut l| {
            l.resets_at = Some(NOW - 30);
            l
        })
        .collect();
    assert_eq!(forecast(&past, NOW), None);
}

#[test]
fn forecast_ends_below_100_at_reset_and_texts() {
    // 0.29 %/h for 10 h to 88 %, reset in 31 h → 97 % at reset.
    let s = 9.0 / (31.0 * HOUR as f64);
    let series: Vec<Limit> = (0..=10)
        .map(|k| {
            let t = NOW - (10 - k) * HOUR;
            lim(
                "seven_day",
                "Weekly",
                "All models",
                88.0 - s * ((NOW - t) as f64),
                NOW + 31 * HOUR,
                t,
            )
        })
        .collect();
    let f = forecast(&series, NOW).unwrap();
    assert_eq!(f.text(), "97% at reset");
    assert_eq!(approx(4 * 60), "~4m");
    assert_eq!(approx(58 * 60), "~1h");
    assert_eq!(approx(80 * 60), "~1h 20m");
    assert_eq!(approx(120 * 60), "~2h");
    assert_eq!(when_text(NOW + 72 * MIN, NOW, 0), "in 1h 12m");
    assert_eq!(when_text(NOW + 45 * MIN, NOW, 0), "in 45m");
    assert_eq!(when_text(NOW + 2 * HOUR, NOW, 0), "in 2h");
    assert_eq!(when_text(NOW + 31 * HOUR, NOW, 0), "Sat 21:00");
    assert_eq!(when_text(NOW + 31 * HOUR, NOW, 7 * HOUR), "Sun 04:00");
    assert_eq!(when_text(NOW + 15 * 86_400, NOW, 0), "Nov 7 14:00");
}

/// Scene E's latest limits with their series (docs/internals/design/spec/e-usage.md
/// "Content").
pub(crate) fn scene_e() -> Vec<AccountLimits> {
    let mon = NOW + 2 * 86_400 + 19 * HOUR; // Mon 2026-10-26 09:00
    let sat = NOW + 31 * HOUR; // Sat 2026-10-24 21:00
    let thu = NOW + 6 * 86_400; // Thu
    let with = |l: Limit, series: Option<Vec<Limit>>| {
        let f = series.and_then(|s| forecast(&s, NOW));
        (l, f)
    };
    let s = 9.0 / (31.0 * HOUR as f64);
    let personal_weekly: Vec<Limit> = (0..=10)
        .map(|k| {
            let t = NOW - (10 - k) * HOUR;
            lim(
                "seven_day",
                "Weekly",
                "All models",
                88.0 - s * ((NOW - t) as f64),
                sat,
                t,
            )
        })
        .collect();
    let spark = |used: f64, name: &str, reset: i64, t: i64| Limit {
        model: Some("GPT-5.3-Codex-Spark".into()),
        ..lim("codex_bengalfox", "Spark", name, used, reset, t)
    };
    let codex =
        |id: &str, g: &str, used: f64, reset: i64| lim(id, g, "weekly", used, reset, NOW - 5 * MIN);
    vec![
        AccountLimits {
            account: "acme-work".into(),
            limits: vec![
                with(five(62.0, NOW + 72 * MIN, NOW), Some(rising())),
                with(
                    lim("seven_day", "Weekly", "All models", 41.0, mon, NOW),
                    None,
                ),
                with(
                    lim("seven_day_opus", "Weekly", "Opus", 58.0, mon, NOW),
                    None,
                ),
                with(
                    lim("seven_day_sonnet", "Weekly", "Sonnet", 22.0, mon, NOW),
                    None,
                ),
                with(
                    lim(
                        "seven_day_oauth_apps",
                        "Weekly",
                        "OAuth apps",
                        3.0,
                        mon,
                        NOW,
                    ),
                    None,
                ),
                with(
                    lim(
                        "spend_limit",
                        "Extra usage",
                        "Spend limit",
                        24.0,
                        NOW + 9 * 86_400,
                        NOW,
                    ),
                    None,
                ),
            ],
        },
        AccountLimits {
            account: "acme-team".into(),
            limits: vec![
                with(five(8.0, NOW + 3 * HOUR, NOW - 2 * HOUR), None),
                with(
                    lim(
                        "seven_day",
                        "Weekly",
                        "All models",
                        12.0,
                        thu,
                        NOW - 2 * HOUR,
                    ),
                    None,
                ),
            ],
        },
        AccountLimits {
            account: "acme".into(),
            limits: vec![
                with(codex("codex", "Codex", 20.0, thu), None),
                with(
                    spark(81.0, "5-hour", NOW + 150 * MIN, NOW - 5 * MIN),
                    Some(vec![
                        spark(81.0, "5-hour", NOW + 150 * MIN, NOW - 20 * MIN),
                        spark(81.0, "5-hour", NOW + 150 * MIN, NOW - 5 * MIN),
                    ]),
                ),
                with(spark(33.0, "weekly", thu, NOW - 5 * MIN), None),
            ],
        },
        AccountLimits {
            account: "acme-biz".into(),
            limits: vec![with(codex("codex", "Codex", 27.0, thu), None)],
        },
        AccountLimits {
            account: "personal".into(),
            limits: vec![
                // Its 5-hour window ended a day ago: no upcoming reset.
                with(five(30.0, NOW - 20 * HOUR, NOW - 86_400), None),
                with(personal_weekly[10].clone(), Some(personal_weekly)),
            ],
        },
        AccountLimits {
            account: "personal".into(),
            limits: vec![with(codex("codex", "Codex", 4.0, thu), None)],
        },
        AccountLimits {
            account: "oss".into(),
            limits: vec![],
        },
    ]
}

#[test]
fn r110_c_next_resets_of_scene_e() {
    let rows = next_resets(&scene_e(), NOW, 0, 6);
    let got: Vec<[&str; 4]> = rows
        .iter()
        .map(|r| {
            [
                r.when.as_str(),
                r.account.as_str(),
                r.what.as_str(),
                r.note.as_str(),
            ]
        })
        .collect();
    assert_eq!(
        got,
        [
            [
                "in 1h 12m",
                "acme-work",
                "5-hour · now 62%",
                "forecast: hits 100% in ~40m"
            ],
            [
                "in 2h 30m",
                "acme",
                "Spark 5-hour · now 81%",
                "GPT-5.3-Codex-Spark only"
            ],
            [
                "Sat 21:00",
                "personal",
                "weekly · now 88%",
                "forecast: 97% at reset"
            ],
            ["Mon 09:00", "acme-work", "weekly ×4 · max 58% Opus", ""],
        ]
    );
    assert_eq!(rows[0].forecast.unwrap().hits_100_in_s, Some(38 * 60));
    assert_eq!(next_resets(&scene_e(), NOW, 0, 2).len(), 2);
}

#[test]
fn limit_events_over_80_blocked_and_reset_once_each() {
    let spark = |used: f64, reset: i64, blocked: bool| Limit {
        blocked,
        model: Some("GPT-5.3-Codex-Spark".into()),
        ..lim("codex_bengalfox", "Spark", "5-hour", used, reset, NOW)
    };
    let a = [spark(79.0, NOW + HOUR, false)];
    let b = [spark(81.0, NOW + HOUR, false)];
    let c = [spark(100.0, NOW + HOUR, true)];
    let d = [spark(0.0, NOW + 6 * HOUR, false)];
    assert_eq!(
        limit_events("acme", &a, &b),
        ["account acme: Spark 5-hour over 80%"]
    );
    assert!(limit_events("acme", &b, &b).is_empty());
    assert_eq!(
        limit_events("acme", &b, &c),
        ["account acme: Spark 5-hour blocked"]
    );
    assert_eq!(
        limit_events("acme", &c, &d),
        ["account acme: Spark 5-hour reset"]
    );
    assert!(limit_events("acme", &[], &d).is_empty());
}

#[test]
fn a_window_past_its_reset_counts_as_zero() {
    // 95 % 5-hour, then left idle: hours after the reset it is not at 95 %.
    let old = five(95.0, NOW - 3 * HOUR, NOW - 4 * HOUR);
    let week = lim(
        "seven_day",
        "Weekly",
        "All models",
        41.0,
        NOW + 50 * HOUR,
        NOW - HOUR,
    );
    let l = as_of(vec![old.clone(), week.clone()], NOW);
    assert_eq!(l[0].used_percent, Some(0.0));
    assert_eq!((l[0].resets_at, l[0].blocked), (None, false));
    assert_eq!(l[1], week);
    assert_eq!(tightest(&l).unwrap().limit_id, "seven_day");
    // Before its reset it keeps its number.
    assert_eq!(as_of(vec![old.clone()], NOW - 4 * HOUR + 60)[0], old);
}

#[test]
fn reset_events_when_the_clock_passes_a_reset() {
    let r = NOW - 10;
    let samples = vec![
        five(70.0, r, NOW - 2 * HOUR),
        five(95.0, r + 30, NOW - HOUR), // a window's resets_at jitters
        lim(
            "seven_day",
            "Weekly",
            "All models",
            41.0,
            NOW + 50 * HOUR,
            NOW - HOUR,
        ),
    ];
    assert_eq!(
        reset_events("acme-work", &samples, NOW - 30, NOW + 30),
        ["account acme-work: 5-hour reset"]
    );
    // Once: the next poll's interval no longer holds it.
    assert!(reset_events("acme-work", &samples, NOW + 30, NOW + 60).is_empty());
    // Already said by a sample of the next window that came before it.
    let mut early = samples.clone();
    early.push(five(2.0, r + 5 * HOUR, r - 60));
    assert!(reset_events("acme-work", &early, NOW - 30, NOW + 30).is_empty());
    // Nothing used: nothing to say.
    assert!(reset_events("acme-work", &[five(0.0, r, NOW - HOUR)], NOW - 30, NOW + 30).is_empty());
}

#[test]
fn limit_events_count_an_ended_window_as_zero() {
    // The new window's first sample comes after the old reset: the clock
    // already said `reset`; crossing 80 % again is said again.
    let old = Limit {
        blocked: true,
        ..five(100.0, NOW - HOUR, NOW - 2 * HOUR)
    };
    let new = five(85.0, NOW + 4 * HOUR, NOW);
    assert_eq!(
        limit_events("acme", &[old], &[new]),
        ["account acme: 5-hour over 80%"]
    );
}
