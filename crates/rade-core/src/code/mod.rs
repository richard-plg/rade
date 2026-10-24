//! Code navigation (PRD R83–R88, ADR-0029, ADR-0030): an LSP client
//! (`lsp`), which server serves a file and the pool of running ones
//! (`servers`), the queries with their text-search fallback (`nav`) and a
//! diff's Impact (`impact`). [`run`] answers one query, [`run_removed`] one
//! about a name on a removed diff line; the app owns the pool and calls
//! them off the GTK thread.

pub mod impact;
pub mod lsp;
pub mod nav;
pub mod servers;

use crate::config::CodeConfig;
use lsp::{Client, Launch, LspError};
use nav::{Answer, Item, Query, TEXT};
use serde_json::json;
use servers::{Lang, Plan, Pool, MAX_CRASHES};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One query's inputs.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub query: Query,
    pub path: PathBuf,
    /// 1-based line and character column.
    pub line: u32,
    pub col: u32,
    /// The Worktree the file is in.
    pub worktree: PathBuf,
    /// The layered `[code]` of its Source.
    pub cfg: CodeConfig,
    /// `[code] enabled` and `[sources.<id>] code`.
    pub enabled: bool,
    /// The Workspace `[env]`.
    pub env: Vec<(String, String)>,
    /// How long to wait for an indexing server before text matches.
    pub wait: Duration,
    /// Per request.
    pub timeout: Duration,
    /// The server root to ask instead of the file's own: a file reached by
    /// navigation outside its Worktree (the std library, a module cache)
    /// is asked through the server it was reached from (0.7.1).
    pub root: Option<PathBuf>,
    /// Callers / callees: levels of the call hierarchy (1 = direct calls).
    pub depth: u32,
}

/// At most this many call hierarchy nodes below the first level per answer,
/// asked for within this long (`--depth`).
const MAX_CALL_NODES: usize = 300;
const DEEPER: Duration = Duration::from_secs(30);

/// `initialize` may take this long.
const START_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 3 } else { 30 });

/// Stop servers in the background (`shutdown`, `exit`, signals).
pub fn stop(servers: Vec<Arc<Client>>, grace: Duration) {
    for s in servers {
        std::thread::spawn(move || s.shutdown(grace));
    }
}

fn line_of(path: &Path, line: u32) -> String {
    std::fs::read(path)
        .map(|b| {
            String::from_utf8_lossy(&b)
                .lines()
                .nth(line.saturating_sub(1) as usize)
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default()
}

/// Text matches for `q` (no server): whole-word matches of the name, the
/// declaring lines for a definition; none for callees and symbols.
fn fallback(q: Query, root: &Path, word: &str, at: (&Path, u32)) -> Vec<Item> {
    match q {
        Query::Def | Query::Impl | Query::Hover => nav::text_matches(root, word, true),
        Query::Refs => nav::text_matches(root, word, false),
        Query::Callers => nav::text_matches(root, word, false)
            .into_iter()
            .filter(|i| !nav::declares(&i.preview, word) && !(i.path == at.0 && i.line == at.1))
            .collect(),
        Query::Symbols => nav::text_outline(at.0),
        Query::Callees | Query::Workspace => vec![],
    }
}

/// A hover from text matches: the first declaring line, as code.
fn text_doc(q: Query, items: &[Item]) -> Option<String> {
    (q == Query::Hover)
        .then(|| items.first().map(|i| format!("```\n{}\n```", i.preview)))
        .flatten()
}

/// A running server, its name and a note about it.
type Connected = (Arc<Client>, String, Option<String>);

/// The server for `c.path` (started if needed) — at `c.root` when given,
/// else at the file's own root: (client, its name, a note), or why there
/// is none (an answer state and hint).
fn connect(pool: &Mutex<Pool<Client>>, c: &Ctx) -> Result<Connected, (&'static str, String)> {
    let Some(lang) = Lang::of(&c.path) else {
        let ext = c
            .path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err((
            "unsupported",
            format!(
                "No language server for .{ext} files (Rust, TypeScript/JavaScript, Python, Go)"
            ),
        ));
    };
    let root = c
        .root
        .clone()
        .unwrap_or_else(|| servers::root_for(&c.path, &c.worktree, lang));
    match servers::plan(lang, &root, &c.worktree, &c.cfg, c.enabled, &c.env) {
        Plan::Off => Err(("off", "Code navigation is off for this repository".into())),
        Plan::Missing { command, hint } => {
            Err(("no-server", format!("{command} is not installed: {hint}")))
        }
        Plan::Start { launch, name, note } => {
            let cl = client(pool, (root, lang), &launch, &name)?;
            Ok((cl, name, note))
        }
    }
}

/// The next level of a call hierarchy under `item` (a node the results popover
/// expands): its callers when `incoming`, else its callees — through the
/// server `c` was answered by, the item's file open meanwhile.
pub fn expand(
    pool: &Mutex<Pool<Client>>,
    c: &Ctx,
    item: &serde_json::Value,
    incoming: bool,
) -> Result<Vec<nav::Call>, String> {
    let (cl, name, _) = connect(pool, c).map_err(|e| e.1)?;
    let name = cl.name.clone().unwrap_or(name);
    let Some(_g) = cl.lock_doc(c.timeout) else {
        return Err(format!("{name} is busy"));
    };
    let file = item["uri"]
        .as_str()
        .and_then(lsp::uri_path)
        .unwrap_or_else(|| c.path.clone());
    let uri = lsp::path_uri(&file);
    let open = std::fs::read(&file).is_ok();
    if open {
        let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap_or_default()).into_owned();
        cl.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": servers::language_id(&file), "version": 1, "text": text}}),
        );
    }
    let r = level(&cl, item, incoming, c.timeout);
    if open {
        cl.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        );
    }
    r.map_err(|e| format!("{name}: {e}"))
}

/// One level of calls under a `CallHierarchyItem`.
fn level(
    cl: &Client,
    item: &serde_json::Value,
    incoming: bool,
    t: Duration,
) -> Result<Vec<nav::Call>, LspError> {
    let m = if incoming {
        "callHierarchy/incomingCalls"
    } else {
        "callHierarchy/outgoingCalls"
    };
    let v = cl.request(m, json!({"item": item}), t)?;
    let origin = item["uri"]
        .as_str()
        .and_then(lsp::uri_path)
        .unwrap_or_default();
    Ok(nav::calls(&v, incoming, &origin, cl.enc))
}

/// `levels` more levels under each node (`--depth`), never into a node
/// already on the path from the root (marked `recursive`), at most
/// `budget` nodes in all and until `until`. A node whose level fails (a
/// timeout, an error) stays unexpanded; the rest of the answer stands.
#[allow(clippy::too_many_arguments)]
fn deepen(
    cl: &Client,
    calls: &mut [nav::Call],
    incoming: bool,
    levels: u32,
    path: &mut Vec<nav::ItemKey>,
    t: Duration,
    budget: &mut usize,
    until: Instant,
) {
    if levels == 0 {
        return;
    }
    for c in calls.iter_mut() {
        let k = nav::item_key(&c.item);
        if path.contains(&k) {
            c.recursive = true;
            continue;
        }
        let left = until.saturating_duration_since(Instant::now());
        if *budget == 0 || left.is_zero() {
            break;
        }
        let Ok(children) = level(cl, &c.item, incoming, t.min(left)) else {
            continue;
        };
        c.children = children;
        c.expanded = true;
        *budget = budget.saturating_sub(c.children.len());
        path.push(k);
        deepen(
            cl,
            &mut c.children,
            incoming,
            levels - 1,
            path,
            t,
            budget,
            until,
        );
        path.pop();
    }
}

/// Answer one query: the server's answer when it has one, else text
/// matches labelled with why.
pub fn run(pool: &Mutex<Pool<Client>>, c: &Ctx) -> Answer {
    let mut a = Answer {
        query: Some(c.query),
        path: c.path.clone(),
        line: c.line,
        col: c.col,
        root: c.worktree.clone(),
        source: TEXT.into(),
        ..Default::default()
    };
    let text = line_of(&c.path, c.line);
    let word = match c.query {
        Query::Symbols | Query::Workspace => Some(
            c.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        _ => nav::word_at(&text, c.col.saturating_sub(1) as usize).map(|w| w.0),
    };
    let Some(word) = word else {
        a.state = "no-word".into();
        a.hint = Some("Put the cursor on a name".into());
        return a;
    };
    a.word = word.clone();
    let text_answer = |mut a: Answer, state: &str, hint: String| {
        a.state = state.into();
        a.hint = Some(hint);
        a.source = TEXT.into();
        a.items = fallback(c.query, &c.worktree, &word, (&c.path, c.line));
        a.doc = text_doc(c.query, &a.items);
        a
    };
    let (client, name, note) = match connect(pool, c) {
        Ok(x) => x,
        Err((state, hint)) => return text_answer(a, state, hint),
    };
    let name = client.name.clone().unwrap_or(name);
    a.hint = note.clone();
    if !client.caps[c.query.capability()]
        .as_bool()
        .unwrap_or(!client.caps[c.query.capability()].is_null())
    {
        let what = match c.query {
            Query::Callers | Query::Callees => "call hierarchy",
            Query::Impl => "implementations",
            Query::Symbols => "symbols",
            Query::Refs => "references",
            Query::Def => "definitions",
            Query::Hover => "hover information",
            Query::Workspace => "workspace symbols",
        };
        let mut h = format!("{name} has no {what}");
        if let Some(n) = &note {
            h = format!("{h}. {n}");
        }
        return text_answer(a, "unsupported", h);
    }
    let deadline = Instant::now() + c.wait;
    loop {
        match ask(&client, c, &text) {
            // While it loads, even a non-empty answer may be from a
            // half-loaded project (a cold tsserver points at the import).
            Ok(got) if !client.busy() => {
                a.state = "ready".into();
                a.source = name.clone();
                (a.items, a.doc, a.calls, a.others) = got;
                return a;
            }
            Ok(_) | Err(LspError::Server(-32802..=-32800, _)) => {
                if Instant::now() >= deadline {
                    return text_answer(
                        a,
                        "indexing",
                        format!("{name} is indexing: text matches until it is ready"),
                    );
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(LspError::Dead(why)) => {
                let why = if why.is_empty() {
                    String::new()
                } else {
                    format!(": {why}")
                };
                return text_answer(
                    a,
                    "error",
                    format!("{name} exited{why}; it restarts on the next request"),
                );
            }
            Err(e) => return text_answer(a, "error", format!("{name}: {e}")),
        }
    }
}

/// Workspace symbols matching `text` (R92): the servers of the languages
/// the Worktree holds (its top-level marker files), started like any
/// navigation; while none can answer (missing, off, indexing past
/// `c.wait`, failed), declarations whose name contains the text, labelled.
/// At most 200.
pub fn workspace(pool: &Mutex<Pool<Client>>, c: &Ctx, text: &str) -> Answer {
    let text = text.trim();
    let mut a = Answer {
        query: Some(Query::Workspace),
        path: c.worktree.clone(),
        word: text.to_string(),
        root: c.worktree.clone(),
        source: TEXT.into(),
        ..Default::default()
    };
    if text.is_empty() {
        a.state = "no-word".into();
        a.hint = Some("Type a symbol name".into());
        return a;
    }
    let deadline = Instant::now() + c.wait;
    let (mut names, mut why, mut indexing) = (Vec::new(), Vec::new(), false);
    let mut found: Vec<Item> = Vec::new();
    // Running servers in this Worktree first, then each other language
    // found in it (a monorepo's packages too), at its usual root.
    let mut targets: Vec<servers::Key> = pool
        .lock()
        .unwrap()
        .list(Instant::now())
        .into_iter()
        .map(|(k, _, _)| k)
        .filter(|k| k.0.starts_with(&c.worktree))
        .collect();
    for lang in servers::Lang::ALL {
        if targets.iter().any(|k| k.1 == lang) {
            continue;
        }
        if let Some(f) = servers::sample_file(&c.worktree, lang) {
            targets.push((servers::root_for(&f, &c.worktree, lang), lang));
        }
    }
    for (root, lang) in targets {
        let (launch, name) =
            match servers::plan(lang, &root, &c.worktree, &c.cfg, c.enabled, &c.env) {
                Plan::Off => {
                    why.push("Code navigation is off for this repository".to_string());
                    continue;
                }
                Plan::Missing { command, hint } => {
                    why.push(format!("{command} is not installed: {hint}"));
                    continue;
                }
                Plan::Start { launch, name, .. } => (launch, name),
            };
        let cl = match client(pool, (root.clone(), lang), &launch, &name) {
            Ok(cl) => cl,
            Err((_, h)) => {
                why.push(h);
                continue;
            }
        };
        let name = cl.name.clone().unwrap_or(name);
        if cl.caps["workspaceSymbolProvider"].is_null()
            || cl.caps["workspaceSymbolProvider"] == json!(false)
        {
            why.push(format!("{name} has no workspace symbols"));
            continue;
        }
        // An open document around the request: tsserver has no project
        // otherwise ("No Project").
        let sample = servers::sample_file(&root, lang);
        let _g = cl.lock_doc(c.timeout);
        if let Some(f) = &sample {
            let body = String::from_utf8_lossy(&std::fs::read(f).unwrap_or_default()).into_owned();
            cl.notify(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": lsp::path_uri(f), "languageId": servers::language_id(f), "version": 1, "text": body}}),
            );
            cl.first_open(
                Duration::from_millis(1500),
                c.wait.max(Duration::from_secs(2)),
            );
        }
        loop {
            match cl.request("workspace/symbol", json!({"query": text}), c.timeout) {
                Ok(v) if !cl.busy() => {
                    let syms = nav::workspace_symbols(&v);
                    let kinds: std::collections::HashMap<_, _> = syms
                        .iter()
                        .map(|(r, k)| ((r.0.clone(), r.1 + 1, r.3.clone()), k.clone()))
                        .collect();
                    let raw = syms.into_iter().map(|(r, _)| r).take(200).collect();
                    found.extend(nav::items(raw, cl.enc).into_iter().map(|mut i| {
                        i.kind = kinds
                            .get(&(i.path.clone(), i.line, i.name.clone()))
                            .map(|k| k.to_string());
                        i
                    }));
                    names.push(name);
                    break;
                }
                Ok(_) | Err(LspError::Server(-32802..=-32800, _)) => {
                    if Instant::now() >= deadline {
                        indexing = true;
                        why.push(format!(
                            "{name} is indexing: text matches until it is ready"
                        ));
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
                Err(e) => {
                    let e = e.to_string();
                    why.push(format!("{name}: {}", e.lines().next().unwrap_or("")));
                    break;
                }
            }
        }
        if let Some(f) = &sample {
            cl.notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": lsp::path_uri(f)}}),
            );
        }
    }
    if names.is_empty() {
        a.state = match () {
            _ if indexing => "indexing",
            _ if why.is_empty() => "unsupported",
            _ if why.iter().all(|w| w.contains("is not installed")) => "no-server",
            _ => "error",
        }
        .into();
        a.hint = Some(if why.is_empty() {
            "No Rust, TypeScript/JavaScript, Python or Go files in this Worktree".into()
        } else {
            why.join("; ")
        });
        a.items = nav::text_symbols(&c.worktree, text);
        return a;
    }
    a.state = "ready".into();
    a.source = names.join(", ");
    a.hint = (!why.is_empty()).then(|| why.join("; "));
    found.truncate(200);
    a.items = found;
    a
}

/// A name on a removed diff line (R87.2, owner decision 4): looked up in
/// the current tree (its declaration there, then the query from it), else
/// `git grep -w` at `base`, labelled as text matches at the base. `c.path`
/// is the diff's file (it may be gone), `c.query` the question.
pub fn run_removed(pool: &Mutex<Pool<Client>>, c: &Ctx, word: &str, base: &str) -> Answer {
    let decls = nav::text_matches(&c.worktree, word, true);
    if let Some(d) = decls.iter().find(|d| d.path == c.path).or(decls.first()) {
        let mut c2 = c.clone();
        (c2.path, c2.line, c2.col) = (d.path.clone(), d.line, d.col);
        let mut a = run(pool, &c2);
        let note = format!(
            "Removed line: {word} looked up in the current tree ({}:{})",
            nav::rel(&d.path, &c.worktree),
            d.line
        );
        a.hint = Some(match a.hint {
            Some(h) => format!("{note}. {h}"),
            None => note,
        });
        return a;
    }
    let short: String = base.chars().take(7).collect();
    let items = nav::base_matches(&c.worktree, base, word)
        .into_iter()
        .filter(|i| match c.query {
            Query::Def | Query::Impl | Query::Hover => nav::declares(&i.preview, word),
            Query::Callers => !nav::declares(&i.preview, word),
            _ => true,
        })
        .collect::<Vec<_>>();
    Answer {
        doc: text_doc(c.query, &items),
        query: Some(c.query),
        path: c.path.clone(),
        line: c.line,
        col: c.col,
        word: word.to_string(),
        root: c.worktree.clone(),
        source: TEXT.into(),
        state: "base".into(),
        hint: Some(format!(
            "{word} is not in the current tree: text matches at the base ({short})"
        )),
        items,
        ..Default::default()
    }
}

/// The running server for `key`, started if needed.
pub(crate) fn client(
    pool: &Mutex<Pool<Client>>,
    key: servers::Key,
    launch: &Launch,
    name: &str,
) -> Result<Arc<Client>, (&'static str, String)> {
    let check = |p: &mut Pool<Client>| {
        if let Some(cl) = p.get(&key, Instant::now()) {
            return Some(Ok(cl));
        }
        (p.crashes(&key) >= MAX_CRASHES).then(|| {
            Err((
                "crashed",
                format!("{name} crashed {MAX_CRASHES} times: Restart it (rade code restart)"),
            ))
        })
    };
    if let Some(r) = check(&mut pool.lock().unwrap()) {
        return r;
    }
    let hung = |p: &Pool<Client>| {
        p.hung_recently(&key, Instant::now()).then(|| {
            Err((
                "error",
                format!(
                    "{name} did not answer `initialize` within {} s; it is tried again in {} minutes, or Restart it (rade code restart)",
                    START_TIMEOUT.as_secs(),
                    servers::HUNG_PAUSE.as_secs() / 60
                ),
            ))
        })
    };
    if let Some(r) = hung(&pool.lock().unwrap()) {
        return r;
    }
    let crashes = pool.lock().unwrap().crashes(&key);
    // One start per key at a time, without holding the pool: a server that
    // hangs in `initialize` never blocks queries for other roots.
    let gate = STARTS
        .lock()
        .unwrap()
        .entry(key.clone())
        .or_default()
        .clone();
    let _starting = gate.lock().unwrap();
    if let Some(r) = check(&mut pool.lock().unwrap()) {
        return r;
    }
    // A start that failed while this one waited is not tried again at
    // once: queued requests don't each wait out a hanging `initialize`.
    if pool.lock().unwrap().crashes(&key) > crashes {
        return Err((
            "error",
            format!("{name} did not start; it is tried again on the next request"),
        ));
    }
    match Client::start(launch, START_TIMEOUT) {
        Ok(cl) => {
            // Its first report says whether it is loading.
            cl.settle(Duration::from_millis(1500));
            let cl = Arc::new(cl);
            let out = pool.lock().unwrap().insert(key, cl.clone(), Instant::now());
            stop(out, Duration::from_secs(2));
            Ok(cl)
        }
        Err(e) => {
            let mut p = pool.lock().unwrap();
            if e.ends_with("initialize timed out") {
                p.hung(&key, Instant::now());
                return hung(&p).unwrap();
            }
            p.failed(&key);
            Err(("error", format!("{name} did not start: {e}")))
        }
    }
}

/// Per-key start locks (see [`client`]).
static STARTS: std::sync::LazyLock<Mutex<std::collections::HashMap<servers::Key, Arc<Mutex<()>>>>> =
    std::sync::LazyLock::new(Mutex::default);

/// Open the file, ask, close it (open documents ignore disk changes, so
/// none stays open; spike finding 8).
/// What a server answered: items, a hover's text, the call hierarchy, the
/// other references.
type Got = (Vec<Item>, Option<String>, Vec<nav::Call>, Vec<Item>);

fn ask(cl: &Client, c: &Ctx, line_text: &str) -> Result<Got, LspError> {
    let t0 = Instant::now();
    let Some(_g) = cl.lock_doc(c.timeout) else {
        return Err(LspError::Timeout);
    };
    // The wait for the lock counts: a query never waits more than its timeout.
    let t = c.timeout.saturating_sub(t0.elapsed());
    let uri = lsp::path_uri(&c.path);
    let text = String::from_utf8_lossy(&std::fs::read(&c.path).unwrap_or_default()).into_owned();
    cl.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": servers::language_id(&c.path), "version": 1, "text": text}}),
    );
    cl.first_open(
        Duration::from_millis(1500),
        c.wait.max(Duration::from_secs(2)),
    );
    let col = lsp::to_lsp_col(line_text, c.col.saturating_sub(1) as usize, cl.enc);
    let pos = nav::position(&c.path, c.line.saturating_sub(1), col);
    let mut doc = None;
    let (mut calls, mut others, mut items_direct) = (Vec::new(), Vec::new(), Vec::new());
    let mut meta: std::collections::HashMap<(u32, String), (&str, u32)> = Default::default();
    let r = (|| {
        Ok(match c.query {
            Query::Hover => {
                doc = nav::hover_text(&cl.request("textDocument/hover", pos, t)?);
                vec![]
            }
            Query::Workspace => vec![],
            Query::Def => nav::locations(&cl.request("textDocument/definition", pos, t)?),
            Query::Impl => nav::locations(&cl.request("textDocument/implementation", pos, t)?),
            Query::Refs => {
                let mut p = pos;
                p["context"] = json!({"includeDeclaration": false});
                nav::locations(&cl.request("textDocument/references", p, t)?)
            }
            Query::Callers | Query::Callees => {
                let incoming = c.query == Query::Callers;
                let roots = cl.request("textDocument/prepareCallHierarchy", pos.clone(), t)?;
                // Deeper levels share one deadline.
                let until = Instant::now() + DEEPER;
                for item in roots.as_array().into_iter().flatten() {
                    let mut level1 = level(cl, item, incoming, t)?;
                    let mut path = vec![nav::item_key(item)];
                    let mut budget = MAX_CALL_NODES;
                    deepen(
                        cl,
                        &mut level1,
                        incoming,
                        c.depth.saturating_sub(1),
                        &mut path,
                        t,
                        &mut budget,
                        until,
                    );
                    calls.extend(level1);
                }
                // Callers: each call site; callees: each at its definition.
                for k in &calls {
                    if incoming {
                        items_direct.extend(k.from_ranges.iter().map(|s| Item {
                            name: Some(k.name.clone()),
                            ..s.clone()
                        }));
                    } else {
                        items_direct.push(Item {
                            path: k.path.clone(),
                            line: k.line,
                            col: k.col,
                            name: Some(k.name.clone()),
                            preview: k.preview.clone(),
                            ..Default::default()
                        });
                    }
                }
                // References that are not calls (a function passed as a value).
                if incoming && !roots.as_array().is_none_or(|a| a.is_empty()) {
                    let mut p = pos;
                    p["context"] = json!({"includeDeclaration": false});
                    if let Ok(v) = cl.request("textDocument/references", p, t) {
                        let refs = nav::items(nav::locations(&v), cl.enc);
                        others = nav::other_refs(refs, &items_direct);
                    }
                }
                vec![]
            }
            Query::Symbols => {
                let v = cl.request(
                    "textDocument/documentSymbol",
                    json!({"textDocument": {"uri": uri}}),
                    t,
                )?;
                meta = nav::outline_meta(&v);
                nav::symbols(&v, &c.path)
            }
        })
    })();
    cl.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );
    r.map(|raw| {
        let mut items = nav::items(raw, cl.enc);
        if !items_direct.is_empty() {
            items_direct.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
            items_direct.dedup_by(|a, b| a.path == b.path && a.line == b.line && a.col == b.col);
            items = items_direct;
        }
        for i in &mut items {
            let k = i.name.clone().map(|n| (i.line, n));
            if let Some((kind, depth)) = k.and_then(|k| meta.get(&k)) {
                i.kind = Some(kind.to_string());
                i.depth = *depth;
            }
        }
        (items, doc, calls, others)
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
