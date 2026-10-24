use super::*;
use crate::diff::parse;
use crate::SplitDir;
use std::path::Path;

const PATCH: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,4 +10,4 @@
 ten
-eleven
+ELEVEN
 twelve
-thirteen
";

fn ws() -> State {
    let mut w = State::new(Path::new("/home/u"));
    w.add_source(Path::new("/r/x"), None).unwrap();
    w.add_worktree("x", "f", Path::new("/d/f"), None).unwrap();
    w
}

fn comment(path: &str, line: u32, text: &str) -> ReviewComment {
    ReviewComment {
        id: 0,
        path: path.into(),
        line,
        end_line: None,
        excerpt: vec![format!("+code at {line}")],
        text: text.into(),
        ..Default::default()
    }
}

#[test]
fn r13_2_anchors_use_new_file_lines() {
    let f = &parse(PATCH, &[])[0];
    let l = &f.hunks[0].lines;
    // One added line.
    assert_eq!(anchor(&[&l[2]]), Some((11, None, vec!["+ELEVEN".into()])));
    // Removed + added + context: new lines 11..12.
    let (a, b, ex) = anchor(&[&l[1], &l[2], &l[3]]).unwrap();
    assert_eq!((a, b), (11, Some(12)));
    assert_eq!(ex, ["-eleven", "+ELEVEN", " twelve"]);
    // Only a removed line: where it was in the new file.
    assert_eq!(anchor(&[&l[1]]).unwrap().0, 11);
    assert_eq!(anchor(&[&l[4]]).unwrap().0, 13);
    assert_eq!(anchor(&[]), None);
}

#[test]
fn r13_2_compose_one_prompt_with_every_location_quote_and_comment() {
    let mut long = comment("b/c.py", 3, "Why?\nExplain.");
    long.end_line = Some(20);
    long.excerpt = (0..15).map(|i| format!(" l{i}")).collect();
    let p = compose("feat-x", &[comment("src/a.rs", 11, "Rename this."), long]);
    assert!(p.starts_with("Review comments on the changes in this worktree (feat-x)."));
    assert!(
        p.contains("\n1. src/a.rs:11\n   > +code at 11\n   Rename this.\n"),
        "{p}"
    );
    assert!(p.contains("\n2. b/c.py:3-20\n   >  l0\n"), "{p}");
    assert!(p.contains("   >  l11\n   > …\n   Why?\n   Explain."), "{p}");
    assert!(!p.contains("l12"));
    assert!(!p.ends_with('\n'));
}

#[test]
fn r13_2_comments_persist_per_worktree_until_taken() {
    let mut w = ws();
    assert!(
        w.add_comment("x", "f", comment("a", 1, " ")).is_err(),
        "empty"
    );
    let a = w.add_comment("x", "f", comment("a", 1, "one")).unwrap();
    let b = w.add_comment("x", "f", comment("b", 2, "two")).unwrap();
    assert_eq!((a, b), (1, 2));
    assert!(
        w.comments("x", "main").unwrap().is_empty(),
        "other Worktree"
    );
    w.edit_comment("x", "f", a, "ONE").unwrap();
    assert!(w.edit_comment("x", "f", 9, "x").is_err());
    let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
    let texts: Vec<&str> = back
        .comments("x", "f")
        .unwrap()
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(texts, ["ONE", "two"]);
    w.edit_comment("x", "f", b, "").unwrap();
    assert_eq!(w.comments("x", "f").unwrap().len(), 1, "empty text deletes");
    assert_eq!(w.take_comments("x", "f").unwrap().len(), 1);
    assert!(w.comments("x", "f").unwrap().is_empty());
    assert_eq!(
        w.add_comment("x", "f", comment("a", 1, "again")).unwrap(),
        1
    );
}

#[test]
fn r13_1_review_pane_opens_in_its_own_tab_and_is_reused() {
    let mut w = ws();
    let (p, wt) = w.find_worktree("x", Some("f")).unwrap();
    // No Tab yet: a new Tab.
    let r = w.open_review(&p, &wt).unwrap();
    assert!(w.pane(&r).unwrap().is_review());
    assert_eq!(w.pane(&r).unwrap().cwd(), Path::new("/d/f"));
    w.close_pane(&r).unwrap();
    let (tab, agent) = w.new_tab(&p, &wt, Some("codex".into()), None).unwrap();
    w.select_tab(&tab).unwrap();
    let r = w.open_review(&p, &wt).unwrap();
    // M14: its own full-width Tab, shown and focused; the Agent's Tab
    // keeps its Layout.
    let t = w.worktree(&p, &wt).unwrap().tab(&tab).unwrap();
    assert_eq!(t.layout.signature(), agent);
    let rt = w.locate(&r).unwrap().tab;
    assert_ne!(rt, tab);
    assert_eq!(
        w.worktree(&p, &wt).unwrap().active_tab.as_deref(),
        Some(rt.as_str())
    );
    assert_eq!(
        w.worktree(&p, &wt).unwrap().tab(&rt).unwrap().label(),
        "Review"
    );
    // Again from the Agent's Tab: that Review Tab comes back.
    w.select_tab(&tab).unwrap();
    assert_eq!(w.open_review(&p, &wt).unwrap(), r, "reused");
    assert_eq!(
        w.worktree(&p, &wt).unwrap().active_tab.as_deref(),
        Some(rt.as_str())
    );
    // Session Restore keeps it.
    let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
    assert!(back.pane(&r).unwrap().is_review());
}

#[test]
fn r13_2_agent_panes_skip_exited_and_non_agents() {
    let mut w = ws();
    let (p, wt) = w.find_worktree("x", Some("f")).unwrap();
    let (_, shell) = w.new_tab(&p, &wt, None, None).unwrap();
    assert!(w.agent_panes(&p, &wt).is_empty());
    let a = w.split(&shell, SplitDir::Right, None, None).unwrap();
    w.set_status(&a, None, None, Some("codex".into()), 1)
        .unwrap();
    assert_eq!(w.agent_panes(&p, &wt).len(), 1);
    w.set_status(&a, Some(AgentStatus::Exited), None, None, 2)
        .unwrap();
    assert!(w.agent_panes(&p, &wt).is_empty());
}

// ---- M11: re-anchoring (path, new-side line, its text + 2 lines context, hunk)

const FILE: &str = "\
use std::io;

fn main() {
    let count = 1;
    println!(\"{}\", count);
}

fn helper() {
    let count = 1;
}
";

/// A comment on `line..=end` of FILE, its anchor captured like the app does.
fn anchored(line: u32, end: Option<u32>) -> ReviewComment {
    let lines: Vec<&str> = FILE.lines().collect();
    let mut c = comment("src/a.rs", line, "note");
    c.end_line = end;
    c.excerpt = (line..=end.unwrap_or(line))
        .map(|n| format!("+{}", lines[n as usize - 1]))
        .collect();
    c.capture(FILE, Some("@@ -1,4 +1,6 @@ fn main() {"));
    c
}

#[test]
fn m11_capture_keeps_two_lines_of_context_and_the_hunk() {
    let c = anchored(4, None);
    let ctx = c.context.as_ref().unwrap();
    assert_eq!(ctx.before, ["", "fn main() {"]);
    assert_eq!(ctx.after, ["    println!(\"{}\", count);", "}"]);
    assert_eq!(ctx.hunk, "@@ -1,4 +1,6 @@ fn main() {");
    // At the top of the file: fewer lines before.
    assert_eq!(anchored(1, None).context.unwrap().before.len(), 0);
    // Survives state.json; old comments without an anchor still load.
    let back: ReviewComment = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back, c);
    let old: ReviewComment =
        serde_json::from_str(r#"{"id":1,"path":"a","line":2,"text":"t"}"#).unwrap();
    assert_eq!((old.context, old.outdated), (None, false));
}

#[test]
fn m11_unchanged_file_keeps_the_comment_where_it_is() {
    let mut c = anchored(4, None);
    assert!(!c.relocate(Some(FILE)), "nothing changed");
    assert_eq!((c.line, c.outdated), (4, false));
}

#[test]
fn m11_lines_inserted_above_move_the_comment_down_by_exact_context() {
    // `let count = 1;` exists twice (lines 4 and 9): context picks line 4's.
    let mut c = anchored(4, Some(5));
    let edited = format!("// header\n// more\n{FILE}");
    assert!(c.relocate(Some(&edited)));
    assert_eq!((c.line, c.end_line, c.outdated), (6, Some(7), false));
    let mut h = anchored(9, None);
    assert!(h.relocate(Some(&edited)));
    assert_eq!(h.line, 11, "the helper's copy follows its own context");
}

#[test]
fn m11_changed_context_falls_back_to_a_nearby_text_match() {
    let mut c = anchored(4, None);
    // The line above changed and the line moved down by one; its own text
    // (re-indented) is still there.
    let edited = FILE
        .replace("fn main() {", "fn main() -> io::Result<()> {\n    // start")
        .replace(
            "    let count = 1;\n    println",
            "        let count = 1;\n    println",
        );
    assert!(c.relocate(Some(&edited)));
    assert_eq!((c.line, c.outdated), (5, false));
    // Its context is refreshed; the next refresh finds the same place.
    assert_eq!(c.context.as_ref().unwrap().before[1], "    // start");
    assert!(!c.relocate(Some(&edited)));
}

#[test]
fn m11_lost_lines_mark_it_outdated_and_it_stays_sendable() {
    let mut c = anchored(5, None);
    let edited = FILE.replace("    println!(\"{}\", count);\n", "");
    assert!(c.relocate(Some(&edited)));
    assert!(c.outdated);
    assert_eq!(c.line, 5, "keeps its last place");
    let p = compose("feat-x", std::slice::from_ref(&c));
    assert!(
        p.contains(
            "1. src/a.rs:5 (outdated: the code there changed since; it was in `fn main() {`)"
        ),
        "{p}"
    );
    assert!(
        p.contains("> +    println!(\"{}\", count);"),
        "the stored excerpt: {p}"
    );
    // The text comes back (edit undone): found again.
    assert!(c.relocate(Some(FILE)));
    assert_eq!((c.line, c.outdated), (5, false));
    // The file is gone: outdated.
    assert!(c.relocate(None));
    assert!(c.outdated);
}

#[test]
fn m11_a_far_away_fuzzy_match_or_a_trivial_line_is_not_trusted() {
    // `}` alone, with its context gone: not re-anchored to another `}`.
    let mut c = anchored(6, None);
    let edited = "fn other() {\n    x();\n}\n";
    assert!(c.relocate(Some(edited)));
    assert!(c.outdated);
    // The text exists only far away (> 100 lines) with other context.
    let mut c = anchored(4, None);
    let far = format!(
        "{}fn z() {{\n        let count = 1;\n}}\n",
        "\n".repeat(200)
    );
    assert!(c.relocate(Some(&far)));
    assert!(c.outdated);
}

#[test]
fn m11_a_comment_on_removed_lines_is_anchored_by_its_context() {
    // Removed lines sit before new line 5: no new-side text, context only.
    let mut c = comment("src/a.rs", 5, "why removed?");
    c.excerpt = vec!["-    old();".into()];
    c.capture(FILE, None);
    let ctx = c.context.clone().unwrap();
    assert_eq!(ctx.before, ["fn main() {", "    let count = 1;"]);
    assert_eq!(ctx.after, ["    println!(\"{}\", count);", "}"]);
    let edited = format!("// one more\n{FILE}");
    assert!(c.relocate(Some(&edited)));
    assert_eq!((c.line, c.outdated), (6, false));
}

#[test]
fn m11_workspace_reanchors_each_worktree_comment_reading_files_once() {
    let mut w = ws();
    let a = anchored(4, None);
    let mut legacy = comment("b.py", 3, "no anchor");
    legacy.excerpt = vec!["+x".into()];
    w.add_comment("x", "f", a).unwrap();
    w.add_comment("x", "f", legacy).unwrap();
    let reads = std::cell::Cell::new(0);
    let edited = format!("\n{FILE}");
    let changed = w
        .reanchor_comments("x", "f", &|p: &str| {
            reads.set(reads.get() + 1);
            (p == "src/a.rs").then(|| edited.clone())
        })
        .unwrap();
    assert!(changed);
    let cs = w.comments("x", "f").unwrap();
    assert_eq!((cs[0].line, cs[0].outdated), (5, false));
    assert_eq!(
        (cs[1].line, cs[1].outdated),
        (3, false),
        "no anchor: left as is"
    );
    assert_eq!(reads.get(), 1, "only files with anchored comments are read");
    assert!(!w
        .reanchor_comments("x", "f", &|_: &str| Some(edited.clone()))
        .unwrap());
}

#[test]
fn m11_a_quote_that_is_not_the_files_lines_gets_no_anchor() {
    // Lines from two hunks (not next to each other in the file).
    let mut c = comment("src/a.rs", 4, "two hunks");
    c.excerpt = vec!["+    let count = 1;".into(), "+fn helper() {".into()];
    c.end_line = Some(8);
    c.capture(FILE, None);
    assert_eq!(c.context, None);
    assert!(!c.relocate(Some("x\n")), "never moved, never outdated");
    // A range whose quote starts later than `line` (CLI, diff covers part).
    let mut c = comment("src/a.rs", 3, "partial");
    c.excerpt = vec!["+    let count = 1;".into()];
    c.capture(FILE, None);
    assert_eq!(c.context, None);
}
