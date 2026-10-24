use super::*;
use crate::agents::SpawnSpec;
use crate::config::Limits;
use crate::preset::builtins;
use std::path::Path;

/// Relative luminance (WCAG 2.x) of `#rrggbb`.
fn luminance(hex: &str) -> f64 {
    let c = |i: usize| {
        let x = u8::from_str_radix(&hex[i..i + 2], 16).unwrap() as f64 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * c(1) + 0.7152 * c(3) + 0.0722 * c(5)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn r10_3_palette_reads_on_light_and_dark() {
    // libadwaita view, window, sidebar and RADE's caption background, per
    // style (R20.1).
    const LIGHT: [&str; 4] = ["#ffffff", "#fafafb", "#ebebed", "#f3f3f5"];
    const DARK: [&str; 4] = ["#1d1d20", "#222226", "#2e2e32", "#2a2a2e"];
    for (dark, backgrounds) in [(false, LIGHT), (true, DARK)] {
        let mut colors: Vec<&str> = AgentStatus::ALL.iter().map(|s| s.color(dark)).collect();
        colors.push(if dark {
            NO_STATUS_COLOR.1
        } else {
            NO_STATUS_COLOR.0
        });
        for c in &colors {
            for bg in backgrounds {
                let r = contrast(c, bg);
                assert!(r >= 3.0, "{c} on {bg}: {r:.2}:1");
            }
        }
        let mut unique = colors.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            6,
            "running/waiting/done/error/interrupted distinct; exited = none"
        );
    }
}

#[test]
fn r10_3_worst_of() {
    use AgentStatus::*;
    assert_eq!(worst([None, None]), None);
    assert_eq!(worst([Some(Done), None, Some(Running)]), Some(Running));
    assert_eq!(worst([Some(Running), Some(Waiting)]), Some(Waiting));
    assert_eq!(
        worst([Some(Waiting), Some(Error), Some(Exited)]),
        Some(Error)
    );
    assert_eq!(worst([Some(Exited), Some(Done)]), Some(Done));
    assert_eq!(worst([Some(Exited)]), Some(Exited));
}

fn spawn(w: &mut State, agent: Option<&str>, cmd: Option<&str>) -> String {
    w.spawn(
        SpawnSpec {
            source: Some("x".into()),
            agent: agent.map(Into::into),
            command: cmd.map(Into::into),
            status: agent.map(|_| AgentStatus::Running),
            now: 100,
            ..Default::default()
        },
        &Limits::default(),
    )
    .unwrap()
}

fn ws() -> State {
    let mut w = State::new(Path::new("/home/u"));
    w.add_source(Path::new("/tmp/x"), None).unwrap();
    w.add_source(Path::new("/tmp/y"), None).unwrap();
    w
}

fn status(w: &State, p: &str) -> (Option<AgentStatus>, bool) {
    let p = w.pane(p).unwrap();
    (p.status, p.status_heuristic)
}

#[test]
fn r10_3_tab_and_source_roll_up() {
    let mut w = ws();
    let a = spawn(&mut w, Some("claude"), Some("claude"));
    let b = spawn(&mut w, Some("codex"), Some("codex"));
    assert_eq!(w.source_status("x"), Some(AgentStatus::Running));
    assert_eq!(w.source_status("y"), None, "no Agents");
    w.set_status(&b, Some(AgentStatus::Waiting), None, None, 110)
        .unwrap();
    assert_eq!(w.source_status("x"), Some(AgentStatus::Waiting));
    let tab = w.tab(&w.locate(&a).unwrap()).unwrap();
    assert_eq!(tab.status(), Some(AgentStatus::Running), "a's own Tab");
    assert_eq!(w.pane(&b).unwrap().status_since, Some(110));
    assert_eq!(w.pane(&a).unwrap().status_since, Some(100), "spawn time");
    // Same status again keeps its since.
    w.set_status(&b, Some(AgentStatus::Waiting), None, None, 120)
        .unwrap();
    assert_eq!(w.pane(&b).unwrap().status_since, Some(110));
}

#[test]
fn r10_2_agy_idle_while_running_is_waiting() {
    // agy hooks report running/done only: silence without a done hook
    // means it is asking something.
    let mut w = ws();
    let p = spawn(&mut w, Some("agy"), Some("agy"));
    w.set_status(&p, Some(AgentStatus::Running), None, None, 100)
        .unwrap();
    w.record_hook(&p, None, 100).unwrap();
    let pr = builtins();
    assert_eq!(
        w.apply_signal(&p, Signal::Idle, &pr, 105).unwrap(),
        None,
        "hook grace"
    );
    assert_eq!(
        w.apply_signal(&p, Signal::Idle, &pr, 111).unwrap(),
        Some(AgentStatus::Waiting)
    );
    assert_eq!(status(&w, &p), (Some(AgentStatus::Waiting), true));
    assert_eq!(w.pane(&p).unwrap().status_source(), Some("heuristic"));
    assert_eq!(
        w.notifications.last().unwrap().text,
        format!("{p} needs input")
    );
    // Only from running.
    assert_eq!(w.apply_signal(&p, Signal::Idle, &pr, 130).unwrap(), None);
    // A hook report clears the heuristic mark.
    w.set_status(&p, Some(AgentStatus::Done), None, None, 131)
        .unwrap();
    assert_eq!(status(&w, &p), (Some(AgentStatus::Done), false));
    assert_eq!(w.pane(&p).unwrap().status_source(), Some("hook"));
    let info = w.pane_infos().into_iter().find(|i| i.pane == p).unwrap();
    assert_eq!(
        (info.status_source.as_deref(), info.status_since),
        (Some("hook"), Some(131))
    );
}

#[test]
fn r10_2_agy_attention_while_running_is_waiting() {
    let mut w = ws();
    let p = spawn(&mut w, Some("agy"), Some("agy"));
    w.record_hook(&p, None, 100).unwrap();
    let pr = builtins();
    assert_eq!(
        w.apply_signal(&p, Signal::Attention, &pr, 120).unwrap(),
        Some(AgentStatus::Waiting)
    );
}

#[test]
fn r10_2_trusted_hooks_are_never_second_guessed() {
    let mut w = ws();
    let pr = builtins();
    for agent in ["claude", "opencode"] {
        let p = spawn(&mut w, Some(agent), Some(agent));
        w.record_hook(&p, None, 100).unwrap();
        for s in [Signal::Idle, Signal::Attention] {
            assert_eq!(w.apply_signal(&p, s, &pr, 200).unwrap(), None, "{agent}");
        }
    }
    // Codex (waiting + done hooks): Codex's own OSC 9 after its Stop hook
    // finds `done` and does nothing; while running, neither rule applies.
    let c = spawn(&mut w, Some("codex"), Some("codex"));
    w.record_hook(&c, None, 100).unwrap();
    assert_eq!(
        w.apply_signal(&c, Signal::Attention, &pr, 200).unwrap(),
        None
    );
    assert_eq!(w.apply_signal(&c, Signal::Idle, &pr, 200).unwrap(), None);
}

#[test]
fn r10_2_hooks_that_never_reported_count_as_none() {
    let mut w = ws();
    let pr = builtins();
    // Untrusted Codex hooks: idle → done.
    let c = spawn(&mut w, Some("codex"), Some("codex"));
    assert_eq!(
        w.apply_signal(&c, Signal::Idle, &pr, 110).unwrap(),
        Some(AgentStatus::Done)
    );
    let d = spawn(&mut w, Some("agy"), Some("agy"));
    assert_eq!(
        w.apply_signal(&d, Signal::Attention, &pr, 110).unwrap(),
        Some(AgentStatus::Waiting),
        "hooks never reported: attention → waiting"
    );
    // A user Preset without hooks behaves the same.
    let mut pr2 = pr.clone();
    let mut aider = pr["claude"].clone();
    aider.hooks = HookKind::None;
    pr2.insert("aider".into(), aider);
    let a = spawn(&mut w, Some("aider"), Some("aider"));
    w.record_hook(&a, None, 100).unwrap(); // a report without a status
    assert_eq!(
        w.apply_signal(&a, Signal::Idle, &pr2, 120).unwrap(),
        Some(AgentStatus::Done)
    );
    // …but a status its hooks did report is trusted like a declared hook.
    let b = spawn(&mut w, Some("aider"), Some("aider"));
    w.record_hook(&b, Some(AgentStatus::Done), 100).unwrap();
    w.set_status(&b, Some(AgentStatus::Running), None, None, 100)
        .unwrap();
    assert_eq!(
        w.apply_signal(&b, Signal::Idle, &pr2, 120).unwrap(),
        Some(AgentStatus::Waiting)
    );
}

#[test]
fn r10_2_non_agent_panes_get_no_status() {
    let mut w = ws();
    let pr = builtins();
    let cmd = spawn(&mut w, None, Some("cargo build"));
    let shell = spawn(&mut w, None, None);
    for p in [&cmd, &shell] {
        for s in [Signal::Idle, Signal::Attention, Signal::CommandFinished] {
            assert_eq!(w.apply_signal(p, s, &pr, 200).unwrap(), None);
        }
        assert_eq!(w.pane(p).unwrap().status, None);
    }
}

#[test]
fn r10_2_command_finished_ends_a_hand_started_agent() {
    let mut w = ws();
    let pr = builtins();
    // `clw` typed in a shell Pane: SessionStart reported --agent claude.
    let shell = spawn(&mut w, None, None);
    w.set_status(
        &shell,
        Some(AgentStatus::Done),
        None,
        Some("claude".into()),
        100,
    )
    .unwrap();
    w.record_hook(&shell, None, 100).unwrap();
    assert_eq!(
        w.apply_signal(&shell, Signal::CommandFinished, &pr, 101)
            .unwrap(),
        Some(AgentStatus::Exited),
        "no grace: the process is gone"
    );
    assert_eq!(
        w.apply_signal(&shell, Signal::CommandFinished, &pr, 102)
            .unwrap(),
        None
    );
    // Restore brings it back as a shell, not a resumed Agent.
    assert_eq!(
        crate::agents::restore_command(w.pane(&shell).unwrap(), &pr),
        None
    );
    // `clw` again: its SessionStart report makes it a live Agent again.
    w.set_status(&shell, None, Some("s2".into()), Some("claude".into()), 110)
        .unwrap();
    assert_eq!(w.pane(&shell).unwrap().status, None);
    assert!(crate::agents::restore_command(w.pane(&shell).unwrap(), &pr).is_some());
    w.set_status(&shell, Some(AgentStatus::Exited), None, None, 120)
        .unwrap();
    // …and after that restart it is a plain shell, so the next restart
    // doesn't resume it either.
    let mut again = w.clone();
    again.prepare_restore(&pr, 0);
    let p = again.pane(&shell).unwrap();
    assert_eq!((p.agent.as_deref(), p.command()), (None, None));
    again.prepare_restore(&pr, 0);
    assert_eq!(again.pane(&shell).unwrap().command(), None);
    // A spawned Agent Pane (its own command) ignores it.
    let spawned = spawn(&mut w, Some("claude"), Some("claude"));
    assert_eq!(
        w.apply_signal(&spawned, Signal::CommandFinished, &pr, 103)
            .unwrap(),
        None
    );
}

#[test]
fn r16_4_codex_hint_when_a_turn_runs_30_s_without_hooks() {
    let mut w = ws();
    let pr = builtins();
    let c = spawn(&mut w, Some("codex"), Some("codex")); // running since 100
    assert_eq!(w.codex_untrusted(&pr, 129), None);
    assert_eq!(w.codex_untrusted(&pr, 130), Some(c.clone()));
    // UserPromptSubmit reported (trusted): no hint.
    w.record_hook(&c, Some(AgentStatus::Running), 101).unwrap();
    assert_eq!(w.codex_untrusted(&pr, 200), None);
    // Other Agents never.
    let mut w = ws();
    spawn(&mut w, Some("claude"), Some("claude"));
    assert_eq!(w.codex_untrusted(&pr, 500), None);
}

#[test]
fn status_metadata_is_cleared_on_restore() {
    let mut w = ws();
    let p = spawn(&mut w, Some("agy"), Some("agy"));
    w.apply_signal(&p, Signal::Idle, &builtins(), 200).unwrap();
    w.prepare_restore(&builtins(), 0);
    let p = w.pane(&p).unwrap();
    assert_eq!(
        (p.status, p.status_since, p.status_heuristic, p.hook_at),
        (None, None, false, None)
    );
}

#[test]
fn r10_3_tooltip_helpers() {
    let mut w = ws();
    let a = spawn(&mut w, Some("claude"), Some("claude"));
    let b = spawn(&mut w, Some("codex"), Some("codex"));
    let c = spawn(&mut w, Some("agy"), Some("agy"));
    w.set_status(&c, Some(AgentStatus::Done), None, None, 150)
        .unwrap();
    let panes = || [&a, &b, &c].map(|id| w.pane(id).unwrap());
    assert_eq!(summary(panes()), "2 running · 1 done");
    assert_eq!(
        worst_pane(panes()).unwrap().id,
        a,
        "running since 100, first"
    );
    w.set_status(&b, Some(AgentStatus::Error), None, None, 160)
        .unwrap();
    let panes = || [&a, &b, &c].map(|id| w.pane(id).unwrap());
    assert_eq!(worst_pane(panes()).unwrap().id, b);
    assert_eq!(summary(panes()), "1 error · 1 running · 1 done");
    assert_eq!(summary([]), "");
}

#[test]
fn r10_2_codex_attention_means_its_turn_ended() {
    // Review: Codex's only OSC 9 is the turn-end reply; untrusted hooks must
    // give `done` so `rade wait --until done` returns.
    let mut w = ws();
    let c = spawn(&mut w, Some("codex"), Some("codex"));
    assert_eq!(
        w.apply_signal(&c, Signal::Attention, &builtins(), 110)
            .unwrap(),
        Some(AgentStatus::Done)
    );
}
