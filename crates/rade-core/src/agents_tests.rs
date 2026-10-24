use super::*;
use crate::layout::SplitDir;
use crate::model::PaneLoc;
use std::path::Path;

fn limits() -> Limits {
    Limits::default()
}

/// State with Source x and a root Agent Pane "lead" alone in its Tab.
fn with_lead() -> (State, String) {
    let mut w = State::new(Path::new("/home/u"));
    w.add_source(Path::new("/tmp/x"), None).unwrap();
    let lead = w
        .spawn(
            SpawnSpec {
                source: Some("x".into()),
                agent: Some("claude".into()),
                name: Some("lead".into()),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap();
    (w, lead)
}

/// Like [`child`] with `max_children` raised past [`crate::layout::GRID_TILES`].
fn child_many(w: &mut State, caller: &str, name: &str) -> Result<String> {
    let l = Limits {
        max_children: 12,
        ..limits()
    };
    w.spawn(
        SpawnSpec {
            caller: Some(caller.into()),
            agent: Some("fake".into()),
            name: Some(name.into()),
            ..Default::default()
        },
        &l,
    )
}

fn child(w: &mut State, caller: &str, name: &str) -> Result<String> {
    w.spawn(
        SpawnSpec {
            caller: Some(caller.into()),
            agent: Some("fake".into()),
            name: Some(name.into()),
            ..Default::default()
        },
        &limits(),
    )
}

fn loc(w: &State, pane: &str) -> PaneLoc {
    w.locate(pane).unwrap()
}

fn tab<'a>(w: &'a State, pane: &str) -> &'a crate::Tab {
    w.tab(&loc(w, pane)).unwrap()
}

#[test]
fn root_spawn_opens_a_new_tab_without_parent() {
    let (mut w, lead) = with_lead();
    let p = w.pane(&lead).unwrap();
    assert_eq!((p.parent.clone(), p.depth), (None, 0));
    let other = w
        .spawn(
            SpawnSpec {
                source: Some("x".into()),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap();
    assert_ne!(loc(&w, &other).tab, loc(&w, &lead).tab);
}

#[test]
fn r2_3_spawn_from_non_agent_caller_opens_new_tab_but_records_parent() {
    let mut w = State::new(Path::new("/home/u"));
    let wt = w.sources[0].active_worktree.clone();
    let (_, shell) = w.new_tab("general", &wt, None, None).unwrap();
    let c = w
        .spawn(
            SpawnSpec {
                caller: Some(shell.clone()),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap();
    assert_ne!(loc(&w, &c).tab, loc(&w, &shell).tab);
    assert_eq!(w.pane(&c).unwrap().parent.as_deref(), Some(shell.as_str()));
    assert_eq!(w.pane(&c).unwrap().depth, 1);
}

#[test]
fn r2_a_spawn_from_agent_goes_to_parents_tab_in_grid_keeping_focus() {
    let (mut w, lead) = with_lead();
    let rev = child(&mut w, &lead, "rev").unwrap();
    let t = tab(&w, &rev);
    assert_eq!(t.id, loc(&w, &lead).tab, "same Tab as the Parent");
    assert_eq!(t.layout.signature(), format!("R({lead},{rev})"));
    assert_eq!(t.grid.as_deref(), Some(lead.as_str()));
    assert_eq!(t.focused, lead, "the Parent keeps focus");
    let p = w.pane(&rev).unwrap();
    assert_eq!((p.parent.as_deref(), p.depth), (Some(lead.as_str()), 1));
    assert_eq!(w.resolve_pane("rev", Some(&lead)).unwrap(), rev);
}

#[test]
fn r2_4_up_to_eight_tiled_then_title_bars_and_closing_promotes() {
    let (mut w, lead) = with_lead();
    let kids: Vec<String> = (1..=10)
        .map(|i| child_many(&mut w, &lead, &format!("c{i}")).unwrap())
        .collect();
    let t = tab(&w, &lead);
    assert_eq!(t.layout.panes().len(), 9, "Parent + 8 tiled");
    let bars: Vec<&str> = t.title_bars.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(bars, [kids[8].as_str(), kids[9].as_str()]);
    assert_eq!(w.pane_infos().len(), 11, "Title Bar Panes are listed");
    // Closing a tiled Subagent promotes the first Title Bar.
    w.close_pane(&kids[1]).unwrap();
    let t = tab(&w, &lead);
    assert_eq!(t.title_bars.len(), 1);
    assert!(t.layout.pane(&kids[8]).is_some());
    // Closing a Title Bar Pane just removes it.
    w.close_pane(&kids[9]).unwrap();
    assert!(tab(&w, &lead).title_bars.is_empty());
}

#[test]
fn r2_4_six_subagents_are_all_tiled() {
    let (mut w, lead) = with_lead();
    for i in 1..=6 {
        child(&mut w, &lead, &format!("c{i}")).unwrap();
    }
    let t = tab(&w, &lead);
    assert!(t.title_bars.is_empty());
    assert_eq!(t.layout.panes().len(), 7);
}

#[test]
fn r2_4_clicking_title_bar_zooms_it() {
    let (mut w, lead) = with_lead();
    let kids: Vec<String> = (1..=9)
        .map(|i| child_many(&mut w, &lead, &format!("c{i}")).unwrap())
        .collect();
    w.focus_pane(&kids[8]).unwrap();
    let t = tab(&w, &lead);
    assert_eq!(t.zoomed.as_deref(), Some(kids[8].as_str()));
    assert_eq!(t.focused, kids[8]);
    w.toggle_zoom(&t.id.clone()).unwrap();
    assert_eq!(tab(&w, &lead).zoomed, None);
}

#[test]
fn r2_4_manual_split_or_resize_stops_auto_arranging() {
    let (mut w, lead) = with_lead();
    let c1 = child(&mut w, &lead, "c1").unwrap();
    w.split(&c1, SplitDir::Down, None, None).unwrap();
    assert_eq!(tab(&w, &lead).grid, None);
    let focus = tab(&w, &lead).focused.clone();
    // The next Subagent goes right of its Parent instead of re-tiling.
    let c2 = child(&mut w, &lead, "c2").unwrap();
    let t = tab(&w, &lead);
    assert!(t
        .layout
        .signature()
        .starts_with(&format!("R(R({lead},{c2})")));
    assert_eq!(t.focused, focus, "spawn does not move focus");

    let (mut w, lead) = with_lead();
    child(&mut w, &lead, "c1").unwrap();
    let tid = loc(&w, &lead).tab;
    assert!(w.set_split_ratio(&tid, &[], 0.4, false));
    assert!(tab(&w, &lead).grid.is_some(), "window resize is not manual");
    assert!(w.set_split_ratio(&tid, &[], 0.3, true));
    assert_eq!(tab(&w, &lead).grid, None);
}

#[test]
fn agent_caller_in_hand_arranged_tab_splits_right_of_itself() {
    let (mut w, lead) = with_lead();
    let other = w.split(&lead, SplitDir::Down, None, None).unwrap();
    w.focus_pane(&other).unwrap();
    let c = child(&mut w, &lead, "c").unwrap();
    let t = tab(&w, &lead);
    assert_eq!(t.layout.signature(), format!("D(R({lead},{c}),{other})"));
    assert_eq!(t.focused, other);
}

#[test]
fn explicit_tab_new_or_split_overrides_the_grid() {
    let (mut w, lead) = with_lead();
    let n = w
        .spawn(
            SpawnSpec {
                caller: Some(lead.clone()),
                tab: Some(TabChoice::New),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap();
    assert_ne!(loc(&w, &n).tab, loc(&w, &lead).tab);
    let d = w
        .spawn(
            SpawnSpec {
                caller: Some(lead.clone()),
                split: Some(SplitDir::Down),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap();
    assert_eq!(tab(&w, &lead).layout.signature(), format!("D({lead},{d})"));
    let e = w
        .spawn(
            SpawnSpec {
                caller: Some(lead.clone()),
                worktree: Some("new".into()),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap_err();
    assert!(e.message.contains("must be created first"));
}

#[test]
fn r2_b_ninth_live_subagent_is_refused_and_exited_ones_free_a_slot() {
    let (mut w, lead) = with_lead();
    let kids: Vec<String> = (1..=8)
        .map(|i| child(&mut w, &lead, &format!("c{i}")).unwrap())
        .collect();
    let e = child(&mut w, &lead, "c9").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Limit);
    assert_eq!(e.kind.exit_code(), 5);
    assert!(
        e.message.contains("limits.max_children = 8"),
        "{}",
        e.message
    );
    w.set_status(&kids[0], Some(AgentStatus::Exited), None, None, 0)
        .unwrap();
    assert!(child(&mut w, &lead, "c9").is_ok());
}

#[test]
fn r2_5_depth_is_limited_to_three() {
    let (mut w, lead) = with_lead();
    let d1 = child(&mut w, &lead, "d1").unwrap();
    let d2 = child(&mut w, &d1, "d2").unwrap();
    let d3 = child(&mut w, &d2, "d3").unwrap();
    assert_eq!(w.pane(&d3).unwrap().depth, 3);
    let e = child(&mut w, &d3, "d4").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Limit);
    assert!(e.message.contains("limits.max_depth = 3"), "{}", e.message);
    // Configurable.
    let deep = Limits {
        max_depth: 4,
        max_children: 8,
    };
    assert!(w
        .spawn(
            SpawnSpec {
                caller: Some(d3),
                ..Default::default()
            },
            &deep
        )
        .is_ok());
}

#[test]
fn closing_a_parent_keeps_its_subagents() {
    let (mut w, lead) = with_lead();
    let a = child(&mut w, &lead, "a").unwrap();
    let b = child(&mut w, &lead, "b").unwrap();
    w.close_pane(&lead).unwrap();
    assert!(w.locate(&a).is_some() && w.locate(&b).is_some());
    let t = tab(&w, &a);
    assert_eq!(t.grid, None);
    assert_eq!(t.layout.signature(), format!("D({a},{b})"));
}

#[test]
fn last_tiled_pane_closing_promotes_a_title_bar_instead_of_closing_tab() {
    let (mut w, lead) = with_lead();
    let kids: Vec<String> = (1..=5)
        .map(|i| child(&mut w, &lead, &format!("c{i}")).unwrap())
        .collect();
    for id in std::iter::once(&lead).chain(&kids[..4]) {
        assert!(!w.close_pane(id).unwrap().tab_closed);
    }
    let t = tab(&w, &kids[4]);
    assert_eq!(t.layout.signature(), kids[4]);
    assert!(t.title_bars.is_empty());
}

#[test]
fn r10_status_hooks_session_id_and_waiting_notification() {
    let (mut w, lead) = with_lead();
    let rev = child(&mut w, &lead, "rev").unwrap();
    w.set_status(&rev, None, Some("sid-1".into()), Some("codex".into()), 1)
        .unwrap();
    let p = w.pane(&rev).unwrap();
    assert_eq!(
        (p.status, p.session_id.as_deref(), p.agent.as_deref()),
        (None, Some("sid-1"), Some("codex"))
    );
    w.set_status(&rev, Some(AgentStatus::Waiting), None, None, 2)
        .unwrap();
    w.set_status(&rev, Some(AgentStatus::Waiting), None, None, 3)
        .unwrap();
    assert_eq!(w.unread(&rev), 1, "one Notification per entry into waiting");
    assert!(w.notifications[0].text.contains("rev needs input"));
    let info = w.pane_infos().into_iter().find(|i| i.pane == rev).unwrap();
    assert_eq!(info.status.as_deref(), Some("waiting"));
    assert_eq!(info.parent.as_deref(), Some(lead.as_str()));
    assert_eq!(info.agent.as_deref(), Some("codex"));
    assert_eq!(info.unread, 1);
    w.focus_pane(&rev).unwrap();
    assert_eq!(w.unread(&rev), 0, "focus reads it");
    assert!(w
        .set_status("p_404", Some(AgentStatus::Done), None, None, 0)
        .is_err());
    w.prepare_restore(&BTreeMap::new(), 0);
    assert_eq!(w.pane(&rev).unwrap().status, None);
}

#[test]
fn r3_1_done_and_error_transitions_raise_one_notification_each() {
    let (mut w, lead) = with_lead();
    let rev = child(&mut w, &lead, "rev").unwrap();
    let texts =
        |w: &State| -> Vec<String> { w.notifications.iter().map(|n| n.text.clone()).collect() };
    w.set_status(&rev, Some(AgentStatus::Running), None, None, 1)
        .unwrap();
    assert!(texts(&w).is_empty(), "running is not news");
    w.set_status(&rev, Some(AgentStatus::Done), None, None, 10)
        .unwrap();
    assert_eq!(texts(&w), ["rev finished"]);
    // A repeated done (e.g. two Stop hooks) without a new turn: nothing.
    w.set_status(&rev, Some(AgentStatus::Done), None, None, 20)
        .unwrap();
    w.set_status(&rev, None, Some("sid".into()), None, 21)
        .unwrap();
    assert_eq!(texts(&w).len(), 1);
    w.set_status(&rev, Some(AgentStatus::Running), None, None, 30)
        .unwrap();
    w.set_status(&rev, Some(AgentStatus::Done), None, None, 40)
        .unwrap();
    assert_eq!(
        texts(&w),
        ["rev finished", "rev finished"],
        "new turn, new entry"
    );
    w.set_status(&rev, Some(AgentStatus::Error), None, None, 50)
        .unwrap();
    w.set_status(&rev, Some(AgentStatus::Error), None, None, 60)
        .unwrap();
    assert_eq!(texts(&w).last().unwrap(), "rev failed");
    assert_eq!(texts(&w).len(), 3);
    w.set_status(&rev, Some(AgentStatus::Exited), None, None, 70)
        .unwrap();
    assert_eq!(texts(&w).len(), 3, "exited raises none");
    assert!(w.notifications.iter().all(|n| n.pane == rev));
}

#[test]
fn status_parse() {
    for s in AgentStatus::ALL {
        assert_eq!(AgentStatus::parse(s.as_str()).unwrap(), s);
    }
    assert_eq!(
        AgentStatus::parse("busy").unwrap_err().kind,
        ErrorKind::Usage
    );
}

#[test]
fn r10_4_wait_conditions() {
    use AgentStatus::*;
    let z = Duration::ZERO;
    let five = Duration::from_secs(5);
    let done = WaitUntil::parse("done").unwrap();
    assert_eq!(done.check(None, "", z, five), None);
    assert_eq!(done.check(Some(Running), "", z, five), None);
    assert_eq!(done.check(Some(Done), "", z, five).as_deref(), Some("done"));
    assert_eq!(
        done.check(Some(Error), "", z, five).as_deref(),
        Some("error")
    );
    assert_eq!(
        done.check(Some(Exited), "", z, five).as_deref(),
        Some("exited")
    );
    let waiting = WaitUntil::parse("waiting").unwrap();
    assert_eq!(waiting.check(Some(Done), "", z, five), None);
    assert!(waiting.check(Some(Waiting), "", z, five).is_some());
    let idle = WaitUntil::parse("idle").unwrap();
    assert_eq!(
        idle.check(Some(Running), "", Duration::from_secs(4), five),
        None
    );
    assert!(idle.check(None, "", five, five).is_some());
    let re = WaitUntil::parse("/REPLY: [A-Z ]+/").unwrap();
    assert!(re.needs_text() && !done.needs_text());
    assert_eq!(
        re.check(None, "x\nREPLY: SAY HI\n", z, five).as_deref(),
        Some("REPLY: SAY HI")
    );
    assert_eq!(re.check(Some(Done), "nothing", z, five), None);
    assert_eq!(WaitUntil::parse("soon").unwrap_err().kind, ErrorKind::Usage);
    assert_eq!(WaitUntil::parse("/(/").unwrap_err().kind, ErrorKind::Usage);
}

#[test]
fn grid_state_survives_serde() {
    let (mut w, lead) = with_lead();
    for i in 1..=5 {
        child(&mut w, &lead, &format!("c{i}")).unwrap();
    }
    w.set_status(&lead, Some(AgentStatus::Done), Some("s".into()), None, 0)
        .unwrap();
    let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
    assert_eq!(back, w);
}

/// Spawn like `pane.spawn` does: the launch command carries the prompt.
fn spawn_cmd(w: &mut State, caller: Option<&str>, agent: Option<&str>, cmd: &str) -> String {
    w.spawn(
        SpawnSpec {
            source: Some("x".into()),
            caller: caller.map(Into::into),
            agent: agent.map(Into::into),
            command: Some(cmd.into()),
            status: Some(AgentStatus::Running),
            ..Default::default()
        },
        &limits(),
    )
    .unwrap()
}

#[test]
fn r11_2_restore_resumes_or_relaunches_without_prompt_and_never_reruns_cmd() {
    let presets = crate::preset::builtins();
    let (mut w, lead) = with_lead();
    w.set_status(&lead, Some(AgentStatus::Done), Some("u-1".into()), None, 0)
        .unwrap();
    w.tab_mut(&loc(&w, &lead))
        .unwrap()
        .pane_mut(&lead)
        .unwrap()
        .kind = crate::PaneKind::Terminal {
        cwd: "/tmp/x".into(),
        command: Some("claude --session-id 'u-1' 'do the whole task'".into()),
    };
    let rev = spawn_cmd(
        &mut w,
        Some(&lead),
        Some("codex"),
        "codex --no-daemon 'review'",
    );
    let hooked = spawn_cmd(
        &mut w,
        Some(&lead),
        Some("codex"),
        "codex --no-daemon 'fix'",
    );
    w.set_status(
        &hooked,
        Some(AgentStatus::Running),
        Some("th-9".into()),
        None,
        0,
    )
    .unwrap();
    // Session started (id known) but never prompted: nothing to resume.
    let fresh = spawn_cmd(
        &mut w,
        Some(&lead),
        Some("claude"),
        "claude --session-id 'u-2'",
    );
    w.set_status(&fresh, None, Some("u-2".into()), Some("claude".into()), 0)
        .unwrap();
    let cmd = spawn_cmd(&mut w, None, None, "make deploy");
    let gone = spawn_cmd(&mut w, None, Some("removed-preset"), "old-agent 'go'");

    w.prepare_restore(&presets, 99);
    let get = |id: &str| w.pane(id).unwrap().clone();
    assert_eq!(
        get(&lead).command(),
        Some("claude --resume 'u-1'"),
        "resume with the RADE-chosen id"
    );
    assert_eq!(
        get(&hooked).command(),
        Some("codex --no-daemon resume 'th-9'"),
        "resume with the hook-reported id"
    );
    assert_eq!(
        get(&rev).command(),
        Some("codex --no-daemon"),
        "no id: Preset without the prompt"
    );
    assert_eq!(
        get(&fresh).command(),
        Some("claude"),
        "never-prompted session: Preset, not a failing resume"
    );
    assert_eq!(get(&cmd).command(), None, "--cmd comes back as a shell");
    assert_eq!(get(&gone).command(), None, "unknown Preset: shell");
    assert_eq!(
        get(&rev).parent.as_deref(),
        Some(lead.as_str()),
        "Parent link kept"
    );
    assert_eq!(get(&lead).session_id.as_deref(), Some("u-1"));
    let statuses: Vec<(String, Option<String>)> = w
        .pane_infos()
        .into_iter()
        .map(|i| (i.pane, i.status))
        .collect();
    assert!(
        statuses
            .iter()
            .all(|(p, s)| [&rev, &hooked, &fresh].contains(&p)
                == (s.as_deref() == Some("interrupted"))),
        "statuses cleared, the mid-turn Agents interrupted: {statuses:?}"
    );
    assert!(
        !serde_json::to_string(&w).unwrap().contains("task'"),
        "no prompt left to replay"
    );
}

#[test]
fn b1_close_removes_a_running_agent_pane_at_once() {
    let (mut w, lead) = with_lead();
    let rev = spawn_cmd(
        &mut w,
        Some(&lead),
        Some("codex"),
        "codex --no-daemon 'review'",
    );
    w.close_pane(&lead).unwrap();
    w.close_pane(&rev).unwrap();
    assert!(w.pane(&lead).is_none() && w.pane(&rev).is_none());
    let saved = serde_json::to_string(&w).unwrap();
    assert!(!saved.contains(&format!("\"{lead}\"")) && !saved.contains(&format!("\"{rev}\"")));
}

#[test]
fn r11_2_restore_reruns_the_recorded_invocation() {
    let presets = crate::preset::builtins();
    let (mut w, lead) = with_lead();
    let inv = Invocation {
        argv: [
            "claude",
            "--dangerously-skip-permissions",
            "--model",
            "opus",
            "do x",
        ]
        .map(String::from)
        .to_vec(),
        env: BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "/h/.claude-work".into())]),
    };
    w.set_invocation(&lead, inv.clone()).unwrap();
    let fresh = spawn_cmd(&mut w, None, Some("claude"), "claude");
    w.set_invocation(&fresh, inv).unwrap();
    w.set_status(&lead, Some(AgentStatus::Done), Some("u-1".into()), None, 0)
        .unwrap();
    assert!(
        serde_json::to_string(&w).unwrap().contains("claude-work"),
        "persisted"
    );
    w.prepare_restore(&presets, 99);
    assert_eq!(
        w.pane(&lead).unwrap().command(),
        Some("CLAUDE_CONFIG_DIR='/h/.claude-work' 'claude' '--dangerously-skip-permissions' '--model' 'opus' --resume 'u-1'")
    );
    assert_eq!(
        w.pane(&fresh).unwrap().command(),
        Some("CLAUDE_CONFIG_DIR='/h/.claude-work' 'claude' '--dangerously-skip-permissions' '--model' 'opus'"),
        "no turn: same invocation, no resume, no prompt"
    );
}

#[test]
fn m11_restore_marks_mid_turn_agents_interrupted() {
    let presets = crate::preset::builtins();
    let (mut w, lead) = with_lead();
    let mut ids = Vec::new();
    for (i, st) in [
        Some(AgentStatus::Running),
        Some(AgentStatus::Waiting),
        Some(AgentStatus::Done),
        Some(AgentStatus::Error),
        Some(AgentStatus::Interrupted),
    ]
    .into_iter()
    .enumerate()
    {
        let id = spawn_cmd(&mut w, Some(&lead), Some("codex"), "codex 'x'");
        w.set_status(&id, st, Some(format!("th-{i}")), None, 1)
            .unwrap();
        ids.push(id);
    }
    // A --cmd Pane mid-"turn" and a Pane whose Preset is gone: shells, no status.
    let cmd = spawn_cmd(&mut w, None, None, "make");
    w.set_status(&cmd, Some(AgentStatus::Running), None, None, 1)
        .unwrap();
    let gone = spawn_cmd(&mut w, None, Some("removed"), "old 'go'");
    w.set_status(&gone, Some(AgentStatus::Running), None, None, 1)
        .unwrap();
    w.prepare_restore(&presets, 50);
    let st = |id: &str| w.pane(id).unwrap().status;
    let i = Some(AgentStatus::Interrupted);
    assert_eq!(
        ids.iter().map(|p| st(p)).collect::<Vec<_>>(),
        [i, i, None, None, i]
    );
    assert_eq!(w.pane(&ids[0]).unwrap().status_since, Some(50));
    assert_eq!(
        w.pane(&ids[0]).unwrap().command(),
        Some("codex --no-daemon resume 'th-0'"),
        "resumed into its conversation"
    );
    assert_eq!((st(&cmd), st(&gone)), (None, None));
    // No Notification for it, and `wait --until done` keeps waiting.
    assert!(w
        .notifications
        .iter()
        .all(|n| !n.text.contains("interrupted")));
    let until = WaitUntil::parse("done").unwrap();
    assert_eq!(
        until.check(i, "", Duration::ZERO, Duration::from_secs(5)),
        None
    );
    assert!(WaitUntil::parse("interrupted").is_ok());
    // It is a reason to look: ranked above running, below waiting.
    assert_eq!(crate::status::worst([Some(AgentStatus::Running), i]), i);
    assert_eq!(
        crate::status::worst([Some(AgentStatus::Waiting), i]),
        Some(AgentStatus::Waiting)
    );
}

#[test]
fn m11_interrupted_ignores_a_done_before_a_new_turn_and_keeps_its_since() {
    let presets = crate::preset::builtins();
    let (mut w, lead) = with_lead();
    let id = spawn_cmd(&mut w, Some(&lead), Some("codex"), "codex 'x'");
    w.set_status(&id, None, Some("th".into()), None, 1).unwrap();
    w.prepare_restore(&presets, 10);
    w.prepare_restore(&presets, 20);
    let p = w.pane(&id).unwrap();
    assert_eq!(
        (p.status, p.status_since),
        (Some(AgentStatus::Interrupted), Some(10))
    );
    // A hook saying `done` on session start: still interrupted.
    w.set_status(&id, Some(AgentStatus::Done), None, None, 21)
        .unwrap();
    assert_eq!(w.pane(&id).unwrap().status, Some(AgentStatus::Interrupted));
    // A new turn, then done: done.
    w.set_status(&id, Some(AgentStatus::Running), None, None, 22)
        .unwrap();
    w.set_status(&id, Some(AgentStatus::Done), None, None, 23)
        .unwrap();
    assert_eq!(w.pane(&id).unwrap().status, Some(AgentStatus::Done));
}

#[test]
fn r104_m_agents_spawned_beside_a_link_checkout_terminal_start_there() {
    let (mut w, lead) = with_lead();
    let pc = Path::new("/tmp/w/.worktrees/.projects/shop/x");
    let ws = w.active_workspace().to_string();
    w.workspaces
        .iter_mut()
        .find(|x| x.id == ws)
        .unwrap()
        .link_checkouts
        .push(crate::checkout::LinkCheckout {
            project: "shop".into(),
            source: "x".into(),
            base: "develop".into(),
            checkout: "auto".into(),
            path: pc.into(),
        });
    assert!(w.set_pane_cwd(&lead, pc));
    w.select_source("x").unwrap();
    w.focus_pane(&lead).unwrap();
    let cwd = |w: &State, p: &str| w.pane(p).unwrap().cwd().to_path_buf();
    // The caller's child, and the `+` menu's Agent (nothing named).
    let c = child(&mut w, &lead, "c").unwrap();
    assert_eq!(cwd(&w, &c), pc);
    let ui = w
        .spawn(
            SpawnSpec {
                agent: Some("claude".into()),
                tab: Some(TabChoice::New),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap();
    assert_eq!(cwd(&w, &ui), pc);
    // `rade spawn --source x` from outside: the Worktree root.
    let out = w
        .spawn(
            SpawnSpec {
                source: Some("x".into()),
                tab: Some(TabChoice::New),
                ..Default::default()
            },
            &limits(),
        )
        .unwrap();
    assert_eq!(cwd(&w, &out), Path::new("/tmp/x"));
}
