use super::*;
use crate::layout::{Direction, SplitDir};
use crate::rpc::ErrorKind;
use std::path::Path;

fn ws() -> State {
    State::new(Path::new("/home/u"))
}

/// State with Source "x" at /tmp/x and one Tab in it.
fn with_x() -> (State, String, String, String) {
    let mut w = ws();
    let id = w.add_source(Path::new("/tmp/x"), None).unwrap();
    let wt = w.source(&id).unwrap().active_worktree.clone();
    let (tab, pane) = w.new_tab(&id, &wt, None, None).unwrap();
    (w, id, tab, pane)
}

fn cwd(w: &State, pane: &str) -> String {
    w.pane(pane).unwrap().cwd().display().to_string()
}

#[test]
fn general_is_always_present_last_and_rooted_at_home() {
    let mut w = ws();
    assert_eq!(w.sources.len(), 1);
    let g = &w.sources[0];
    assert!(g.is_general());
    assert_eq!(g.path, Path::new("/home/u"));
    assert_eq!(g.worktrees[0].name, "main");
    w.add_source(Path::new("/a"), None).unwrap();
    w.add_source(Path::new("/b"), None).unwrap();
    assert_eq!(w.sources.last().unwrap().id, GENERAL_ID);
    assert_eq!(w.sources[0].id, "a");
}

#[test]
fn general_cannot_be_removed_or_moved() {
    let mut w = ws();
    assert_eq!(
        w.remove_source("general").unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(
        w.remove_source("General").unwrap_err().kind,
        ErrorKind::Usage
    );
    assert!(w.move_source("general", 0).is_err());
}

#[test]
fn add_source_names_ids_and_rejects_duplicates() {
    let mut w = ws();
    assert_eq!(
        w.add_source(Path::new("/code/My App"), None).unwrap(),
        "my-app"
    );
    assert_eq!(
        w.add_source(Path::new("/other/my-app"), None).unwrap(),
        "my-app-2"
    );
    assert_eq!(w.add_source(Path::new("/q"), Some("Nice")).unwrap(), "nice");
    assert_eq!(w.source("nice").unwrap().name, "Nice");
    assert_eq!(
        w.add_source(Path::new("/q"), None).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(
        w.add_source(Path::new("rel"), None).unwrap_err().kind,
        ErrorKind::Usage
    );
    // A Source may live in $HOME's subtree or even at $HOME itself.
    assert!(w.add_source(Path::new("/home/u"), Some("home")).is_ok());
}

#[test]
fn remove_source_returns_it_and_falls_back_to_general() {
    let (mut w, id, _, pane) = with_x();
    w.focus_pane(&pane).unwrap();
    assert_eq!(w.active_source, id);
    let removed = w.remove_source("x").unwrap();
    assert_eq!(
        removed.worktrees[0].tabs.len(),
        1,
        "caller closes these terminals"
    );
    assert_eq!(w.active_source, GENERAL_ID);
    assert!(w.locate(&pane).is_none());
    assert_eq!(w.remove_source("x").unwrap_err().kind, ErrorKind::NotFound);
}

#[test]
fn move_source_keeps_general_last() {
    let mut w = ws();
    for p in ["/a", "/b", "/c"] {
        w.add_source(Path::new(p), None).unwrap();
    }
    w.move_source("c", 0).unwrap();
    w.move_source("a", 99).unwrap();
    let ids: Vec<_> = w.sources.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["c", "b", "a", "general"]);
}

#[test]
fn r1_a_new_terminal_cwd_is_source_root() {
    let (w, _, _, pane) = with_x();
    assert_eq!(cwd(&w, &pane), "/tmp/x");
}

#[test]
fn r1_b_general_terminal_cwd_is_home() {
    let mut w = ws();
    let wt = w.sources[0].active_worktree.clone();
    let (_, pane) = w.new_tab(GENERAL_ID, &wt, None, None).unwrap();
    assert_eq!(cwd(&w, &pane), "/home/u");
}

#[test]
fn r1_4_split_uses_worktree_root_not_focused_cwd() {
    let (mut w, _, _, pane) = with_x();
    assert!(w.set_pane_cwd(&pane, Path::new("/tmp/x/deep/dir")));
    let new = w.split(&pane, SplitDir::Right, None, None).unwrap();
    assert_eq!(cwd(&w, &new), "/tmp/x");
}

#[test]
fn new_tab_selects_only_when_none_active() {
    let (mut w, id, t1, _) = with_x();
    let wt = w.source(&id).unwrap().active_worktree.clone();
    let (t2, _) = w
        .new_tab(&id, &wt, Some("htop".into()), Some("mon".into()))
        .unwrap();
    let wtr = w.worktree(&id, &wt).unwrap();
    assert_eq!(wtr.active_tab.as_deref(), Some(t1.as_str()));
    assert_eq!(wtr.tab(&t2).unwrap().name, "Tab 2");
    w.select_tab(&t2).unwrap();
    assert_eq!(
        w.worktree(&id, &wt).unwrap().active_tab.as_deref(),
        Some(t2.as_str())
    );
}

#[test]
fn split_focuses_new_pane_and_clears_zoom() {
    let (mut w, _, tab, pane) = with_x();
    w.toggle_zoom(&tab).unwrap();
    let new = w
        .split(&pane, SplitDir::Down, None, Some("b".into()))
        .unwrap();
    let loc = w.locate(&new).unwrap();
    let t = w.tab(&loc).unwrap();
    assert_eq!(t.focused, new);
    assert_eq!(t.zoomed, None);
    assert_eq!(t.layout.signature(), format!("D({pane},{new})"));
}

#[test]
fn close_pane_collapses_and_last_pane_closes_tab() {
    let (mut w, id, tab, p1) = with_x();
    let p2 = w.split(&p1, SplitDir::Right, None, None).unwrap();
    let c = w.close_pane(&p2).unwrap();
    assert!(!c.tab_closed);
    let t = w.tab(&w.locate(&p1).unwrap()).unwrap();
    assert_eq!(t.focused, p1, "focus falls back to a surviving pane");
    assert_eq!(t.layout.signature(), p1);
    let c = w.close_pane(&p1).unwrap();
    assert!(c.tab_closed);
    assert_eq!(c.loc.tab, tab);
    let wt = &w.source(&id).unwrap().worktrees[0];
    assert!(wt.tabs.is_empty());
    assert_eq!(wt.active_tab, None);
}

#[test]
fn closing_active_tab_selects_next_then_previous() {
    let (mut w, id, t1, _) = with_x();
    let wt = w.source(&id).unwrap().active_worktree.clone();
    let (t2, _) = w.new_tab(&id, &wt, None, None).unwrap();
    let (t3, _) = w.new_tab(&id, &wt, None, None).unwrap();
    w.select_tab(&t2).unwrap();
    w.close_tab(&t2).unwrap();
    assert_eq!(
        w.worktree(&id, &wt).unwrap().active_tab.as_deref(),
        Some(t3.as_str())
    );
    w.close_tab(&t3).unwrap();
    assert_eq!(
        w.worktree(&id, &wt).unwrap().active_tab.as_deref(),
        Some(t1.as_str())
    );
}

#[test]
fn close_zoomed_pane_unzooms() {
    let (mut w, _, tab, p1) = with_x();
    let p2 = w.split(&p1, SplitDir::Right, None, None).unwrap();
    assert_eq!(w.toggle_zoom(&tab).unwrap().as_deref(), Some(p2.as_str()));
    w.close_pane(&p2).unwrap();
    assert_eq!(w.tab(&w.locate(&p1).unwrap()).unwrap().zoomed, None);
}

#[test]
fn r1_7_zoom_toggles_without_changing_layout() {
    let (mut w, _, tab, p1) = with_x();
    let p2 = w.split(&p1, SplitDir::Right, None, None).unwrap();
    w.set_split_ratio(&tab, &[], 0.3, true);
    let before = w.clone();
    assert_eq!(w.toggle_zoom(&tab).unwrap(), Some(p2.clone()));
    assert_eq!(w.toggle_zoom(&tab).unwrap(), None);
    assert_eq!(w, before);
}

#[test]
fn directional_focus_follows_geometry() {
    let (mut w, _, _, p1) = with_x();
    let p2 = w.split(&p1, SplitDir::Right, None, None).unwrap();
    let p3 = w.split(&p2, SplitDir::Down, None, None).unwrap();
    w.focus_pane(&p1).unwrap();
    assert_eq!(w.focus_direction(Direction::Right), Some(p2.clone()));
    assert_eq!(w.focus_direction(Direction::Down), Some(p3.clone()));
    assert_eq!(w.focus_direction(Direction::Left), Some(p1.clone()));
    assert_eq!(w.focus_direction(Direction::Left), None);
}

#[test]
fn r1_c_focus_switches_source_and_back_restores_everything() {
    let (mut w, x, xt, xp1) = with_x();
    let xp2 = w.split(&xp1, SplitDir::Right, None, None).unwrap();
    w.focus_pane(&xp1).unwrap();
    let y = w.add_source(Path::new("/tmp/y"), None).unwrap();
    let ywt = w.source(&y).unwrap().active_worktree.clone();
    let (_, yp) = w.new_tab(&y, &ywt, None, None).unwrap();
    let (yt2, _) = w.new_tab(&y, &ywt, None, None).unwrap();
    w.select_tab(&yt2).unwrap();
    let xs = w.source(&x).unwrap().clone();
    w.focus_pane(&yp).unwrap();
    assert_eq!(w.active_source, y);
    w.select_source(&x).unwrap();
    assert_eq!(w.source(&x).unwrap(), &xs, "x untouched while away");
    let t = w.active_tab().unwrap();
    assert_eq!(
        (t.id.as_str(), t.focused.as_str()),
        (xt.as_str(), xp1.as_str())
    );
    assert!(w.locate(&xp2).is_some());
}

#[test]
fn focus_pane_moves_zoom_to_it() {
    let (mut w, _, tab, p1) = with_x();
    let p2 = w.split(&p1, SplitDir::Right, None, None).unwrap();
    w.toggle_zoom(&tab).unwrap();
    w.focus_pane(&p1).unwrap();
    assert_eq!(
        w.tab(&w.locate(&p2).unwrap()).unwrap().zoomed.as_deref(),
        Some(p1.as_str())
    );
    assert_eq!(
        w.focus_direction(Direction::Right),
        None,
        "no directional focus while zoomed"
    );
}

#[test]
fn cycle_tab_wraps() {
    let (mut w, id, t1, _) = with_x();
    let wt = w.source(&id).unwrap().active_worktree.clone();
    let (t2, _) = w.new_tab(&id, &wt, None, None).unwrap();
    w.select_source(&id).unwrap();
    w.cycle_tab(1);
    assert_eq!(w.active_tab().unwrap().id, t2);
    w.cycle_tab(1);
    assert_eq!(w.active_tab().unwrap().id, t1);
    w.cycle_tab(-1);
    assert_eq!(w.active_tab().unwrap().id, t2);
}

#[test]
fn rename_and_reorder_tabs() {
    let (mut w, id, t1, p1) = with_x();
    let wt = w.source(&id).unwrap().active_worktree.clone();
    let (t2, _) = w.new_tab(&id, &wt, None, None).unwrap();
    w.move_tab(&t2, 0).unwrap();
    w.rename_tab(&t1, "build").unwrap();
    w.rename_pane(&p1, Some("server")).unwrap();
    let wtr = w.worktree(&id, &wt).unwrap();
    assert_eq!(wtr.tabs[0].id, t2);
    assert_eq!(wtr.tabs[1].name, "build");
    assert_eq!(w.pane(&p1).unwrap().name.as_deref(), Some("server"));
}

#[test]
fn resolve_pane_by_id_name_scope_and_ambiguity() {
    let (mut w, x, _, xp) = with_x();
    w.rename_pane(&xp, Some("rev")).unwrap();
    let y = w.add_source(Path::new("/tmp/y"), None).unwrap();
    let ywt = w.source(&y).unwrap().active_worktree.clone();
    let (_, yp) = w.new_tab(&y, &ywt, None, Some("rev".into())).unwrap();
    assert_eq!(w.resolve_pane(&xp, None).unwrap(), xp);
    assert_eq!(w.resolve_pane("rev", Some(&yp)).unwrap(), yp);
    assert_eq!(w.resolve_pane("rev", Some(&xp)).unwrap(), xp);
    let e = w.resolve_pane("rev", None).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Ambiguous);
    assert!(e.message.contains(&xp) && e.message.contains(&yp));
    assert_eq!(
        w.resolve_pane("nope", None).unwrap_err().kind,
        ErrorKind::NotFound
    );
    let _ = x;
}

#[test]
fn pane_infos_list_every_pane() {
    let (mut w, _, tab, p1) = with_x();
    w.split(&p1, SplitDir::Right, None, Some("b".into()))
        .unwrap();
    let infos = w.pane_infos();
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0].source, "x");
    assert_eq!(infos[0].worktree, "main");
    assert_eq!(infos[0].tab, tab);
    assert_eq!(infos[1].name.as_deref(), Some("b"));
    assert_eq!(infos[1].cwd, "/tmp/x");
}

#[test]
fn pane_infos_carry_the_account() {
    let (mut w, _, _, p1) = with_x();
    let loc = w.locate(&p1).unwrap();
    w.tab_mut(&loc).unwrap().pane_mut(&p1).unwrap().account = Some("acme-team".into());
    let infos = w.pane_infos();
    assert_eq!(infos[0].account.as_deref(), Some("acme-team"));
    let v = serde_json::to_value(&infos[0]).unwrap();
    assert_eq!(v["account"], "acme-team");
}

#[test]
fn ids_are_unique_and_survive_serde() {
    let (mut w, _, _, p1) = with_x();
    let p2 = w.split(&p1, SplitDir::Right, None, None).unwrap();
    let json = serde_json::to_string(&w).unwrap();
    let mut back: State = serde_json::from_str(&json).unwrap();
    assert_eq!(back, w);
    let p3 = back.split(&p2, SplitDir::Down, None, None).unwrap();
    assert!(p3 != p1 && p3 != p2);
}

#[test]
fn ensure_general_repairs_loaded_state() {
    let (w, _, _, p1) = with_x();
    let mut v = serde_json::to_value(&w).unwrap();
    // Drop General and the id counter, as an old or hand-edited file might.
    v["sources"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["id"] != "general");
    v.as_object_mut().unwrap().remove("next_id");
    v["active_source"] = "gone".into();
    let mut back: State = serde_json::from_value(v).unwrap();
    back.ensure_general(Path::new("/home/v"));
    assert_eq!(back.sources.last().unwrap().path, Path::new("/home/v"));
    assert_eq!(back.active_source, GENERAL_ID);
    let n = back.split(&p1, SplitDir::Right, None, None).unwrap();
    assert!(back.locate(&n).is_some());
    let all: Vec<String> = back.pane_infos().into_iter().map(|i| i.pane).collect();
    assert_eq!(all.len(), 2);
    assert_ne!(all[0], all[1]);
}

#[test]
fn slug_examples() {
    assert_eq!(slug("My App!"), "my-app");
    assert_eq!(slug("--"), "source");
    assert_eq!(slug("rade"), "rade");
}

#[test]
fn r104_m_split_and_new_tab_beside_a_link_checkout_terminal_start_there() {
    let (mut w, id, _, pane) = with_x();
    let pc = Path::new("/tmp/w/.worktrees/.projects/shop/x");
    let ws = w.active_workspace().to_string();
    w.workspaces
        .iter_mut()
        .find(|x| x.id == ws)
        .unwrap()
        .link_checkouts
        .push(crate::checkout::LinkCheckout {
            project: "shop".into(),
            source: id.clone(),
            base: "develop".into(),
            checkout: "auto".into(),
            path: pc.into(),
        });
    assert!(w.set_pane_cwd(&pane, &pc.join("src")));
    let split = w.split(&pane, SplitDir::Right, None, None).unwrap();
    assert_eq!(cwd(&w, &split), pc.display().to_string());
    let wt = w.source(&id).unwrap().active_worktree.clone();
    let (_, tab) = w.new_tab_beside(&id, &wt, Some(&pane)).unwrap();
    assert_eq!(cwd(&w, &tab), pc.display().to_string());
    // A Terminal in the main checkout itself keeps the Worktree root.
    assert!(w.set_pane_cwd(&pane, Path::new("/tmp/x/deep")));
    let split = w.split(&pane, SplitDir::Right, None, None).unwrap();
    assert_eq!(cwd(&w, &split), "/tmp/x");
    let (_, tab) = w.new_tab_beside(&id, &wt, Some(&pane)).unwrap();
    assert_eq!(cwd(&w, &tab), "/tmp/x");
}

#[test]
fn r19_7_a_new_tab_goes_right_after_the_active_one() {
    let (mut w, id, t1, _) = with_x();
    let wt = w.source(&id).unwrap().active_worktree.clone();
    let (t2, _) = w.new_tab(&id, &wt, None, None).unwrap();
    let (t3, _) = w.new_tab(&id, &wt, None, None).unwrap();
    let order = |w: &State| -> Vec<String> {
        w.worktree(&id, &wt)
            .unwrap()
            .tabs
            .iter()
            .map(|t| t.id.clone())
            .collect()
    };
    // The CLI's `new_tab` appends.
    assert_eq!(order(&w), [t1.clone(), t2.clone(), t3.clone()]);
    // The UI's New Tab goes right after the active one.
    let (t4, _) = w.new_tab_beside(&id, &wt, None).unwrap();
    assert_eq!(order(&w), [t1.clone(), t4.clone(), t2.clone(), t3.clone()]);
    w.select_tab(&t2).unwrap();
    let (t5, _) = w.new_tab_beside(&id, &wt, None).unwrap();
    assert_eq!(
        order(&w),
        [t1.clone(), t4.clone(), t2.clone(), t5.clone(), t3.clone()]
    );
    // An Agent Tab the `+` menu spawned (appended, then moved after t1).
    let (t6, _) = w.new_tab(&id, &wt, None, None).unwrap();
    w.move_tab_after(&t6, &t1);
    assert_eq!(order(&w), [t1, t6, t4, t2, t5, t3]);
}
