//! The Changes card's view model (R120, R121): groups, rows as a list or a
//! tree, sorts, the commit menu, the smart-commit decision, the subject
//! warning, the message history, the action button and the dialog texts.
//! GTK-free; the widgets live in `rade-app`.

use crate::config::{ScmSort, ScmView};
use crate::scm::{Section, Status};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Merge,
    Staged,
    Changes,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Merge => "MERGE CHANGES",
            Group::Staged => "STAGED CHANGES",
            Group::Changes => "CHANGES",
        }
    }
}

/// One file of a group. `section` is the side its diff shows (`None` for a
/// conflicted file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: String,
    pub orig: Option<String>,
    pub letter: char,
    pub section: Option<Section>,
}

/// The groups of a card in fixed order (Merge, Staged, Changes), empty
/// ones dropped. A file staged and modified again is in Staged (index
/// letter) and in Changes (working tree letter).
pub fn groups(st: &Status) -> Vec<(Group, Vec<Item>)> {
    let (mut merge, mut staged, mut changes) = (Vec::new(), Vec::new(), Vec::new());
    for c in &st.files {
        let item = |letter: char, section: Option<Section>| Item {
            path: c.path.clone(),
            orig: c.orig.clone(),
            letter,
            section,
        };
        if c.conflicted() {
            merge.push(item('!', None));
        } else if c.untracked() {
            changes.push(item('U', Some(Section::Untracked)));
        } else {
            if c.in_section(Section::Staged) {
                staged.push(item(c.x, Some(Section::Staged)));
            }
            if c.y != '.' {
                changes.push(item(c.y, Some(Section::Unstaged)));
            }
        }
    }
    [
        (Group::Merge, merge),
        (Group::Staged, staged),
        (Group::Changes, changes),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// `path` has no trailing slash (`src/ui`), `name` has one (`src/ui/`).
    Folder {
        path: String,
        name: String,
        depth: usize,
        folded: bool,
    },
    /// `index` into the items the rows were built from.
    File {
        index: usize,
        name: String,
        dir: String,
        depth: usize,
    },
}

fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    }
}

fn status_rank(letter: char) -> usize {
    "MARCDU!".find(letter).unwrap_or(7)
}

/// Rows of a group. List: one File per item (`dir` = its folder with a
/// trailing `/`), sorted by `sort`. Tree: folders first, then files, each
/// by lowercase name; a folder whose only child is a folder is joined
/// (`src/ui/`); children of a folded folder are left out.
pub fn rows(items: &[Item], view: ScmView, sort: ScmSort, folded: &HashSet<String>) -> Vec<Row> {
    match view {
        ScmView::List => {
            let mut idx: Vec<usize> = (0..items.len()).collect();
            let name_of = |i: usize| split_path(&items[i].path).1.to_lowercase();
            match sort {
                ScmSort::Path => idx.sort_by(|a, b| items[*a].path.cmp(&items[*b].path)),
                ScmSort::Name => idx.sort_by(|a, b| {
                    (name_of(*a), &items[*a].path).cmp(&(name_of(*b), &items[*b].path))
                }),
                ScmSort::Status => idx.sort_by(|a, b| {
                    (status_rank(items[*a].letter), &items[*a].path)
                        .cmp(&(status_rank(items[*b].letter), &items[*b].path))
                }),
            }
            idx.into_iter()
                .map(|i| {
                    let (dir, name) = split_path(&items[i].path);
                    Row::File {
                        index: i,
                        name: name.to_string(),
                        dir: dir.to_string(),
                        depth: 0,
                    }
                })
                .collect()
        }
        ScmView::Tree => {
            let mut root = Node::default();
            for (i, it) in items.iter().enumerate() {
                let mut node = &mut root;
                let parts: Vec<&str> = it.path.split('/').collect();
                let (file, dirs) = parts.split_last().expect("a path has a part");
                for d in dirs {
                    node = node.dirs.entry((*d).to_string()).or_default();
                }
                node.files.push((file.to_string(), i));
            }
            let mut out = Vec::new();
            emit(&root, "", 0, folded, &mut out);
            out
        }
    }
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<(String, usize)>,
}

fn emit(node: &Node, prefix: &str, depth: usize, folded: &HashSet<String>, out: &mut Vec<Row>) {
    let mut dirs: Vec<(&String, &Node)> = node.dirs.iter().collect();
    dirs.sort_by_key(|(n, _)| n.to_lowercase());
    for (name, child) in dirs {
        // Join a chain of folders that each hold only one folder.
        let (mut joined, mut cur) = (name.clone(), child);
        while cur.files.is_empty() && cur.dirs.len() == 1 {
            let (n, c) = cur.dirs.iter().next().expect("one dir");
            joined = format!("{joined}/{n}");
            cur = c;
        }
        let path = format!("{prefix}{joined}");
        let is_folded = folded.contains(&path);
        out.push(Row::Folder {
            path: path.clone(),
            name: format!("{joined}/"),
            depth,
            folded: is_folded,
        });
        if !is_folded {
            emit(cur, &format!("{path}/"), depth + 1, folded, out);
        }
    }
    let mut files: Vec<&(String, usize)> = node.files.iter().collect();
    files.sort_by_key(|(n, _)| n.to_lowercase());
    for (name, index) in files {
        out.push(Row::File {
            index: *index,
            name: name.clone(),
            dir: String::new(),
            depth,
        });
    }
}

/// What the commit area knows (R121-b).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommitState {
    pub staged: usize,
    pub changes: usize,
    pub message: bool,
    pub head: bool,
    pub head_pushed: bool,
}

impl CommitState {
    fn any_change(&self) -> bool {
        self.staged > 0 || self.changes > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    pub enabled: bool,
}

/// The Commit ▾ items in D9 order with their enablement.
pub fn commit_menu(s: &CommitState) -> Vec<MenuItem> {
    let rewritable = s.head && !s.head_pushed;
    vec![
        MenuItem {
            label: "Commit",
            enabled: commit_enabled(s),
        },
        MenuItem {
            label: "Commit (Amend)",
            enabled: rewritable,
        },
        MenuItem {
            label: "Commit & Push",
            enabled: commit_enabled(s),
        },
        MenuItem {
            label: "Commit All",
            enabled: s.any_change() && s.message,
        },
        MenuItem {
            label: "Undo Last Commit",
            enabled: rewritable,
        },
    ]
}

/// The Commit button: a message and any change.
pub fn commit_enabled(s: &CommitState) -> bool {
    s.message && s.any_change()
}

/// Commit asks "Stage all <n> changes and commit?" (D10).
pub fn smart_prompt(s: &CommitState) -> bool {
    s.message && s.staged == 0 && s.changes > 0
}

/// A first line over 72 characters (characters, not bytes).
pub fn subject_warning(message: &str) -> Option<String> {
    let n = message.lines().next().unwrap_or("").chars().count();
    (n > 72).then(|| format!("Subject: {n} characters (keep it ≤ 72)"))
}

/// Up / Down through the last commit messages, newest first, and back to
/// the draft being typed.
#[derive(Clone, Debug)]
pub struct History {
    messages: Vec<String>,
    draft: String,
    at: Option<usize>,
}

impl History {
    pub fn new(messages: Vec<String>, draft: String) -> Self {
        Self {
            messages,
            draft,
            at: None,
        }
    }

    /// The next older message; `None` at the oldest (or without any).
    pub fn up(&mut self) -> Option<&str> {
        let next = self.at.map_or(0, |i| i + 1);
        if next >= self.messages.len() {
            return None;
        }
        self.at = Some(next);
        Some(&self.messages[next])
    }

    /// The next newer message, then the draft; `None` when already on it.
    pub fn down(&mut self) -> Option<&str> {
        match self.at? {
            0 => {
                self.at = None;
                Some(&self.draft)
            }
            i => {
                self.at = Some(i - 1);
                Some(&self.messages[i - 1])
            }
        }
    }
}

/// D12: `Publish Branch` / `Sync Changes ↑a ↓b` on a card without changes.
pub fn action_button(st: &Status, has_changes: bool, has_commits: bool) -> Option<String> {
    if has_changes || st.branch.is_none() {
        return None;
    }
    if st.own_upstream().is_none() {
        return has_commits.then(|| "Publish Branch".to_string());
    }
    (st.ahead + st.behind > 0).then(|| format!("Sync Changes ↑{} ↓{}", st.ahead, st.behind))
}

/// The card header's button: `("sync", "↑a ↓b")` with an own upstream,
/// `("publish", "")` without, `None` when detached.
pub fn sync_label(st: &Status) -> Option<(&'static str, String)> {
    st.branch.as_ref()?;
    Some(if st.own_upstream().is_some() {
        ("sync", format!("↑{} ↓{}", st.ahead, st.behind))
    } else {
        ("publish", String::new())
    })
}

/// `PROJECT · SHOP` / `SOURCE · SHOP-API`.
pub fn header_title(project: Option<&str>, source: &str) -> String {
    match project {
        Some(p) => format!("PROJECT · {}", p.to_uppercase()),
        None => format!("SOURCE · {}", source.to_uppercase()),
    }
}

/// The message box's placeholder.
pub fn placeholder(branch: Option<&str>) -> String {
    format!(
        "Message (Ctrl+Enter to commit on ‘{}’)",
        branch.unwrap_or("(detached)")
    )
}

/// The discard dialog's (heading, body, button), §5.2.
pub fn discard_dialog(
    names: &[&str],
    tracked: usize,
    untracked: usize,
) -> (String, String, String) {
    let n = tracked + untracked;
    if n == 1 {
        let name = names.first().copied().unwrap_or("");
        return if tracked == 1 {
            (
                format!("Discard changes in “{name}”?"),
                "Its unstaged changes are lost; staged changes stay.".into(),
                "Discard".into(),
            )
        } else {
            (
                format!("Delete “{name}”?"),
                "The file is not tracked by git. It goes to the trash.".into(),
                "Move to Trash".into(),
            )
        };
    }
    let mut clauses = Vec::new();
    if tracked > 0 {
        clauses.push(if tracked == 1 {
            "Unstaged changes in 1 tracked file are lost".to_string()
        } else {
            format!("Unstaged changes in {tracked} tracked files are lost")
        });
    }
    if untracked > 0 {
        clauses.push(if untracked == 1 {
            "1 untracked file goes to the trash".to_string()
        } else {
            format!("{untracked} untracked files go to the trash")
        });
    }
    (
        format!("Discard changes in {n} files?"),
        format!("{}. Staged changes stay.", clauses.join("; ")),
        format!("Discard {n} Files"),
    )
}

#[cfg(test)]
#[path = "scm_view_tests.rs"]
mod tests;
