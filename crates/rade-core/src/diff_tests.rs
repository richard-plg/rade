use super::*;
use crate::git::tests::{repo, sh};

const PATCH: &str = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,4 +1,5 @@ fn main
 one
-two
+TWO
+two and a half
 three
 four
@@ -10,2 +11,2 @@
 ten
-eleven
+ELEVEN
\\ No newline at end of file
diff --git a/old name.txt b/new name.txt
similarity index 90%
rename from old name.txt
rename to new name.txt
index 1..2 100644
--- a/old name.txt\t
+++ b/new name.txt\t
@@ -1 +1 @@
-x
+y
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 3..0000000
--- a/gone.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-a
-b
diff --git a/img.png b/img.png
new file mode 100644
index 0000000..4
Binary files /dev/null and b/img.png differ
diff --git a/fresh.txt b/fresh.txt
new file mode 100644
index 0000000..5
--- /dev/null
+++ b/fresh.txt
@@ -0,0 +1 @@
+hello
diff --git \"a/caf\\303\\251 \\\"q\\\".txt\" \"b/caf\\303\\251 \\\"q\\\".txt\"
index 6..7 100644
--- \"a/caf\\303\\251 \\\"q\\\".txt\"
+++ \"b/caf\\303\\251 \\\"q\\\".txt\"
@@ -1 +1 @@
--- not a header
+++ not a header either
";

#[test]
fn r13_1_parses_files_status_counts_and_line_numbers() {
    let f = parse(PATCH, &["fresh.txt".into()]);
    let summary: Vec<(&str, char, u32, u32, bool)> = f
        .iter()
        .map(|f| (f.path.as_str(), f.status, f.added, f.removed, f.binary))
        .collect();
    assert_eq!(
        summary,
        [
            ("src/a.rs", 'M', 3, 2, false),
            ("new name.txt", 'R', 1, 1, false),
            ("gone.txt", 'D', 0, 2, false),
            ("img.png", 'A', 0, 0, true),
            ("fresh.txt", 'U', 1, 0, false),
            ("café \"q\".txt", 'M', 1, 1, false),
        ]
    );
    assert_eq!(f[1].old_path.as_deref(), Some("old name.txt"));

    let h = &f[0].hunks;
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].header, "@@ -1,4 +1,5 @@ fn main");
    type Row<'a> = (LineKind, Option<u32>, Option<u32>, u32, &'a str);
    let nums: Vec<Row> = h[0]
        .lines
        .iter()
        .map(|l| (l.kind, l.old, l.new, l.at, l.text.as_str()))
        .collect();
    use LineKind::*;
    assert_eq!(
        nums,
        [
            (Context, Some(1), Some(1), 1, "one"),
            (Removed, Some(2), None, 2, "two"),
            (Added, None, Some(2), 2, "TWO"),
            (Added, None, Some(3), 3, "two and a half"),
            (Context, Some(3), Some(4), 4, "three"),
            (Context, Some(4), Some(5), 5, "four"),
        ]
    );
    // Second hunk restarts numbering; the "\ No newline" marker is skipped.
    assert_eq!(h[1].lines.len(), 3);
    assert_eq!(
        (h[1].lines[2].new, h[1].lines[2].text.as_str()),
        (Some(12), "ELEVEN")
    );
    // A deleted file's removed lines anchor at new line 1.
    assert!(f[2].hunks[0].lines.iter().all(|l| l.at == 1));
    // Content lines that look like headers stay content.
    assert_eq!(f[5].hunks[0].lines[0].text, "-- not a header");
    assert_eq!(f[5].hunks[0].lines[1].text, "++ not a header either");
    assert_eq!(f[0].line_count(), 11);
    assert!(parse("", &[]).is_empty());
}

#[test]
fn r13_1_changes_since_merge_base_with_uncommitted_and_untracked() {
    let r = repo("diff");
    // Main moves on after the branch point: not part of the branch's diff.
    sh(&r, &["checkout", "-qb", "feat"]);
    std::fs::write(r.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    sh(&r, &["commit", "-qam", "branch work"]);
    sh(&r, &["checkout", "-q", "main"]);
    std::fs::write(r.join("main-only.txt"), "m").unwrap();
    sh(&r, &["add", "-A"]);
    sh(&r, &["commit", "-qm", "main moves"]);
    sh(&r, &["checkout", "-q", "feat"]);
    // Staged, unstaged, untracked (with a space), ignored.
    std::fs::write(r.join("staged.txt"), "s\n").unwrap();
    sh(&r, &["add", "staged.txt"]);
    std::fs::write(r.join("a.txt"), "one\nTWO\nthree\nfour\n").unwrap();
    std::fs::write(r.join("un tracked.txt"), "u1\nu2\n").unwrap();
    std::fs::write(r.join(".gitignore"), "*.log\n").unwrap();
    std::fs::write(r.join("x.log"), "ignored").unwrap();
    // Pathspec magic in a name must not hide the other untracked files.
    std::fs::write(r.join(":odd*"), "o\n").unwrap();
    let index_before = std::fs::read(r.join(".git/index")).unwrap();

    let c = changes(&r, None).unwrap();
    assert_eq!(c.branch.as_deref(), Some("main"));
    let got: Vec<(&str, char, u32, u32)> = c
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status, f.added, f.removed))
        .collect();
    assert_eq!(
        got,
        [
            (".gitignore", 'U', 1, 0),
            (":odd*", 'U', 1, 0),
            ("a.txt", 'M', 2, 1),
            ("staged.txt", 'A', 1, 0),
            ("un tracked.txt", 'U', 2, 0),
        ]
    );
    assert!(!c.truncated);
    // The Worktree's own index is untouched (no intent-to-add leaked).
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index_before);
    let status = git::run(&r, &["status", "--porcelain"]).unwrap();
    assert!(status.contains("?? \"un tracked.txt\"") || status.contains("?? un tracked.txt"));

    // On main itself: base = HEAD, uncommitted work only.
    // -a: x.log too, else it shows once .gitignore is stashed (unless a
    // global excludes file hides it, as on the dev machine but not in CI).
    sh(&r, &["stash", "-qa"]);
    sh(&r, &["checkout", "-q", "main"]);
    std::fs::write(r.join("a.txt"), "changed\n").unwrap();
    let c = changes(&r, None).unwrap();
    assert_eq!(c.files.len(), 1);
    assert_eq!(c.files[0].path, "a.txt");

    // Not a repository.
    let plain = std::env::temp_dir().join(format!("rade-diff-plain-{}", std::process::id()));
    std::fs::create_dir_all(&plain).unwrap();
    assert!(changes(&plain, None).is_err());
    std::fs::remove_dir_all(plain).unwrap();
    std::fs::remove_dir_all(r).unwrap();
}

#[test]
fn r13_1_unborn_head_diffs_everything() {
    let d = std::env::temp_dir().join(format!("rade-diff-unborn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    sh(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("f.txt"), "x\n").unwrap();
    let c = changes(&d, None).unwrap();
    assert_eq!(c.files.len(), 1);
    assert_eq!(c.files[0].status, 'U');
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn r13_1_index_copy_keeps_the_mtime() {
    let r = repo("racy");
    let (from, to) = (r.join(".git/index"), r.join("copy"));
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    std::fs::File::options()
        .write(true)
        .open(&from)
        .unwrap()
        .set_modified(old)
        .unwrap();
    copy_index(&from, &to).unwrap();
    assert_eq!(std::fs::metadata(&to).unwrap().modified().unwrap(), old);
    assert_eq!(std::fs::read(&from).unwrap(), std::fs::read(&to).unwrap());
    std::fs::remove_dir_all(r).unwrap();
}

#[test]
fn m11_rows_list_every_file_with_folds_by_path() {
    use std::collections::HashSet;
    let patch = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,2 @@
-x
+y
@@ -9 +9 @@
 z
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1 +1 @@
-p
+q
";
    let files = parse(patch, &[]);
    let none = HashSet::new();
    let all = rows(&files, &none, &HashSet::new());
    use Row::*;
    assert_eq!(
        all,
        [
            File(0),
            Hunk(0, 0),
            Line(0, 0, 0),
            Line(0, 0, 1),
            Hunk(0, 1),
            Line(0, 1, 0),
            File(1),
            Binary(1),
            File(2),
            Hunk(2, 0),
            Line(2, 0, 0),
            Line(2, 0, 1)
        ]
    );
    let folded_file: HashSet<String> = ["a.rs".to_string()].into();
    let folded_hunk: HashSet<(String, String)> =
        [("b.rs".to_string(), "@@ -1 +1 @@".to_string())].into();
    assert_eq!(
        rows(&files, &folded_file, &folded_hunk),
        [File(0), File(1), Binary(1), File(2), Hunk(2, 0)]
    );
}

#[test]
fn r28_3_a_recorded_base_hides_its_own_commits() {
    let r = repo("diff-base");
    sh(&r, &["checkout", "-qb", "beacon-dev"]);
    std::fs::write(r.join("dev.txt"), "dev\n").unwrap();
    sh(&r, &["add", "-A"]);
    sh(&r, &["commit", "-qm", "dev only"]);
    sh(&r, &["checkout", "-qb", "otp"]);
    std::fs::write(r.join("mine.txt"), "mine\n").unwrap();
    let paths = |c: &Changes| c.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths(&changes(&r, None).unwrap()), ["dev.txt", "mine.txt"]);
    let c = changes(&r, Some("beacon-dev")).unwrap();
    assert_eq!(
        (c.branch.as_deref(), paths(&c)),
        (Some("beacon-dev"), vec!["mine.txt".to_string()])
    );
    // A base that is gone: the main branch again.
    assert_eq!(
        changes(&r, Some("no-such")).unwrap().branch.as_deref(),
        Some("main")
    );
    let _ = std::fs::remove_dir_all(&r);
}
