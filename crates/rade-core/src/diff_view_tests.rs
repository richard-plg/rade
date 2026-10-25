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
