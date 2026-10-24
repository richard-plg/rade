use super::*;
use AgentStatus::*;

const NOW: u64 = 100_000;

fn fact(pane: &str, status: AgentStatus, ago: u64, unread: bool, ws: &str, place: &str) -> Fact {
    Fact {
        pane: pane.into(),
        name: pane.into(),
        agent: "claude".into(),
        model: Some("opus-5.5".into()),
        status: Some(status),
        since: NOW - ago,
        unread,
        workspace: ws.to_lowercase(),
        workspace_name: ws.into(),
        place: place.into(),
        ..Default::default()
    }
}

/// The mockup's scene (spec G, `overview-mode/mockup.html#a`): 16 Agents
/// in 5 Workspaces.
pub fn mockup_scene() -> Vec<Fact> {
    let mut tests = fact(
        "tests",
        Waiting,
        240,
        true,
        "ACME",
        "Shop / shop-web / checkout-v2",
    );
    tests.note = Some("asked which fixtures".into());
    tests.parent = Some("planner".into());
    tests.agent = "codex".into();
    let mut migrate = fact(
        "migrate",
        Error,
        720,
        true,
        "globex",
        "Fleet / api / pg17-upgrade",
    );
    migrate.note = Some("cargo test failed · 3 failures".into());
    let mut lsp = fact(
        "lsp-fix",
        Running,
        1000,
        false,
        "oss",
        "rust-analyzer / hover-fix",
    );
    lsp.changed_at = Some(NOW - 840);
    let fan = |i: u32| {
        let mut f = fact(
            &format!("checkout-api-{i}"),
            Running,
            480,
            false,
            "ACME",
            "Shop / shop-api",
        );
        f.fanout = Some("checkout-api".into());
        f
    };
    vec![
        tests,
        fact(
            "approve-deps",
            Waiting,
            180,
            true,
            "personal",
            "Blog / site",
        ),
        migrate,
        fact(
            "docs-bot",
            Interrupted,
            3600,
            false,
            "personal",
            "Ghi chú / Việt",
        ),
        fact(
            "planner",
            Done,
            120,
            true,
            "ACME",
            "Shop / shop-web / checkout-v2",
        ),
        fact("judge", Done, 540, true, "ACME", "Shop / shop-web"),
        fact(
            "perf-img",
            Running,
            360,
            false,
            "ACME",
            "Shop / shop-web / perf-img",
        ),
        fact(
            "indexer",
            Running,
            1320,
            false,
            "globex",
            "Fleet / ingest / batch-v3",
        ),
        lsp,
        fan(1),
        fan(2),
        fan(3),
        fact(
            "seed-data",
            Done,
            3600,
            false,
            "ACME",
            "Shop / shop-api / main",
        ),
        fact(
            "dev-server",
            Done,
            7200,
            false,
            "ACME",
            "Shop / shop-web / main",
        ),
        fact(
            "etl-dry",
            Done,
            10800,
            false,
            "globex",
            "Fleet / ingest / main",
        ),
        fact("scratch", Exited, 18000, false, "default", "Scratch / ~"),
    ]
}

fn workspaces() -> Vec<(String, String)> {
    ["ACME", "globex", "personal", "oss", "default"]
        .iter()
        .map(|n| (n.to_lowercase(), n.to_string()))
        .collect()
}

fn keys(s: &Section) -> Vec<String> {
    s.cards.iter().map(|c| c.key()).collect()
}

fn listed(ov: &Overview) -> Vec<String> {
    ov.sections.iter().flat_map(keys).collect()
}

fn open(q: &Query) -> Overview {
    Overview::build(&mockup_scene(), q, &workspaces())
}

#[test]
fn r117_a_bands_order_fanout_parent_and_quiet_fold() {
    let ov = open(&Query::default());
    let titles: Vec<&str> = ov.sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Needs you", "Finished, not seen", "Running", "Quiet"]
    );
    // waiting before error before interrupted; newest first within each.
    assert_eq!(
        keys(&ov.sections[0]),
        ["approve-deps", "tests", "migrate", "docs-bot"]
    );
    assert_eq!(keys(&ov.sections[1]), ["planner", "judge"]);
    assert_eq!(
        keys(&ov.sections[2]),
        ["perf-img", "fanout:checkout-api", "lsp-fix", "indexer"]
    );
    let Card::Fanout { members, .. } = &ov.sections[2].cards[1] else {
        panic!("Fan-out card");
    };
    assert_eq!(members.len(), 3);
    assert_eq!(ov.sections[2].cards[1].status_text(NOW), "3 running");
    let Card::Agent(t) = &ov.sections[0].cards[1] else {
        panic!()
    };
    assert_eq!(t.parent.as_deref(), Some("planner"));
    // Quiet: everything while it fits (no cap from the app).
    assert_eq!(
        keys(&ov.sections[3]),
        ["seed-data", "dev-server", "etl-dry", "scratch"]
    );
    assert_eq!(ov.sections[3].more, 0);
    assert_eq!((ov.total, ov.workspaces), (16, 5));
    let chips: Vec<String> = ov.counts.chips().into_iter().map(|c| c.2).collect();
    assert_eq!(
        chips,
        [
            "2 need input",
            "1 failed",
            "1 interrupted",
            "2 finished, unseen",
            "6 running",
            "4 quiet"
        ]
    );
    assert_eq!(ov.per_workspace["acme"], 9);
    assert_eq!(ov.sections[0].cards[1].status_text(NOW), "needs input · 4m");
    assert_eq!(ov.sections[0].cards[2].status_text(NOW), "failed · 12m");
}

#[test]
fn r117_a_quiet_folds_only_past_the_room_left() {
    // The app has room for 2 Quiet rows of 1 card: 2 shown, `+2 more`.
    let q = Query {
        quiet_cap: Some(2),
        ..Default::default()
    };
    let ov = open(&q);
    assert_eq!(keys(&ov.sections[3]), ["seed-data", "dev-server"]);
    assert_eq!(ov.sections[3].more, 2);
    // The band's count is all of them, folded or not.
    assert_eq!(ov.sections[3].total(), 4);
    // Room for all 4 (or more): nothing folded.
    let q = Query {
        quiet_cap: Some(4),
        ..Default::default()
    };
    assert_eq!(open(&q).sections[3].more, 0);
    // Unfolded by its `+n more`: all, whatever the cap.
    let mut q = Query {
        quiet_cap: Some(2),
        ..Default::default()
    };
    q.unfolded.insert(Band::Quiet);
    let ov = open(&q);
    assert_eq!(ov.sections[3].cards.len(), 4);
    assert_eq!(ov.sections[3].more, 0);
    let mut many: Vec<Fact> = (0..11)
        .map(|i| fact(&format!("r{i:02}"), Running, 10 + i, false, "ACME", "x"))
        .collect();
    many.push(fact("w", Waiting, 5, true, "ACME", "x"));
    let ov = Overview::build(&many, &Query::default(), &workspaces());
    assert_eq!(ov.sections[1].cards.len(), RUNNING_SHOWN);
    assert_eq!(ov.sections[1].more, 3);
    // Needs you never folds.
    assert_eq!(ov.sections[0].more, 0);
}

#[test]
fn r117_b_frozen_while_open_then_reordered_on_reopen() {
    let q = Query::default();
    let ov = open(&q);
    let mut facts = mockup_scene();
    let p = facts.iter_mut().find(|f| f.pane == "perf-img").unwrap();
    p.status = Some(Waiting);
    p.since = NOW;
    let again = ov.refresh(&facts, &q, &workspaces());
    assert_eq!(again.find("perf-img"), Some((2, 0)));
    let Card::Agent(f) = again.card((2, 0)).unwrap() else {
        panic!()
    };
    assert_eq!(f.status, Some(Waiting));
    assert_eq!(listed(&again), listed(&ov));
    // A new Agent goes after the known ones of its band.
    facts.push(fact("new-one", Waiting, 0, true, "oss", "x"));
    let again = ov.refresh(&facts, &q, &workspaces());
    assert_eq!(again.sections[0].cards.last().unwrap().key(), "new-one");
    // Reopened: perf-img is newest in Needs you.
    let reopened = Overview::build(&facts, &q, &workspaces());
    assert_eq!(reopened.find("perf-img"), Some((0, 1)));
    assert_eq!(reopened.sections[0].cards[0].key(), "new-one");
}

#[test]
fn r117_c_filter_by_words_workspace_and_band() {
    let find = |text: &str| {
        let q = Query {
            text: text.into(),
            ..Default::default()
        };
        listed(&open(&q))
    };
    assert_eq!(find("mig"), ["migrate"]);
    assert_eq!(find("globex"), ["migrate", "indexer", "etl-dry"]);
    // `err` is in `error` and in `interrupted`.
    assert_eq!(find("err"), ["migrate", "docs-bot"]);
    assert_eq!(find("failed"), ["migrate"]);
    assert_eq!(find("fixtures"), ["tests"]);
    assert_eq!(find("viet"), ["docs-bot"]);
    assert_eq!(find("VIỆT"), ["docs-bot"]);
    assert_eq!(find("api 2"), ["fanout:checkout-api"]);
    let q = Query {
        workspace: Some("globex".into()),
        ..Default::default()
    };
    assert_eq!(listed(&open(&q)), ["migrate", "indexer", "etl-dry"]);
    let q = Query {
        band: Some(Band::Running),
        ..Default::default()
    };
    assert_eq!(open(&q).sections.len(), 1);
    let q = Query {
        text: "zzz".into(),
        ..Default::default()
    };
    assert_eq!(
        open(&q).empty_line(&q).as_deref(),
        Some("No Agent matches \"zzz\" · Esc clears")
    );
    let none = Overview::build(&[], &Query::default(), &workspaces());
    assert_eq!(
        none.empty_line(&Query::default()).as_deref(),
        Some("No Agents running · Ctrl+Shift+P > Spawn Agent")
    );
}

#[test]
fn r117_c_by_workspace_columns_worst_first() {
    let q = Query {
        group: Group::Workspace,
        ..Default::default()
    };
    let ov = open(&q);
    let titles: Vec<&str> = ov.sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["ACME", "globex", "personal", "oss", "default"]);
    assert_eq!(keys(&ov.sections[1]), ["migrate", "indexer", "etl-dry"]);
    assert_eq!(ov.sections[0].cards[0].key(), "tests");
}

#[test]
fn r117_d_arrows_tab_flip_and_first_selection() {
    let ov = open(&Query::default());
    let g = Group::Status;
    let s = |at, m| step(&ov, 4, g, at, m);
    assert_eq!(initial(&ov, None), Some((0, 0)));
    assert_eq!(s((0, 3), Move::Right), (1, 0));
    assert_eq!(s((0, 0), Move::Left), (3, 3));
    assert_eq!(s((0, 1), Move::Down), (1, 1));
    assert_eq!(s((1, 1), Move::Down), (2, 1));
    assert_eq!(s((2, 3), Move::Up), (1, 1));
    assert_eq!(s((3, 1), Move::Down), (0, 1));
    assert_eq!(s((0, 2), Move::NextSection), (1, 0));
    assert_eq!(s((0, 2), Move::PrevSection), (3, 0));
    // One press and release: stays open.
    let mut f = Flip::opened();
    assert!(!f.release());
    // Three presses: the third card, then jump.
    let mut f = Flip::opened();
    let mut at = initial(&ov, None).unwrap();
    for _ in 0..2 {
        assert!(f.press());
        at = s(at, Move::Next);
    }
    assert!(f.release());
    assert_eq!(ov.card(at).unwrap().key(), "migrate");
    assert!(!f.press());
    // Nothing needs you: the Agent focused before.
    let calm: Vec<Fact> = mockup_scene()
        .into_iter()
        .filter(|f| band_of(f) != Band::NeedsYou)
        .collect();
    let ov = Overview::build(&calm, &Query::default(), &workspaces());
    let at = initial(&ov, Some("indexer")).unwrap();
    assert_eq!(ov.card(at).unwrap().key(), "indexer");
    assert_eq!(initial(&ov, None), Some((0, 0)));
    // By Workspace: Left / Right between columns, Up / Down inside.
    let q = Query {
        group: Group::Workspace,
        ..Default::default()
    };
    let ov = open(&q);
    let w = |at, m| step(&ov, 4, Group::Workspace, at, m);
    assert_eq!(w((0, 5), Move::Right), (1, 2));
    assert_eq!(w((1, 0), Move::Up), (1, 2));
    assert_eq!(w((0, 0), Move::Left), (4, 0));
}

#[test]
fn r117_e_quiet_after_ten_minutes_of_an_unchanged_screen() {
    let mut f = fact("lsp-fix", Running, 1000, false, "oss", "x");
    f.changed_at = Some(NOW - 599);
    assert_eq!(
        Card::Agent(Box::new(f.clone())).status_text(NOW),
        "running · 16m"
    );
    f.changed_at = Some(NOW - 600);
    assert_eq!(
        Card::Agent(Box::new(f.clone())).status_text(NOW),
        "running · quiet 10m"
    );
    // Only a running Agent is ever quiet.
    f.status = Some(Waiting);
    assert_eq!(
        Card::Agent(Box::new(f)).status_text(NOW),
        "needs input · 16m"
    );
}

#[test]
fn facts_list_every_agent_pane_of_every_workspace() {
    let mut ws = State::new(std::path::Path::new("/home/u"));
    let a = ws
        .spawn(
            crate::agents::SpawnSpec {
                name: Some("lead".into()),
                agent: Some("claude".into()),
                status: Some(Waiting),
                now: NOW,
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
    ws.spawn(
        crate::agents::SpawnSpec {
            caller: Some(a.clone()),
            name: Some("child".into()),
            agent: Some("codex".into()),
            status: Some(Running),
            now: NOW,
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    // A plain shell is not an Agent.
    ws.spawn(crate::agents::SpawnSpec::default(), &Default::default())
        .unwrap();
    ws.notify(&a, None, "asked which fixtures", NOW);
    let facts = facts(&ws, &|_| "Scratch / ~".into());
    let names: Vec<&str> = facts.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["lead", "child"]);
    assert_eq!(facts[0].note.as_deref(), Some("asked which fixtures"));
    assert!(facts[0].unread);
    assert_eq!(facts[1].parent.as_deref(), Some("lead"));
    assert_eq!(facts[1].workspace, "default");
}

#[test]
fn r117_e_restored_agent_without_status_reads_idle_without_an_age() {
    // Session Restore resumes an Agent with no status and no `since`.
    let mut f = fact("codex", Running, 0, false, "default", "Scratch");
    f.status = None;
    f.since = 0;
    assert_eq!(Card::Agent(Box::new(f.clone())).status_text(NOW), "idle");
    // A status whose time is unknown shows no age either.
    f.status = Some(Done);
    assert_eq!(Card::Agent(Box::new(f)).status_text(NOW), "done");
}

#[test]
fn facts_name_an_unnamed_pane_like_the_navigator() {
    let mut ws = State::new(std::path::Path::new("/home/u"));
    ws.spawn(
        crate::agents::SpawnSpec {
            agent: Some("codex".into()),
            now: NOW,
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    let facts = facts(&ws, &|_| "Scratch".into());
    assert_eq!(facts[0].name, "codex");
}

/// A Codex screen: the answer, then its prompt line and footer.
const CODEX_SCREEN: &str = "\
• Served the prototype again, HTTP 200.
  Worked for 26s • Oct 10 at 3:41 PM
  ↳ Recap: the prototype is served at
           http://localhost:8766/index.html again.

› Ask Codex to do anything
  GPT-6-Astra medium · ~/workspaces/personal/RADE · Review the UI…
  ? for shortcuts                                   ⚠ 3 warnings · f2 to view
";

/// A Claude Code screen: the answer, then its input box and status lines.
const CLAUDE_SCREEN: &str = "\
✻ Cooked for 56m 1s · done Tuesday, Oct 6, 10:42 AM
※ recap: fixing code navigation; 0.7.1 is released. Next: install it
  and run the live tests.
                                                  Not logged in · Run /login
────────────────────────────────────────────────────────────────────────
❯
────────────────────────────────────────────────────────────────────────
    Opus 5.5 (1M context) ⚡medium   ~/workspaces/personal/RADE   redesign…
   ctx 54%
  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 2 agents
";

#[test]
fn r117_h_tail_skips_the_agents_input_box_and_footer() {
    assert_eq!(
        tail_of(CODEX_SCREEN, 3),
        [
            "  Worked for 26s • Oct 10 at 3:41 PM",
            "  ↳ Recap: the prototype is served at",
            "           http://localhost:8766/index.html again.",
        ]
    );
    assert_eq!(
        tail_of(CLAUDE_SCREEN, 3),
        // The right-aligned notice over the box is chrome too.
        [
            "✻ Cooked for 56m 1s · done Tuesday, Oct 6, 10:42 AM",
            "※ recap: fixing code navigation; 0.7.1 is released. Next: install it",
            "  and run the live tests.",
        ]
    );
    // A plain program: its last lines, tabs as spaces, blank lines skipped.
    assert_eq!(tail_of("one\n\ntwo\na\tb\n\n", 3), ["one", "two", "a    b"]);
    // Only chrome on screen: nothing rather than the footer.
    assert!(tail_of("› Ask Codex to do anything\n  ? for shortcuts\n", 3).is_empty());
}

/// The 1440 × 900 Overview: the body under the chips, above the hints.
const BODY: Rect = Rect {
    x: 40,
    y: 120,
    w: 1360,
    h: 740,
};

fn card_at(x: i32, y: i32) -> Rect {
    Rect {
        x,
        y,
        w: 333,
        h: 150,
    }
}

fn clear_of(a: Rect, b: Rect) -> bool {
    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
}

fn inside(a: Rect, b: Rect) -> bool {
    a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h
}

#[test]
fn r117_n_preview_right_left_below_above_clamped_never_over_its_card() {
    let w = preview_width(1440);
    assert_eq!(w, 600);
    assert_eq!(preview_width(1000), 460);

    // Column 1: right of the card, top-aligned with it.
    let c = card_at(40, 140);
    let p = preview_place(c, BODY, (w, 420));
    assert_eq!(
        (p.side, p.rect.x, p.rect.y),
        (Side::Right, 40 + 333 + PREVIEW_GAP, 140)
    );
    assert_eq!((p.rect.w, p.rect.h), (600, 420));

    // The last column: no room on the right → on the left.
    let c = card_at(1067, 140);
    let p = preview_place(c, BODY, (w, 420));
    assert_eq!(
        (p.side, p.rect.x, p.rect.y),
        (Side::Left, 1067 - PREVIEW_GAP - 600, 140)
    );

    // Low on the screen: still beside the card, pushed up into the body.
    let c = card_at(40, 780);
    let p = preview_place(c, BODY, (w, 420));
    assert_eq!((p.side, p.rect.y), (Side::Right, 120 + 740 - 420));

    // Taller than the body: as tall as the body.
    let p = preview_place(card_at(40, 140), BODY, (w, 2000));
    assert_eq!((p.rect.y, p.rect.h), (120, 740));

    // Narrow (900 px, 2 columns at 200 %): neither side fits → below the
    // card's row, else above it, x clamped into the body.
    let body = Rect {
        x: 40,
        y: 160,
        w: 820,
        h: 480,
    };
    let w = preview_width(900);
    let c = Rect {
        x: 455,
        y: 180,
        w: 405,
        h: 150,
    };
    let p = preview_place(c, body, (w, 260));
    assert_eq!(p.side, Side::Below);
    assert_eq!((p.rect.x, p.rect.y), (860 - w, 180 + 150 + PREVIEW_GAP));
    let c = Rect { y: 470, ..c };
    let p = preview_place(c, body, (w, 260));
    assert_eq!((p.side, p.rect.y), (Side::Above, 470 - PREVIEW_GAP - 260));
    // Room for it neither below nor above: beside, narrower (≥ 300 px)…
    let c = Rect { y: 400, ..c };
    let p = preview_place(c, body, (w, 400));
    assert_eq!((p.side, p.rect.x), (Side::Left, 40));
    assert_eq!((p.rect.w, p.rect.h), (455 - PREVIEW_GAP - 40, 400));
    // … else (one column) the larger of below / above, shortened.
    let one = Rect {
        x: 40,
        y: 400,
        w: 820,
        h: 150,
    };
    let p = preview_place(one, body, (w, 400));
    assert_eq!(p.side, Side::Above);
    assert_eq!((p.rect.y, p.rect.h), (160, 400 - PREVIEW_GAP - 160));

    // Every case: inside the body, never over its card.
    for (c, body, win) in [
        (card_at(40, 140), BODY, 1440),
        (card_at(383, 500), BODY, 1440),
        (card_at(1067, 780), BODY, 1440),
        (card_at(725, 120), BODY, 1440),
        (
            Rect {
                x: 40,
                y: 170,
                w: 405,
                h: 36,
            },
            body,
            900,
        ),
        (
            Rect {
                x: 455,
                y: 600,
                w: 405,
                h: 36,
            },
            body,
            900,
        ),
        (
            Rect {
                x: 40,
                y: 300,
                w: 820,
                h: 150,
            },
            body,
            900,
        ),
    ] {
        for h in [120, 420, 900] {
            let p = preview_place(c, body, (preview_width(win), h));
            assert!(inside(p.rect, body), "{c:?} {h}: {p:?} outside");
            assert!(clear_of(p.rect, c), "{c:?} {h}: {p:?} over its card");
        }
    }
}

#[test]
fn r117_n_preview_text_keeps_paragraphs_and_cuts_the_input_box() {
    let mut screen = String::new();
    for i in 1..=60 {
        screen.push_str(&format!("line {i}\n"));
        if i % 10 == 0 {
            screen.push_str("\n\n\n");
        }
    }
    screen.push_str("────────────────────────────────────────\n❯ \n────────────────────────────────────────\n  ? for shortcuts\n");
    let p = preview_of(&screen, PREVIEW_LINES);
    assert_eq!(p.len(), PREVIEW_LINES);
    assert_eq!(p.last().map(String::as_str), Some("line 60"));
    // Runs of blank lines become one; none at either end.
    assert!(!p[0].is_empty());
    assert!(p.windows(2).all(|w| !(w[0].is_empty() && w[1].is_empty())));
    assert!(p.iter().any(String::is_empty));
    // Codex: its prompt line and footer cut.
    assert_eq!(
        preview_of(CODEX_SCREEN, 24).last().map(String::as_str),
        tail_of(CODEX_SCREEN, 1).last().map(String::as_str)
    );
}
