//! A diff's Impact (PRD R88, ADR-0030): each symbol the diff of a Worktree
//! against its base changes, adds or removes, with its callers outside the
//! diff. With a server: the symbols are its `documentSymbol` ranges that
//! the changed lines fall in, the callers its call hierarchy (functions)
//! or references (the rest), plus whole-word matches in test files it did
//! not return (spike: mocks typed `any`), labelled text. A removed symbol
//! gets the text matches still in the tree. Without a server (missing,
//! off, loading past the budget, hung) the symbols come from declarations
//! at or above the changed lines and the callers from text matches, all
//! labelled. [`render`] is the bounded text for Reviewer, Judge and
//! acceptance prompts and `rade code impact`.

use super::lsp::{self, Client, LspError};
use super::nav::{self, Sym};
use super::servers::{self, Lang, Plan, Pool};
use crate::config::CodeConfig;
use crate::diff::{FileDiff, LineKind};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// A place that uses a changed symbol.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Site {
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    /// The calling function (call hierarchy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub preview: String,
    /// In a test file.
    pub test: bool,
    /// A whole-word text match, not the server's answer.
    pub text: bool,
}

/// A symbol the diff changes, adds or removes, and its callers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Changed {
    pub name: String,
    /// `function`, `method`, `struct`, … (`symbol` when unknown).
    pub kind: String,
    /// `changed`, `added` or `removed`.
    pub change: String,
    /// Its declaration (in the working tree; at the base for a removed one).
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    /// Found from declarations near the changed lines (no server).
    pub text: bool,
    /// Uses outside the diff (for a removed one: uses left in the tree).
    pub callers: Vec<Site>,
}

/// `code.impact`'s answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Impact {
    /// The Worktree.
    pub root: PathBuf,
    /// What it is compared with, and the merge-base commit.
    pub branch: Option<String>,
    pub base: String,
    /// The servers that answered (`rust-analyzer, typescript-language-server`), or `text`.
    pub source: String,
    /// `ready`, `indexing` (a server was still loading: its files are text
    /// matches), `partial` (the budget ran out), `text` (no server).
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub symbols: Vec<Changed>,
    /// How long it took.
    pub ms: u64,
}

/// What [`compute`] needs to know about the Worktree.
#[derive(Clone, Debug)]
pub struct Inputs {
    pub worktree: PathBuf,
    /// The Worktree's recorded base (R28.3), else its Source's main branch.
    pub base: Option<String>,
    pub cfg: CodeConfig,
    pub enabled: bool,
    pub env: Vec<(String, String)>,
    /// Past it, text matches.
    pub budget: Duration,
    /// How long to wait for a server that is still loading (at most the
    /// budget); 0 = text matches for its files at once (the UI asks again).
    pub wait: Duration,
}

/// At most this many symbols are asked about.
const MAX_SYMBOLS: usize = 60;
/// Callers kept per symbol.
const MAX_CALLERS: usize = 50;

/// What a file's diff touches on the new side (1-based lines).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Touched {
    /// Added lines.
    pub added: HashSet<u32>,
    /// The same, in order.
    pub lines: Vec<u32>,
    /// Where lines were removed: between line `g - 1` and line `g`.
    pub gaps: Vec<u32>,
}

pub fn touched(f: &FileDiff) -> Touched {
    let mut t = Touched::default();
    for l in f.hunks.iter().flat_map(|h| &h.lines) {
        match (l.kind, l.new) {
            (LineKind::Added, Some(n)) => {
                t.added.insert(n);
                t.lines.push(n);
            }
            (LineKind::Removed, _) => t.gaps.push(l.at),
            _ => {}
        }
    }
    t.lines.sort_unstable();
    t.gaps.sort_unstable();
    t.gaps.dedup();
    // A removal next to an added line is that line's change.
    t.gaps
        .retain(|g| !t.added.contains(g) && !t.added.contains(&(g - 1)));
    t
}

/// The innermost symbol (not a function's local, not a module) holding
/// each added line, or both lines around a removal, once each: `added`
/// when the line with its name is new and no removed line declared that
/// name (`gone`), else `changed`.
pub fn pick(syms: &[Sym], t: &Touched, gone: &HashSet<String>) -> Vec<(Sym, &'static str)> {
    let mut out: Vec<(Sym, &'static str)> = Vec::new();
    // 0-based (first, last) lines that must lie in the symbol.
    let mut spans: Vec<(u32, u32)> = t.lines.iter().map(|&l| (l - 1, l - 1)).collect();
    spans.extend(t.gaps.iter().filter(|&&g| g >= 2).map(|&g| (g - 2, g - 1)));
    spans.sort_unstable();
    for (a, b) in spans {
        let best = syms
            .iter()
            .filter(|s| !s.local && !(1..=4).contains(&s.kind) && s.start <= a && b <= s.end)
            .min_by_key(|s| s.end - s.start);
        if let Some(s) = best {
            if !out.iter().any(|(o, _)| o == s) {
                let new = t.added.contains(&(s.sel_line + 1)) && !gone.contains(short(&s.name));
                out.push((s.clone(), if new { "added" } else { "changed" }));
            }
        }
    }
    out
}

static DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:[\w$]+(?:\([^)]*\))?\s+)*?(fn|function\*?|def|func|class|struct|enum|trait|interface|type)\s+([A-Za-z_$][\w$]*)").unwrap()
});
static ARROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=]*)?=\s*(?:async\s*)?(?:function\b|\([^)]*\)\s*(?::[^=]*)?=>|[A-Za-z_$][\w$]*\s*=>)").unwrap()
});
static METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:(?:public|private|protected|static|async|readonly|override|get|set)\s+)*([A-Za-z_$][\w$]*)\s*(?:<[^>]*>)?\s*\([^;]*\)\s*(?::[^;{]*)?\{\s*$").unwrap()
});

/// The name a line declares and its kind (the text fallback): `fn x`,
/// `function x`, `def x`, `class x`, …, `const x = (…) =>`, a method
/// `x(…) {`. Comments declare nothing.
pub fn declared(line: &str) -> Option<(String, &'static str)> {
    let t = line.trim_start();
    if ["//", "#", "*", "/*", "--"]
        .iter()
        .any(|c| t.starts_with(c))
    {
        return None;
    }
    if let Some(c) = DECL.captures(line) {
        let kind = match &c[1] {
            "class" => "class",
            "struct" => "struct",
            "enum" => "enum",
            "trait" => "trait",
            "interface" => "interface",
            "type" => "type",
            _ => "function",
        };
        return Some((c[2].to_string(), kind));
    }
    if let Some(c) = ARROW.captures(line) {
        return Some((c[1].to_string(), "function"));
    }
    let c = METHOD.captures(line)?;
    let n = &c[1];
    let kw = [
        "if", "for", "while", "switch", "catch", "function", "return", "else", "match", "with",
    ];
    (!kw.contains(&n)).then(|| (n.to_string(), "method"))
}

fn indent(l: &str) -> usize {
    l.chars().take_while(|c| c.is_whitespace()).count()
}

/// Text fallback for one file's new `lines`: for each touched line, the
/// declaration on it, else the nearest one above that is indented less
/// (the enclosing function or type). (name, kind, line, col, change).
pub fn text_pick(
    lines: &[&str],
    t: &Touched,
    gone: &HashSet<String>,
) -> Vec<(String, &'static str, u32, u32, &'static str)> {
    let added = &t.added;
    // A removal counts at the line before it, unless that line is blank or
    // closes a top-level block (a whole declaration was removed there).
    let mut at_lines = t.lines.clone();
    at_lines.extend(t.gaps.iter().filter_map(|&g| {
        let l = *lines.get((g as usize).checked_sub(2)?)?;
        (!l.trim().is_empty() && !(indent(l) == 0 && l.trim_start().starts_with('}')))
            .then_some(g - 1)
    }));
    at_lines.sort_unstable();
    at_lines.dedup();
    let mut out: Vec<(String, &'static str, u32, u32, &'static str)> = Vec::new();
    for &t in &at_lines {
        let Some(at) = lines.get(t as usize - 1) else {
            continue;
        };
        let blank = at.trim().is_empty();
        let found = (1..=t).rev().find_map(|n| {
            let l = lines[n as usize - 1];
            let d = declared(l)?;
            (n == t || blank || indent(l) < indent(at)).then_some((d, n))
        });
        let Some(((name, kind), n)) = found else {
            continue;
        };
        if out.iter().any(|o| o.0 == name && o.2 == n) {
            continue;
        }
        let l = lines[n as usize - 1];
        let col = word_col(l, &name);
        let new = added.contains(&n) && !gone.contains(&name);
        out.push((name, kind, n, col, if new { "added" } else { "changed" }));
    }
    out
}

/// 1-based character column of `word` (whole word) in `line`.
pub(crate) fn word_col(line: &str, word: &str) -> u32 {
    Regex::new(&format!(r"\b{}\b", regex::escape(word)))
        .ok()
        .and_then(|re| re.find(line))
        .map_or(1, |m| line[..m.start()].chars().count() as u32 + 1)
}

/// A test file: under `test(s)/`, `__tests__/`, `spec/`, `e2e/`, or named
/// `*.test.*`, `*.spec.*`, `*_test.*`, `test_*`, `tests.rs`, `*_tests.rs`.
pub fn is_test(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    let (file, dirs) = parts.split_last().unwrap_or((&"", &[]));
    dirs.iter()
        .any(|d| matches!(*d, "test" | "tests" | "__tests__" | "spec" | "e2e"))
        || file.contains(".test.")
        || file.contains(".spec.")
        || file.starts_with("test_")
        || file
            .rsplit_once('.')
            .is_some_and(|(stem, _)| stem.ends_with("_test") || stem.ends_with("_tests"))
        || *file == "tests.rs"
}

/// Names too common for useful text matches.
fn too_common(name: &str) -> bool {
    name.chars().count() < 4
        || name.contains(' ')
        || [
            "new",
            "from",
            "into",
            "default",
            "clone",
            "drop",
            "main",
            "init",
            "render",
            "test",
            "call",
            "apply",
            "constructor",
            "toString",
            "then",
            "next",
            "build",
            "update",
            "handle",
        ]
        .contains(&name)
}

fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// The diff's added lines per file: is this place part of the diff?
type Added = HashMap<PathBuf, HashSet<u32>>;

fn in_diff(added: &Added, path: &Path, line: u32) -> bool {
    added.get(path).is_some_and(|a| a.contains(&line))
}

fn site(i: nav::Item, root: &Path, name: Option<String>, text: bool) -> Site {
    Site {
        test: is_test(&nav::rel(&i.path, root)),
        path: i.path,
        line: i.line,
        col: i.col,
        name: name.or(i.name),
        preview: i.preview,
        text,
    }
}

/// Files that hold no calls (docs, data, lockfiles).
fn not_code(p: &Path) -> bool {
    let e = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        e,
        "md" | "markdown"
            | "txt"
            | "json"
            | "lock"
            | "toml"
            | "yaml"
            | "yml"
            | "csv"
            | "log"
            | "svg"
    ) || p
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// An import line names a symbol without using it.
fn imports(line: &str) -> bool {
    let t = line.trim_start();
    ["import ", "use ", "from ", "export {", "pub use "]
        .iter()
        .any(|k| t.starts_with(k))
}

/// Text matches of `name` outside the diff, in code, not declaring or
/// importing it.
fn text_uses(root: &Path, name: &str, added: &Added, tests_only: bool) -> Vec<Site> {
    nav::text_matches(root, name, false)
        .into_iter()
        .filter(|i| {
            !nav::declares(&i.preview, name)
                && !imports(&i.preview)
                && !not_code(&i.path)
                && !in_diff(added, &i.path, i.line)
        })
        .map(|i| site(i, root, None, true))
        .filter(|s| !tests_only || s.test)
        .take(MAX_CALLERS)
        .collect()
}

/// Notify a server that the diff's files under its root changed on disk
/// (spike: client-watch servers miss Agents' edits otherwise).
fn notify_changed(cl: &Client, files: &[FileDiff], wt: &Path, root: &Path) {
    let mut changes = Vec::new();
    for f in files {
        let p = wt.join(&f.path);
        if p.starts_with(root) {
            let t = match f.status {
                'A' | 'U' | 'R' => 1,
                'D' => 3,
                _ => 2,
            };
            changes.push(json!({"uri": lsp::path_uri(&p), "type": t}));
        }
        if let Some(o) = &f.old_path {
            changes.push(json!({"uri": lsp::path_uri(&wt.join(o)), "type": 3}));
        }
    }
    if !changes.is_empty() {
        cl.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": changes}),
        );
    }
}

fn left(deadline: Instant) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(60))
}

/// Callers of one symbol from the server: incoming calls (each call site)
/// for a function, else references.
fn ask_callers(
    cl: &Client,
    path: &Path,
    s: &Sym,
    deadline: Instant,
) -> Result<Vec<nav::Raw>, LspError> {
    let pos = nav::position(path, s.sel_line, s.sel_col);
    let calls = !cl.caps["callHierarchyProvider"].is_null()
        && cl.caps["callHierarchyProvider"] != Value::Bool(false);
    if nav::callable(s.kind) && calls {
        let items = cl.request(
            "textDocument/prepareCallHierarchy",
            pos.clone(),
            left(deadline),
        )?;
        if let Some(items) = items.as_array().filter(|a| !a.is_empty()) {
            let mut out = Vec::new();
            for item in items {
                let v = cl.request(
                    "callHierarchy/incomingCalls",
                    json!({"item": item}),
                    left(deadline),
                )?;
                // Each call site, as the results popover's callers (R85).
                out.extend(nav::incoming(&v));
            }
            return Ok(out);
        }
    }
    let mut p = pos;
    p["context"] = json!({"includeDeclaration": false});
    Ok(nav::locations(&cl.request(
        "textDocument/references",
        p,
        left(deadline),
    )?))
}

/// Names declared on the diff's removed lines (a symbol whose declaration
/// line is new is `added` only when its name is not among them).
fn gone_names(files: &[FileDiff]) -> HashSet<String> {
    files
        .iter()
        .flat_map(|f| f.hunks.iter().flat_map(|h| &h.lines))
        .filter(|l| l.kind == LineKind::Removed)
        .filter_map(|l| Some(declared(&l.text)?.0))
        .collect()
}

/// One file through its server: open, symbols, callers, close. `cut` is
/// set when the deadline stopped it early.
#[allow(clippy::too_many_arguments)]
fn server_file(
    cl: &Client,
    wt: &Path,
    f: &FileDiff,
    added: &Added,
    gone: &HashSet<String>,
    deadline: Instant,
    budget: &mut usize,
    cut: &mut bool,
) -> Result<Vec<Changed>, LspError> {
    let path = wt.join(&f.path);
    let Some(_g) = cl.lock_doc(left(deadline)) else {
        return Err(LspError::Timeout);
    };
    let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned();
    let uri = lsp::path_uri(&path);
    cl.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": servers::language_id(&path), "version": 1, "text": text}}),
    );
    cl.first_open(
        Duration::from_millis(1500),
        left(deadline).min(Duration::from_secs(30)),
    );
    let r = (|| {
        let v = cl.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
            left(deadline),
        )?;
        let t = touched(f);
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::new();
        for (s, change) in pick(&nav::symbol_ranges(&v), &t, gone) {
            if Instant::now() >= deadline || *budget == 0 {
                *cut = true;
                break;
            }
            *budget -= 1;
            let raw = ask_callers(cl, &path, &s, deadline)?;
            let own = |i: &nav::Item| i.path == path && (s.start + 1..=s.end + 1).contains(&i.line);
            let callers: Vec<Site> = nav::items(raw, cl.enc)
                .into_iter()
                .filter(|i| !own(i) && !in_diff(added, &i.path, i.line))
                .map(|i| site(i, wt, None, false))
                .take(MAX_CALLERS)
                .collect();
            let line = lines.get(s.sel_line as usize).copied().unwrap_or("");
            out.push(Changed {
                kind: nav::kind_word(s.kind).into(),
                change: change.into(),
                path: path.clone(),
                line: s.sel_line + 1,
                col: lsp::from_lsp_col(line, s.sel_col, cl.enc) as u32 + 1,
                text: false,
                callers,
                name: s.name,
            });
        }
        Ok(out)
    })();
    cl.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );
    r
}

/// One file by text: declarations at or above the changed lines, their
/// whole-word uses outside the diff.
/// `left` symbols may still be listed; past `deadline` their callers
/// are not looked up (`cut`).
struct Limit {
    left: usize,
    deadline: Instant,
    cut: bool,
}

impl Limit {
    /// Callers by text for `name`, if time is left.
    fn uses(&mut self, wt: &Path, name: &str, added: &Added) -> Vec<Site> {
        if too_common(name) {
            return vec![];
        }
        if Instant::now() >= self.deadline {
            self.cut = true;
            return vec![];
        }
        text_uses(wt, name, added, false)
    }
}

fn text_file(
    wt: &Path,
    f: &FileDiff,
    added: &Added,
    gone: &HashSet<String>,
    lim: &mut Limit,
) -> Vec<Changed> {
    let path = wt.join(&f.path);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let picked = text_pick(&lines, &touched(f), gone);
    if picked.len() > lim.left {
        lim.cut = true;
    }
    let n = picked.len().min(lim.left);
    lim.left -= n;
    picked
        .into_iter()
        .take(n)
        .map(|(name, kind, line, col, change)| Changed {
            callers: lim.uses(wt, &name, added),
            name,
            kind: kind.into(),
            change: change.into(),
            path: path.clone(),
            line,
            col,
            text: true,
        })
        .collect()
}

/// Symbols declared on removed lines and declared nowhere in the tree now:
/// each with the text matches still using it.
fn removed(wt: &Path, files: &[FileDiff], added: &Added, lim: &mut Limit) -> Vec<Changed> {
    let mut out: Vec<Changed> = Vec::new();
    for f in files.iter().filter(|f| !f.binary) {
        if lim.left == 0 || Instant::now() >= lim.deadline {
            lim.cut |= f.removed > 0;
            continue;
        }
        let now = std::fs::read_to_string(wt.join(&f.path)).unwrap_or_default();
        let declared_now: HashSet<String> =
            now.lines().filter_map(|l| Some(declared(l)?.0)).collect();
        for l in f.hunks.iter().flat_map(|h| &h.lines) {
            if l.kind != LineKind::Removed {
                continue;
            }
            let Some((name, kind)) = declared(&l.text) else {
                continue;
            };
            if declared_now.contains(&name) || out.iter().any(|o| o.name == name) {
                continue;
            }
            if lim.left == 0 || Instant::now() >= lim.deadline {
                lim.cut = true;
                break;
            }
            if !nav::text_matches(wt, &name, true).is_empty() {
                continue;
            }
            lim.left -= 1;
            out.push(Changed {
                callers: lim.uses(wt, &name, added),
                col: word_col(&l.text, &name),
                name,
                kind: kind.into(),
                change: "removed".into(),
                path: wt.join(f.old_path.as_deref().unwrap_or(&f.path)),
                line: l.old.unwrap_or(1),
                text: true,
            });
        }
    }
    out
}

/// The Impact of `inp.worktree`'s diff against its base; `servers` false =
/// text only (no server is started or asked).
pub fn compute(
    pool: &Mutex<Pool<Client>>,
    inp: &Inputs,
    servers: bool,
) -> crate::rpc::Result<Impact> {
    let t0 = Instant::now();
    let deadline = t0 + inp.budget;
    let wt = &inp.worktree;
    let ch = crate::diff::changes(wt, inp.base.as_deref())?;
    let files: Vec<&FileDiff> = ch.files.iter().filter(|f| !f.binary).collect();
    let added: Added = files
        .iter()
        .map(|f| (wt.join(&f.path), touched(f).added))
        .collect();
    let gone = gone_names(&ch.files);
    let mut by_server: Vec<((PathBuf, Lang), Vec<&FileDiff>)> = Vec::new();
    let mut by_text: Vec<&FileDiff> = Vec::new();
    for f in files.iter().copied().filter(|f| f.status != 'D') {
        match Lang::of(Path::new(&f.path)).filter(|_| servers) {
            Some(lang) => {
                let key = (servers::root_for(&wt.join(&f.path), wt, lang), lang);
                match by_server.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, v)) => v.push(f),
                    None => by_server.push((key, vec![f])),
                }
            }
            None => by_text.push(f),
        }
    }
    let mut out = Impact {
        root: wt.clone(),
        branch: ch.branch.clone(),
        base: ch.base.clone(),
        ..Default::default()
    };
    let mut names: Vec<String> = Vec::new();
    let mut hints: Vec<String> = Vec::new();
    let (mut indexing, mut cut) = (false, false);
    let mut budget = MAX_SYMBOLS;
    for ((root, lang), fs) in by_server {
        let (launch, name) = match servers::plan(lang, &root, wt, &inp.cfg, inp.enabled, &inp.env) {
            Plan::Start { launch, name, .. } => (launch, name),
            Plan::Off => {
                hints.push("Code navigation is off for this repository".into());
                by_text.extend(fs);
                continue;
            }
            Plan::Missing { command, hint } => {
                hints.push(format!("{command} is not installed: {hint}"));
                by_text.extend(fs);
                continue;
            }
        };
        let cl = match super::client(pool, (root.clone(), lang), &launch, &name) {
            Ok(c) => c,
            Err((_, h)) => {
                hints.push(h);
                by_text.extend(fs);
                continue;
            }
        };
        let name = cl.name.clone().unwrap_or(name);
        notify_changed(&cl, &ch.files, wt, &root);
        let ready_by = (t0 + inp.wait).min(deadline);
        while cl.busy() && cl.alive() && Instant::now() < ready_by {
            std::thread::sleep(Duration::from_millis(200));
        }
        if cl.busy() {
            indexing = true;
            hints.push(format!("{name} is indexing: text matches for its files"));
            by_text.extend(fs);
            continue;
        }
        let mut used = false;
        for f in fs {
            if Instant::now() >= deadline || budget == 0 {
                cut = true;
                by_text.push(f);
                continue;
            }
            match server_file(&cl, wt, f, &added, &gone, deadline, &mut budget, &mut cut) {
                Ok(v) => {
                    used = true;
                    out.symbols.extend(v);
                }
                Err(e) => {
                    hints.push(format!("{name}: {e}: text matches for {}", f.path));
                    by_text.push(f);
                }
            }
        }
        if used && !names.contains(&name) {
            names.push(name);
        }
    }
    // What the server found, plus test files it missed (labelled text).
    for s in out.symbols.iter_mut() {
        let n = short(&s.name).to_string();
        if too_common(&n) || s.callers.len() >= MAX_CALLERS {
            continue;
        }
        if Instant::now() >= deadline {
            cut = true;
            break;
        }
        for t in text_uses(wt, &n, &added, true) {
            let own = t.path == s.path && t.line == s.line;
            if !own
                && !s
                    .callers
                    .iter()
                    .any(|c| c.path == t.path && c.line == t.line)
            {
                s.callers.push(t);
            }
        }
        s.callers
            .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    }
    let mut lim = Limit {
        left: budget,
        deadline,
        cut: false,
    };
    for f in by_text.iter() {
        out.symbols
            .extend(text_file(wt, f, &added, &gone, &mut lim));
    }
    out.symbols.extend(removed(wt, &ch.files, &added, &mut lim));
    cut |= lim.cut;
    out.state = match (names.is_empty(), indexing, cut) {
        (_, true, _) => "indexing",
        (_, _, true) => "partial",
        (true, _, _) => "text",
        _ => "ready",
    }
    .into();
    if cut {
        hints.push(format!(
            "Stopped after {} s or {MAX_SYMBOLS} symbols: the rest are text matches",
            inp.budget.as_secs()
        ));
    }
    out.source = if names.is_empty() {
        nav::TEXT.into()
    } else {
        names.join(", ")
    };
    hints.dedup();
    out.hint = (!hints.is_empty()).then(|| hints.join(". "));
    out.ms = t0.elapsed().as_millis() as u64;
    Ok(out)
}

/// [`compute`] within `inp.budget`: the servers get three quarters of it
/// on a thread the caller abandons when it runs out (a server hanging even
/// while it starts can't hold the caller); then text matches get the rest.
pub fn within(pool: &'static Mutex<Pool<Client>>, inp: Inputs) -> crate::rpc::Result<Impact> {
    let t0 = Instant::now();
    let first = inp.budget.mul_f64(0.75);
    let (tx, rx) = mpsc::channel();
    let mut i2 = inp.clone();
    i2.budget = first;
    i2.wait = i2.wait.min(first);
    std::thread::spawn(move || {
        let _ = tx.send(compute(pool, &i2, true));
    });
    match rx.recv_timeout(first + Duration::from_millis(300)) {
        Ok(r) => r,
        Err(_) => {
            let mut text = inp.clone();
            text.budget = inp
                .budget
                .saturating_sub(t0.elapsed())
                .max(Duration::from_secs(1));
            let mut i = compute(pool, &text, false)?;
            let why = format!(
                "No answer from the language server within {} s: text matches",
                first.as_secs()
            );
            i.hint = Some(match i.hint {
                Some(h) => format!("{why}. {h}"),
                None => why,
            });
            Ok(i)
        }
    }
}

/// `2 changed · 3 call sites in 2 files (1 in tests) · 1 removed, still used`.
pub fn summary(i: &Impact) -> String {
    if i.symbols.is_empty() {
        return "No changed symbols found".into();
    }
    let live: Vec<&Changed> = i.symbols.iter().filter(|s| s.change != "removed").collect();
    let sites: Vec<&Site> = live.iter().flat_map(|s| &s.callers).collect();
    let files: HashSet<&PathBuf> = sites.iter().map(|s| &s.path).collect();
    let tests = sites.iter().filter(|s| s.test).count();
    let mut parts = vec![format!("{} changed", live.len())];
    let mut c = match sites.len() {
        1 => "1 call site".to_string(),
        n => format!("{n} call sites"),
    };
    if files.len() > 1 {
        c += &format!(" in {} files", files.len());
    }
    if tests > 0 {
        c += &format!(" ({tests} in tests)");
    }
    parts.push(c);
    let gone = i.symbols.iter().filter(|s| s.change == "removed").count();
    let used = i
        .symbols
        .iter()
        .filter(|s| s.change == "removed" && !s.callers.is_empty())
        .count();
    if used > 0 {
        parts.push(format!("{used} removed, still used"));
    } else if gone > 0 {
        parts.push(format!("{gone} removed"));
    }
    parts.join(" · ")
}

/// Where the answer comes from, for one line under the summary.
pub fn source_line(i: &Impact) -> String {
    match (i.source.as_str(), i.state.as_str()) {
        (nav::TEXT, _) => "text matches".into(),
        (s, "indexing") => format!("{s}; text matches where it was indexing"),
        (s, "partial") => format!("{s}; text matches past the time budget"),
        (s, _) => s.to_string(),
    }
}

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        s.chars().take(n).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// The Impact as text, at most `max_lines` lines (prompts, `rade code impact`).
pub fn render(i: &Impact, max_lines: usize) -> String {
    let rel = |p: &Path| nav::rel(p, &i.root);
    let mut lines = vec![format!("{} · {}", summary(i), source_line(i))];
    if let Some(h) = &i.hint {
        lines.push(format!("# {}", cut(h, 300)));
    }
    for s in &i.symbols {
        let sign = match s.change.as_str() {
            "added" => "+",
            "removed" => "-",
            _ => "~",
        };
        let what = match (s.change.as_str(), s.callers.len()) {
            ("removed", 0) => "no uses left".to_string(),
            ("removed", n) => format!(
                "still used at {n} place{} (text matches)",
                if n == 1 { "" } else { "s" }
            ),
            (_, 0) => "no callers outside the diff".to_string(),
            (_, 1) => "1 call site".to_string(),
            (_, n) => format!("{n} call sites"),
        };
        lines.push(format!(
            "{sign} {} ({}{}) {}:{} — {what}",
            s.name,
            s.kind,
            if s.text && s.change != "removed" {
                ", text"
            } else {
                ""
            },
            rel(&s.path),
            s.line
        ));
        for c in &s.callers {
            let mut tags = Vec::new();
            if c.test {
                tags.push("test");
            }
            if c.text {
                tags.push("text match");
            }
            let tags = if tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", tags.join(", "))
            };
            let name = c
                .name
                .as_deref()
                .map(|n| format!(" {n}"))
                .unwrap_or_default();
            lines.push(format!(
                "    {}:{}{name}{tags}: {}",
                rel(&c.path),
                c.line,
                cut(&c.preview, 100)
            ));
        }
    }
    if lines.len() > max_lines {
        let more = lines.len() - (max_lines - 1);
        lines.truncate(max_lines - 1);
        lines.push(format!(
            "… {more} more lines: run `rade code impact` in the Worktree"
        ));
    }
    lines.join("\n") + "\n"
}

/// The section appended to a Reviewer's, Judge's or acceptance Agent's
/// prompt (R88.4): bounded, or nothing when no symbol changed.
pub fn prompt_section(i: &Impact, title: &str) -> String {
    if i.symbols.is_empty() {
        return String::new();
    }
    format!(
        "\n## Impact{title}\nSymbols this diff changes and their callers outside the diff, \
         from RADE's language servers (`rade code impact`). Check that each call site still \
         works with the change.\n```\n{}```\n",
        render(i, 60)
    )
}

#[cfg(test)]
#[path = "impact_tests.rs"]
mod tests;
