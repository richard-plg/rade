use super::*;
use crate::usage::report::Period;

/// Scene E's time: Fri 2026-10-23 14:00 (UTC, offset 0 in these tests).
const NOW: i64 = 1_792_764_000;
const MIN: i64 = 60;

fn lim(id: &str, group: &str, name: &str, used: f64, reset: i64) -> Limit {
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
        t: NOW,
        ..Limit::default()
    }
}

fn five(used: f64) -> Limit {
    lim("five_hour", "Session", "5-hour", used, NOW + 72 * MIN)
}

fn week(used: f64) -> Limit {
    lim("seven_day", "Weekly", "All models", used, NOW + 3 * 86_400)
}

fn spark(used: f64) -> Limit {
    Limit {
        model: Some("GPT-5.3-Codex-Spark".into()),
        ..lim("codex_bengalfox", "Spark", "5-hour", used, NOW + 150 * MIN)
    }
}

#[test]
fn short_labels() {
    assert_eq!(short("5-hour"), "5h");
    assert_eq!(short("weekly"), "wk");
    assert_eq!(short("Spark 5-hour"), "Spark 5h");
    assert_eq!(short("Opus weekly"), "Opus wk");
    assert_eq!(short("Premium"), "Premium");
}

#[test]
fn r110_i_menu_details_of_scene_f() {
    let work = vec![
        week(41.0),
        five(62.0),
        lim("seven_day_opus", "Weekly", "Opus", 58.0, NOW),
    ];
    assert_eq!(
        menu_detail(UsageState::Ok, &work),
        ("5h 62% · wk 41%".to_string(), false)
    );
    // acme-team: the weekly 12 % is the Tightest limit; the pair keeps its order.
    assert_eq!(
        menu_detail(UsageState::Ok, &[five(8.0), week(12.0)]).0,
        "5h 8% · wk 12%"
    );
    // acme (codex): the Tightest limit alone, hot from 80 %.
    let acme = vec![
        lim("codex", "Codex", "weekly", 33.0, NOW + 86_400),
        spark(81.0),
    ];
    assert_eq!(
        menu_detail(UsageState::Ok, &acme),
        ("Spark 5h 81%".to_string(), true)
    );
    assert!(!menu_detail(UsageState::Ok, &[spark(79.9)]).1);
    assert!(menu_detail(UsageState::Ok, &[spark(80.0)]).1);
    // An Opus limit above the pair: it alone.
    let opus = vec![
        five(10.0),
        week(20.0),
        lim("seven_day_opus", "Weekly", "Opus", 70.0, NOW),
    ];
    assert_eq!(menu_detail(UsageState::Ok, &opus).0, "Opus wk 70%");
    // No data: never 0 %; a format change says so.
    assert_eq!(menu_detail(UsageState::NoData, &[]).0, "no data yet");
    assert_eq!(menu_detail(UsageState::Ok, &[]).0, "no data yet");
    assert_eq!(
        menu_detail(UsageState::FormatChanged, &[five(5.0)]).0,
        FORMAT_CHANGED
    );
}

#[test]
fn r110_f_status_cell_text_meter_and_tooltip() {
    let c = status_cell(AgentKind::Claude, "acme-work", &[five(62.0), week(41.0)]).unwrap();
    assert_eq!(c.text(), "claude 5h 62% · wk 41%");
    assert_eq!(
        (c.lead.as_str(), c.tail.as_str()),
        ("claude 5h", "62% · wk 41%")
    );
    assert_eq!(c.tooltip, "Claude · acme-work");
    assert_eq!(meter_fill(36, c.percent as f64), 22);
    let x = status_cell(AgentKind::Codex, "acme", &[spark(81.0)]).unwrap();
    assert_eq!((x.text().as_str(), x.hot), ("codex Spark 5h 81%", true));
    // No figures → hidden.
    assert!(status_cell(AgentKind::Claude, "oss", &[]).is_none());
}

#[test]
fn meters_tokens_and_times() {
    assert_eq!(meter_fill(56, 62.0), 35);
    assert_eq!(meter_fill(56, 4.0), 2);
    assert_eq!(meter_fill(56, 140.0), 56);
    assert_eq!(meter_fill(56, -3.0), 0);
    assert_eq!(tokens_text(12_400_000), "12.4M");
    assert_eq!(tokens_text(900_000), "900k");
    assert_eq!(tokens_text(950_000_000), "950.0M");
    assert_eq!(tokens_text(1_500), "1.5k");
    assert_eq!(tokens_text(300), "300");
    assert_eq!(tokens_text(0), "0");
    assert_eq!(updated_text(Some(NOW - 40), NOW), "40s");
    assert_eq!(updated_text(Some(NOW - 7_200), NOW), "2h");
    assert_eq!(updated_text(Some(NOW - 3 * 86_400), NOW), "3d");
    assert_eq!(updated_text(None, NOW), "—");
    assert_eq!(tokens_head(Period::Week), "TOKENS 7D");
    assert_eq!(tokens_head(Period::Today), "TOKENS TODAY");
    assert_eq!(tokens_head(Period::Month), "TOKENS 30D");
    // The 7 days ending Fri 2026-10-23 (the mockup's F S S M T W T ends on a Thursday).
    let days = Period::Week.day_list(NOW, 0);
    let letters: Vec<String> = days.iter().map(|d| day_letter(d)).collect();
    assert_eq!(letters.join(" "), "S S M T W T F");
    assert_eq!(day_letter("2026-10-22"), "T");
    assert_eq!(day_letter("2026-10-16"), "F");
}

#[test]
fn footer_models_and_detail_texts() {
    let by = vec![
        ("claude-opus-4-5".to_string(), 7_000u64),
        ("claude-opus-4-1".to_string(), 800),
        ("claude-sonnet-4-5".to_string(), 2_200),
    ];
    assert_eq!(model_split(&by), "opus 78% · sonnet 22%");
    assert_eq!(model_split(&[]), "");
    assert_eq!(model_short("gpt-5.3-codex"), "gpt-5.3-codex");

    let spend = Limit {
        used_usd: Some(12.0),
        limit_usd: Some(50.0),
        period: Some("month".into()),
        ..lim(
            "spend_limit",
            "Extra usage",
            "Spend limit",
            24.0,
            NOW + 9 * 86_400,
        )
    };
    assert_eq!(value_text(&spend), "$12");
    assert_eq!(reset_text(&spend, NOW, 0), "of $50 /mo");
    assert_eq!(value_text(&five(62.0)), "62%");
    assert_eq!(reset_text(&five(62.0), NOW, 0), "1h 12m");
    // Mon 2026-10-26 09:00.
    let mon = lim(
        "seven_day",
        "Weekly",
        "All models",
        41.0,
        NOW + 2 * 86_400 + 19 * 3_600,
    );
    assert_eq!(reset_text(&mon, NOW, 0), "Mon 09:00");
    assert_eq!(dollars(12.5), "$12.50");
    let none = Limit {
        used_percent: None,
        ..five(0.0)
    };
    assert_eq!(value_text(&none), "—");

    assert_eq!(
        detail_sub(
            AgentKind::Claude,
            Some("Max 20x"),
            "/home/r/.claude-work",
            "/home/r"
        ),
        "Claude Code · Max 20x · ~/.claude-work"
    );
    assert_eq!(
        detail_sub(AgentKind::Codex, None, "/srv/codex", "/home/r"),
        "Codex · /srv/codex"
    );
    assert_eq!(
        detail_sub(AgentKind::Codex, None, "/home/rx/c", "/home/r"),
        "Codex · /home/rx/c"
    );
}

fn rising() -> Vec<Limit> {
    (0..=40)
        .map(|k| Limit {
            t: NOW - (40 - k) * MIN,
            ..five(22.0 + k as f64)
        })
        .collect()
}

fn usage(agent: AgentKind, name: &str, samples: &[Vec<Limit>]) -> AccountUsage {
    AccountUsage {
        agent: Some(agent),
        name: name.into(),
        limits: samples
            .iter()
            .map(|s| (s.last().unwrap().clone(), limits::forecast(s, NOW)))
            .collect(),
        login: String::new(),
    }
}

#[test]
fn r125_k_an_account_on_the_same_login_is_never_the_other() {
    // `cl` (~/.claude written out) and `clp` (the default login) are one
    // quota: the Toast for cl never offers clp.
    let mut cl = usage(AgentKind::Claude, "cl", &[rising(), vec![week(41.0)]]);
    let mut clp = usage(
        AgentKind::Claude,
        "clp",
        &[vec![five(8.0)], vec![week(12.0)]],
    );
    let mut clw = usage(
        AgentKind::Claude,
        "clw",
        &[vec![five(20.0)], vec![week(30.0)]],
    );
    cl.login = "claude-aaaa".into();
    clp.login = "claude-aaaa".into();
    clw.login = "claude-bbbb".into();
    let t = forecast_toast(&[cl.clone(), clp.clone()], &[], NOW, 0).unwrap();
    assert_eq!(t.other, None);
    let t = forecast_toast(&[cl, clp, clw], &[], NOW, 0).unwrap();
    assert_eq!(t.other.as_deref(), Some("clw"));
}

#[test]
fn r110_g_forecast_toast_text_other_account_and_once_per_window() {
    let accounts = vec![
        usage(
            AgentKind::Claude,
            "acme-work",
            &[rising(), vec![week(41.0)]],
        ),
        usage(
            AgentKind::Claude,
            "acme-team",
            &[vec![five(8.0)], vec![week(12.0)]],
        ),
        usage(AgentKind::Codex, "acme", &[vec![spark(30.0)]]),
    ];
    let t = forecast_toast(&accounts, &[], NOW, 0).unwrap();
    assert_eq!(
        t.title(),
        "acme-work will hit its 5-hour limit in ~40m at this pace"
    );
    assert_eq!(
        (t.name.as_str(), t.account.as_str()),
        ("acme-work", "acme-work")
    );
    assert_eq!(t.sub, "62% now · resets in 1h 12m · acme-team is at 8%");
    assert_eq!(t.action().as_deref(), Some("Next agents use acme-team"));
    assert!(!t.per_model);
    assert_eq!(
        t.activity_line(),
        "account acme-work: will hit its 5-hour limit in ~40m at this pace"
    );
    // Once per limit per window.
    assert!(forecast_toast(&accounts, std::slice::from_ref(&t.key), NOW, 0).is_none());
    // A new window (another resets_at) may show it again.
    let mut later = rising();
    for l in &mut later {
        l.resets_at = Some(NOW + 80 * MIN);
    }
    let again = vec![usage(AgentKind::Claude, "acme-work", &[later])];
    assert!(forecast_toast(&again, std::slice::from_ref(&t.key), NOW, 0).is_some());
}

#[test]
fn r110_g_forecast_toast_without_other_and_none_when_flat() {
    // The only other account is hot itself: no action.
    let accounts = vec![
        usage(AgentKind::Claude, "acme-work", &[rising()]),
        usage(AgentKind::Claude, "acme-team", &[vec![five(85.0)]]),
        usage(AgentKind::Codex, "acme", &[vec![spark(3.0)]]),
    ];
    let t = forecast_toast(&accounts, &[], NOW, 0).unwrap();
    assert_eq!(t.other, None);
    assert_eq!(t.action(), None);
    assert_eq!(t.sub, "62% now · resets in 1h 12m");
    // Flat: no forecast, no Toast.
    let flat = vec![usage(
        AgentKind::Claude,
        "acme-work",
        &[(0..=10)
            .map(|k| Limit {
                t: NOW - (10 - k) * MIN,
                ..five(40.0)
            })
            .collect()],
    )];
    assert!(forecast_toast(&flat, &[], NOW, 0).is_none());
}

#[test]
fn r110_g_per_model_limit_suggests_a_model_first() {
    let sp: Vec<Limit> = (0..=20)
        .map(|k| Limit {
            t: NOW - (20 - k) * MIN,
            ..spark(60.0 + k as f64)
        })
        .collect();
    let accounts = vec![
        usage(AgentKind::Codex, "acme", &[sp]),
        usage(
            AgentKind::Codex,
            "acme-biz",
            &[vec![lim("codex", "Codex", "weekly", 27.0, NOW + 86_400)]],
        ),
    ];
    let t = forecast_toast(&accounts, &[], NOW, 0).unwrap();
    assert!(t.per_model);
    assert_eq!(
        t.title(),
        "acme will hit its Spark 5-hour limit in ~20m at this pace"
    );
    assert_eq!(
        t.sub,
        "80% now · resets in 2h 30m · only GPT-5.3-Codex-Spark is limited: another model is not · acme-biz is at 27%"
    );
    assert_eq!(t.action().as_deref(), Some("Next agents use acme-biz"));
}
