//! File tree data (R8): one directory's entries with `.gitignore` applied
//! (the `ignore` crate, parent ignore files included), and git status from
//! `git status --porcelain=v1 -z` rolled up onto the folders above; and
//! which Material Icon Theme icon a row shows ([`Icons`]).

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    /// Hidden by `.gitignore` (listed only when showing ignored files).
    pub ignored: bool,
}

/// Git status colors, weakest first: a folder shows its strongest child.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GitState {
    Untracked,
    Added,
    Modified,
    Deleted,
    Conflicted,
}

impl GitState {
    pub fn as_str(self) -> &'static str {
        match self {
            GitState::Untracked => "untracked",
            GitState::Added => "added",
            GitState::Modified => "modified",
            GitState::Deleted => "deleted",
            GitState::Conflicted => "conflicted",
        }
    }
}

fn walk(dir: &Path, filters: bool) -> Vec<(PathBuf, bool)> {
    ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .standard_filters(filters)
        .hidden(false)
        .parents(true)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() == 1)
        .map(|e| {
            let dir = e.file_type().is_some_and(|t| t.is_dir())
                || (e.path_is_symlink() && e.path().is_dir());
            (e.into_path(), dir)
        })
        .collect()
}

/// `dir`'s entries, folders first, then by name (case-insensitive).
/// `.git` is never listed; ignored entries only with `show_ignored`.
pub fn list_dir(dir: &Path, show_ignored: bool) -> Vec<Entry> {
    let kept: Vec<PathBuf> = walk(dir, true).into_iter().map(|(p, _)| p).collect();
    let all = if show_ignored {
        walk(dir, false)
    } else {
        walk(dir, true)
    };
    let mut out: Vec<Entry> = all
        .into_iter()
        .map(|(path, is_dir)| Entry {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            ignored: !kept.contains(&path),
            dir: is_dir,
            path,
        })
        .collect();
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn state_of(xy: &[u8]) -> Option<GitState> {
    let (x, y) = (xy[0], xy[1]);
    Some(match (x, y) {
        (b'?', b'?') => GitState::Untracked,
        (b'!', b'!') => return None,
        (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => GitState::Conflicted,
        (_, b'D') | (b'D', _) => GitState::Deleted,
        (b'A', _) => GitState::Added,
        _ => GitState::Modified,
    })
}

/// Parse `git status --porcelain=v1 -z`: `XY path\0`, renames/copies
/// followed by `orig\0`. Paths are relative to the repository top level.
pub fn parse_porcelain(out: &[u8]) -> Vec<(PathBuf, GitState)> {
    let mut items = out.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut res = Vec::new();
    while let Some(rec) = items.next() {
        if rec.len() < 4 {
            continue;
        }
        let (xy, path) = (&rec[..2], &rec[3..]);
        if matches!(xy[0], b'R' | b'C') {
            items.next(); // the original name
        }
        if let Some(s) = state_of(xy) {
            let path = String::from_utf8_lossy(path);
            res.push((PathBuf::from(path.trim_end_matches('/')), s));
        }
    }
    res
}

/// Absolute path → state, for files and every folder up to `top`
/// (a folder keeps its strongest child; deletions show as modified there).
pub fn status_map(top: &Path, entries: &[(PathBuf, GitState)]) -> HashMap<PathBuf, GitState> {
    let mut map: HashMap<PathBuf, GitState> = HashMap::new();
    for (rel, state) in entries {
        let abs = top.join(rel);
        map.insert(abs.clone(), *state);
        let folder_state = match state {
            GitState::Deleted => GitState::Modified,
            s => *s,
        };
        let mut dir = abs.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(top) && *d != top) {
            let e = map.entry(d.to_path_buf()).or_insert(folder_state);
            *e = (*e).max(folder_state);
            dir = d.parent();
        }
    }
    map
}

/// Run git in `root`: the status of every changed path below the
/// repository's top level; `None` outside a repository.
pub fn git_status(root: &Path) -> Option<HashMap<PathBuf, GitState>> {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            // Never write the index: its change would trigger another refresh.
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    let top = git(&["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim());
    let out = git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    Some(status_map(&top, &parse_porcelain(&out.stdout)))
}

/// Name → icon maps of one appearance (keys lowercase; values SVG stems).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IconMaps {
    file_names: HashMap<String, String>,
    file_extensions: HashMap<String, String>,
    folder_names: HashMap<String, String>,
    folder_names_expanded: HashMap<String, String>,
}

/// Catppuccin's VS Code icons mapping (ADR-0037), as vendored by
/// `crates/rade-app/assets/file-icons/vendor.py` (`map.json`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Icons {
    file: String,
    folder: String,
    folder_expanded: String,
    #[serde(flatten)]
    dark: IconMaps,
    light: IconMaps,
}

impl Icons {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// The light theme's own map first, then the shared one.
    fn find(
        &self,
        light: bool,
        key: &str,
        pick: fn(&IconMaps) -> &HashMap<String, String>,
    ) -> Option<&str> {
        light
            .then(|| pick(&self.light).get(key))
            .flatten()
            .or_else(|| pick(&self.dark).get(key))
            .map(String::as_str)
    }

    /// Like VS Code: exact file name (case-insensitive), then the longest
    /// dotted suffix (`d.ts` before `ts`), else the default. VS Code's
    /// languageIds step needs language detection and is skipped.
    pub fn file(&self, name: &str, light: bool) -> &str {
        let name = name.to_lowercase();
        self.find(light, &name, |m| &m.file_names)
            .or_else(|| {
                name.match_indices('.')
                    .find_map(|(i, _)| self.find(light, &name[i + 1..], |m| &m.file_extensions))
            })
            .unwrap_or(&self.file)
    }

    /// Folder by name (case-insensitive), open or closed.
    pub fn folder(&self, name: &str, expanded: bool, light: bool) -> &str {
        let name = name.to_lowercase();
        let (hit, default) = if expanded {
            (
                self.find(light, &name, |m| &m.folder_names_expanded),
                &self.folder_expanded,
            )
        } else {
            (self.find(light, &name, |m| &m.folder_names), &self.folder)
        };
        hit.unwrap_or(default)
    }
}

/// `p` relative to the tree's `root` (R129.3): `.` for the root itself,
/// `None` for a path outside it.
pub fn relative_path(root: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(root).ok()?;
    Some(if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().into_owned()
    })
}

/// What Insert Path in Agent types (R129.3): the relative path, shell-quoted
/// like a drop (R5.2), then a space; never Enter.
pub fn insert_text(root: &Path, p: &Path) -> Option<String> {
    let rel = relative_path(root, p)?;
    Some(format!("{} ", crate::paste::escape_paths(&[rel])?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use GitState::*;

    #[test]
    fn r129_a_relative_path_and_insert_text() {
        let root = Path::new("/w/repo");
        assert_eq!(
            relative_path(root, Path::new("/w/repo/src/a.rs")).as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(relative_path(root, root).as_deref(), Some("."));
        assert_eq!(relative_path(root, Path::new("/w/other/a.rs")), None);
        assert_eq!(relative_path(root, Path::new("/w/repository/a")), None);
        assert_eq!(
            insert_text(root, Path::new("/w/repo/a b.rs")).as_deref(),
            Some("'a b.rs' ")
        );
        assert_eq!(
            insert_text(root, Path::new("/w/repo/src/a.rs")).as_deref(),
            Some("src/a.rs ")
        );
        assert_eq!(insert_text(root, Path::new("/x")), None);
    }

    fn icons() -> Icons {
        Icons::from_json(include_str!("../../rade-app/assets/file-icons/map.json")).unwrap()
    }

    #[test]
    fn r8_icons_file_precedence_like_vs_code() {
        let i = icons();
        let f = |n| i.file(n, false).to_string();
        // Exact names (case-insensitive) beat extensions.
        assert_eq!(f("package.json"), "package-json");
        assert_eq!(f("README.md"), "readme");
        assert_eq!(f("Dockerfile"), "docker");
        assert_eq!(f(".gitignore"), "git");
        // Longest dotted suffix first, then the plain extension.
        assert_eq!(f("types.d.ts"), "typescript-def");
        assert_eq!(f("app.test.ts"), f("x.test.ts"));
        assert_ne!(f("app.test.ts"), f("app.ts"));
        assert_eq!(f("app.ts"), "typescript");
        assert_eq!(f("main.rs"), "rust");
        assert_eq!(f("MAIN.RS"), "rust");
        assert_eq!(f("Cargo.toml"), "cargo");
        assert_eq!(f("Cargo.lock"), "cargo-lock");
        assert_eq!(f(".env"), "env");
        assert_eq!(f("noext"), "_file");
        assert_eq!(f("weird.zzqq"), "_file");
    }

    #[test]
    fn r8_icons_folders_open_and_closed() {
        let i = icons();
        assert_eq!(i.folder("src", false, false), "folder_src");
        assert_eq!(i.folder("SRC", true, false), "folder_src_open");
        assert_eq!(i.folder("docs", false, false), "folder_docs");
        assert_eq!(i.folder("tests", false, false), "folder_tests");
        assert_eq!(i.folder("crates", false, false), "folder_packages");
        assert_eq!(i.folder("node_modules", true, false), "folder_node_open");
        assert_eq!(i.folder(".github", false, false), "folder_github");
        assert_eq!(i.folder(".vscode", false, false), "folder_vscode");
        assert_eq!(i.folder("zzqq", false, false), "_folder");
        assert_eq!(i.folder("zzqq", true, false), "_folder_open");
    }

    #[test]
    fn r8_icons_light_overrides_then_shared() {
        let i = Icons::from_json(
            r#"{"file":"file","folder":"folder","folderExpanded":"folder-open",
                "fileNames":{"a.txt":"a"},"fileExtensions":{"rs":"rust","txt":"doc"},
                "folderNames":{"src":"folder-src"},"folderNamesExpanded":{},
                "light":{"fileNames":{},"fileExtensions":{"rs":"rust_light"},
                         "folderNames":{},"folderNamesExpanded":{}}}"#,
        )
        .unwrap();
        assert_eq!(i.file("x.rs", true), "rust_light");
        assert_eq!(i.file("x.rs", false), "rust");
        assert_eq!(i.file("A.TXT", true), "a");
        assert_eq!(i.file("b.txt", true), "doc");
        assert_eq!(i.folder("src", false, true), "folder-src");
    }

    #[test]
    fn r8_icons_every_mapped_svg_is_vendored() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rade-app/assets/file-icons/icons");
        let i = icons();
        let all = [&i.dark, &i.light].into_iter().flat_map(|m| {
            [
                &m.file_names,
                &m.file_extensions,
                &m.folder_names,
                &m.folder_names_expanded,
            ]
            .into_iter()
            .flat_map(|h| h.values())
        });
        for stem in all.chain([&i.file, &i.folder, &i.folder_expanded]) {
            assert!(dir.join(format!("{stem}.svg")).is_file(), "{stem}");
        }
    }

    #[test]
    fn r8_3_parses_porcelain_z() {
        let out = b" M src/a.rs\0A  new.rs\0?? notes/x.md\0R  b2.rs\0b.rs\0 D gone.rs\0UU both.rs\0AM added-then-edited.rs\0!! target/\0";
        assert_eq!(
            parse_porcelain(out),
            vec![
                ("src/a.rs".into(), Modified),
                ("new.rs".into(), Added),
                ("notes/x.md".into(), Untracked),
                ("b2.rs".into(), Modified),
                ("gone.rs".into(), Deleted),
                ("both.rs".into(), Conflicted),
                ("added-then-edited.rs".into(), Added),
            ]
        );
        assert!(parse_porcelain(b"").is_empty());
    }

    #[test]
    fn r8_3_folders_show_their_strongest_child() {
        let m = status_map(
            Path::new("/r"),
            &[
                ("a/b/new.md".into(), Untracked),
                ("a/c.rs".into(), Modified),
                ("d/x.rs".into(), Deleted),
                ("e/f.rs".into(), Added),
            ],
        );
        assert_eq!(m[Path::new("/r/a/b/new.md")], Untracked);
        assert_eq!(m[Path::new("/r/a/b")], Untracked);
        assert_eq!(m[Path::new("/r/a")], Modified);
        assert_eq!(
            m[Path::new("/r/d")],
            Modified,
            "a deletion marks its folder modified"
        );
        assert_eq!(m[Path::new("/r/d/x.rs")], Deleted);
        assert_eq!(m[Path::new("/r/e")], Added);
        assert!(!m.contains_key(Path::new("/r")));
    }

    fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rade-tree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::create_dir_all(d.join("target/debug")).unwrap();
        std::fs::write(d.join(".gitignore"), "target/\n*.log\n").unwrap();
        std::fs::write(d.join("src/.gitignore"), "gen.rs\n").unwrap();
        for f in [
            "README.md",
            "b.txt",
            "x.log",
            "src/main.rs",
            "src/gen.rs",
            "src/deep/z.rs",
        ] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        d
    }

    fn git(d: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(d)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn r8_3_lists_with_gitignore_and_the_toggle() {
        let d = repo("list");
        git(&d, &["init", "-q"]);
        let names = |v: Vec<Entry>| -> Vec<(String, bool, bool)> {
            v.into_iter().map(|e| (e.name, e.dir, e.ignored)).collect()
        };
        let s = |n: &str, dir, ign| (n.to_string(), dir, ign);
        assert_eq!(
            names(list_dir(&d, false)),
            [
                s("src", true, false),
                s(".gitignore", false, false),
                s("b.txt", false, false),
                s("README.md", false, false)
            ],
            ".git never, target/ and *.log hidden, folders first"
        );
        assert_eq!(
            names(list_dir(&d, true)),
            [
                s("src", true, false),
                s("target", true, true),
                s(".gitignore", false, false),
                s("b.txt", false, false),
                s("README.md", false, false),
                s("x.log", false, true),
            ]
        );
        // A subfolder listing honours its own and its parents' ignore files.
        assert_eq!(
            names(list_dir(&d.join("src"), false)),
            [
                s("deep", true, false),
                s(".gitignore", false, false),
                s("main.rs", false, false)
            ]
        );
        assert!(list_dir(&d.join("src"), true)
            .iter()
            .any(|e| e.name == "gen.rs" && e.ignored));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn r8_3_git_status_of_a_real_repo() {
        let d = repo("status");
        git(&d, &["init", "-q"]);
        git(&d, &["add", "."]);
        git(&d, &["commit", "-qm", "init"]);
        std::fs::write(d.join("src/main.rs"), "changed").unwrap();
        std::fs::write(d.join("src/deep/new.rs"), "new").unwrap();
        std::fs::remove_file(d.join("b.txt")).unwrap();
        let top = std::fs::canonicalize(&d).unwrap();
        let m = git_status(&d).unwrap();
        assert_eq!(m.get(&top.join("src/main.rs")), Some(&Modified));
        assert_eq!(m.get(&top.join("src/deep/new.rs")), Some(&Untracked));
        assert_eq!(m.get(&top.join("src/deep")), Some(&Untracked));
        assert_eq!(m.get(&top.join("src")), Some(&Modified));
        assert_eq!(m.get(&top.join("b.txt")), Some(&Deleted));
        assert_eq!(
            m.get(&top.join("x.log")),
            None,
            "ignored files are not listed"
        );
        assert!(git_status(Path::new("/")).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }
}
