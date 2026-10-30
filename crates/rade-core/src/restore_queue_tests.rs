use super::*;
use crate::agents::SpawnSpec;
use crate::config::Limits;
use std::path::Path;

fn spawn(st: &mut State, source: &str, name: &str) -> String {
    st.spawn(
        SpawnSpec {
            source: Some(source.into()),
            agent: Some("claude".into()),
            name: Some(name.into()),
            ..Default::default()
        },
        &Limits::default(),
    )
    .unwrap()
}

/// Workspaces `default` (Source `x`), `a` (`api`, `web`) and `b` (`ops`),
/// each Source with Agents; `web` has a second Tab and a Viewer.
fn scene() -> (State, Vec<String>) {
    let mut st = State::new(Path::new("/home/u"));
    st.add_source(Path::new("/tmp/x"), None).unwrap();
    st.add_workspace(Path::new("/w/a"), "A").unwrap();
    st.add_workspace(Path::new("/w/b"), "B").unwrap();
    st.add_source(Path::new("/w/a/api"), None).unwrap();
    st.add_source(Path::new("/w/a/web"), None).unwrap();
    st.add_source(Path::new("/w/b/ops"), None).unwrap();
    let x1 = spawn(&mut st, "x", "x1");
    let api1 = spawn(&mut st, "api", "api1");
    let web1 = spawn(&mut st, "web", "web1");
    let web2 = spawn(&mut st, "web", "web2");
    let ops1 = spawn(&mut st, "ops", "ops1");
    // A shell Tab in `web` (not the one on screen) and a Viewer beside web1.
    let wt = st.source("web").unwrap().active_worktree.clone();
    let (_, shell) = st.new_tab("web", &wt, None, None).unwrap();
    st.open_viewer(&web1, Path::new("/w/a/web/README.md"), None, false)
        .unwrap();
    st.switch_workspace("a").unwrap();
    st.select_source("web").unwrap();
    st.focus_pane(&web2).unwrap();
    (st, vec![x1, api1, web1, web2, ops1, shell])
}

#[test]
fn r130_a_the_shown_pane_first_then_its_tab_worktree_workspace_and_the_rest() {
    let (st, ids) = scene();
    let [x1, api1, web1, web2, ops1, shell] = <[String; 6]>::try_from(ids).unwrap();
    let order = order(&st);
    // The focused Pane, its Tab, the Worktree's other Tab, then the rest of
    // Workspace A, then `b`'s and `default`'s in Workspace order.
    assert_eq!(order[0], web2);
    assert_eq!(order[1], web1);
    assert_eq!(order[2], shell);
    assert_eq!(order[3], api1);
    let rest: Vec<&str> = order[4..].iter().map(String::as_str).collect();
    assert_eq!(rest, [x1.as_str(), ops1.as_str()]);
    // Only Terminals: the Viewer is built at once.
    assert_eq!(order.len(), 6);
    assert!(order.iter().all(|p| !st.pane(p).unwrap().is_viewer()));
}

#[test]
fn r130_a_a_paused_source_starts_nothing() {
    let (mut st, ids) = scene();
    st.sources
        .iter_mut()
        .find(|s| s.id == "api")
        .unwrap()
        .paused = true;
    assert!(!order(&st).contains(&ids[1]));
}

#[test]
fn r130_b_agents_are_spread_and_a_promoted_pane_leaves_the_queue() {
    let ids: Vec<String> = (1..=5).map(|i| format!("p{i}")).collect();
    let mut q = Queue::new(ids);
    // p3 is a shell: no gap after it.
    let agent = |p: &str| p != "p3";
    assert_eq!(q.next(0, agent), Step::Start("p1".into()));
    assert_eq!(q.next(100, agent), Step::Wait(200));
    // p4 is shown at 150 ms: it starts out of turn and leaves the queue.
    assert!(q.promote("p4"));
    assert!(!q.promote("p4"));
    assert_eq!(q.next(300, agent), Step::Start("p2".into()));
    assert_eq!(q.next(600, agent), Step::Start("p3".into()));
    assert_eq!(q.next(600, agent), Step::Start("p5".into()));
    assert_eq!(q.next(900, agent), Step::Done);
    assert!(q.is_empty());
}
