//! Viewer Panes (R7.2, R7.3): where a file opens, navigation with Back,
//! what kind of content a file is, and the `rade-file:` URIs the Markdown
//! renderer loads documents and images through.
//!
//! Placement: a Tab has at most one Viewer that files open into. Opening a
//! file in a Tab that already has a Viewer shows it there (the previous file
//! goes on its Back list); otherwise a new Viewer splits right of the Pane
//! the file was opened from.

use crate::layout::{BackEntry, Pane, PaneKind, SplitDir};
use crate::link::normalize;
use crate::model::{PaneLoc, State};
use crate::rpc::{Error, Result};
use std::path::{Path, PathBuf};

/// Files above this are not shown (R7.2).
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// PDFs above this are not shown (R114.1).
pub const PDF_MAX_BYTES: u64 = 50 * 1024 * 1024;
/// Most entries kept on a Viewer's Back list.
const MAX_BACK: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Markdown,
    /// Rendered by WebKitGTK's PDF viewer (R114.1).
    Pdf,
    /// Rendered, with a Source view (R114.2).
    Html,
    Image,
    Text,
    TooLarge(u64),
    Binary,
    Directory,
    Missing,
}

pub fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("md" | "markdown" | "mdown" | "mkd")
    )
}

fn ext_is(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico")
    )
}

/// What the Viewer shows for `path`: by extension, size, and whether the
/// first 8 KiB hold a NUL byte (binary).
pub fn classify(path: &Path) -> Content {
    let Ok(meta) = std::fs::metadata(path) else {
        return Content::Missing;
    };
    if meta.is_dir() {
        return Content::Directory;
    }
    if ext_is(path, &["pdf"]) {
        return if meta.len() > PDF_MAX_BYTES {
            Content::TooLarge(meta.len())
        } else {
            Content::Pdf
        };
    }
    if meta.len() > MAX_BYTES {
        return Content::TooLarge(meta.len());
    }
    if is_image(path) {
        return Content::Image;
    }
    let mut head = [0u8; 8192];
    let n = std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
        .unwrap_or(0);
    if head[..n].contains(&0) {
        Content::Binary
    } else if is_markdown(path) {
        Content::Markdown
    } else if ext_is(path, &["html", "htm", "xhtml"]) {
        Content::Html
    } else {
        Content::Text
    }
}

/// Content zoom steps of a Viewer (R113.5), like a browser's.
pub const ZOOM_STEPS: [f64; 15] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0,
];

/// The next zoom step from `z`: `dir` > 0 in, < 0 out, 0 = 100 %.
pub fn zoom_step(z: f64, dir: i32) -> f64 {
    match dir.signum() {
        0 => 1.0,
        1 => ZOOM_STEPS
            .iter()
            .copied()
            .find(|s| *s > z + 1e-6)
            .unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1]),
        _ => ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|s| *s < z - 1e-6)
            .unwrap_or(ZOOM_STEPS[0]),
    }
}

/// The zoom as the header shows it: `125%`.
pub fn zoom_label(z: f64) -> String {
    format!("{}%", (z * 100.0).round() as i64)
}

/// `rade-file:///abs/path` with everything but unreserved characters and
/// `/` percent-encoded.
pub fn file_uri(path: &Path) -> String {
    let mut s = String::from("rade-file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s += &format!("%{b:02X}");
        }
    }
    s
}

/// The file a `rade-file:` URI names (query and fragment dropped).
pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("rade-file://")?.split(['?', '#']).next()?;
    let p = PathBuf::from(crate::link::percent_decode(rest));
    p.is_absolute().then(|| normalize(&p))
}

/// A rendered document may load `path` (relative images) only inside
/// `root`: the Viewer's Worktree, or the document's folder when it lives
/// outside it. Symlinks are followed before checking.
pub fn allowed(path: &Path, root: &Path) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| normalize(p));
    real(path).starts_with(real(root))
}

/// The root a Viewer's document may read from (see [`allowed`]).
pub fn access_root(file: &Path, worktree: &Path) -> PathBuf {
    if normalize(file).starts_with(normalize(worktree)) {
        worktree.to_path_buf()
    } else {
        file.parent().unwrap_or(Path::new("/")).to_path_buf()
    }
}

/// argv for the Viewer's editor action (R7.2), VS Code style:
/// `<editor> <worktree> --goto <file>[:line]` for a file inside the
/// Worktree, else `<editor> --goto <file>[:line]`. VS Code focuses the
/// window that already has the folder open (one per folder; checked with
/// 1.139) and opens a new one otherwise. `editor` may carry arguments.
pub fn editor_argv(editor: &str, file: &Path, line: Option<u32>, worktree: &Path) -> Vec<String> {
    let mut argv: Vec<String> = editor.split_whitespace().map(String::from).collect();
    if normalize(file).starts_with(normalize(worktree)) {
        argv.push(worktree.display().to_string());
    }
    argv.push("--goto".into());
    argv.push(match line {
        Some(l) => format!("{}:{l}", file.display()),
        None => file.display().to_string(),
    });
    argv
}

/// Add a place to a Back / Forward list (at most [`MAX_BACK`]).
fn push(list: &mut Vec<BackEntry>, path: PathBuf, line: Option<u32>) {
    list.push(BackEntry { path, line });
    if list.len() > MAX_BACK {
        list.remove(0);
    }
}

impl Pane {
    pub fn viewer(id: String, path: PathBuf, line: Option<u32>) -> Self {
        let mut p = Pane::new(id, PathBuf::new(), None, None);
        p.kind = PaneKind::Viewer {
            path,
            line,
            back: Vec::new(),
            forward: Vec::new(),
        };
        p
    }
}

impl State {
    fn viewer_kind(&mut self, pane: &str) -> Result<&mut PaneKind> {
        let loc = self.locate_or_err(pane)?;
        match &mut self.tab_mut(&loc)?.pane_mut(pane).unwrap().kind {
            k @ PaneKind::Viewer { .. } => Ok(k),
            _ => Err(Error::usage(format!("pane {pane} is not a Viewer"))),
        }
    }

    /// The Viewer a plain open in `loc`'s Tab goes to (R113.3): the one
    /// focused last, else the first.
    fn tab_viewer(&self, loc: &PaneLoc) -> Option<String> {
        let tab = self.tab(loc)?;
        let viewers: Vec<String> = tab
            .panes()
            .into_iter()
            .filter(|p| p.is_viewer())
            .map(|p| p.id.clone())
            .collect();
        tab.last_viewer
            .clone()
            .filter(|l| viewers.contains(l))
            .or_else(|| viewers.into_iter().next())
    }

    /// Open `path` from Pane `from` (R7.1, R113.3): into `from` when it is a
    /// Viewer, else into the Tab's last focused Viewer, else a new Viewer
    /// split right of `from`. `focus` also shows and focuses it. Returns
    /// the Viewer's id.
    pub fn open_viewer(
        &mut self,
        from: &str,
        path: &Path,
        line: Option<u32>,
        focus: bool,
    ) -> Result<String> {
        let loc = self.locate_or_err(from)?;
        let existing = if self.pane(from).is_some_and(|p| p.is_viewer()) {
            Some(from.to_string())
        } else {
            self.tab_viewer(&loc)
        };
        let id = match existing {
            Some(id) => {
                self.viewer_navigate(&id, path, line)?;
                id
            }
            None => {
                let pane = Pane::viewer(self.next("p"), path.into(), line);
                self.split_with(from, SplitDir::Right, pane, false)?
            }
        };
        if focus {
            self.focus_pane(&id)?;
        }
        Ok(id)
    }

    /// Open to the Side (R113.2): always a new Viewer, split right of `from`.
    pub fn open_viewer_side(
        &mut self,
        from: &str,
        path: &Path,
        line: Option<u32>,
        focus: bool,
    ) -> Result<String> {
        self.locate_or_err(from)?;
        let pane = Pane::viewer(self.next("p"), path.into(), line);
        let id = self.split_with(from, SplitDir::Right, pane, false)?;
        if focus {
            self.focus_pane(&id)?;
        }
        Ok(id)
    }

    /// Open `path` in a Worktree (file tree click, `rade open` without a
    /// Pane): beside its active Tab's focused Pane, or in a new Tab.
    pub fn open_viewer_in(
        &mut self,
        source: &str,
        worktree: &str,
        path: &Path,
        line: Option<u32>,
        focus: bool,
    ) -> Result<String> {
        self.open_viewer_at(source, worktree, path, line, focus, false)
    }

    /// [`Self::open_viewer_in`], or with `side` Open to the Side (R113.2).
    pub fn open_viewer_at(
        &mut self,
        source: &str,
        worktree: &str,
        path: &Path,
        line: Option<u32>,
        focus: bool,
        side: bool,
    ) -> Result<String> {
        let wt = self
            .worktree(source, worktree)
            .ok_or_else(|| Error::not_found(format!("no worktree {source}/{worktree}")))?;
        let from = wt
            .active_tab
            .as_deref()
            .and_then(|t| wt.tab(t))
            .map(|t| t.focused.clone());
        match from {
            Some(from) if side => self.open_viewer_side(&from, path, line, focus),
            Some(from) => self.open_viewer(&from, path, line, focus),
            None => {
                let pane = Pane::viewer(self.next("p"), path.into(), line);
                let (tab, id) = self.new_tab_with(source, worktree, pane)?;
                if focus {
                    self.select_tab(&tab)?;
                    self.focus_pane(&id)?;
                }
                Ok(id)
            }
        }
    }

    /// Show another file in a Viewer; the current one goes on its Back
    /// list (Forward is cleared). The same file only moves the line.
    pub fn viewer_navigate(&mut self, pane: &str, to: &Path, to_line: Option<u32>) -> Result<()> {
        let PaneKind::Viewer {
            path,
            line,
            back,
            forward,
        } = self.viewer_kind(pane)?
        else {
            unreachable!()
        };
        if path != to || *line != to_line {
            forward.clear();
        }
        if path != to {
            push(back, std::mem::replace(path, to.into()), *line);
        }
        *line = to_line;
        Ok(())
    }

    /// A code navigation jump (R84.3): like [`Self::viewer_navigate`],
    /// but a new line in the same file goes on Back too.
    pub fn viewer_jump(&mut self, pane: &str, to: &Path, to_line: Option<u32>) -> Result<()> {
        let PaneKind::Viewer {
            path,
            line,
            back,
            forward,
        } = self.viewer_kind(pane)?
        else {
            unreachable!()
        };
        if path != to || *line != to_line {
            push(back, std::mem::replace(path, to.into()), *line);
            forward.clear();
        }
        *line = to_line;
        Ok(())
    }

    /// A code navigation jump from a Pane that is not a Viewer (the Review
    /// Pane, R87.3): its Tab's Viewer jumps (its place goes on Back, Forward
    /// is cleared), else a new Viewer opens right of it. Returns the Viewer.
    pub fn viewer_jump_from(&mut self, from: &str, to: &Path, line: Option<u32>) -> Result<String> {
        let loc = self.locate_or_err(from)?;
        let existing = self.tab_viewer(&loc);
        match existing {
            Some(id) => {
                self.viewer_jump(&id, to, line)?;
                Ok(id)
            }
            None => {
                let pane = Pane::viewer(self.next("p"), to.into(), line);
                self.split_with(from, SplitDir::Right, pane, false)
            }
        }
    }

    /// Back to the previous place (R7.3); the current one goes on
    /// Forward. False if there is none.
    pub fn viewer_back(&mut self, pane: &str) -> Result<bool> {
        self.viewer_step(pane, true)
    }

    /// Forward again (Alt+Right, R84.3); false if there is none.
    pub fn viewer_forward(&mut self, pane: &str) -> Result<bool> {
        self.viewer_step(pane, false)
    }

    fn viewer_step(&mut self, pane: &str, back_: bool) -> Result<bool> {
        let PaneKind::Viewer {
            path,
            line,
            back,
            forward,
        } = self.viewer_kind(pane)?
        else {
            unreachable!()
        };
        let (from, to) = if back_ {
            (back, forward)
        } else {
            (forward, back)
        };
        match from.pop() {
            Some(prev) => {
                push(to, std::mem::replace(path, prev.path), *line);
                *line = prev.line;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Where a Pane lives, for callers outside the crate.
    pub fn pane_loc(&self, pane: &str) -> Option<PaneLoc> {
        self.locate(pane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;

    #[test]
    fn r7_2_editor_argv_opens_the_worktree_folder_then_the_file() {
        let wt = Path::new("/code/x");
        assert_eq!(
            editor_argv("code", Path::new("/code/x/src/main.rs"), Some(42), wt),
            ["code", "/code/x", "--goto", "/code/x/src/main.rs:42"]
        );
        assert_eq!(
            editor_argv("code", Path::new("/code/x/README.md"), None, wt),
            ["code", "/code/x", "--goto", "/code/x/README.md"]
        );
        // Outside the Worktree: just the file.
        assert_eq!(
            editor_argv("code", Path::new("/etc/hosts"), Some(3), wt),
            ["code", "--goto", "/etc/hosts:3"]
        );
        assert_eq!(
            editor_argv("code", Path::new("/code/x/../y/a.rs"), Some(1), wt),
            ["code", "--goto", "/code/x/../y/a.rs:1"]
        );
        assert_eq!(
            editor_argv("codium --new-window", Path::new("/code/x/a"), Some(1), wt),
            ["codium", "--new-window", "/code/x", "--goto", "/code/x/a:1"]
        );
    }

    fn ws() -> (State, String) {
        let mut w = State::new(Path::new("/h"));
        let p = w.add_source(Path::new("/code/x"), None).unwrap();
        let wt = w.source(&p).unwrap().worktrees[0].id.clone();
        let (_, pane) = w.new_tab(&p, &wt, None, None).unwrap();
        (w, pane)
    }

    fn viewer_of<'a>(w: &'a State, id: &str) -> (&'a Path, Option<u32>, &'a [BackEntry]) {
        match &w.pane(id).unwrap().kind {
            PaneKind::Viewer {
                path, line, back, ..
            } => (path, *line, back),
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn r7_2_opens_split_right_then_reuses_the_tabs_viewer() {
        let (mut w, term) = ws();
        let v = w
            .open_viewer(&term, Path::new("/code/x/src/main.rs"), Some(42), true)
            .unwrap();
        let loc = w.locate(&term).unwrap();
        let tab = w.tab(&loc).unwrap();
        match &tab.layout {
            Layout::Split { dir, a, b, .. } => {
                assert_eq!(*dir, SplitDir::Right);
                assert_eq!(a.panes()[0].id, term);
                assert_eq!(b.panes()[0].id, v);
            }
            l => panic!("{l:?}"),
        }
        assert_eq!(tab.focused, v, "focus = true");
        assert_eq!(
            viewer_of(&w, &v),
            (Path::new("/code/x/src/main.rs"), Some(42), &[][..])
        );

        // Second file, from the Terminal or the Viewer: same Viewer, Back list.
        let again = w
            .open_viewer(&term, Path::new("/code/x/README.md"), None, false)
            .unwrap();
        assert_eq!(again, v);
        assert_eq!(w.tab(&loc).unwrap().panes().len(), 2);
        let (p, l, back) = viewer_of(&w, &v);
        assert_eq!((p, l), (Path::new("/code/x/README.md"), None));
        assert_eq!(
            back,
            [BackEntry {
                path: "/code/x/src/main.rs".into(),
                line: Some(42)
            }]
        );
        assert_eq!(
            w.open_viewer(&v, Path::new("/code/x/README.md"), Some(3), false)
                .unwrap(),
            v
        );
        assert_eq!(viewer_of(&w, &v).2.len(), 1, "same file: line only");
    }

    #[test]
    fn r7_3_back_navigation() {
        let (mut w, term) = ws();
        let v = w
            .open_viewer(&term, Path::new("/a.md"), None, false)
            .unwrap();
        w.viewer_navigate(&v, Path::new("/b.md"), None).unwrap();
        w.viewer_navigate(&v, Path::new("/c.md"), Some(5)).unwrap();
        assert!(w.viewer_back(&v).unwrap());
        assert_eq!(viewer_of(&w, &v).0, Path::new("/b.md"));
        assert!(w.viewer_back(&v).unwrap());
        assert_eq!(viewer_of(&w, &v).0, Path::new("/a.md"));
        assert!(!w.viewer_back(&v).unwrap());
        assert!(w.viewer_back(&term).is_err(), "not a Viewer");
    }

    #[test]
    fn r87_3_a_jump_from_the_review_uses_its_tabs_viewer() {
        let (mut w, term) = ws();
        // No Viewer yet: one opens beside the Pane.
        let v = w
            .viewer_jump_from(&term, Path::new("/x/a.rs"), Some(3))
            .unwrap();
        assert_eq!(viewer_of(&w, &v).1, Some(3));
        assert!(viewer_of(&w, &v).2.is_empty());
        // Then the same Viewer jumps: Back returns.
        let again = w
            .viewer_jump_from(&term, Path::new("/x/a.rs"), Some(9))
            .unwrap();
        assert_eq!(again, v);
        assert_eq!(viewer_of(&w, &v).2.len(), 1);
        assert!(w.viewer_back(&v).unwrap());
        assert_eq!(viewer_of(&w, &v).1, Some(3));
        assert!(w
            .viewer_jump_from("nope", Path::new("/x/a.rs"), None)
            .is_err());
    }

    #[test]
    fn r84_3_jumps_back_and_forward() {
        let (mut w, term) = ws();
        let v = w
            .open_viewer(&term, Path::new("/x/a.rs"), Some(10), false)
            .unwrap();
        let fwd = |w: &State| match &w.pane(&v).unwrap().kind {
            PaneKind::Viewer { forward, .. } => forward.clone(),
            _ => panic!(),
        };
        // A definition in the same file is a place of its own.
        w.viewer_jump(&v, Path::new("/x/a.rs"), Some(40)).unwrap();
        w.viewer_jump(&v, Path::new("/x/b.rs"), Some(3)).unwrap();
        w.viewer_jump(&v, Path::new("/x/b.rs"), Some(3)).unwrap();
        assert_eq!(viewer_of(&w, &v).2.len(), 2, "the same place is not pushed");
        assert!(w.viewer_back(&v).unwrap());
        assert_eq!(
            (viewer_of(&w, &v).0, viewer_of(&w, &v).1),
            (Path::new("/x/a.rs"), Some(40))
        );
        assert!(w.viewer_back(&v).unwrap());
        assert_eq!(viewer_of(&w, &v).1, Some(10));
        assert!(!w.viewer_back(&v).unwrap());
        assert_eq!(fwd(&w).len(), 2);
        assert!(w.viewer_forward(&v).unwrap());
        assert_eq!(viewer_of(&w, &v).1, Some(40));
        assert!(w.viewer_forward(&v).unwrap());
        assert_eq!(
            (viewer_of(&w, &v).0, viewer_of(&w, &v).1),
            (Path::new("/x/b.rs"), Some(3))
        );
        assert!(!w.viewer_forward(&v).unwrap());
        // A new jump after Back clears Forward.
        w.viewer_back(&v).unwrap();
        w.viewer_jump(&v, Path::new("/x/c.rs"), Some(1)).unwrap();
        assert!(fwd(&w).is_empty());
        assert!(!w.viewer_forward(&v).unwrap());
        // Opening a file the old way clears it too, the same file at
        // another line included (acceptance L7).
        w.viewer_back(&v).unwrap();
        w.viewer_navigate(&v, Path::new("/x/d.md"), None).unwrap();
        assert!(fwd(&w).is_empty());
        w.viewer_jump(&v, Path::new("/x/e.rs"), Some(1)).unwrap();
        w.viewer_back(&v).unwrap();
        w.viewer_navigate(&v, Path::new("/x/d.md"), Some(9))
            .unwrap();
        assert!(fwd(&w).is_empty());
    }

    /// Live bug (M4 live run): Back returned to the file but not its line.
    #[test]
    fn r7_3_back_restores_the_line() {
        let (mut w, term) = ws();
        let v = w
            .open_viewer(&term, Path::new("/code/x/src/main.rs"), Some(42), false)
            .unwrap();
        w.open_viewer(&term, Path::new("/code/x/src/lib.rs"), Some(7), false)
            .unwrap();
        w.viewer_navigate(&v, Path::new("/code/x/README.md"), None)
            .unwrap();
        assert!(w.viewer_back(&v).unwrap());
        assert_eq!(
            (viewer_of(&w, &v).0, viewer_of(&w, &v).1),
            (Path::new("/code/x/src/lib.rs"), Some(7))
        );
        assert!(w.viewer_back(&v).unwrap());
        assert_eq!(
            (viewer_of(&w, &v).0, viewer_of(&w, &v).1),
            (Path::new("/code/x/src/main.rs"), Some(42))
        );
    }

    #[test]
    fn back_list_reads_pre_m6_bare_paths() {
        let k: PaneKind = serde_json::from_str(
            r#"{"type":"viewer","path":"/c.md","back":["/a.md",{"path":"/b.rs","line":3}]}"#,
        )
        .unwrap();
        let PaneKind::Viewer { back, .. } = k else {
            panic!()
        };
        assert_eq!(
            back,
            [
                BackEntry {
                    path: "/a.md".into(),
                    line: None
                },
                BackEntry {
                    path: "/b.rs".into(),
                    line: Some(3)
                }
            ]
        );
        let json = serde_json::to_string(&back).unwrap();
        assert_eq!(json, r#"[{"path":"/a.md"},{"path":"/b.rs","line":3}]"#);
    }

    #[test]
    fn r7_2_viewer_survives_state_json_and_restore() {
        let (mut w, term) = ws();
        let v = w
            .open_viewer(&term, Path::new("/code/x/a.rs"), Some(7), false)
            .unwrap();
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("\"type\":\"viewer\""), "{json}");
        let mut back: State = serde_json::from_str(&json).unwrap();
        back.prepare_restore(&crate::preset::builtins(), 0);
        assert_eq!(
            viewer_of(&back, &v),
            (Path::new("/code/x/a.rs"), Some(7), &[][..])
        );
        assert!(!back.pane(&v).unwrap().is_program());
    }

    #[test]
    fn opens_in_a_worktree_without_tabs_as_a_new_tab() {
        let mut w = State::new(Path::new("/h"));
        let p = w.add_source(Path::new("/code/y"), None).unwrap();
        let wt = w.source(&p).unwrap().worktrees[0].id.clone();
        let v = w
            .open_viewer_in(&p, &wt, Path::new("/code/y/a.md"), None, true)
            .unwrap();
        assert_eq!(w.active_tab().unwrap().focused, v);
        assert_eq!(w.active().0.id, p);
        // Then beside the focused Pane: reuses that Tab's Viewer.
        assert_eq!(
            w.open_viewer_in(&p, &wt, Path::new("/code/y/b.md"), None, true)
                .unwrap(),
            v
        );
    }

    #[test]
    fn close_viewer_collapses_split() {
        let (mut w, term) = ws();
        let v = w
            .open_viewer(&term, Path::new("/a.rs"), None, true)
            .unwrap();
        w.close_pane(&v).unwrap();
        let loc = w.locate(&term).unwrap();
        assert!(matches!(w.tab(&loc).unwrap().layout, Layout::Leaf { .. }));
        assert_eq!(w.tab(&loc).unwrap().focused, term);
    }

    #[test]
    fn classify_by_extension_size_and_nul() {
        let d = std::env::temp_dir().join(format!("rade-viewer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = |n: &str, b: &[u8]| {
            std::fs::write(d.join(n), b).unwrap();
            classify(&d.join(n))
        };
        assert_eq!(f("a.md", b"# hi"), Content::Markdown);
        assert_eq!(f("A.MARKDOWN", b"x"), Content::Markdown);
        assert_eq!(f("a.rs", b"fn main() {}"), Content::Text);
        assert_eq!(f("a.bin", b"\x7fELF\0\0"), Content::Binary);
        assert_eq!(f("a.png", b"\x89PNG\0"), Content::Image);
        let big = std::fs::File::create(d.join("big.log")).unwrap();
        big.set_len(MAX_BYTES + 1).unwrap();
        assert_eq!(
            classify(&d.join("big.log")),
            Content::TooLarge(MAX_BYTES + 1)
        );
        assert_eq!(classify(&d.join("nope")), Content::Missing);
        assert_eq!(classify(&d), Content::Directory);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn r113_a_open_to_the_side_and_the_last_focused_viewer() {
        let (mut w, term) = ws();
        let a = w
            .open_viewer(&term, Path::new("/a.md"), None, false)
            .unwrap();
        // Open to the Side: a second Viewer right of the first.
        let b = w
            .open_viewer_side(&a, Path::new("/b.md"), Some(3), false)
            .unwrap();
        assert_ne!(a, b);
        let loc = w.locate(&term).unwrap();
        let ids: Vec<String> = w
            .tab(&loc)
            .unwrap()
            .panes()
            .iter()
            .map(|p| p.id.clone())
            .collect();
        assert_eq!(ids, [term.clone(), a.clone(), b.clone()]);
        assert_eq!(viewer_of(&w, &b).0, Path::new("/b.md"));
        // A plain open from the Terminal goes to the last focused Viewer.
        w.focus_pane(&a).unwrap();
        w.focus_pane(&term).unwrap();
        assert_eq!(
            w.open_viewer(&term, Path::new("/c.md"), None, false)
                .unwrap(),
            a
        );
        w.focus_pane(&b).unwrap();
        w.focus_pane(&term).unwrap();
        assert_eq!(
            w.open_viewer(&term, Path::new("/d.md"), None, false)
                .unwrap(),
            b
        );
        // From a Viewer: that Viewer; Back is per Viewer.
        assert_eq!(
            w.open_viewer(&a, Path::new("/e.md"), None, false).unwrap(),
            a
        );
        assert_eq!(viewer_of(&w, &a).2.last().unwrap().path, Path::new("/c.md"));
        assert_eq!(viewer_of(&w, &b).2.last().unwrap().path, Path::new("/b.md"));
        // The last focused one closed: another Viewer of the Tab.
        w.close_pane(&b).unwrap();
        assert_eq!(
            w.open_viewer(&term, Path::new("/f.md"), None, false)
                .unwrap(),
            a
        );
        // Opening to the side from a Terminal: right of the Terminal.
        let c = w
            .open_viewer_side(&term, Path::new("/g.md"), None, true)
            .unwrap();
        assert_eq!(w.tab(&loc).unwrap().focused, c);
        assert_eq!(w.tab(&loc).unwrap().panes().len(), 3);
    }

    #[test]
    fn r113_e_zoom_steps_and_label() {
        assert_eq!(zoom_step(1.0, 1), 1.1);
        assert_eq!(zoom_step(1.0, -1), 0.9);
        assert_eq!(zoom_step(1.3, 1), 1.5);
        assert_eq!(zoom_step(1.3, -1), 1.25);
        assert_eq!(zoom_step(4.0, 1), 4.0);
        assert_eq!(zoom_step(0.25, -1), 0.25);
        assert_eq!(zoom_step(2.7, 0), 1.0);
        // An image's fit scale (0.37) steps to the neighbours.
        assert_eq!(zoom_step(0.37, 1), 0.5);
        assert_eq!(zoom_step(0.37, -1), 0.33);
        assert_eq!(zoom_label(1.25), "125%");
        assert_eq!(zoom_label(0.33), "33%");
    }

    #[test]
    fn r114_a_pdf_and_html_are_rendered_kinds() {
        let d = std::env::temp_dir().join(format!("rade-viewer-r114-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = |n: &str, b: &[u8]| {
            std::fs::write(d.join(n), b).unwrap();
            classify(&d.join(n))
        };
        assert_eq!(f("a.pdf", b"%PDF-1.7\n\0\x01"), Content::Pdf);
        assert_eq!(f("B.PDF", b"%PDF-1.4"), Content::Pdf);
        assert_eq!(f("a.html", b"<h1>x</h1>"), Content::Html);
        assert_eq!(f("a.htm", b"<p>x"), Content::Html);
        // A PDF may be larger than the text limit, up to PDF_MAX_BYTES.
        let big = std::fs::File::create(d.join("big.pdf")).unwrap();
        big.set_len(MAX_BYTES * 4).unwrap();
        assert_eq!(classify(&d.join("big.pdf")), Content::Pdf);
        big.set_len(PDF_MAX_BYTES + 1).unwrap();
        assert_eq!(
            classify(&d.join("big.pdf")),
            Content::TooLarge(PDF_MAX_BYTES + 1)
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn r7_3_rade_file_uris_and_access_root() {
        let p = Path::new("/w/My Docs/ä#1.md");
        let uri = file_uri(p);
        assert_eq!(uri, "rade-file:///w/My%20Docs/%C3%A4%231.md");
        assert_eq!(uri_path(&uri).as_deref(), Some(p));
        assert_eq!(
            uri_path("rade-file:///w/docs/../img/a.png?x#frag").as_deref(),
            Some(Path::new("/w/img/a.png"))
        );
        assert_eq!(
            uri_path("rade-file:///w/a.rs:12").as_deref(),
            Some(Path::new("/w/a.rs:12"))
        );
        assert_eq!(uri_path("https://x/a.png"), None);

        assert!(allowed(Path::new("/w/docs/img.png"), Path::new("/w")));
        assert!(!allowed(Path::new("/w/../etc/passwd"), Path::new("/w")));
        assert!(!allowed(Path::new("/etc/passwd"), Path::new("/w")));
        assert_eq!(
            access_root(Path::new("/w/d/a.md"), Path::new("/w")),
            Path::new("/w")
        );
        assert_eq!(
            access_root(Path::new("/o/a.md"), Path::new("/w")),
            Path::new("/o")
        );
    }
}
