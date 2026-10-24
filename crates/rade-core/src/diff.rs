//! A Worktree's changes for its Review Pane (R13.1): what is included, the
//! git calls that produce it, and the unified-diff parser.
//!
//! Included (like Orca's "since base" view): everything that differs between
//! the merge-base of HEAD with the Source's main branch and the working
//! tree — commits on the branch, staged and unstaged edits, and untracked
//! files that are not ignored (shown as added). Not included: ignored files,
//! the contents of submodules (their commit change shows), binary contents
//! (the file is listed as binary). Without a main branch (or on it) the base
//! is HEAD, i.e. uncommitted work only; before the first commit, everything.

use crate::git;
use crate::rpc::{Error, Result};
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    /// New-file line this row sits at: its own number, or for a removed
    /// line the number of the new line that follows (Review Comment anchor).
    pub at: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ context` line.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    /// Path in the new tree (the old one for a deleted file).
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `U` untracked.
    pub status: char,
    pub binary: bool,
    pub added: u32,
    pub removed: u32,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn line_count(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len() + 1).sum()
    }
}

/// Undo git's C-style quoting of unusual path names (`"a\"b\303\251"`).
fn unquote(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_string();
    };
    let mut bytes = Vec::new();
    let mut it = inner.bytes().peekable();
    while let Some(b) = it.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match it.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(d @ b'0'..=b'7') => {
                let mut v = (d - b'0') as u32;
                for _ in 0..2 {
                    if let Some(&n @ b'0'..=b'7') = it.peek() {
                        v = v * 8 + (n - b'0') as u32;
                        it.next();
                    }
                }
                bytes.push(v as u8);
            }
            Some(o) => bytes.push(o),
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Path from a `---`/`+++` line: `None` for `/dev/null`.
fn side_path(rest: &str, prefix: &str) -> Option<String> {
    // git ends a name containing a space with a tab.
    let rest = rest.strip_suffix('\t').unwrap_or(rest);
    if rest == "/dev/null" {
        return None;
    }
    let p = unquote(rest);
    Some(p.strip_prefix(prefix).unwrap_or(&p).to_string())
}

/// Path from `diff --git a/P b/P` when both sides are equal (no rename).
fn header_path(rest: &str) -> String {
    if let Some(q) = rest.strip_prefix('"') {
        let end = q.find("\" ").map_or(rest.len(), |i| i + 2);
        let a = unquote(&rest[..end]);
        return a.strip_prefix("a/").unwrap_or(&a).to_string();
    }
    let n = rest.len().saturating_sub(5) / 2; // "a/" P " b/" P
    rest.get(2..2 + n).unwrap_or(rest).to_string()
}

fn hunk_starts(h: &str) -> (u32, u32) {
    // @@ -a[,b] +c[,d] @@
    let mut it = h.split_whitespace().skip(1);
    let num = |s: Option<&str>| {
        s.and_then(|s| s[1..].split(',').next()?.parse().ok())
            .unwrap_or(0)
    };
    (num(it.next()), num(it.next()))
}

/// Parse `git diff` output into files, hunks and numbered lines. Paths in
/// `untracked` get status `U`.
pub fn parse(patch: &str, untracked: &[String]) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old_n, mut new_n) = (0u32, 0u32);
    let mut in_hunk = false;
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.push(FileDiff {
                path: header_path(rest),
                old_path: None,
                status: 'M',
                binary: false,
                added: 0,
                removed: 0,
                hunks: Vec::new(),
            });
            in_hunk = false;
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if line.starts_with("@@ ") {
            (old_n, new_n) = hunk_starts(line);
            f.hunks.push(Hunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
            in_hunk = true;
            continue;
        }
        if in_hunk {
            let (kind, text) = match line.as_bytes().first() {
                Some(b'+') => (LineKind::Added, &line[1..]),
                Some(b'-') => (LineKind::Removed, &line[1..]),
                Some(b' ') => (LineKind::Context, &line[1..]),
                Some(b'\\') => continue, // "\ No newline at end of file"
                None => (LineKind::Context, ""),
                _ => {
                    in_hunk = false;
                    continue;
                }
            };
            let (old, new) = match kind {
                LineKind::Added => (None, Some(new_n)),
                LineKind::Removed => (Some(old_n), None),
                LineKind::Context => (Some(old_n), Some(new_n)),
            };
            match kind {
                LineKind::Added => f.added += 1,
                LineKind::Removed => f.removed += 1,
                LineKind::Context => {}
            }
            f.hunks.last_mut().unwrap().lines.push(DiffLine {
                kind,
                old,
                new,
                at: new_n.max(1),
                text: text.to_string(),
            });
            if old.is_some() {
                old_n += 1;
            }
            if new.is_some() {
                new_n += 1;
            }
            continue;
        }
        if line.starts_with("new file mode") {
            f.status = 'A';
        } else if line.starts_with("deleted file mode") {
            f.status = 'D';
        } else if let Some(p) = line.strip_prefix("rename from ") {
            f.status = 'R';
            f.old_path = Some(unquote(p));
        } else if let Some(p) = line.strip_prefix("rename to ") {
            f.path = unquote(p);
        } else if let Some(p) = line.strip_prefix("--- ") {
            if f.status != 'R' {
                if let Some(p) = side_path(p, "a/") {
                    f.path = p;
                }
            }
        } else if let Some(p) = line.strip_prefix("+++ ") {
            if let Some(p) = side_path(p, "b/") {
                f.path = p;
            }
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            f.binary = true;
        }
    }
    for f in &mut files {
        if untracked.contains(&f.path) {
            f.status = 'U';
        }
    }
    files
}

/// One row of the Review Pane's all-files list (indices into
/// [`Changes::files`], its hunks and lines).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A file's header (status, path, +/−); a click folds the file.
    File(usize),
    /// A hunk's `@@` header; a click folds the hunk.
    Hunk(usize, usize),
    Line(usize, usize, usize),
    /// "Binary file" in place of lines.
    Binary(usize),
    /// A Review Comment's card under its last line (R118.4): its id.
    Comment(u64),
    /// The comment composer under the selected lines (R118.4).
    Composer,
}

/// The rows to show: every file's header, then (unless the file is folded)
/// its hunks' headers and (unless that hunk is folded) their lines. Folds
/// are kept by path (a hunk's by path and `@@` header), so they survive a
/// refresh; a hunk whose header changed unfolds rather than another
/// folding in its place.
pub fn rows(
    files: &[FileDiff],
    folded_files: &std::collections::HashSet<String>,
    folded_hunks: &std::collections::HashSet<(String, String)>,
) -> Vec<Row> {
    let mut out = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        out.push(Row::File(fi));
        if folded_files.contains(&f.path) {
            continue;
        }
        if f.binary {
            out.push(Row::Binary(fi));
            continue;
        }
        for (hi, h) in f.hunks.iter().enumerate() {
            out.push(Row::Hunk(fi, hi));
            if !folded_hunks.contains(&(f.path.clone(), h.header.clone())) {
                out.extend((0..h.lines.len()).map(|li| Row::Line(fi, hi, li)));
            }
        }
    }
    out
}

/// The Review's base and changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Changes {
    /// The main branch it is compared with, if one was found.
    pub branch: Option<String>,
    /// The commit (or empty tree) diffed against.
    pub base: String,
    pub files: Vec<FileDiff>,
    /// Bytes of diff dropped beyond [`MAX_PATCH_BYTES`].
    pub truncated: bool,
}

/// Stop reading a diff past this size; the file list says so.
pub const MAX_PATCH_BYTES: usize = 20 << 20;

/// Compute a Worktree's changes (see the module doc for what is included).
/// Reads only: untracked files are marked intent-to-add in a throwaway copy
/// of the index (`GIT_INDEX_FILE`), never in the Worktree's own index.
/// `base`: the Worktree's recorded base (R28.3), else (or when that ref is
/// gone) the Source's main branch.
pub fn changes(dir: &Path, base: Option<&str>) -> Result<Changes> {
    changes_with(dir, base, false)
}

/// [`changes`], with whitespace-only changes hidden when `ignore_space`
/// (`git diff -w`, R118.6).
pub fn changes_with(dir: &Path, base: Option<&str>, ignore_space: bool) -> Result<Changes> {
    git::require_toplevel(dir)?;
    let branch = base
        .map(|b| git::start_point(dir, b))
        .filter(|b| git::run(dir, &["rev-parse", "--verify", "--quiet", b]).is_ok())
        .or_else(|| git::default_branch(dir));
    let head = git::run(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok();
    let base = match (&branch, &head) {
        (_, None) => git::run(dir, &["hash-object", "-t", "tree", "/dev/null"])?,
        (Some(b), Some(_)) => git::run(dir, &["merge-base", b, "HEAD"])
            .or_else(|_| git::run(dir, &["rev-parse", "HEAD"]))?,
        (None, Some(h)) => h.clone(),
    };
    since(dir, branch, base.trim(), ignore_space)
}

/// Changes of the working tree at `dir` against commit (or tree) `base`
/// (a Fan-out's base commit, R15.3); `branch` is only reported.
pub fn changes_since(dir: &Path, branch: Option<String>, base: &str) -> Result<Changes> {
    since(dir, branch, base, false)
}

/// [`changes_since`] with whitespace-only changes hidden or not.
pub fn since(
    dir: &Path,
    branch: Option<String>,
    base: &str,
    ignore_space: bool,
) -> Result<Changes> {
    let base = base.to_string();
    let untracked: Vec<String> =
        git::run(dir, &["ls-files", "-z", "--others", "--exclude-standard"])?
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

    let index = dir.join(git::run(dir, &["rev-parse", "--git-path", "index"])?.trim());
    let tmp = temp_index();
    let _cleanup = Cleanup(&tmp);
    if index.exists() {
        copy_index(&index, &tmp).map_err(|e| Error::internal(format!("index copy: {e}")))?;
    }
    if !untracked.is_empty() {
        let mut c = git::command(dir)
            .args([
                "add",
                "--intent-to-add",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ])
            .env("GIT_INDEX_FILE", &tmp)
            // Names like `:x` or `a*` are paths, not pathspec magic.
            .env("GIT_LITERAL_PATHSPECS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
        let mut stdin = c.stdin.take().unwrap();
        let _ = stdin.write_all(untracked.join("\0").as_bytes());
        drop(stdin);
        let _ = c.wait();
    }
    let mut child = git::command(dir)
        .args(["diff", "-M", "--no-color", "--no-ext-diff"])
        .args(ignore_space.then_some("-w"))
        .args(["--src-prefix=a/", "--dst-prefix=b/", &base])
        .env("GIT_INDEX_FILE", &tmp)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
    // Read at most the limit (a huge untracked file must not fill memory).
    let mut bytes = Vec::new();
    let mut stdout = child.stdout.take().unwrap();
    use std::io::Read;
    let _ = (&mut stdout)
        .take(MAX_PATCH_BYTES as u64 + 1)
        .read_to_end(&mut bytes);
    let truncated = bytes.len() > MAX_PATCH_BYTES;
    drop(stdout);
    if truncated {
        let _ = child.kill();
    }
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let status = child
        .wait()
        .map_err(|e| Error::internal(format!("git diff: {e}")))?;
    if !truncated && !status.success() {
        return Err(Error::internal(format!("git diff: {}", err.trim())));
    }
    if truncated {
        bytes.truncate(MAX_PATCH_BYTES);
        let cut = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        bytes.truncate(cut);
    }
    let patch = String::from_utf8_lossy(&bytes);
    Ok(Changes {
        branch,
        base,
        files: parse(&patch, &untracked),
        truncated,
    })
}

/// Copy the index keeping its mtime: git trusts a cached stat only for
/// entries older than the index file ("racy git"), so a fresh mtime would
/// hide a same-size edit made in the second the entry was recorded (found
/// live: a Review missed such an edit).
/// A fresh path for a throwaway index (`GIT_INDEX_FILE`).
pub(crate) fn temp_index() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|d| d.is_dir())
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            "rade-index-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ))
}

pub(crate) fn copy_index(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from, to)?;
    let mtime = std::fs::metadata(from)?.modified()?;
    std::fs::File::options()
        .write(true)
        .open(to)?
        .set_modified(mtime)
}

pub(crate) struct Cleanup<'a>(pub(crate) &'a Path);
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
        let _ = std::fs::remove_file(self.0.with_extension("lock"));
    }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
