//! Navigation queries (R84, R86): definition, references, callers and
//! callees (call hierarchy), implementations and a file's symbols → one
//! [`Answer`] of locations with names and line previews; the text-search
//! fallback (whole word, labelled) while a server indexes, is missing or
//! failed; grouping by file for the results popover and the CLI's text output.

use super::lsp::{from_lsp_col, path_uri, to_lsp_col, uri_path, Enc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Query {
    Def,
    Refs,
    Callers,
    Callees,
    Impl,
    Symbols,
    /// The type and docs of the name (`textDocument/hover`, R90).
    Hover,
    /// Symbols of the Worktree matching a text (`workspace/symbol`, R92).
    Workspace,
}

impl Query {
    pub fn parse(s: &str) -> Option<Query> {
        serde_json::from_value(json!(s)).ok()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Query::Def => "def",
            Query::Refs => "refs",
            Query::Callers => "callers",
            Query::Callees => "callees",
            Query::Impl => "impl",
            Query::Symbols => "symbols",
            Query::Hover => "hover",
            Query::Workspace => "workspace",
        }
    }

    /// The results popover's title.
    pub fn title(self, word: &str) -> String {
        match self {
            Query::Def => format!("Definition of {word}"),
            Query::Refs => format!("References to {word}"),
            Query::Callers => format!("Callers of {word}"),
            Query::Callees => format!("Calls from {word}"),
            Query::Impl => format!("Implementations of {word}"),
            Query::Symbols => format!("Outline of {word}"),
            Query::Hover => word.to_string(),
            Query::Workspace => format!("Symbols matching {word}"),
        }
    }

    /// The server capability it needs.
    pub fn capability(self) -> &'static str {
        match self {
            Query::Def => "definitionProvider",
            Query::Refs => "referencesProvider",
            Query::Callers | Query::Callees => "callHierarchyProvider",
            Query::Impl => "implementationProvider",
            Query::Symbols => "documentSymbolProvider",
            Query::Hover => "hoverProvider",
            Query::Workspace => "workspaceSymbolProvider",
        }
    }
}

/// One result: a place in a file (1-based line and character column).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    /// The caller / callee / symbol name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The line, trimmed.
    #[serde(default)]
    pub preview: String,
    /// A symbol's kind (`function`, `struct`, …): outline and workspace symbols.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Outline: how deep it is nested (its parent is the nearest item before
    /// it one level up, whose name prefixes its own).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub depth: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Where answers come from.
pub const TEXT: &str = "text";

/// A query's answer (`code.query`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub query: Option<Query>,
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    /// The name at the position (the file's name for `symbols`).
    pub word: String,
    /// The Worktree (results are shown relative to it).
    pub root: PathBuf,
    /// The server's name, or `text` for text matches.
    pub source: String,
    /// `ready`, `indexing`, `no-server`, `off`, `crashed`, `error`,
    /// `unsupported`, `no-word`; `base` = a removed line's name found only
    /// at the base (text matches there, R87.2).
    pub state: String,
    /// One line for the owner: the install command, why text matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub items: Vec<Item>,
    /// Hover: the server's Markdown (a text match: the declaring line as code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Callers / callees: the call hierarchy (`items` = its first level's
    /// call sites for callers, its callees for callees).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<Call>,
    /// Callers: references that are not calls (`f = cond ? name : …`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<Item>,
}

impl Answer {
    pub fn text(&self) -> bool {
        self.source == TEXT
    }

    /// Where it came from: `rust-analyzer`, `text matches (indexing)`, …
    pub fn summary_source(&self) -> String {
        match (self.text(), self.state.as_str()) {
            (true, "indexing") => "text matches (indexing)".to_string(),
            (true, "base") => "text matches at the base".to_string(),
            (true, _) => "text matches".to_string(),
            (false, _) => self.source.clone(),
        }
    }

    /// `3 results in 2 files · rust-analyzer` / `… · text matches (indexing)`.
    pub fn summary(&self) -> String {
        let files = group(&self.items).len();
        let n = self.items.len();
        let what = if self.query == Some(Query::Symbols) {
            "symbol"
        } else {
            "result"
        };
        let plural = |n: usize, w: &str| match n {
            1 => format!("1 {w}"),
            n => format!("{n} {w}s"),
        };
        let mut s = match n {
            0 => format!("No {what}s"),
            // `9 call sites in 3 callers`.
            n if self.query == Some(Query::Callers) && !self.calls.is_empty() => format!(
                "{} in {}",
                plural(n, "call site"),
                plural(self.calls.len(), "caller")
            ),
            n if self.query == Some(Query::Callees) && !self.calls.is_empty() => {
                plural(n, "callee")
            }
            n => plural(n, what),
        };
        if files > 1 {
            s += &format!(" in {files} files");
        }
        if !self.others.is_empty() {
            s += &format!(" · {}", plural(self.others.len(), "other reference"));
        }
        let how = self.summary_source();
        if !how.is_empty() {
            s += &format!(" · {how}");
        }
        s
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The identifier at character `col` (0-based) of `line`, or ending just
/// before it: (word, its first column).
pub fn word_at(line: &str, col: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let mut at = col.min(chars.len());
    if at == chars.len() || !is_word(chars[at]) {
        if at > 0 && is_word(chars[at - 1]) {
            at -= 1;
        } else {
            return None;
        }
    }
    let mut s = at;
    while s > 0 && is_word(chars[s - 1]) {
        s -= 1;
    }
    let mut e = at;
    while e < chars.len() && is_word(chars[e]) {
        e += 1;
    }
    let w: String = chars[s..e].iter().collect();
    (!w.chars().next()?.is_ascii_digit()).then_some((w, s))
}

/// A keyword of the supported languages (Rust, TypeScript/JavaScript,
/// Python, Go) that is never a name elsewhere: never shown as a link
/// under Ctrl (0.7.1). Words that are names in another language or that a
/// server navigates (`map`, `range`, `type`, `new`, `from`, `default`,
/// `match`, `self`, `super`, `crate`) are not here.
pub fn is_keyword(w: &str) -> bool {
    const K: &[&str] = &[
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "def",
        "defer",
        "del",
        "do",
        "dyn",
        "elif",
        "else",
        "enum",
        "except",
        "export",
        "extends",
        "false",
        "False",
        "finally",
        "fn",
        "for",
        "func",
        "function",
        "if",
        "impl",
        "import",
        "in",
        "interface",
        "lambda",
        "let",
        "mod",
        "mut",
        "nil",
        "None",
        "not",
        "null",
        "or",
        "and",
        "package",
        "pass",
        "pub",
        "raise",
        "return",
        "static",
        "struct",
        "switch",
        "this",
        "throw",
        "trait",
        "true",
        "True",
        "try",
        "typeof",
        "undefined",
        "unsafe",
        "use",
        "var",
        "void",
        "where",
        "while",
        "with",
        "yield",
    ];
    K.contains(&w)
}

/// `textDocument/*` params at (0-based line, server column).
pub fn position(path: &Path, line0: u32, col: u32) -> Value {
    json!({"textDocument": {"uri": path_uri(path)}, "position": {"line": line0, "character": col}})
}

/// A raw place from a server: (file, 0-based line, server column).
pub type Raw = (PathBuf, u32, u32, Option<String>);

fn start_of(range: &Value) -> (u32, u32) {
    (
        range["start"]["line"].as_u64().unwrap_or(0) as u32,
        range["start"]["character"].as_u64().unwrap_or(0) as u32,
    )
}

/// `Location | Location[] | LocationLink[] | null`.
pub fn locations(v: &Value) -> Vec<Raw> {
    let one = |l: &Value| {
        let (uri, range) = match l.get("targetUri") {
            Some(u) => (
                u,
                l.get("targetSelectionRange").unwrap_or(&l["targetRange"]),
            ),
            None => (&l["uri"], &l["range"]),
        };
        let p = uri_path(uri.as_str()?)?;
        let (line, col) = start_of(range);
        Some((p, line, col, None))
    };
    match v {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        Value::Null => vec![],
        l => one(l).into_iter().collect(),
    }
}

/// `callHierarchy/incomingCalls`: every call site (each `fromRange`, at
/// least one per caller), named by the caller.
pub fn incoming(v: &Value) -> Vec<Raw> {
    raw_calls(v, "from")
        .into_iter()
        .flat_map(|(item, sites)| {
            let (p, name) = (item.0.clone(), item.3.clone());
            sites
                .into_iter()
                .map(move |(l, c)| (p.clone(), l, c, name.clone()))
        })
        .collect()
}

/// `callHierarchy/outgoingCalls`: each callee at its definition.
pub fn outgoing(v: &Value) -> Vec<Raw> {
    raw_calls(v, "to")
        .into_iter()
        .map(|(item, _)| item)
        .collect()
}

/// `CallHierarchyIncomingCall[]` (`side` = `from`) or
/// `CallHierarchyOutgoingCall[]` (`to`): (the other function at its name,
/// its call sites as server positions — in the caller's file for incoming
/// calls, in the asked function's for outgoing ones; for an incoming call
/// without `fromRanges`, the caller's name).
fn raw_calls(v: &Value, side: &str) -> Vec<(Raw, Vec<(u32, u32)>)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let it = &c[side];
            let p = uri_path(it["uri"].as_str()?)?;
            let (line, col) = start_of(it.get("selectionRange").unwrap_or(&it["range"]));
            let mut sites: Vec<(u32, u32)> = c["fromRanges"]
                .as_array()
                .into_iter()
                .flatten()
                .map(start_of)
                .collect();
            if sites.is_empty() && side == "from" {
                sites.push((line, col));
            }
            Some(((p, line, col, it["name"].as_str().map(String::from)), sites))
        })
        .collect()
}

/// One node of a call hierarchy (R85, 0.7.1): a caller (or callee) at its
/// name, the call sites, and — asked for one level at a time — its own
/// callers (callees).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Where it is declared (1-based line and character column).
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    #[serde(default)]
    pub preview: String,
    /// Each call: in the caller for callers, in the asked function for callees.
    #[serde(default)]
    pub from_ranges: Vec<Item>,
    /// Its callers (callees), when asked for (`--depth`, or expanded).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Call>,
    /// Its children were asked for (an empty list then means none).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expanded: bool,
    /// It is already on the path from the root: not expanded again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recursive: bool,
    /// The server's `CallHierarchyItem`, to ask for the next level.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub item: Value,
}

/// A `CallHierarchyItem`'s identity (its URI and name position): a node
/// already on the path from the root is not expanded again.
pub type ItemKey = (String, u64, u64);

pub fn item_key(item: &Value) -> ItemKey {
    let at = item.get("selectionRange").unwrap_or(&item["range"]);
    (
        item["uri"].as_str().unwrap_or("").to_string(),
        at["start"]["line"].as_u64().unwrap_or(0),
        at["start"]["character"].as_u64().unwrap_or(0),
    )
}

impl Call {
    /// Call sites here and below.
    pub fn site_count(&self) -> usize {
        self.from_ranges.len() + self.children.iter().map(Call::site_count).sum::<usize>()
    }
}

/// Incoming / outgoing calls → nodes with their call sites (`origin` = the
/// asked function's file, where outgoing calls' sites are), sorted by file
/// and line; the server's items kept for the next level.
pub fn calls(v: &Value, incoming: bool, origin: &Path, enc: Enc) -> Vec<Call> {
    let side = if incoming { "from" } else { "to" };
    let items: Vec<&Value> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| &c[side])
        .collect();
    let mut out: Vec<Call> = raw_calls(v, side)
        .into_iter()
        .zip(items)
        .map(|((at, sites), item)| {
            let decl = self::items(vec![at.clone()], enc).pop().unwrap_or_default();
            let in_file = if incoming {
                at.0.clone()
            } else {
                origin.to_path_buf()
            };
            Call {
                name: at.3.clone().unwrap_or_default(),
                kind: item["kind"].as_u64().map(|k| kind_word(k).to_string()),
                path: at.0,
                line: decl.line,
                col: decl.col,
                preview: decl.preview,
                from_ranges: self::items(
                    sites
                        .into_iter()
                        .map(|(l, c)| (in_file.clone(), l, c, None))
                        .collect(),
                    enc,
                ),
                item: item.clone(),
                ..Default::default()
            }
        })
        .collect();
    out.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
    out
}

/// A reference line that only imports the name (`import { x } from`,
/// `use a::x;`, `from m import x`): not a use worth listing.
pub fn import_line(preview: &str) -> bool {
    let t = preview.trim_start();
    [
        "import ", "import{", "use ", "pub use ", "from ", "export {", "export *", "} from ",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

/// References that are not call sites (R85, 0.7.1): `refs` minus the
/// places in `sites` (same file, line and column), imports dropped.
pub fn other_refs(refs: Vec<Item>, sites: &[Item]) -> Vec<Item> {
    refs.into_iter()
        .filter(|r| {
            !import_line(&r.preview)
                && !sites
                    .iter()
                    .any(|s| s.path == r.path && s.line == r.line && s.col == r.col)
        })
        .collect()
}

/// `textDocument/documentSymbol` (nested `DocumentSymbol` or flat
/// `SymbolInformation`), flattened with `Parent.child` names.
pub fn symbols(v: &Value, path: &Path) -> Vec<Raw> {
    fn walk(v: &Value, path: &Path, prefix: &str, out: &mut Vec<Raw>) {
        for s in v.as_array().into_iter().flatten() {
            let name = s["name"].as_str().unwrap_or("?");
            let full = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}.{name}")
            };
            let (p, range) = match s.get("location") {
                Some(l) => (
                    l["uri"].as_str().and_then(uri_path).unwrap_or(path.into()),
                    &l["range"],
                ),
                None => (path.into(), s.get("selectionRange").unwrap_or(&s["range"])),
            };
            let (line, col) = start_of(range);
            out.push((p, line, col, Some(full.clone())));
            if let Some(c) = s.get("children") {
                walk(c, path, &full, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(v, path, "", &mut out);
    out
}

/// `textDocument/hover` contents (`MarkupContent`, `MarkedString`, or a
/// list of them) as one Markdown text; None when empty.
pub fn hover_text(v: &Value) -> Option<String> {
    fn one(c: &Value) -> String {
        match c {
            Value::String(s) => s.clone(),
            Value::Object(o) if o.contains_key("language") => format!(
                "```{}\n{}\n```",
                o["language"].as_str().unwrap_or(""),
                o["value"].as_str().unwrap_or("").trim_end()
            ),
            Value::Object(o) => o.get("value").and_then(Value::as_str).unwrap_or("").into(),
            _ => String::new(),
        }
    }
    let c = v.get("contents")?;
    let parts: Vec<String> = match c {
        Value::Array(a) => a.iter().map(one).collect(),
        c => vec![one(c)],
    };
    let t = parts
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!t.trim().is_empty()).then(|| t.trim().to_string())
}

/// `workspace/symbol` (`SymbolInformation[]` or `WorkspaceSymbol[]`, whose
/// location may have no range): places with names and kind words; only
/// `file:` URIs.
pub fn workspace_symbols(v: &Value) -> Vec<(Raw, String)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let l = &s["location"];
            let p = uri_path(l["uri"].as_str()?)?;
            let (line, col) = l.get("range").map_or((0, 0), start_of);
            let kind = kind_word(s["kind"].as_u64().unwrap_or(0));
            let kind = match s["containerName"].as_str().filter(|c| !c.is_empty()) {
                Some(c) => format!("{kind} in {c}"),
                None => kind.to_string(),
            };
            Some(((p, line, col, s["name"].as_str().map(String::from)), kind))
        })
        .collect()
}

/// `textDocument/documentSymbol` → (1-based line of the name, full
/// `Parent.child` name) → (kind word, depth), for the outline's rows.
pub fn outline_meta(v: &Value) -> HashMap<(u32, String), (&'static str, u32)> {
    fn walk(
        v: &Value,
        prefix: &str,
        depth: u32,
        out: &mut HashMap<(u32, String), (&'static str, u32)>,
    ) {
        for s in v.as_array().into_iter().flatten() {
            let name = s["name"].as_str().unwrap_or("?");
            let full = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}.{name}")
            };
            let r = s
                .get("selectionRange")
                .or(s.get("range"))
                .unwrap_or(&s["location"]["range"]);
            let line = start_of(r).0 + 1;
            out.insert(
                (line, full.clone()),
                (kind_word(s["kind"].as_u64().unwrap_or(0)), depth),
            );
            if let Some(c) = s.get("children") {
                walk(c, &full, depth + 1, out);
            }
        }
    }
    let mut out = HashMap::new();
    walk(v, "", 0, &mut out);
    out
}

/// A file's outline without a server: the names its lines declare (the
/// Impact text fallback's rules), with kinds.
pub fn text_outline(path: &Path) -> Vec<Item> {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned();
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let (name, kind) = super::impact::declared(l)?;
            Some(Item {
                depth: 0,
                path: path.to_path_buf(),
                line: i as u32 + 1,
                col: super::impact::word_col(l, &name),
                preview: preview(l),
                kind: Some(kind.into()),
                name: Some(name),
            })
        })
        .collect()
}

/// Workspace symbols without a server: declarations under `root` whose
/// name contains `query` (any case), at most 200.
pub fn text_symbols(root: &Path, query: &str) -> Vec<Item> {
    if query.trim().is_empty() {
        return vec![];
    }
    let o = crate::search::Options {
        query: query.trim().to_string(),
        ..Default::default()
    };
    let Ok(r) = crate::search::search(root, &o, 2000, &|| false) else {
        return vec![];
    };
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    for f in r.files {
        for h in f.hits {
            let Some((name, kind)) = super::impact::declared(&h.preview) else {
                continue;
            };
            if name.to_lowercase().contains(&q) && out.len() < 200 {
                out.push(Item {
                    depth: 0,
                    path: root.join(&f.path),
                    line: h.line,
                    col: super::impact::word_col(&h.preview, &name),
                    preview: h.preview.clone(),
                    kind: Some(kind.into()),
                    name: Some(name),
                });
            }
        }
    }
    out
}

/// A `documentSymbol` with its whole range (Impact, R88): 0-based lines,
/// the name's server column, the LSP `SymbolKind`; `local` = inside a
/// function or method (its locals are not symbols of the diff).
#[derive(Clone, Debug, PartialEq)]
pub struct Sym {
    pub name: String,
    pub kind: u64,
    pub start: u32,
    pub end: u32,
    pub sel_line: u32,
    pub sel_col: u32,
    pub local: bool,
}

/// Function, method, constructor (`SymbolKind` 12, 6, 9).
pub fn callable(kind: u64) -> bool {
    matches!(kind, 6 | 9 | 12)
}

/// A `SymbolKind` as a word.
pub fn kind_word(kind: u64) -> &'static str {
    match kind {
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        22 => "enum member",
        23 => "struct",
        26 => "type parameter",
        _ => "symbol",
    }
}

/// `textDocument/documentSymbol` flattened with whole ranges (nested
/// `DocumentSymbol` or flat `SymbolInformation`), `Parent.child` names.
pub fn symbol_ranges(v: &Value) -> Vec<Sym> {
    fn walk(v: &Value, prefix: &str, local: bool, out: &mut Vec<Sym>) {
        for s in v.as_array().into_iter().flatten() {
            let name = s["name"].as_str().unwrap_or("?");
            let full = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}.{name}")
            };
            let range = s.get("range").unwrap_or(&s["location"]["range"]);
            let sel = s.get("selectionRange").unwrap_or(range);
            let (start, _) = start_of(range);
            let end = range["end"]["line"].as_u64().map_or(start, |l| l as u32);
            let (sel_line, sel_col) = start_of(sel);
            let kind = s["kind"].as_u64().unwrap_or(0);
            out.push(Sym {
                name: full.clone(),
                kind,
                start,
                end: end.max(start),
                sel_line,
                sel_col,
                local,
            });
            if let Some(c) = s.get("children") {
                walk(c, &full, local || callable(kind), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(v, "", false, &mut out);
    out
}

/// Raw places → items: character columns and previews from the files
/// (read once each), sorted by file and line, duplicates dropped.
pub fn items(raw: Vec<Raw>, enc: Enc) -> Vec<Item> {
    let mut files: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut out: Vec<Item> = raw
        .into_iter()
        .map(|(path, line0, col, name)| {
            let lines = files.entry(path.clone()).or_insert_with(|| {
                std::fs::read(&path)
                    .map(|b| {
                        String::from_utf8_lossy(&b)
                            .lines()
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default()
            });
            let text = lines.get(line0 as usize).map(String::as_str).unwrap_or("");
            Item {
                depth: 0,
                kind: None,
                col: from_lsp_col(text, col, enc) as u32 + 1,
                line: line0 + 1,
                preview: preview(text),
                path,
                name,
            }
        })
        .collect();
    out.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
    out.dedup_by(|a, b| a.path == b.path && a.line == b.line && a.col == b.col);
    out
}

fn preview(line: &str) -> String {
    let t = line.trim();
    if t.chars().count() > 200 {
        t.chars().take(200).collect::<String>() + "…"
    } else {
        t.to_string()
    }
}

/// The server column of a character position in `file`.
pub fn lsp_col(file: &Path, line: u32, col: u32, enc: Enc) -> u32 {
    let text = std::fs::read(file).unwrap_or_default();
    let text = String::from_utf8_lossy(&text);
    let l = text
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("");
    to_lsp_col(l, col.saturating_sub(1) as usize, enc)
}

/// A line that declares `word` (the definition fallback): `fn word`,
/// `struct word`, `class word`, `function word`, `const word =`, `word(…) {`.
pub fn declares(line: &str, word: &str) -> bool {
    let w = regex::escape(word);
    let re = regex::Regex::new(&format!(
        r"(\b(fn|struct|enum|trait|type|mod|const|static|let|impl|macro_rules!|class|interface|function|var|namespace|def|func)\s+(<[^>]*>\s*)?{w}\b)|(\bfunc\s*\([^)]*\)\s*{w}\b)|(^\s*(export\s+)?(default\s+)?(async\s+)?(public\s+|private\s+|protected\s+|static\s+)*{w}\s*(<[^>]*>)?\s*\([^;]*$)"
    ))
    .unwrap();
    re.is_match(line)
}

/// Whole-word matches of `word` under `root` (`.gitignore` applied),
/// capped; `def` keeps the lines that declare it.
pub fn text_matches(root: &Path, word: &str, def: bool) -> Vec<Item> {
    let o = crate::search::Options {
        query: word.to_string(),
        case: true,
        word: true,
        ..Default::default()
    };
    let Ok(r) = crate::search::search(root, &o, 500, &|| false) else {
        return vec![];
    };
    let mut out = Vec::new();
    for f in r.files {
        for h in f.hits {
            if def && !declares(&h.preview, word) {
                continue;
            }
            out.push(Item {
                depth: 0,
                kind: None,
                path: root.join(&f.path),
                line: h.line,
                col: 1,
                name: None,
                preview: h.preview,
            });
        }
    }
    // The real column of the word on the line.
    for it in out.iter_mut() {
        if let Ok(t) = std::fs::read_to_string(&it.path) {
            if let Some(l) = t.lines().nth(it.line as usize - 1) {
                let re = regex::Regex::new(&format!(r"\b{}\b", regex::escape(word))).unwrap();
                if let Some(m) = re.find(l) {
                    it.col = l[..m.start()].chars().count() as u32 + 1;
                }
            }
        }
    }
    out
}

/// Whole-word matches of `word` in the tree at commit `base` (`git grep`;
/// a removed line's name that is gone from the working tree, R87.2),
/// capped at 500; paths under `root` (the file may no longer exist).
pub fn base_matches(root: &Path, base: &str, word: &str) -> Vec<Item> {
    let out = crate::git::command(root)
        .args([
            "grep", "-n", "-w", "-I", "--column", "--null", "-F", "-e", word, base, "--",
        ])
        .output();
    let Ok(out) = out else { return vec![] };
    let text = String::from_utf8_lossy(&out.stdout);
    let prefix = format!("{base}:");
    text.lines()
        .filter_map(|l| {
            let mut it = l.strip_prefix(&prefix)?.splitn(4, '\0');
            let (path, line, col, t) = (it.next()?, it.next()?, it.next()?, it.next()?);
            let byte = col.parse::<usize>().ok()?.saturating_sub(1);
            Some(Item {
                depth: 0,
                kind: None,
                path: root.join(path),
                line: line.parse().ok()?,
                col: t.get(..byte).map_or(1, |p| p.chars().count() as u32 + 1),
                name: None,
                preview: preview(t),
            })
        })
        .take(500)
        .collect()
}

/// Items grouped by file in order: (path, indices).
pub fn group(items: &[Item]) -> Vec<(PathBuf, Vec<usize>)> {
    let mut out: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        match out.iter_mut().find(|(p, _)| p == &it.path) {
            Some((_, v)) => v.push(i),
            None => out.push((it.path.clone(), vec![i])),
        }
    }
    out
}

/// A path relative to `root` when inside it.
pub fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

/// The CLI's text output: a `# …` header, then `path:line:col` lines
/// (tab, name, tab, preview); text matches say so in the header.
pub fn render(a: &Answer) -> String {
    let q = a.query.map_or("", |q| q.as_str());
    let mut out = match (a.query, &a.doc) {
        // A hover is its text, not a count of results.
        (Some(Query::Hover), _) => format!("# hover {} · {}", a.word, a.summary_source()),
        _ => format!("# {q} {} · {}", a.word, a.summary()),
    };
    if let Some(h) = &a.hint {
        out += &format!("\n# {h}");
    }
    out.push('\n');
    if let Some(d) = &a.doc {
        out += d;
        out.push('\n');
    }
    let line = |out: &mut String, it: &Item, name: &str| {
        *out += &format!(
            "{}:{}:{}\t{name}\t{}\n",
            rel(&it.path, &a.root),
            it.line,
            it.col,
            it.preview
        );
    };
    // A call hierarchy: callers as each call site named by its caller,
    // callees at their definitions; deeper levels indented.
    fn tree(out: &mut String, a: &Answer, calls: &[Call], depth: usize) {
        for c in calls {
            let name = if c.recursive {
                format!("{} (recursive)", c.name)
            } else {
                c.name.clone()
            };
            let at = |p: &Path, l: u32, col: u32, preview: &str| {
                format!(
                    "{}{}:{l}:{col}\t{name}\t{preview}\n",
                    "  ".repeat(depth),
                    rel(p, &a.root)
                )
            };
            if a.query == Some(Query::Callers) {
                for s in &c.from_ranges {
                    *out += &at(&s.path, s.line, s.col, &s.preview);
                }
            } else {
                *out += &at(&c.path, c.line, c.col, &c.preview);
            }
            tree(out, a, &c.children, depth + 1);
        }
    }
    if a.calls.is_empty() {
        for it in &a.items {
            let name = match (&it.name, &it.kind) {
                (Some(n), Some(k)) => format!("{n} ({k})"),
                (n, _) => n.clone().unwrap_or_default(),
            };
            line(&mut out, it, &name);
        }
    } else {
        tree(&mut out, a, &a.calls, 0);
    }
    if !a.others.is_empty() {
        out += &format!("# other references (not calls): {}\n", a.others.len());
        for it in &a.others {
            line(&mut out, it, "");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r84_word_at_cursor_or_just_before() {
        let l = "    let total = self.pricing.total(dto);";
        assert_eq!(word_at(l, 30), Some(("total".into(), 29)));
        assert_eq!(
            word_at(l, 34),
            Some(("total".into(), 29)),
            "right after the name"
        );
        assert_eq!(word_at(l, 2), None, "indentation");
        assert_eq!(word_at("x = 42", 5), None, "a number is not a name");
        assert_eq!(word_at("$el.foo_bar", 0), Some(("$el".into(), 0)));
        assert_eq!(word_at("é_ö()", 1), Some(("é_ö".into(), 0)));
        assert!(is_keyword("let") && is_keyword("function") && !is_keyword("greet"));
        assert!(!is_keyword("new") && !is_keyword("from") && !is_keyword("default"));
    }

    #[test]
    fn r84_locations_links_and_single() {
        let u = path_uri(Path::new("/w/a.rs"));
        let loc = json!({"uri": u, "range": {"start": {"line": 2, "character": 3}, "end": {}}});
        assert_eq!(locations(&loc), vec![("/w/a.rs".into(), 2, 3, None)]);
        assert_eq!(locations(&json!([loc, loc])).len(), 2);
        let link = json!([{"targetUri": u, "targetRange": {"start": {"line": 9, "character": 0}},
                           "targetSelectionRange": {"start": {"line": 9, "character": 7}}}]);
        assert_eq!(locations(&link), vec![("/w/a.rs".into(), 9, 7, None)]);
        assert!(locations(&Value::Null).is_empty());
    }

    #[test]
    fn r85_call_hierarchy_callers_at_call_sites_callees_at_definitions() {
        let u = path_uri(Path::new("/w/m.rs"));
        let inc = json!([{"from": {"name": "main", "uri": u, "selectionRange": {"start": {"line": 0, "character": 3}}},
                          "fromRanges": [{"start": {"line": 4, "character": 8}}]}]);
        assert_eq!(
            incoming(&inc),
            vec![("/w/m.rs".into(), 4, 8, Some("main".into()))]
        );
        // Every fromRange; none → the caller's name.
        let two = json!([{"from": {"name": "main", "uri": u, "selectionRange": {"start": {"line": 0, "character": 3}}},
                          "fromRanges": [{"start": {"line": 4, "character": 8}}, {"start": {"line": 6, "character": 2}}]},
                         {"from": {"name": "f", "uri": u, "selectionRange": {"start": {"line": 8, "character": 3}}}}]);
        assert_eq!(
            incoming(&two),
            vec![
                ("/w/m.rs".into(), 4, 8, Some("main".into())),
                ("/w/m.rs".into(), 6, 2, Some("main".into())),
                ("/w/m.rs".into(), 8, 3, Some("f".into()))
            ]
        );
        assert!(import_line("import { momentWithTZ } from './utils';"));
        assert!(import_line("use crate::utils::tz;") && !import_line("const m = tz;"));
        let out = json!([{"to": {"name": "greet", "uri": u, "selectionRange": {"start": {"line": 9, "character": 7}}},
                          "fromRanges": [{"start": {"line": 4, "character": 8}}]}]);
        assert_eq!(
            outgoing(&out),
            vec![("/w/m.rs".into(), 9, 7, Some("greet".into()))]
        );
    }

    #[test]
    fn r86_symbols_nested_and_flat() {
        let p = Path::new("/w/a.rs");
        let nested = json!([{"name": "S", "range": {"start": {"line": 0, "character": 0}},
            "selectionRange": {"start": {"line": 0, "character": 7}},
            "children": [{"name": "new", "selectionRange": {"start": {"line": 2, "character": 11}}, "range": {}}]}]);
        assert_eq!(
            symbols(&nested, p),
            vec![
                (p.into(), 0, 7, Some("S".into())),
                (p.into(), 2, 11, Some("S.new".into()))
            ]
        );
        let flat = json!([{"name": "f", "location": {"uri": path_uri(p), "range": {"start": {"line": 5, "character": 0}}}}]);
        assert_eq!(symbols(&flat, p), vec![(p.into(), 5, 0, Some("f".into()))]);
    }

    #[test]
    fn r88_symbol_ranges_and_locals() {
        let r = |a: u32, b: u32| json!({"start": {"line": a, "character": 0}, "end": {"line": b, "character": 1}});
        let v = json!([{"name": "S", "kind": 23, "range": r(0, 9), "selectionRange": r(0, 0),
            "children": [{"name": "run", "kind": 6, "range": r(2, 6), "selectionRange": {"start": {"line": 2, "character": 7}},
                "children": [{"name": "tmp", "kind": 13, "range": r(3, 3)}]}]},
            {"name": "f", "kind": 12, "location": {"uri": "file:///w/a.rs", "range": r(11, 12)}}]);
        let s = symbol_ranges(&v);
        let got: Vec<(&str, u32, u32, u32, u32, bool)> = s
            .iter()
            .map(|s| {
                (
                    s.name.as_str(),
                    s.start,
                    s.end,
                    s.sel_line,
                    s.sel_col,
                    s.local,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("S", 0, 9, 0, 0, false),
                ("S.run", 2, 6, 2, 7, false),
                ("S.run.tmp", 3, 3, 3, 0, true),
                ("f", 11, 12, 11, 0, false)
            ]
        );
        assert_eq!(kind_word(6), "method");
        assert!(callable(12) && !callable(23));
    }

    fn tmp(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rade-nav-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (f, t) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
        d
    }

    #[test]
    fn r84_items_previews_columns_sorted_and_deduped() {
        let d = tmp(
            "items",
            &[
                ("b.rs", "x\n  let é = 😀greet();\n"),
                ("a.rs", "fn greet() {}\n"),
            ],
        );
        let raw = vec![
            (d.join("b.rs"), 1, 15, None), // UTF-16: "  let é = 😀" = 12 units… col of g
            (d.join("a.rs"), 0, 3, Some("greet".into())),
            (d.join("a.rs"), 0, 3, Some("greet".into())),
        ];
        let it = items(raw, Enc::Utf8);
        assert_eq!(it.len(), 2);
        assert_eq!(
            (it[0].line, it[0].col, it[0].preview.as_str()),
            (1, 4, "fn greet() {}")
        );
        // UTF-8: "  let é = 😀" is 2+3+1+2+3+4 = 15 bytes → character 11.
        assert_eq!((it[1].line, it[1].col), (2, 12));
        assert_eq!(it[1].preview, "let é = 😀greet();");
        assert_eq!(lsp_col(&d.join("b.rs"), 2, 12, Enc::Utf16), 12);
        assert_eq!(group(&it).len(), 2);
    }

    #[test]
    fn r84_text_fallback_whole_word_and_declarations() {
        let d = tmp(
            "text",
            &[
                ("src/lib.rs", "pub fn greet(n: &str) {}\nfn greeting() {}\n"),
                ("src/main.rs", "fn main() {\n    greet(\"x\");\n}\n"),
                (
                    "web/a.ts",
                    "export function greet() {}\nclass Greeter {\n  greet(x: number) {\n",
                ),
                ("target/x.rs", "greet"),
                (".gitignore", "target\n"),
            ],
        );
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&d)
            .status()
            .unwrap();
        let refs = text_matches(&d, "greet", false);
        let at: Vec<String> = refs
            .iter()
            .map(|i| format!("{}:{}:{}", rel(&i.path, &d), i.line, i.col))
            .collect();
        assert_eq!(
            at,
            [
                "src/lib.rs:1:8",
                "src/main.rs:2:5",
                "web/a.ts:1:17",
                "web/a.ts:3:3"
            ]
        );
        let defs: Vec<String> = text_matches(&d, "greet", true)
            .iter()
            .map(|i| format!("{}:{}", rel(&i.path, &d), i.line))
            .collect();
        assert_eq!(
            defs,
            ["src/lib.rs:1", "web/a.ts:1", "web/a.ts:3"],
            "a method declaration counts, the call does not"
        );
        assert!(declares("    async greet(a, b) {", "greet"));
        assert!(!declares("    greet(a);", "greet"));
        assert!(declares("export const greet = () => 1;", "greet"));
        // Python and Go (M30).
        assert!(declares("def greet(name):", "greet"));
        assert!(declares("    async def greet(self):", "greet"));
        assert!(declares("func Greet(s string) string {", "Greet"));
        assert!(declares("func (g *G) Greet() {", "Greet"));
        assert!(!declares("x := Greet()", "Greet"));
    }

    #[test]
    fn r84_summary_and_render() {
        let a = Answer {
            query: Some(Query::Refs),
            word: "greet".into(),
            root: "/w".into(),
            source: TEXT.into(),
            state: "indexing".into(),
            hint: Some("rust-analyzer is indexing".into()),
            items: vec![
                Item {
                    depth: 0,
                    kind: None,
                    path: "/w/a.rs".into(),
                    line: 1,
                    col: 8,
                    name: None,
                    preview: "pub fn greet()".into(),
                },
                Item {
                    depth: 0,
                    kind: None,
                    path: "/w/b.rs".into(),
                    line: 2,
                    col: 5,
                    name: Some("main".into()),
                    preview: "greet()".into(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            a.summary(),
            "2 results in 2 files · text matches (indexing)"
        );
        assert_eq!(
            render(&a),
            "# refs greet · 2 results in 2 files · text matches (indexing)\n# rust-analyzer is indexing\na.rs:1:8\t\tpub fn greet()\nb.rs:2:5\tmain\tgreet()\n"
        );
        let mut b = a.clone();
        b.source = "rust-analyzer".into();
        b.state = "ready".into();
        b.items.truncate(1);
        assert_eq!(b.summary(), "1 result · rust-analyzer");
        assert_eq!(Query::parse("callers"), Some(Query::Callers));
        assert_eq!(Query::parse("x"), None);
    }

    #[test]
    fn r90_hover_contents_every_shape() {
        let md = json!({"contents": {"kind": "markdown", "value": "```rust\nfn greet()\n```\n---\nSays hi."}});
        assert_eq!(
            hover_text(&md).unwrap(),
            "```rust\nfn greet()\n```\n---\nSays hi."
        );
        let marked = json!({"contents": [{"language": "go", "value": "func F()"}, "F does it."]});
        assert_eq!(
            hover_text(&marked).unwrap(),
            "```go\nfunc F()\n```\n\nF does it."
        );
        assert_eq!(hover_text(&json!({"contents": "plain"})).unwrap(), "plain");
        assert_eq!(
            hover_text(&json!({"contents": {"kind": "markdown", "value": "  "}})),
            None
        );
        assert_eq!(hover_text(&json!({"contents": []})), None);
        assert_eq!(hover_text(&Value::Null), None);
        assert_eq!(Query::parse("hover"), Some(Query::Hover));
        assert_eq!(Query::Hover.capability(), "hoverProvider");
        assert_eq!(Query::parse("workspace"), Some(Query::Workspace));
        assert_eq!(Query::Workspace.capability(), "workspaceSymbolProvider");
        assert_eq!(Query::Workspace.title("gre"), "Symbols matching gre");
    }

    #[test]
    fn r92_workspace_symbols_information_and_workspace_symbol() {
        let u = path_uri(Path::new("/w/a.rs"));
        let v = json!([
            {"name": "greet", "kind": 12, "containerName": "lib",
             "location": {"uri": u, "range": {"start": {"line": 4, "character": 7}}}},
            {"name": "Greeter", "kind": 23, "location": {"uri": u}},
            {"name": "nowhere", "kind": 12, "location": {"uri": "untitled:x"}}
        ]);
        let got = workspace_symbols(&v);
        assert_eq!(
            got,
            vec![
                (
                    ("/w/a.rs".into(), 4, 7, Some("greet".into())),
                    "function in lib".to_string()
                ),
                (
                    ("/w/a.rs".into(), 0, 0, Some("Greeter".into())),
                    "struct".to_string()
                ),
            ]
        );
        assert!(workspace_symbols(&Value::Null).is_empty());
    }

    #[test]
    fn r91_r92_text_outline_and_text_symbols() {
        let d = std::env::temp_dir().join(format!("rade-nav-sym-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(
            d.join("src/lib.rs"),
            "pub fn greet() {}\n// fn not_me() {}\nstruct Greeter;\nfn helper() { greet(); }\n",
        )
        .unwrap();
        std::fs::write(
            d.join("m.py"),
            "class Greeting:\n    def greet_all(self):\n        pass\n",
        )
        .unwrap();
        let o = text_outline(&d.join("src/lib.rs"));
        let names: Vec<_> = o
            .iter()
            .map(|i| {
                (
                    i.name.clone().unwrap(),
                    i.line,
                    i.col,
                    i.kind.clone().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("greet".into(), 1, 8, "function".into()),
                ("Greeter".into(), 3, 8, "struct".into()),
                ("helper".into(), 4, 4, "function".into())
            ]
        );
        let s = text_symbols(&d, "GREE");
        let names: Vec<_> = s
            .iter()
            .map(|i| format!("{}:{}", rel(&i.path, &d), i.name.clone().unwrap()))
            .collect();
        assert_eq!(
            names,
            [
                "m.py:Greeting",
                "m.py:greet_all",
                "src/lib.rs:greet",
                "src/lib.rs:Greeter"
            ]
        );
        assert!(text_symbols(&d, "").is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn r91_outline_depths_with_dotted_names_and_container_names() {
        let r = |l: u32| json!({"start": {"line": l, "character": 0}});
        let v = json!([{"name": "app.get('/') callback", "kind": 12, "range": r(0), "selectionRange": r(0),
                        "children": [{"name": "inner", "kind": 13, "range": r(1), "selectionRange": r(1)}]}]);
        let m = outline_meta(&v);
        assert_eq!(
            m[&(1, "app.get('/') callback".to_string())],
            ("function", 0)
        );
        assert_eq!(
            m[&(2, "app.get('/') callback.inner".to_string())],
            ("variable", 1)
        );
        let u = path_uri(Path::new("/w/a.rs"));
        let w =
            json!([{"name": "new", "kind": 6, "containerName": "Greeter", "location": {"uri": u}}]);
        assert_eq!(workspace_symbols(&w)[0].1, "method in Greeter");
        let a = Answer {
            query: Some(Query::Symbols),
            source: "x".into(),
            items: vec![Item::default(); 2],
            ..Default::default()
        };
        assert_eq!(a.summary(), "2 symbols · x");
    }
}
