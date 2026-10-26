use super::*;
use crate::diff::DiffLine;

fn line(kind: LineKind, text: &str) -> DiffLine {
    DiffLine {
        kind,
        old: None,
        new: None,
        at: 1,
        text: text.into(),
    }
}

#[test]
fn r122_a_side_by_side_pairs() {
    use LineKind::*;
    let h = Hunk {
        header: "@@ -1,5 +1,5 @@".into(),
        lines: vec![
            line(Context, "ctx"),
            line(Removed, "a"),
            line(Removed, "b"),
            line(Added, "A"),
            line(Context, "ctx"),
            line(Added, "C"),
        ],
    };
    let p = |l: Option<usize>, r: Option<usize>| Pair { left: l, right: r };
    assert_eq!(
        side_by_side(&h),
        vec![
            p(Some(0), Some(0)),
            p(Some(1), Some(3)),
            p(Some(2), None),
            p(Some(4), Some(4)),
            p(None, Some(5)),
        ]
    );
    // More added than removed: the rest against filler.
    let h = Hunk {
        header: String::new(),
        lines: vec![
            line(Removed, "a"),
            line(Added, "A"),
            line(Added, "B"),
            line(Removed, "c"),
        ],
    };
    assert_eq!(
        side_by_side(&h),
        vec![p(Some(0), Some(1)), p(None, Some(2)), p(Some(3), None)]
    );
}

#[test]
fn r122_c_next_prev_wrap() {
    use LineKind::*;
    let mut kinds: Vec<Option<LineKind>> = vec![None; 40];
    for i in [4, 5, 10, 30, 31] {
        kinds[i] = Some(if i % 2 == 0 { Added } else { Removed });
    }
    kinds[7] = Some(Context);
    let starts = change_starts(&kinds);
    assert_eq!(starts, vec![4, 10, 30]);
    assert_eq!(next_change(&starts, Some(0)), Some(4));
    assert_eq!(next_change(&starts, Some(10)), Some(30));
    assert_eq!(next_change(&starts, Some(30)), Some(4));
    assert_eq!(prev_change(&starts, Some(4)), Some(30));
    assert_eq!(prev_change(&starts, Some(30)), Some(10));
    assert_eq!(next_change(&starts, None), Some(4));
    assert_eq!(prev_change(&starts, None), Some(30));
    assert_eq!(next_change(&[], Some(0)), None);
}

#[test]
fn r123_a_quick_marks() {
    let u0 = "\
diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,2 +0,0 @@
-top
-top2
@@ -2 +2 @@
-old
+new
@@ -4,0 +5,2 @@
+x
+y
@@ -9 +8,0 @@
-gone
";
    let m: Vec<(u32, MarkKind)> = quick_marks(u0)
        .into_iter()
        .map(|m| (m.line, m.kind))
        .collect();
    use MarkKind::*;
    assert_eq!(
        m,
        vec![
            (1, Deleted),
            (2, Modified),
            (5, Added),
            (6, Added),
            (9, Deleted)
        ]
    );
    assert!(quick_marks("").is_empty());
}

fn model() -> (crate::model::State, String, String) {
    let mut w = crate::model::State::new(std::path::Path::new("/home/u"));
    let id = w.add_source(std::path::Path::new("/tmp/x"), None).unwrap();
    let wt = w.source(&id).unwrap().active_worktree.clone();
    (w, id, wt)
}

#[test]
fn r122_f_diff_pane_serde_roundtrip() {
    let k = crate::PaneKind::Diff {
        root: "/r".into(),
        path: "src/a.rs".into(),
        section: crate::scm::Section::Unstaged,
    };
    let v = serde_json::to_value(&k).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"type": "diff", "root": "/r", "path": "src/a.rs", "section": "unstaged"})
    );
    assert_eq!(serde_json::from_value::<crate::PaneKind>(v).unwrap(), k);
    let p = crate::Pane::diff(
        "p_1".into(),
        "/r".into(),
        "a".into(),
        crate::scm::Section::Staged,
    );
    assert!(p.is_diff());
    assert_eq!(p.cwd(), std::path::Path::new("/r"));
}

#[test]
fn open_diff_reuses_the_tabs_diff_pane_else_splits_right_else_new_tab() {
    use crate::scm::Section::*;
    let (mut w, id, wt) = model();
    let root = std::path::Path::new("/tmp/x");
    // No Tab: a new Tab holds the Diff Pane.
    let d = w.open_diff(&id, &wt, root, "a.rs", Unstaged, true).unwrap();
    assert!(w.pane(&d).unwrap().is_diff());
    assert_eq!(w.worktree(&id, &wt).unwrap().tabs.len(), 1);
    // A Terminal Tab: the Diff Pane splits right of its focused Pane.
    let (tab, term) = w.new_tab(&id, &wt, None, None).unwrap();
    w.select_tab(&tab).unwrap();
    let d2 = w.open_diff(&id, &wt, root, "b.rs", Staged, false).unwrap();
    assert_ne!(d2, d);
    let t = w.worktree(&id, &wt).unwrap().tab(&tab).unwrap();
    assert_eq!(t.layout.panes().len(), 2);
    assert_eq!(t.focused, term, "focus stays when not asked");
    assert!(matches!(
        &w.pane(&d2).unwrap().kind,
        crate::PaneKind::Diff { path, section: Staged, .. } if path == "b.rs"
    ));
    // Again: the same Pane navigates.
    let d3 = w
        .open_diff(&id, &wt, root, "c.rs", Untracked, true)
        .unwrap();
    assert_eq!(d3, d2);
    assert!(matches!(
        &w.pane(&d3).unwrap().kind,
        crate::PaneKind::Diff { path, section: Untracked, .. } if path == "c.rs"
    ));
    assert_eq!(w.worktree(&id, &wt).unwrap().tab(&tab).unwrap().focused, d2);
    assert!(w.diff_navigate(&term, root, "x", Staged).is_err());
}
