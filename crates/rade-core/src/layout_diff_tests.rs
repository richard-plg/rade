use super::*;
use crate::agents::{SpawnSpec, TabChoice};
use crate::config::Limits;
use crate::model::State;
use std::path::Path;

fn leaf(p: &str) -> Shape {
    Shape::Leaf(p.into())
}

fn right(a: Shape, b: Shape) -> Shape {
    Shape::Split(SplitDir::Right, Box::new(a), Box::new(b))
}

/// A State with Source x and `n` Agents spawned as `rade spawn --tab current`
/// from outside RADE does it: each splits the shown Tab's focused Pane.
fn chain(n: usize) -> State {
    let mut w = State::new(Path::new("/home/u"));
    w.add_source(Path::new("/tmp/x"), None).unwrap();
    for i in 0..n {
        let p = spawn(
            &mut w,
            &format!("a{i}"),
            (i > 0).then_some(TabChoice::Current),
        );
        if i == 0 {
            w.focus_pane(&p).unwrap(); // its Tab is the one on screen
        }
    }
    w
}

fn spawn(w: &mut State, name: &str, tab: Option<TabChoice>) -> String {
    w.spawn(
        SpawnSpec {
            source: Some("x".into()),
            agent: Some("claude".into()),
            name: Some(name.into()),
            tab,
            ..Default::default()
        },
        &Limits::default(),
    )
    .unwrap()
}

fn shape(w: &State) -> Shape {
    Shape::of(&w.active_tab().unwrap().layout)
}

#[test]
fn r142_h_a_spawn_into_a_tab_of_40_panes_moves_only_the_split_pane() {
    let mut w = chain(40);
    let before = shape(&w);
    let focused = w.active_tab().unwrap().focused.clone();
    let new = spawn(&mut w, "a40", Some(TabChoice::Current));
    let step = reconcile(Some(&before), &shape(&w));
    // The split Pane moves into one new split widget beside the new Pane;
    // the other 39 Panes and every other split widget stay.
    assert_eq!(step.placed(), vec![focused.as_str(), new.as_str()]);
    assert_eq!(step.new_splits(), 1);
}

#[test]
fn r142_h_an_unchanged_layout_keeps_everything() {
    let w = chain(12);
    let s = shape(&w);
    assert_eq!(reconcile(Some(&s), &s), Step::Keep);
    // A status report changes no shape.
    let mut w2 = w.clone();
    let p = w2.active_tab().unwrap().focused.clone();
    w2.set_status(&p, Some(crate::AgentStatus::Running), None, None, 1)
        .unwrap();
    assert_eq!(reconcile(Some(&s), &shape(&w2)), Step::Keep);
}

#[test]
fn r142_h_closing_a_pane_moves_only_its_sibling() {
    // R(a, R(b, c)) → R(a, b): a stays, b moves up a level.
    let old = right(leaf("a"), right(leaf("b"), leaf("c")));
    let new = right(leaf("a"), leaf("b"));
    let step = reconcile(Some(&old), &new);
    assert_eq!(
        step,
        Step::Reuse(Box::new(Step::Keep), Box::new(Step::Place("b".into())))
    );
    assert_eq!(step.new_splits(), 0);
}

#[test]
fn r142_h_nothing_built_places_every_pane() {
    let new = right(
        leaf("a"),
        Shape::Split(SplitDir::Down, Box::new(leaf("b")), Box::new(leaf("c"))),
    );
    let step = reconcile(None, &new);
    assert_eq!(step.placed(), vec!["a", "b", "c"]);
    assert_eq!(step.new_splits(), 2);
    // A split turned the other way is a new widget too.
    let turned = Shape::Split(SplitDir::Down, Box::new(leaf("a")), Box::new(leaf("b")));
    let step = reconcile(Some(&right(leaf("a"), leaf("b"))), &turned);
    assert_eq!(step.placed(), vec!["a", "b"]);
    assert_eq!(step.new_splits(), 1);
}

#[test]
fn r142_h_content_follows_zoom_title_bars_and_one_pane() {
    let mut w = chain(3);
    let tab = w.active_tab().unwrap().clone();
    assert_eq!(Content::of(&tab, false), Content::Tiled);
    let one = Content::of(&tab, true);
    assert!(
        matches!(&one, Content::One { main, others } if *main == tab.focused && others.len() == 2)
    );
    assert!(!Content::Tiled.same_frame(&one));
    let id = tab.id.clone();
    w.toggle_zoom(&id).unwrap();
    let z = Content::of(w.active_tab().unwrap(), false);
    assert_eq!(z, Content::Zoomed(tab.focused.clone()));
    assert!(!Content::Tiled.same_frame(&z));
    // An Agent Grid gaining a Title Bar keeps its outer widgets.
    let a = Content::Grid {
        bars: vec!["p_1".into()],
    };
    let b = Content::Grid {
        bars: vec!["p_1".into(), "p_2".into()],
    };
    assert!(a.same_frame(&b));
    assert!(!a.same_frame(&Content::Stacked { bars: vec![] }));
}

#[test]
fn r142_h_a_shell_frame_is_rebuilt_when_its_tab_gets_a_second_pane() {
    let shell = Pane::new("p_1".into(), "/w".into(), None, None);
    assert_eq!(
        FrameKey::of(&shell, 1),
        FrameKey {
            header: false,
            close: false
        }
    );
    assert_eq!(
        FrameKey::of(&shell, 2),
        FrameKey {
            header: true,
            close: true
        }
    );
    // An Agent's header stays; its Close button comes with the 2nd Pane.
    let mut agent = shell.clone();
    agent.agent = Some("claude".into());
    assert_eq!(
        FrameKey::of(&agent, 1),
        FrameKey {
            header: true,
            close: false
        }
    );
    assert_eq!(FrameKey::of(&agent, 3), FrameKey::of(&agent, 2));
}
