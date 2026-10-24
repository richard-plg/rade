use super::*;
use crate::config::Limits;
use crate::fanout::{MemberSpawn, Start};
use crate::preset::{builtins, Preset};
use crate::{PaneKind, SpawnSpec};
use std::collections::BTreeMap;
use std::path::Path;

/// Source `x` (/tmp/x) with Tabs a, b, c, d in its main Worktree.
fn four_tabs() -> (State, String, Vec<String>) {
    let mut w = State::new(Path::new("/home/u"));
    let x = w.add_source(Path::new("/tmp/x"), None).unwrap();
    let wt = w.source(&x).unwrap().active_worktree.clone();
    let tabs = (0..4)
        .map(|_| w.new_tab(&x, &wt, None, None).unwrap().0)
        .collect();
    (w, x, tabs)
}

/// A two-Agent Fan-out in Source x; its Tab id.
fn add_fanout(w: &mut State) -> String {
    let mut members = Vec::new();
    for i in 1..=2 {
        let name = format!("fanout-1-{i}");
        let (_, wt) = w
            .add_worktree("x", &name, &Path::new("/d").join(&name), None)
            .unwrap();
        members.push(MemberSpawn {
            label: format!("fake-{i}"),
            agent: "fake".into(),
            worktree: wt,
            command: "fake 'go'".into(),
            session_id: None,
            account: None,
        });
    }
    let s = Start {
        source: "x".into(),
        id: "fanout-1".into(),
        prompt: "p".into(),
        base: "main".into(),
        base_commit: "abc".into(),
        members,
        caller: None,
        now: 1,
    };
    w.start_fanout(s, &Limits::default()).unwrap().0
}

#[test]
fn r19_1_close_others_and_right() {
    let (w, _, t) = four_tabs();
    let set = w.tab_close_set(&t[1], CloseMode::Others).unwrap();
    assert_eq!(set.close, [t[0].clone(), t[2].clone(), t[3].clone()]);
    assert!(set.kept.is_empty());
    let set = w.tab_close_set(&t[1], CloseMode::Right).unwrap();
    assert_eq!(set.close, [t[2].clone(), t[3].clone()]);
    assert!(w
        .tab_close_set(&t[3], CloseMode::Right)
        .unwrap()
        .close
        .is_empty());
    assert_eq!(
        w.tab_close_set(&t[2], CloseMode::Only).unwrap().close,
        [t[2].clone()]
    );
    assert_eq!(
        w.tab_close_set("t_999", CloseMode::Only).unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn r19_1_fanout_tabs_are_never_closed_on_the_side() {
    let (mut w, _, t) = four_tabs();
    let f = add_fanout(&mut w);
    let set = w.tab_close_set(&t[0], CloseMode::Others).unwrap();
    assert_eq!(set.close, t[1..].to_vec());
    assert_eq!(set.kept, [(f.clone(), "fanout-1".to_string())]);
    let set = w.tab_close_set(&t[3], CloseMode::Right).unwrap();
    assert!(set.close.is_empty());
    assert_eq!(set.kept.len(), 1);
    let e = w.tab_close_set(&f, CloseMode::Only).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(e.message.contains("rade fanout discard fanout-1"), "{e}");
    // Its running Agents are what a close of the Fan-out Tab would stop.
    assert_eq!(w.busy_agents(&[f]).len(), 2);
}

#[test]
fn r19_1_busy_agents_lists_running_and_waiting_only() {
    let (mut w, x, t) = four_tabs();
    let spawn = |w: &mut State, name: &str| {
        w.spawn(
            SpawnSpec {
                source: Some(x.clone()),
                agent: Some("claude".into()),
                name: Some(name.into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap()
    };
    let a = spawn(&mut w, "a");
    let b = spawn(&mut w, "b");
    let c = spawn(&mut w, "c");
    w.set_status(&a, Some(AgentStatus::Running), None, None, 1)
        .unwrap();
    w.set_status(&b, Some(AgentStatus::Waiting), None, None, 1)
        .unwrap();
    w.set_status(&c, Some(AgentStatus::Done), None, None, 1)
        .unwrap();
    let tabs: Vec<String> = [&a, &b, &c]
        .iter()
        .map(|p| w.locate(p).unwrap().tab)
        .chain(t)
        .collect();
    let busy = w.busy_agents(&tabs);
    assert_eq!(
        busy,
        [
            (a, "a".to_string(), AgentStatus::Running),
            (b, "b".to_string(), AgentStatus::Waiting)
        ]
    );
}

#[test]
fn r19_1_duplicate_opens_a_shell_at_the_focused_cwd_next_to_it() {
    let (mut w, x, t) = four_tabs();
    let src = w
        .worktree(&x, &w.source(&x).unwrap().active_worktree)
        .unwrap()
        .tabs[1]
        .focused
        .clone();
    w.set_pane_cwd(&src, Path::new("/tmp/x/src"));
    let (dup, pane) = w.duplicate_tab(&t[1]).unwrap();
    let wt = w
        .worktree(&x, &w.source(&x).unwrap().active_worktree)
        .unwrap();
    let ids: Vec<&str> = wt.tabs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, [&t[0], &t[1], &dup, &t[2], &t[3]]);
    assert_eq!(wt.active_tab.as_deref(), Some(dup.as_str()));
    let p = w.pane(&pane).unwrap();
    assert_eq!(p.cwd(), Path::new("/tmp/x/src"));
    assert!(p.command().is_none() && p.agent.is_none(), "a plain shell");
    // A Viewer focused: the Worktree root.
    let file = Path::new("/tmp/x/README.md");
    let v = w.open_viewer(&src, file, None, true).unwrap();
    w.focus_pane(&v).unwrap();
    let (_, p2) = w.duplicate_tab(&t[1]).unwrap();
    assert_eq!(w.pane(&p2).unwrap().cwd(), Path::new("/tmp/x"));
}

#[test]
fn r19_1_resolve_tab_by_id_or_name() {
    let (mut w, _, t) = four_tabs();
    w.rename_tab(&t[2], "build").unwrap();
    assert_eq!(w.resolve_tab(&t[0], None).unwrap(), t[0]);
    assert_eq!(w.resolve_tab("build", None).unwrap(), t[2]);
    assert_eq!(w.resolve_tab("Tab 1", None).unwrap(), t[0]);
    w.rename_tab(&t[3], "build").unwrap();
    assert_eq!(
        w.resolve_tab("build", None).unwrap_err().kind,
        ErrorKind::Ambiguous
    );
    // The caller's Worktree first.
    let y = w.add_source(Path::new("/tmp/y"), None).unwrap();
    let ywt = w.source(&y).unwrap().active_worktree.clone();
    let (yt, yp) = w.new_tab(&y, &ywt, None, None).unwrap();
    w.rename_tab(&yt, "build").unwrap();
    assert_eq!(w.resolve_tab("build", Some(&yp)).unwrap(), yt);
    assert_eq!(
        w.resolve_tab("nope", None).unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn r19_4_source_rename_keeps_the_id_and_refuses_clashes() {
    let (mut w, x, _) = four_tabs();
    let y = w.add_source(Path::new("/tmp/y"), None).unwrap();
    assert_eq!(w.rename_source(&x, "  Shop  ").unwrap(), x);
    assert_eq!(w.source(&x).unwrap().name, "Shop");
    assert_eq!(w.find_source("Shop").unwrap().id, "x", "id stable");
    for bad in ["", "  ", "y"] {
        assert_eq!(
            w.rename_source(&x, bad).unwrap_err().kind,
            ErrorKind::Usage,
            "{bad:?}"
        );
    }
    w.rename_source(&y, "Why").unwrap();
    assert!(w.rename_source(&x, "Why").is_err());
    assert!(w.rename_source(&x, "Shop").is_ok(), "its own name again");
    // General: a display name too; its id stays `general`.
    w.rename_source("general", "Home").unwrap();
    assert_eq!(w.find_source("Home").unwrap().id, GENERAL_ID);
}

#[test]
fn r19_4_pane_names_unique_per_worktree_and_not_id_like() {
    let (mut w, x, t) = four_tabs();
    let wt = w.source(&x).unwrap().active_worktree.clone();
    let pane = |w: &State, i: usize| w.worktree(&x, &wt).unwrap().tabs[i].focused.clone();
    let (a, b) = (pane(&w, 0), pane(&w, 1));
    w.rename_pane(&a, Some(" server ")).unwrap();
    assert_eq!(w.pane(&a).unwrap().name.as_deref(), Some("server"));
    let e = w.rename_pane(&b, Some("server")).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(e.message.contains(&a), "{e}");
    w.rename_pane(&a, Some("server")).unwrap(); // its own name
    assert!(w.rename_pane(&b, Some("p_12")).is_err());
    w.rename_pane(&b, Some("p_x")).unwrap();
    // Another Worktree may reuse it; the caller's scope still resolves.
    let y = w.add_source(Path::new("/tmp/y"), None).unwrap();
    let ywt = w.source(&y).unwrap().active_worktree.clone();
    let (_, yp) = w.new_tab(&y, &ywt, None, None).unwrap();
    w.rename_pane(&yp, Some("server")).unwrap();
    assert_eq!(w.resolve_pane("server", Some(&yp)).unwrap(), yp);
    // Blank clears it.
    w.rename_pane(&a, Some("  ")).unwrap();
    assert_eq!(w.pane(&a).unwrap().name, None);
    let _ = t;
}

fn presets() -> BTreeMap<String, Preset> {
    let mut p = builtins();
    let mut fake = p["claude"].clone();
    fake.command = "fake".into();
    p.insert("fake".into(), fake);
    p
}

#[test]
fn r19_3_pause_keeps_panes_and_prepares_their_resume() {
    let (mut w, x, _) = four_tabs();
    let agent = w
        .spawn(
            SpawnSpec {
                source: Some(x.clone()),
                agent: Some("claude".into()),
                command: Some("claude 'hi'".into()),
                session_id: Some("s-1".into()),
                status: Some(AgentStatus::Running),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    w.set_status(&agent, Some(AgentStatus::Running), None, None, 5)
        .unwrap();
    let before: Vec<String> = w.pane_infos().into_iter().map(|i| i.pane).collect();
    let presets = presets();
    assert_eq!(
        w.pause_source("x", &|_, _| presets.clone(), false, 10)
            .unwrap(),
        x
    );
    assert!(w.is_paused(&x));
    let after: Vec<String> = w.pane_infos().into_iter().map(|i| i.pane).collect();
    assert_eq!(before, after, "Tabs, Layouts and Panes stay");
    let p = w.pane(&agent).unwrap();
    assert_eq!(p.status, Some(AgentStatus::Interrupted), "cut mid-turn");
    let cmd = p.command().unwrap();
    assert!(
        cmd.contains("--resume") && cmd.contains("s-1") && !cmd.contains("hi"),
        "{cmd}"
    );
    // Its exit hooks can't overwrite that, but ids still land.
    w.set_status(
        &agent,
        Some(AgentStatus::Done),
        Some("s-2".into()),
        None,
        11,
    )
    .unwrap();
    let p = w.pane(&agent).unwrap();
    assert_eq!(p.status, Some(AgentStatus::Interrupted));
    assert_eq!(p.session_id.as_deref(), Some("s-2"));
    // Nothing new opens in it.
    let wt = w.source(&x).unwrap().active_worktree.clone();
    let e = w.new_tab(&x, &wt, None, None).unwrap_err();
    assert!(e.message.contains("rade source resume x"), "{e}");
    assert!(w.split(&agent, crate::SplitDir::Right, None, None).is_err());
    let e = w
        .spawn(
            SpawnSpec {
                source: Some(x.clone()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    // Paused survives a restart (state.json) and restore keeps it as is.
    let mut back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
    back.prepare_restore(&presets, 20);
    assert!(back.is_paused(&x));
    assert_eq!(
        back.pane(&agent).unwrap().status,
        Some(AgentStatus::Interrupted)
    );
    assert_eq!(back.pane(&agent).unwrap().status_since, Some(10));
    // Pausing again does nothing; resume lets it run again.
    assert!(back
        .pause_source("x", &|_, _| presets.clone(), false, 30)
        .is_ok());
    back.resume_source("x").unwrap();
    assert!(!back.is_paused(&x));
    assert!(back.new_tab(&x, &wt, None, None).is_ok());
    assert!(back
        .pane(&agent)
        .unwrap()
        .command()
        .unwrap()
        .contains("--resume"));
}

#[test]
fn r19_3_general_cant_pause_and_a_working_fanout_needs_force() {
    let (mut w, x, _) = four_tabs();
    let presets = presets();
    assert_eq!(
        w.pause_source("general", &|_, _| presets.clone(), true, 1)
            .unwrap_err()
            .kind,
        ErrorKind::Usage
    );
    let tab = add_fanout(&mut w);
    let e = w
        .pause_source("x", &|_, _| presets.clone(), false, 2)
        .unwrap_err();
    assert!(e.message.contains("--force"), "{e}");
    assert!(!w.is_paused(&x));
    w.pause_source("x", &|_, _| presets.clone(), true, 3)
        .unwrap();
    let f = w.fanout("fanout-1").unwrap();
    assert_eq!(f.done(), 0, "cut Agents are not finished");
    assert_eq!(w.interrupted_members(f).len(), 2);
    let panes = w
        .worktree(&x, &w.source(&x).unwrap().worktrees[0].id)
        .unwrap()
        .tab(&tab)
        .unwrap()
        .pane_ids();
    assert!(panes
        .iter()
        .all(|p| matches!(w.pane(p).unwrap().kind, PaneKind::Terminal { .. })));
}

#[test]
fn r19_3_another_sources_fanout_is_untouched_by_a_pause() {
    let (mut w, _, _) = four_tabs();
    add_fanout(&mut w);
    let y = w.add_source(Path::new("/tmp/y"), None).unwrap();
    w.pause_source(&y, &|_, _| presets(), false, 1).unwrap();
    let f = w.fanout("fanout-1").unwrap();
    assert_eq!(f.done(), 0);
    assert!(w.interrupted_members(f).is_empty(), "x's Agents still run");
}

#[test]
fn r19_5_search_count_like_ghostty() {
    assert_eq!(search_count(-1, -1), "");
    assert_eq!(search_count(17, -1), "0/17");
    assert_eq!(search_count(17, 2), "3/17");
    assert_eq!(search_count(0, -1), "0/0");
}

#[test]
fn r19_3_a_paused_sources_fanout_and_setup_are_left_intact() {
    let (mut w, x, _) = four_tabs();
    let tab = add_fanout(&mut w);
    let main = w.source(&x).unwrap().worktrees[0].id.clone();
    let members = w.worktree(&x, &main).unwrap().tab(&tab).unwrap().pane_ids();
    for p in &members {
        w.set_status(p, Some(AgentStatus::Done), None, None, 2)
            .unwrap();
    }
    // A Worktree whose first Terminal still has a setup to run.
    let (_, wt) = w
        .add_worktree("x", "side", Path::new("/d/side"), Some("npm ci".into()))
        .unwrap();
    w.pause_source("x", &|_, _| presets(), false, 3).unwrap();
    let judge = crate::fanout::JudgeSpawn {
        agent: "fake".into(),
        command: "fake".into(),
        session_id: None,
        dir: "/tmp/j".into(),
        read_only: true,
        now: 4,
        account: None,
    };
    assert!(w.add_judge("fanout-1", judge).is_err());
    assert!(
        w.fanout("fanout-1").unwrap().judge.is_none(),
        "no Judge added"
    );
    assert!(w.move_to_own_worktree(&members[0]).is_err());
    assert!(w.pane(&members[0]).is_some(), "the Agent stays in its Tab");
    assert!(w.new_tab("x", &wt, None, None).is_err());
    assert_eq!(
        w.worktree("x", &wt).unwrap().setup.as_deref(),
        Some("npm ci"),
        "setup still pending"
    );
}

#[test]
fn r19_9_closing_a_terminal_asks_and_names_a_working_agent() {
    let (mut w, x, _) = four_tabs();
    let a = w
        .spawn(
            SpawnSpec {
                source: Some(x.clone()),
                agent: Some("codex".into()),
                name: Some("fixer".into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    let shell = w.split(&a, crate::SplitDir::Right, None, None).unwrap();
    let p = w.close_pane_prompt(&shell).unwrap();
    assert_eq!(p.heading, "Close Pane “Terminal”?");
    assert_eq!(p.body, "Its processes will be stopped.");
    assert_eq!(p.action, "Close Pane");
    w.set_status(&a, Some(AgentStatus::Running), None, None, 1)
        .unwrap();
    let p = w.close_pane_prompt(&a).unwrap();
    assert_eq!(p.heading, "Close Pane “fixer”?");
    assert!(w.set_pane_title(&shell, "~/src/x"));
    assert_eq!(
        w.close_pane_prompt(&shell).unwrap().heading,
        "Close Pane “Terminal — x”?"
    );
    assert_eq!(
        p.body,
        "fixer (codex) is still working and will be stopped."
    );
    // A Viewer has nothing to lose: it closes at once.
    let v = w
        .open_viewer(&a, Path::new("/tmp/x/README.md"), None, true)
        .unwrap();
    assert_eq!(w.close_pane_prompt(&v), None);
    assert_eq!(w.close_pane_prompt("p_999"), None);
}

#[test]
fn r19_9_closing_tabs_always_asks() {
    let (mut w, x, t) = four_tabs();
    let set = w.tab_close_set(&t[0], CloseMode::Only).unwrap();
    let label = w
        .worktrees()
        .flat_map(|(_, wt)| &wt.tabs)
        .find(|tab| tab.id == t[0])
        .map(|tab| tab.label());
    let p = w.close_tabs_prompt(&set);
    assert_eq!(p.heading, format!("Close Tab “{}”?", label.unwrap()));
    assert_eq!(p.body, "Its Panes and their processes will be stopped.");
    assert_eq!(p.action, "Close Tab");
    let a = w
        .spawn(
            SpawnSpec {
                source: Some(x.clone()),
                agent: Some("claude".into()),
                name: Some("a".into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    w.set_status(&a, Some(AgentStatus::Waiting), None, None, 1)
        .unwrap();
    let set = w.tab_close_set(&t[0], CloseMode::Others).unwrap();
    let p = w.close_tabs_prompt(&set);
    assert_eq!(p.heading, format!("Close {} Tabs?", set.close.len()));
    let names: String = set
        .close
        .iter()
        .map(|id| {
            let t = w
                .worktrees()
                .flat_map(|(_, wt)| &wt.tabs)
                .find(|t| &t.id == id);
            format!("• {}\n", t.unwrap().label())
        })
        .collect();
    assert_eq!(
        p.body,
        format!("{names}\nThese Agents are still working and will be stopped:\n\n• a — waiting")
    );
    assert_eq!(p.action, "Close Tabs");
}
