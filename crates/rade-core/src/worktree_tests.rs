use super::*;
use crate::agents::SpawnSpec;
use crate::config::Limits;
use std::path::PathBuf;

fn with_x() -> State {
    let mut w = State::new(Path::new("/home/u"));
    w.add_source(Path::new("/r/x"), None).unwrap();
    w
}

fn gw(path: &str, branch: Option<&str>) -> GitWorktree {
    GitWorktree {
        path: path.into(),
        branch: branch.map(Into::into),
        ..Default::default()
    }
}

#[test]
fn r12_add_find_select_and_new_terminals_start_in_the_worktree() {
    let mut w = with_x();
    let (p, wt) = w
        .add_worktree("x", "feat-x", Path::new("/d/wt/x/feat-x"), None)
        .unwrap();
    assert_eq!(p, "x");
    assert_eq!(
        w.find_worktree("x", Some("feat-x")).unwrap(),
        (p.clone(), wt.clone())
    );
    assert_eq!(w.find_worktree("x", Some(&wt)).unwrap().1, wt);
    assert!(w.find_worktree("x", Some("nope")).is_err());
    assert!(
        w.add_worktree("x", "feat-x", Path::new("/o"), None)
            .is_err(),
        "names unique"
    );
    assert!(w
        .add_worktree("general", "g", Path::new("/o"), None)
        .is_err());
    assert!(!w.is_main_worktree("x", &wt));

    w.select_worktree("x", "feat-x").unwrap();
    assert_eq!(w.active().1.id, wt);
    // R12 AC / R1.4: a new Terminal's cwd is the Worktree root.
    let (_, pane) = w.new_tab(&p, &wt, None, None).unwrap();
    assert_eq!(
        w.pane(&pane).unwrap().cwd(),
        &PathBuf::from("/d/wt/x/feat-x")
    );
    // RADE_WORKTREE = its name (pane_infos).
    let info = w.pane_infos().into_iter().find(|i| i.pane == pane).unwrap();
    assert_eq!(info.worktree, "feat-x");
}

#[test]
fn r12_3_remove_takes_its_panes_and_never_the_main_checkout() {
    let mut w = with_x();
    let (p, wt) = w.add_worktree("x", "f", Path::new("/d/f"), None).unwrap();
    let (_, pane) = w.new_tab(&p, &wt, None, None).unwrap();
    w.select_worktree("x", "f").unwrap();
    let main = w.source("x").unwrap().worktrees[0].id.clone();
    assert!(w.remove_worktree("x", &main).is_err());
    let removed = w.remove_worktree("x", "f").unwrap();
    assert_eq!(removed.tabs[0].pane_ids(), vec![pane.clone()]);
    assert!(w.pane(&pane).is_none());
    assert_eq!(w.active().1.id, main, "falls back to the main checkout");
}

#[test]
fn r12_add_after_an_import_of_the_same_folder_keeps_it() {
    // M13 race: `rade worktree list` imported the new Worktree between
    // `git worktree add` and the model update.
    let mut w = with_x();
    let same = |a: &Path, b: &Path| a == b;
    let got = w.import_worktrees("x", &[gw("/d/feat", Some("feat"))], &same);
    assert_eq!(got, ["feat"]);
    let (_, id) = w.find_worktree("x", Some("feat")).unwrap();
    let r = w.add_worktree("x", "feat", Path::new("/d/feat"), Some("make".into()));
    assert_eq!(r.unwrap().1, id, "the imported one, not an error");
    assert_eq!(w.source("x").unwrap().worktrees.len(), 2);
    assert_eq!(w.worktree("x", &id).unwrap().setup.as_deref(), Some("make"));
    // Another folder under a taken name is still refused.
    assert!(w
        .add_worktree("x", "feat", Path::new("/d/other"), None)
        .is_err());
}

#[test]
fn r12_imports_worktrees_created_outside_rade() {
    let mut w = with_x();
    w.add_worktree("x", "mine", Path::new("/d/mine"), None)
        .unwrap();
    let listed = vec![
        gw("/r/x", Some("main")),    // the Source itself
        gw("/d/mine", Some("mine")), // known
        gw("/o/hotfix", Some("fix/1")),
        gw("/o/detached", None),
        gw("/o/n", Some("new")),     // reserved → new-2
        gw("/o/mine", Some("mine")), // name taken → mine-2
        GitWorktree {
            prunable: true,
            ..gw("/o/gone", Some("gone"))
        },
        GitWorktree {
            bare: true,
            ..gw("/o/bare", None)
        },
    ];
    let same = |a: &Path, b: &Path| a == b;
    let added = w.import_worktrees("x", &listed, &same);
    assert_eq!(added, ["fix/1", "detached", "new-2", "mine-2"]);
    assert!(
        w.import_worktrees("x", &listed, &same).is_empty(),
        "idempotent"
    );
    assert!(w.import_worktrees("general", &listed, &same).is_empty());
}

#[test]
fn r12_2_setup_runs_once_in_the_first_terminal_and_survives_a_restart() {
    let mut w = with_x();
    let (p, wt) = w
        .add_worktree("x", "s", Path::new("/d/s"), Some("make deps".into()))
        .unwrap();
    assert!(w
        .add_worktree("x", "blank", Path::new("/d/b"), Some("  ".into()))
        .is_ok());
    assert_eq!(
        w.worktree("x", &w.find_worktree("x", Some("blank")).unwrap().1)
            .unwrap()
            .setup,
        None
    );
    // Pending setup is saved (the app may quit before a Terminal opens).
    let json = serde_json::to_string(&w).unwrap();
    let mut w: State = serde_json::from_str(&json).unwrap();
    assert_eq!(
        w.worktree(&p, &wt).unwrap().setup.as_deref(),
        Some("make deps")
    );

    // The first Terminal (here a spawned Agent) takes it; later ones don't.
    let first = w
        .spawn(
            SpawnSpec {
                source: Some("x".into()),
                worktree: Some("s".into()),
                agent: Some("fake".into()),
                command: Some("fake-agent".into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    assert_eq!(w.pane(&first).unwrap().setup.as_deref(), Some("make deps"));
    assert_eq!(
        w.pane(&first).unwrap().command(),
        Some("fake-agent"),
        "model command unchanged"
    );
    let second = w.split(&first, crate::SplitDir::Down, None, None).unwrap();
    assert_eq!(w.pane(&second).unwrap().setup, None);
    assert_eq!(w.worktree(&p, &wt).unwrap().setup, None);
    // Transient on the Pane: a restored Pane never re-runs it.
    let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
    assert_eq!(back.pane(&first).unwrap().setup, None);
}

#[test]
fn r12_2_setup_runs_before_the_program_or_is_typed_into_the_shell() {
    let (c, input) = setup_launch("npm ci && echo 'ok'", Some("codex 'hi'"));
    let c = c.unwrap();
    assert!(input.is_none());
    assert!(c.contains("sh -c 'npm ci && echo '\\''ok'\\'''"), "{c}");
    assert!(c.ends_with("\ncodex 'hi'"), "{c}");
    // A shell keeps its own startup (shell integration); setup is typed.
    assert_eq!(setup_launch("make", None), (None, Some("make\n".into())));
    // It runs: setup output, failure notice, then the program.
    let run = |setup: &str| {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(setup_launch(setup, Some("echo PROGRAM")).0.unwrap())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let out = run("echo SETUP-OUT; exit 4");
    let (a, b, c) = (
        out.find("SETUP-OUT").unwrap(),
        out.find("setup failed (exit 4)").unwrap(),
        out.find("PROGRAM").unwrap(),
    );
    assert!(a < b && b < c, "{out}");
    assert!(!out.contains(SETUP_DONE));
    let out = run("true");
    assert!(
        out.find(SETUP_DONE).unwrap() < out.find("PROGRAM").unwrap(),
        "{out}"
    );
}

#[test]
fn r10_3_worktree_status_is_worst_of_its_panes() {
    let mut w = with_x();
    let (p, wt) = w.add_worktree("x", "f", Path::new("/d/f"), None).unwrap();
    assert_eq!(w.worktree_status(&p, &wt), None);
    let a = w
        .spawn(
            SpawnSpec {
                source: Some("x".into()),
                worktree: Some("f".into()),
                agent: Some("fake".into()),
                status: Some(AgentStatus::Running),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    w.set_status(&a, Some(AgentStatus::Waiting), None, None, 1)
        .unwrap();
    assert_eq!(w.worktree_status(&p, &wt), Some(AgentStatus::Waiting));
    let main = w.source("x").unwrap().worktrees[0].id.clone();
    assert_eq!(w.worktree_status(&p, &main), None);
}

#[test]
fn r99_recorded_base_in_a_project_is_never_none() {
    // A link without `base`: the Worktree records the main branch it was
    // cut from, not nothing (which would follow a later set-base).
    assert_eq!(
        recorded_base(true, None, Some("main")).as_deref(),
        Some("main")
    );
    // The link's base or `--base` wins over the main branch.
    assert_eq!(
        recorded_base(true, Some("develop"), Some("main")).as_deref(),
        Some("develop")
    );
    // No main branch known: nothing to record.
    assert_eq!(recorded_base(true, None, None), None);
    // Outside a Project only `--base` is recorded.
    assert_eq!(recorded_base(false, None, Some("main")), None);
    assert_eq!(
        recorded_base(false, Some("hotfix"), Some("main")).as_deref(),
        Some("hotfix")
    );
}
