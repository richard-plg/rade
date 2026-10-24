//! Which language server serves a file and how it starts (R83, ADR-0029):
//! languages by extension, the root folder (one server per root and
//! language, never shared across Worktrees), the built-in commands and
//! settings from the spike (docs/research/code-nav-spike.md) under the
//! layered `[code.server.<lang>]`, command lookup (the repository's
//! `node_modules/.bin`, then PATH), install hints, and the pool: lazy
//! start, LRU cap, idle stop, crash count.

use super::lsp::Launch;
use crate::config::{CodeConfig, ServerConfig};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    TypeScript,
    Python,
    Go,
}

impl Lang {
    pub fn of(path: &Path) -> Option<Lang> {
        match path.extension()?.to_str()? {
            "rs" => Some(Lang::Rust),
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => Some(Lang::TypeScript),
            "py" | "pyi" => Some(Lang::Python),
            "go" => Some(Lang::Go),
            _ => None,
        }
    }

    /// The `[code.server.<id>]` key.
    pub fn id(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::TypeScript => "typescript",
            Lang::Python => "python",
            Lang::Go => "go",
        }
    }

    pub const ALL: [Lang; 4] = [Lang::Rust, Lang::TypeScript, Lang::Python, Lang::Go];

    /// The built-in commands, in order of preference (the first one
    /// installed runs; M30: basedpyright when pyright is missing).
    fn default_commands(self) -> Vec<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        match self {
            Lang::Rust => vec![v(&["rust-analyzer"])],
            Lang::TypeScript => vec![v(&["typescript-language-server", "--stdio"])],
            Lang::Python => vec![
                v(&["pyright-langserver", "--stdio"]),
                v(&["basedpyright-langserver", "--stdio"]),
            ],
            Lang::Go => vec![v(&["gopls"])],
        }
    }

    /// Files at a folder's top that say it holds code of this language.
    fn markers(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &["Cargo.toml"],
            Lang::TypeScript => &["tsconfig.json", "jsconfig.json", "package.json"],
            Lang::Python => &[
                "pyproject.toml",
                "setup.py",
                "setup.cfg",
                "pyrightconfig.json",
                "requirements.txt",
            ],
            Lang::Go => &["go.work", "go.mod"],
        }
    }

    /// The one line shown when the server is missing (owner decision 1:
    /// RADE never installs).
    pub fn install_hint(self) -> &'static str {
        match self {
            Lang::Rust => "rustup component add rust-analyzer",
            Lang::TypeScript => "npm i -g typescript-language-server typescript@5",
            Lang::Python => "npm i -g pyright (or: pipx install basedpyright)",
            Lang::Go => "go install golang.org/x/tools/gopls@latest",
        }
    }

    /// Built-in `settings` (spike recommendations: no `cargo check` on
    /// save, the server watches files, its own target dir; M30: workspace
    /// symbols include functions, gopls's stay in the workspace).
    fn settings(self) -> Value {
        match self {
            Lang::Rust => json!({"rust-analyzer": {
                "checkOnSave": false,
                "files": {"watcher": "server"},
                "cargo": {"targetDir": true},
                "workspace": {"symbol": {"search": {"kind": "all_symbols"}}},
            }}),
            Lang::TypeScript => json!({}),
            Lang::Python => json!({"python": {"analysis": {"diagnosticMode": "openFilesOnly"}}}),
            Lang::Go => json!({"gopls": {"symbolScope": "workspace"}}),
        }
    }
}

/// The LSP `languageId` of a file.
pub fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "tsx" => "typescriptreact",
        "jsx" => "javascriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "py" | "pyi" => "python",
        "go" => "go",
        _ => "typescript",
    }
}

/// Folders of installed dependencies.
const DEPENDENCY_DIRS: &[&str] = &["node_modules", "site-packages", ".venv", "venv"];

/// The folder a server for `file` serves, inside `worktree`: Rust → the
/// outermost folder with a `Cargo.toml` (the cargo workspace); TS/JS → the
/// nearest with a `tsconfig.json` / `jsconfig.json` / `package.json`;
/// Python → the nearest with a `pyproject.toml` / `setup.py` / … ; Go → the
/// outermost `go.work`, else the nearest `go.mod`; the Worktree when there
/// is none. A file inside a dependency folder (`node_modules`, a
/// virtualenv's `site-packages`, …) belongs to the project that holds the
/// folder, never a server of its own (0.7.1).
pub fn root_for(file: &Path, worktree: &Path, lang: Lang) -> PathBuf {
    let mut found = None;
    let mut work = None;
    let dep = file.strip_prefix(worktree).ok().and_then(|r| {
        let n = r
            .components()
            .position(|c| DEPENDENCY_DIRS.contains(&c.as_os_str().to_str().unwrap_or("")))?;
        Some(worktree.join(r.components().take(n).collect::<PathBuf>()))
    });
    let mut dir = match &dep {
        Some(d) => Some(d.as_path()),
        None => file.parent(),
    };
    while let Some(d) = dir {
        if !d.starts_with(worktree) {
            break;
        }
        if lang == Lang::Go && d.join("go.work").is_file() {
            work = Some(d.to_path_buf());
        }
        let hit = match lang {
            Lang::Go => d.join("go.mod").is_file(),
            _ => lang.markers().iter().any(|m| d.join(m).is_file()),
        };
        if hit && (found.is_none() || lang == Lang::Rust) {
            found = Some(d.to_path_buf());
        }
        if d == worktree {
            break;
        }
        dir = d.parent();
    }
    work.or(found).unwrap_or_else(|| worktree.to_path_buf())
}

/// A file of `lang` under `root` (`.gitignore` applied, the first few
/// thousand entries): opened around a workspace-symbol request, since
/// tsserver has no project without an open file.
pub fn sample_file(root: &Path, lang: Lang) -> Option<PathBuf> {
    ignore::WalkBuilder::new(root)
        .build()
        .take(5000)
        .filter_map(Result::ok)
        .map(|e| e.into_path())
        .find(|p| p.is_file() && Lang::of(p) == Some(lang))
}

/// `major.minor` of the repository's TypeScript (`node_modules/typescript`
/// from `root` up to the Worktree).
pub fn ts_version(root: &Path, worktree: &Path) -> Option<(u32, u32)> {
    let mut d = Some(root);
    while let Some(dir) = d {
        let f = dir.join("node_modules/typescript/package.json");
        if let Ok(t) = std::fs::read_to_string(&f) {
            let v: Value = serde_json::from_str(&t).ok()?;
            let mut it = v["version"].as_str()?.split('.');
            return Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?));
        }
        if dir == worktree {
            break;
        }
        d = dir.parent();
    }
    None
}

/// `cmd` as a file: a path as is, else `node_modules/.bin/<cmd>` from
/// `root` up to the Worktree (when given), else on `path` (`$PATH`, the
/// Workspace's).
pub fn find_program(
    cmd: &str,
    root: Option<&Path>,
    worktree: &Path,
    path: &str,
) -> Option<PathBuf> {
    let exec = |p: &Path| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        return exec(&p).then_some(p);
    }
    let mut d = root;
    while let Some(dir) = d {
        let p = dir.join("node_modules/.bin").join(cmd);
        if exec(&p) {
            return Some(p);
        }
        if dir == worktree || !dir.starts_with(worktree) {
            break;
        }
        d = dir.parent();
    }
    std::env::split_paths(path)
        .map(|d| d.join(cmd))
        .find(|p| exec(p))
}

/// Deep merge of JSON objects (`over` wins).
fn merge(into: &mut Value, over: &Value) {
    match (into, over) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b.clone(),
    }
}

/// `~/x` → `$HOME/x`.
fn expand_home(p: &str) -> String {
    match p.strip_prefix("~/") {
        Some(rest) => crate::paths::home_dir(&crate::paths::process_env)
            .join(rest)
            .display()
            .to_string(),
        None => p.to_string(),
    }
}

/// What starting a server for a file comes to.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// `[code] enabled = false` or `[sources.<id>] code = false`.
    Off,
    /// The command is not installed: the install hint.
    Missing { command: String, hint: String },
    Start {
        launch: Box<Launch>,
        /// Its display name (`rust-analyzer`).
        name: String,
        /// A note shown with results (an old TypeScript).
        note: Option<String>,
    },
}

/// The plan for `lang` at `root` from the layered config.
pub fn plan(
    lang: Lang,
    root: &Path,
    worktree: &Path,
    cfg: &CodeConfig,
    enabled: bool,
    env: &[(String, String)],
) -> Plan {
    if !enabled {
        return Plan::Off;
    }
    let own = cfg.server.get(lang.id()).cloned().unwrap_or_default();
    let ServerConfig {
        command,
        settings,
        init,
        tsserver_path,
    } = own;
    let own = !command.is_empty();
    let path = env
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    // A repository's own `node_modules/.bin` only for TS/JS servers (an
    // untrusted repository must not supply `rust-analyzer`).
    let nm = (lang == Lang::TypeScript).then_some(root);
    let choices = if own {
        vec![command]
    } else {
        lang.default_commands()
    };
    let Some((argv, prog)) = choices
        .iter()
        .find_map(|a| find_program(&a[0], nm, worktree, &path).map(|p| (a.clone(), p)))
    else {
        let argv = &choices[0];
        return Plan::Missing {
            command: argv[0].clone(),
            hint: if !own {
                lang.install_hint().into()
            } else {
                format!(
                    "`{}` is not on PATH ([code.server.{}] command)",
                    argv[0],
                    lang.id()
                )
            },
        };
    };
    let name = Path::new(&argv[0])
        .file_name()
        .map_or(argv[0].clone(), |n| n.to_string_lossy().into_owned());
    let mut s = lang.settings();
    if let Some(t) = settings {
        merge(&mut s, &serde_json::to_value(t).unwrap_or_default());
    }
    // A virtual environment in the root: pyright's interpreter (imports).
    let venv = root.join(".venv/bin/python");
    if lang == Lang::Python && venv.is_file() {
        s["python"]["pythonPath"] = json!(venv.display().to_string());
    }
    let mut i = match lang {
        // rust-analyzer and gopls also read their settings as initializationOptions.
        Lang::Rust => s["rust-analyzer"].clone(),
        Lang::Go => s["gopls"].clone(),
        Lang::TypeScript => json!({"disableAutomaticTypingAcquisition": true}),
        Lang::Python => json!({}),
    };
    let mut note = None;
    if lang == Lang::TypeScript {
        let old = ts_version(root, worktree).is_none_or(|v| v.0 < 5);
        match (old, tsserver_path) {
            (true, Some(p)) => merge(&mut i, &json!({"tsserver": {"path": expand_home(&p)}})),
            (true, None) => {
                if let Some((a, b)) = ts_version(root, worktree) {
                    note = Some(format!(
                        "This repository's TypeScript {a}.{b} is too old for code navigation: \
                         `npm i -g typescript@5` and set code.server.typescript.tsserver_path"
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some(t) = init {
        merge(&mut i, &serde_json::to_value(t).unwrap_or_default());
    }
    let mut argv = argv;
    argv[0] = prog.to_string_lossy().into_owned();
    Plan::Start {
        launch: Box::new(Launch {
            argv,
            root: root.to_path_buf(),
            env: env.to_vec(),
            init: i,
            settings: s,
        }),
        name,
        note,
    }
}

/// What the pool needs of a server.
pub trait Server {
    fn alive(&self) -> bool;
    /// A dead one leaving the pool: make sure its processes are gone.
    fn kill(&self) {}
}

impl Server for super::lsp::Client {
    fn alive(&self) -> bool {
        super::lsp::Client::alive(self)
    }
    fn kill(&self) {
        super::lsp::Client::kill(self)
    }
}

pub type Key = (PathBuf, Lang);

struct Entry<C> {
    key: Key,
    server: Arc<C>,
    last: Instant,
}

/// Running servers: at most `max`, stopped after `idle` without a
/// request; crashes counted per key until a manual restart.
pub struct Pool<C> {
    entries: Vec<Entry<C>>,
    crashes: HashMap<Key, u32>,
    /// When a server last gave no answer to `initialize` in time.
    hung: HashMap<Key, Instant>,
    pub max: usize,
    pub idle: Duration,
}

/// How long a server that hung in `initialize` is not started again.
pub const HUNG_PAUSE: Duration = Duration::from_secs(5 * 60);

/// Crashes after which a key is not restarted until `code.restart`.
pub const MAX_CRASHES: u32 = 3;

impl<C: Server> Default for Pool<C> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            crashes: HashMap::new(),
            hung: HashMap::new(),
            max: 4,
            idle: Duration::from_secs(15 * 60),
        }
    }
}

impl<C: Server> Pool<C> {
    pub fn configure(&mut self, c: &CodeConfig) {
        self.max = c.max_servers.max(1);
        self.idle = Duration::from_secs_f64((c.idle_minutes * 60.0).max(1.0));
    }

    /// The live server for `key` (touched). A dead one is removed and
    /// counted as a crash.
    pub fn get(&mut self, key: &Key, now: Instant) -> Option<Arc<C>> {
        let i = self.entries.iter().position(|e| &e.key == key)?;
        if !self.entries[i].server.alive() {
            self.entries.remove(i).server.kill();
            *self.crashes.entry(key.clone()).or_default() += 1;
            return None;
        }
        self.entries[i].last = now;
        Some(self.entries[i].server.clone())
    }

    pub fn crashes(&self, key: &Key) -> u32 {
        self.crashes.get(key).copied().unwrap_or(0)
    }

    /// A server that failed to start counts as a crash.
    pub fn failed(&mut self, key: &Key) {
        *self.crashes.entry(key.clone()).or_default() += 1;
    }

    /// Add a started server; returns the least recently used ones over
    /// `max` (to stop).
    pub fn insert(&mut self, key: Key, server: Arc<C>, now: Instant) -> Vec<Arc<C>> {
        self.entries.retain(|e| e.key != key);
        self.entries.push(Entry {
            key,
            server,
            last: now,
        });
        let mut out = Vec::new();
        while self.entries.len() > self.max {
            let i = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last)
                .map(|(i, _)| i)
                .unwrap();
            out.push(self.entries.remove(i).server);
        }
        out
    }

    /// Idle ones (to stop) and dead ones (counted as crashes) leave.
    pub fn reap(&mut self, now: Instant) -> Vec<Arc<C>> {
        let mut out = Vec::new();
        let idle = self.idle;
        let mut crashed = Vec::new();
        self.entries.retain(|e| {
            if !e.server.alive() {
                crashed.push(e.key.clone());
                e.server.kill();
                false
            } else if now.duration_since(e.last) >= idle {
                out.push(e.server.clone());
                false
            } else {
                true
            }
        });
        for k in crashed {
            *self.crashes.entry(k).or_default() += 1;
        }
        out
    }

    /// Servers whose root is under `dir` (a removed Worktree), or all.
    pub fn take_under(&mut self, dir: Option<&Path>) -> Vec<Arc<C>> {
        let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|e| dir.is_none_or(|d| e.key.0.starts_with(d)));
        self.entries = keep;
        gone.into_iter().map(|e| e.server).collect()
    }

    /// A start that got no answer to `initialize` in time: a crash, and
    /// no new start for [`HUNG_PAUSE`] (M29 open item: each request would
    /// otherwise wait out another start).
    pub fn hung(&mut self, key: &Key, now: Instant) {
        self.failed(key);
        self.hung.insert(key.clone(), now);
    }

    pub fn hung_recently(&self, key: &Key, now: Instant) -> bool {
        self.hung
            .get(key)
            .is_some_and(|t| now.duration_since(*t) < HUNG_PAUSE)
    }

    /// `code.restart`: forget the crashes, stop the running one.
    pub fn restart(&mut self, key: &Key) -> Option<Arc<C>> {
        self.crashes.remove(key);
        self.hung.remove(key);
        let i = self.entries.iter().position(|e| &e.key == key)?;
        Some(self.entries.remove(i).server)
    }

    /// (key, server, idle for) per running server, most recent first.
    pub fn list(&self, now: Instant) -> Vec<(Key, Arc<C>, Duration)> {
        let mut v: Vec<_> = self
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.server.clone(), now.duration_since(e.last)))
            .collect();
        v.sort_by_key(|x| x.2);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fake(Cell<bool>, Cell<u32>);
    // Tests are single-threaded.
    unsafe impl Sync for Fake {}
    impl Server for Fake {
        fn alive(&self) -> bool {
            self.0.get()
        }
        fn kill(&self) {
            self.1.set(self.1.get() + 1);
        }
    }

    fn key(n: &str) -> Key {
        (PathBuf::from(format!("/w/{n}")), Lang::Rust)
    }

    #[test]
    fn r83_languages_and_ids() {
        assert_eq!(Lang::of(Path::new("a/b.rs")), Some(Lang::Rust));
        for e in ["ts", "tsx", "js", "jsx", "mjs", "cts"] {
            assert_eq!(
                Lang::of(Path::new(&format!("x.{e}"))),
                Some(Lang::TypeScript),
                "{e}"
            );
        }
        assert_eq!(Lang::of(Path::new("x.py")), Some(Lang::Python));
        assert_eq!(Lang::of(Path::new("x.pyi")), Some(Lang::Python));
        assert_eq!(Lang::of(Path::new("x.go")), Some(Lang::Go));
        assert_eq!(Lang::of(Path::new("x.c")), None);
        assert_eq!(language_id(Path::new("a.py")), "python");
        assert_eq!(language_id(Path::new("a.go")), "go");
        assert_eq!((Lang::Python.id(), Lang::Go.id()), ("python", "go"));
        assert_eq!(language_id(Path::new("a.tsx")), "typescriptreact");
        assert_eq!(language_id(Path::new("a.js")), "javascript");
    }

    #[test]
    fn r83_roots_cargo_workspace_and_nearest_package() {
        let t = tempdir();
        let w = t.join("wt");
        for d in ["crates/a/src", "web/app/src", "web/lib"] {
            std::fs::create_dir_all(w.join(d)).unwrap();
        }
        std::fs::write(w.join("Cargo.toml"), "").unwrap();
        std::fs::write(w.join("crates/a/Cargo.toml"), "").unwrap();
        std::fs::write(w.join("web/app/package.json"), "{}").unwrap();
        std::fs::write(w.join("package.json"), "{}").unwrap();
        assert_eq!(root_for(&w.join("crates/a/src/lib.rs"), &w, Lang::Rust), w);
        assert_eq!(
            root_for(&w.join("web/app/src/x.ts"), &w, Lang::TypeScript),
            w.join("web/app")
        );
        assert_eq!(root_for(&w.join("web/lib/y.ts"), &w, Lang::TypeScript), w);
        // Never above the Worktree.
        std::fs::write(t.join("Cargo.toml"), "").unwrap();
        let w2 = t.join("other");
        std::fs::create_dir_all(&w2).unwrap();
        assert_eq!(root_for(&w2.join("m.rs"), &w2, Lang::Rust), w2);
    }

    #[test]
    fn r83_a_dependency_file_belongs_to_the_project_holding_it() {
        let t = tempdir();
        let w = t.join("wt");
        let dep = w.join("web/app/node_modules/moment/src");
        std::fs::create_dir_all(&dep).unwrap();
        std::fs::create_dir_all(w.join(".venv/lib/python3.12/site-packages/req")).unwrap();
        std::fs::write(w.join("web/app/package.json"), "{}").unwrap();
        std::fs::write(w.join("web/app/node_modules/moment/package.json"), "{}").unwrap();
        std::fs::write(w.join("pyproject.toml"), "").unwrap();
        std::fs::write(
            w.join(".venv/lib/python3.12/site-packages/req/setup.py"),
            "",
        )
        .unwrap();
        assert_eq!(
            root_for(&dep.join("moment.d.ts"), &w, Lang::TypeScript),
            w.join("web/app"),
            "not a server of its own at node_modules/moment"
        );
        assert_eq!(
            root_for(
                &w.join(".venv/lib/python3.12/site-packages/req/api.py"),
                &w,
                Lang::Python
            ),
            w
        );
    }

    fn tempdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rade-code-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn exe(p: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn r83_plan_missing_off_node_modules_first_and_settings() {
        let t = tempdir();
        let w = t.join("wt");
        let bin = t.join("bin");
        std::fs::create_dir_all(&w).unwrap();
        let env = vec![("PATH".to_string(), bin.display().to_string())];
        let c = CodeConfig::default();
        assert_eq!(plan(Lang::Rust, &w, &w, &c, false, &env), Plan::Off);
        match plan(Lang::Rust, &w, &w, &c, true, &env) {
            Plan::Missing { hint, .. } => assert_eq!(hint, "rustup component add rust-analyzer"),
            p => panic!("{p:?}"),
        }
        match plan(Lang::TypeScript, &w, &w, &c, true, &env) {
            Plan::Missing { hint, .. } => assert!(hint.contains("typescript@5"), "{hint}"),
            p => panic!("{p:?}"),
        }
        // A repository can't supply its own rust-analyzer.
        exe(&w.join("node_modules/.bin/rust-analyzer"));
        assert!(matches!(
            plan(Lang::Rust, &w, &w, &c, true, &env),
            Plan::Missing { .. }
        ));
        exe(&bin.join("rust-analyzer"));
        let Plan::Start { launch, name, .. } = plan(Lang::Rust, &w, &w, &c, true, &env) else {
            panic!()
        };
        assert_eq!(name, "rust-analyzer");
        assert_eq!(
            launch.argv,
            [bin.join("rust-analyzer").display().to_string()]
        );
        assert_eq!(
            launch.settings["rust-analyzer"]["checkOnSave"],
            json!(false)
        );
        assert_eq!(
            launch.settings["rust-analyzer"]["files"]["watcher"],
            json!("server")
        );
        assert_eq!(
            launch.settings["rust-analyzer"]["cargo"]["targetDir"],
            json!(true)
        );
        assert_eq!(launch.init["checkOnSave"], json!(false));
        // The repository's node_modules/.bin wins over PATH.
        exe(&bin.join("typescript-language-server"));
        exe(&w.join("node_modules/.bin/typescript-language-server"));
        let Plan::Start { launch, .. } = plan(Lang::TypeScript, &w, &w, &c, true, &env) else {
            panic!()
        };
        assert_eq!(
            launch.argv[0],
            w.join("node_modules/.bin/typescript-language-server")
                .display()
                .to_string()
        );
        assert_eq!(launch.argv[1], "--stdio");
        assert_eq!(
            launch.init["disableAutomaticTypingAcquisition"],
            json!(true)
        );
        // An owner's command and settings.
        let c: CodeConfig = toml::from_str(
            "[server.rust]\ncommand = [\"rust-analyzer\", \"-v\"]\nsettings = { rust-analyzer = { cargo = { features = \"all\" } } }\n",
        )
        .unwrap();
        let Plan::Start { launch, .. } = plan(Lang::Rust, &w, &w, &c, true, &env) else {
            panic!()
        };
        assert_eq!(launch.argv[1], "-v");
        assert_eq!(
            launch.settings["rust-analyzer"]["cargo"],
            json!({"features": "all", "targetDir": true})
        );
        let c: CodeConfig = toml::from_str("[server.rust]\ncommand = [\"nope-ls\"]\n").unwrap();
        match plan(Lang::Rust, &w, &w, &c, true, &env) {
            Plan::Missing { hint, .. } => assert!(hint.contains("nope-ls"), "{hint}"),
            p => panic!("{p:?}"),
        }
    }

    #[test]
    fn r83_old_typescript_uses_the_owners_tsserver_or_says_so() {
        let t = tempdir();
        let w = t.join("wt");
        exe(&w.join("node_modules/.bin/typescript-language-server"));
        std::fs::create_dir_all(w.join("node_modules/typescript")).unwrap();
        std::fs::write(
            w.join("node_modules/typescript/package.json"),
            r#"{"version": "3.7.2"}"#,
        )
        .unwrap();
        assert_eq!(ts_version(&w, &w), Some((3, 7)));
        let c = CodeConfig::default();
        let Plan::Start { launch, note, .. } = plan(Lang::TypeScript, &w, &w, &c, true, &[]) else {
            panic!()
        };
        assert!(note.unwrap().contains("TypeScript 3.7 is too old"));
        assert!(launch.init.get("tsserver").is_none());
        let c: CodeConfig =
            toml::from_str("[server.typescript]\ntsserver_path = \"/opt/ts5/lib\"\n").unwrap();
        let Plan::Start { launch, note, .. } = plan(Lang::TypeScript, &w, &w, &c, true, &[]) else {
            panic!()
        };
        assert_eq!(note, None);
        assert_eq!(launch.init["tsserver"]["path"], json!("/opt/ts5/lib"));
        std::fs::write(
            w.join("node_modules/typescript/package.json"),
            r#"{"version": "5.9.3"}"#,
        )
        .unwrap();
        let Plan::Start { launch, .. } = plan(Lang::TypeScript, &w, &w, &c, true, &[]) else {
            panic!()
        };
        assert!(
            launch.init.get("tsserver").is_none(),
            "TS ≥ 5: the repository's own"
        );
    }

    #[test]
    fn r93_python_and_go_roots() {
        let t = tempdir();
        let w = t.join("wt");
        for d in ["svc/app", "tools/cli/cmd", "lib/x"] {
            std::fs::create_dir_all(w.join(d)).unwrap();
        }
        std::fs::write(w.join("svc/pyproject.toml"), "").unwrap();
        assert_eq!(
            root_for(&w.join("svc/app/m.py"), &w, Lang::Python),
            w.join("svc")
        );
        assert_eq!(root_for(&w.join("lib/x/m.py"), &w, Lang::Python), w);
        // Go: the nearest go.mod, unless a go.work above it.
        std::fs::write(w.join("tools/cli/go.mod"), "module cli\n").unwrap();
        assert_eq!(
            root_for(&w.join("tools/cli/cmd/main.go"), &w, Lang::Go),
            w.join("tools/cli")
        );
        std::fs::write(w.join("go.work"), "go 1.23\n").unwrap();
        assert_eq!(root_for(&w.join("tools/cli/cmd/main.go"), &w, Lang::Go), w);
        // A sample file per language (workspace symbols open one).
        std::fs::write(w.join("tools/cli/cmd/main.go"), "package main\n").unwrap();
        assert_eq!(
            sample_file(&w, Lang::Go),
            Some(w.join("tools/cli/cmd/main.go"))
        );
        assert_eq!(sample_file(&w, Lang::Rust), None);
    }

    #[test]
    fn r93_python_go_plans_alternatives_hints_settings() {
        let t = tempdir();
        let w = t.join("wt");
        let bin = t.join("bin");
        std::fs::create_dir_all(&w).unwrap();
        let env = vec![("PATH".to_string(), bin.display().to_string())];
        let c = CodeConfig::default();
        match plan(Lang::Python, &w, &w, &c, true, &env) {
            Plan::Missing { command, hint } => {
                assert_eq!(command, "pyright-langserver");
                assert_eq!(hint, "npm i -g pyright (or: pipx install basedpyright)");
            }
            p => panic!("{p:?}"),
        }
        match plan(Lang::Go, &w, &w, &c, true, &env) {
            Plan::Missing { hint, .. } => {
                assert_eq!(hint, "go install golang.org/x/tools/gopls@latest")
            }
            p => panic!("{p:?}"),
        }
        // A repository can't supply its own Python server.
        exe(&w.join("node_modules/.bin/pyright-langserver"));
        assert!(matches!(
            plan(Lang::Python, &w, &w, &c, true, &env),
            Plan::Missing { .. }
        ));
        // basedpyright when pyright is not installed.
        exe(&bin.join("basedpyright-langserver"));
        let Plan::Start { launch, name, .. } = plan(Lang::Python, &w, &w, &c, true, &env) else {
            panic!()
        };
        assert_eq!(name, "basedpyright-langserver");
        assert_eq!(launch.argv[1], "--stdio");
        assert!(launch.settings["python"].get("pythonPath").is_none());
        exe(&bin.join("pyright-langserver"));
        exe(&w.join(".venv/bin/python"));
        let Plan::Start { launch, name, .. } = plan(Lang::Python, &w, &w, &c, true, &env) else {
            panic!()
        };
        assert_eq!(name, "pyright-langserver");
        assert_eq!(
            launch.settings["python"]["pythonPath"],
            json!(w.join(".venv/bin/python").display().to_string())
        );
        // An owner's command is not swapped for an alternative.
        let own: CodeConfig =
            toml::from_str("[server.python]\ncommand = [\"ty\", \"server\"]\n").unwrap();
        assert!(matches!(
            plan(Lang::Python, &w, &w, &own, true, &env),
            Plan::Missing { .. }
        ));
        exe(&bin.join("gopls"));
        let Plan::Start { launch, name, .. } = plan(Lang::Go, &w, &w, &c, true, &env) else {
            panic!()
        };
        assert_eq!(name, "gopls");
        assert_eq!(launch.argv.len(), 1);
        assert_eq!(launch.settings["gopls"]["symbolScope"], json!("workspace"));
        assert_eq!(launch.init["symbolScope"], json!("workspace"));
        // rust-analyzer: workspace symbols include functions.
        exe(&bin.join("rust-analyzer"));
        let Plan::Start { launch, .. } = plan(Lang::Rust, &w, &w, &c, true, &env) else {
            panic!()
        };
        assert_eq!(
            launch.settings["rust-analyzer"]["workspace"]["symbol"]["search"]["kind"],
            json!("all_symbols")
        );
    }

    #[test]
    fn r83_a_server_that_hung_in_initialize_is_not_started_again_at_once() {
        let t0 = Instant::now();
        let mut p: Pool<Fake> = Pool::default();
        assert!(!p.hung_recently(&key("h"), t0));
        p.hung(&key("h"), t0);
        assert_eq!(p.crashes(&key("h")), 1);
        assert!(p.hung_recently(&key("h"), t0 + Duration::from_secs(60)));
        assert!(!p.hung_recently(&key("h"), t0 + HUNG_PAUSE));
        p.hung(&key("h"), t0);
        p.restart(&key("h"));
        assert!(!p.hung_recently(&key("h"), t0), "Restart tries at once");
    }

    #[test]
    fn r83_pool_lru_idle_crashes_and_removal() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut p: Pool<Fake> = Pool::default();
        p.configure(&CodeConfig {
            max_servers: 2,
            idle_minutes: 1.0,
            ..Default::default()
        });
        let a = Arc::new(Fake(Cell::new(true), Cell::new(0)));
        assert!(p.insert(key("a"), a.clone(), s(0)).is_empty());
        assert!(p
            .insert(
                key("b"),
                Arc::new(Fake(Cell::new(true), Cell::new(0))),
                s(1)
            )
            .is_empty());
        // `a` used last: `b` is the least recently used when `c` starts.
        assert!(p.get(&key("a"), s(2)).is_some());
        let out = p.insert(
            key("c"),
            Arc::new(Fake(Cell::new(true), Cell::new(0))),
            s(3),
        );
        assert_eq!(out.len(), 1);
        assert!(p.get(&key("b"), s(3)).is_none());
        assert_eq!(p.list(s(3)).len(), 2);
        // Idle: `a` last used at 2 s, `c` at 3 s; at 62.5 s only `a` goes.
        assert_eq!(p.reap(s(62) + Duration::from_millis(500)).len(), 1);
        assert!(p.get(&key("c"), s(63)).is_some());
        // A dead server is a crash, found by get or reap.
        let d = Arc::new(Fake(Cell::new(true), Cell::new(0)));
        p.insert(key("d"), d.clone(), s(63));
        d.0.set(false);
        assert!(p.get(&key("d"), s(64)).is_none());
        assert_eq!(p.crashes(&key("d")), 1);
        assert_eq!(
            d.1.get(),
            1,
            "a dead server's processes are killed and reaped"
        );
        p.failed(&key("d"));
        p.failed(&key("d"));
        assert_eq!(p.crashes(&key("d")), MAX_CRASHES);
        assert!(p.restart(&key("d")).is_none());
        assert_eq!(p.crashes(&key("d")), 0);
        // A removed Worktree stops its servers only.
        p.insert(
            key("x/sub"),
            Arc::new(Fake(Cell::new(true), Cell::new(0))),
            s(65),
        );
        assert_eq!(p.take_under(Some(Path::new("/w/x"))).len(), 1);
        assert_eq!(p.take_under(None).len(), 1, "quit: the rest");
    }
}
