//! `run` against tests/e2e/fake-lsp.py (python3): the whole client.

use super::*;
use std::sync::Mutex;

fn fake() -> String {
    format!("{}/../../tests/e2e/fake-lsp.py", env!("CARGO_MANIFEST_DIR"))
}

fn repo(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rade-code-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_file(d.with_extension("log"));
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    std::fs::write(
        d.join("src/lib.rs"),
        "pub fn greet() {}\n// é😀 greet\nfn helper() { greet(); }\nfn crash_now() {}\n",
    )
    .unwrap();
    std::fs::write(
        d.join("src/main.rs"),
        "fn main() {\n    let é = 1; greet();\n}\n",
    )
    .unwrap();
    std::fs::write(
        d.join(".fake-lsp.json"),
        r#"{"greet": {"def": ["src/lib.rs:1:8"], "refs": ["src/main.rs:2:16", "src/lib.rs:3:15"],
            "callers": [{"name": "main", "at": "src/main.rs:2:16"}, {"name": "helper", "at": "src/lib.rs:3:15"}]},
            "helper": {"hover": "```rust\nfn helper()\n```\n---\nCalls greet.", "def": ["src/lib.rs:3:4"], "callees": [{"name": "greet", "at": "src/lib.rs:1:8"}]},
            "@symbols": {"src/lib.rs": [{"name": "greet", "at": "src/lib.rs:1:8"}]},
            "@workspace": [{"name": "greet", "at": "src/lib.rs:1:8"}, {"name": "helper", "at": "src/lib.rs:3:4"},
                           {"name": "Greeting", "at": "src/main.rs:1:4", "kind": 23}]}"#,
    )
    .unwrap();
    d
}

fn ctx(d: &Path, q: Query, file: &str, line: u32, col: u32, extra: &[(&str, &str)]) -> Ctx {
    let cfg: CodeConfig = toml::from_str(&format!(
        "[server.rust]\ncommand = [\"python3\", \"{}\"]\n",
        fake()
    ))
    .unwrap();
    let log = d.with_extension("log").display().to_string();
    let mut env = vec![("FAKE_LSP_LOG".to_string(), log)];
    env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    Ctx {
        query: q,
        path: d.join(file),
        line,
        col,
        worktree: d.to_path_buf(),
        cfg,
        enabled: true,
        env,
        wait: Duration::ZERO,
        timeout: Duration::from_secs(5),
        root: None,
        depth: 1,
    }
}

fn at(a: &Answer) -> Vec<String> {
    a.items
        .iter()
        .map(|i| {
            format!(
                "{}:{}:{}{}",
                nav::rel(&i.path, &a.root),
                i.line,
                i.col,
                i.name.as_ref().map(|n| format!(" {n}")).unwrap_or_default()
            )
        })
        .collect()
}

#[test]
fn r84_r85_queries_through_a_server() {
    let d = repo("q");
    let pool = Mutex::new(Pool::default());
    // Past a multi-byte character: utf-8 columns both ways.
    let a = run(&pool, &ctx(&d, Query::Def, "src/main.rs", 2, 18, &[]));
    assert_eq!(
        (a.state.as_str(), a.source.as_str(), a.word.as_str()),
        ("ready", "fake-lsp", "greet")
    );
    assert_eq!(at(&a), ["src/lib.rs:1:8"]);
    let a = run(&pool, &ctx(&d, Query::Refs, "src/lib.rs", 1, 8, &[]));
    assert_eq!(at(&a), ["src/lib.rs:3:15", "src/main.rs:2:16"]);
    let a = run(&pool, &ctx(&d, Query::Callers, "src/lib.rs", 1, 9, &[]));
    assert_eq!(at(&a), ["src/lib.rs:3:15 helper", "src/main.rs:2:16 main"]);
    let a = run(&pool, &ctx(&d, Query::Callees, "src/lib.rs", 3, 5, &[]));
    assert_eq!(at(&a), ["src/lib.rs:1:8 greet"]);
    let a = run(&pool, &ctx(&d, Query::Symbols, "src/lib.rs", 1, 1, &[]));
    assert_eq!(at(&a), ["src/lib.rs:1:8 greet"]);
    let a = run(&pool, &ctx(&d, Query::Def, "src/lib.rs", 2, 1, &[]));
    assert_eq!(a.state, "no-word");
    // One server for all of it; the built-in rust settings reached it.
    let log = std::fs::read_to_string(d.with_extension("log")).unwrap();
    assert_eq!(
        log.lines().filter(|l| l.starts_with("start ")).count(),
        1,
        "{log}"
    );
    assert!(log.contains("utf-8"), "{log}");
    assert!(log.contains(r#"config [{"checkOnSave": false, "files": {"watcher": "server"}, "cargo": {"targetDir": true}, "workspace": {"symbol": {"search": {"kind": "all_symbols"}}}}]"#), "{log}");
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
}

#[test]
fn r84_indexing_gives_text_matches_then_the_server() {
    let d = repo("idx");
    let pool = Mutex::new(Pool::default());
    let c = ctx(
        &d,
        Query::Refs,
        "src/lib.rs",
        1,
        8,
        &[("FAKE_LSP_INDEX_MS", "4000")],
    );
    let a = run(&pool, &c);
    assert_eq!((a.state.as_str(), a.source.as_str()), ("indexing", "text"));
    assert!(a.hint.as_deref().unwrap().contains("is indexing"));
    assert!(
        at(&a).contains(&"src/main.rs:2:16".to_string()),
        "{:?}",
        at(&a)
    );
    let mut c = c;
    c.wait = Duration::from_secs(10);
    let a = run(&pool, &c);
    assert_eq!((a.state.as_str(), a.source.as_str()), ("ready", "fake-lsp"));
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
}

#[test]
fn r83_missing_off_crash_restart_and_shutdown() {
    let d = repo("crash");
    let pool = Mutex::new(Pool::default());
    let mut c = ctx(&d, Query::Refs, "src/lib.rs", 1, 8, &[]);
    c.cfg.server.clear();
    c.env
        .push(("PATH".into(), d.join("nobin").display().to_string()));
    let a = run(&pool, &c);
    assert_eq!(a.state, "no-server");
    assert_eq!(
        a.hint.as_deref(),
        Some("rust-analyzer is not installed: rustup component add rust-analyzer")
    );
    assert!(
        at(&a).contains(&"src/main.rs:2:16".to_string()),
        "text matches still: {:?}",
        at(&a)
    );
    let mut c = ctx(&d, Query::Refs, "src/lib.rs", 1, 8, &[]);
    c.enabled = false;
    assert_eq!(run(&pool, &c).state, "off");
    // A crash: text matches now, a new server on the next request.
    let a = run(&pool, &ctx(&d, Query::Def, "src/lib.rs", 4, 5, &[]));
    assert_eq!(a.state, "error");
    assert!(
        a.hint.as_deref().unwrap().contains("crashed on purpose"),
        "{:?}",
        a.hint
    );
    let a = run(&pool, &ctx(&d, Query::Def, "src/main.rs", 2, 16, &[]));
    assert_eq!(a.state, "ready");
    let log = std::fs::read_to_string(d.with_extension("log")).unwrap();
    assert_eq!(
        log.lines().filter(|l| l.starts_with("start ")).count(),
        2,
        "{log}"
    );
    // Shutdown: `shutdown` without params, then `exit`.
    let all = pool.lock().unwrap().take_under(None);
    for s in all {
        s.shutdown(Duration::from_secs(2));
        assert!(!s.alive());
    }
    let log = std::fs::read_to_string(d.with_extension("log")).unwrap();
    assert!(
        log.contains("shutdown") && log.contains(" False\n"),
        "no params: {log}"
    );
    assert!(log.lines().any(|l| l.starts_with("exit ")), "{log}");
}

#[test]
fn r83_three_crashes_stop_restarts_until_restart() {
    let d = repo("three");
    let pool = Mutex::new(Pool::default());
    let mut states = vec![];
    for _ in 0..6 {
        states.push(run(&pool, &ctx(&d, Query::Def, "src/lib.rs", 4, 5, &[])).state);
    }
    assert_eq!(
        states,
        ["error", "error", "error", "crashed", "crashed", "crashed"]
    );
    let log = std::fs::read_to_string(d.with_extension("log")).unwrap();
    assert_eq!(
        log.lines().filter(|l| l.starts_with("start ")).count(),
        3,
        "{log}"
    );
    let a = run(&pool, &ctx(&d, Query::Def, "src/main.rs", 2, 16, &[]));
    assert_eq!(a.state, "crashed", "{:?}", a.hint);
    pool.lock().unwrap().restart(&(d.clone(), Lang::Rust));
    assert_eq!(
        run(&pool, &ctx(&d, Query::Def, "src/main.rs", 2, 16, &[])).state,
        "ready"
    );
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
}

#[test]
fn r83_queued_requests_on_a_hung_server_each_wait_their_own_timeout() {
    // M28 acceptance (Low): requests behind one that a hung server never
    // answers gave up one after another.
    let d = repo("hung");
    let pool = std::sync::Arc::new(Mutex::new(Pool::default()));
    let mut c = ctx(
        &d,
        Query::Refs,
        "src/lib.rs",
        1,
        8,
        &[("FAKE_LSP_HANG_ON", "textDocument/references")],
    );
    c.timeout = Duration::from_secs(2);
    // Start it first (not part of the timing).
    let _ = run(
        &pool,
        &ctx(
            &d,
            Query::Def,
            "src/main.rs",
            2,
            18,
            &[("FAKE_LSP_HANG_ON", "textDocument/references")],
        ),
    );
    let t = std::time::Instant::now();
    let hs: Vec<_> = (0..3)
        .map(|_| {
            let (p, c) = (pool.clone(), c.clone());
            std::thread::spawn(move || run(&p, &c))
        })
        .collect();
    for h in hs {
        let a = h.join().unwrap();
        assert_eq!(a.state, "error", "{a:?}");
        assert!(a.text());
    }
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    for s in pool.lock().unwrap().take_under(None) {
        s.kill();
    }
}

#[test]
fn r83_a_server_hung_in_initialize_fails_fast_afterwards() {
    // M29 open item: every request waited out another 30 s start.
    let d = repo("inithang");
    let pool = Mutex::new(Pool::default());
    let c = ctx(
        &d,
        Query::Def,
        "src/main.rs",
        2,
        18,
        &[("FAKE_LSP_HANG_ON", "initialize")],
    );
    let t = std::time::Instant::now();
    let a = run(&pool, &c);
    assert!(t.elapsed() >= START_TIMEOUT, "{:?}", t.elapsed());
    let hint = a.hint.clone().unwrap();
    assert!(hint.contains("did not answer `initialize`"), "{hint}");
    assert_eq!((a.state.as_str(), a.text()), ("error", true));
    let t = std::time::Instant::now();
    let b = run(&pool, &c);
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(b.hint, a.hint);
    // Restart forgets it: the next request starts it again.
    let key = (d.clone(), servers::Lang::Rust);
    assert!(pool
        .lock()
        .unwrap()
        .hung_recently(&key, std::time::Instant::now()));
    pool.lock().unwrap().restart(&key);
    assert!(!pool
        .lock()
        .unwrap()
        .hung_recently(&key, std::time::Instant::now()));
}

#[test]
fn r84_an_empty_answer_from_an_idle_server_is_ready() {
    let d = repo("empty");
    let pool = Mutex::new(Pool::default());
    assert_eq!(
        run(&pool, &ctx(&d, Query::Def, "src/main.rs", 2, 19, &[])).state,
        "ready"
    );
    std::thread::sleep(Duration::from_millis(2200));
    let a = run(&pool, &ctx(&d, Query::Callers, "src/lib.rs", 3, 5, &[]));
    assert_eq!(
        (a.state.as_str(), a.items.len()),
        ("ready", 0),
        "{:?}",
        a.hint
    );
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
}

/// Acceptance M4 (2026-10-05): a cold typescript-language-server answered
/// the first definition from a half-loaded project (the import line).
/// Needs a real server: `RADE_TEST_TSLS=<typescript-language-server>`
/// `RADE_TEST_TS_NM=<a node_modules with typescript 5>`.
#[test]
#[ignore]
fn real_tsls_first_definition_is_not_the_import() {
    let (Ok(tsls), Ok(nm)) = (
        std::env::var("RADE_TEST_TSLS"),
        std::env::var("RADE_TEST_TS_NM"),
    ) else {
        return;
    };
    let d = std::env::temp_dir().join(format!("rade-tsls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("package.json"), "{}").unwrap();
    std::fs::write(
        d.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true}, "include": ["src"]}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(&nm, d.join("node_modules")).unwrap();
    std::fs::write(
        d.join("src/a.ts"),
        "export function total(xs: number[]): number {\n  return xs.length;\n}\n",
    )
    .unwrap();
    std::fs::write(
        d.join("src/b.ts"),
        "import { total } from \"./a\";\nexport const n = total([1]);\n",
    )
    .unwrap();
    let cfg: CodeConfig = toml::from_str(&format!(
        "[server.typescript]\ncommand = [\"{tsls}\", \"--stdio\"]\n"
    ))
    .unwrap();
    let pool = Mutex::new(Pool::default());
    let c = Ctx {
        query: Query::Def,
        path: d.join("src/b.ts"),
        line: 2,
        col: 18,
        worktree: d.clone(),
        cfg,
        enabled: true,
        env: vec![],
        wait: Duration::from_secs(20),
        timeout: Duration::from_secs(10),
        root: None,
        depth: 1,
    };
    let a = run(&pool, &c);
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
    assert_eq!(at(&a), ["src/a.ts:1:17"], "{:?} {:?}", a.state, a.hint);
}

#[test]
fn r90_r91_r92_hover_outline_and_workspace_symbols() {
    let d = repo("m30");
    let pool = Mutex::new(Pool::default());
    // Hover: the server's Markdown.
    let a = run(&pool, &ctx(&d, Query::Hover, "src/lib.rs", 3, 5, &[]));
    assert_eq!((a.state.as_str(), a.source.as_str()), ("ready", "fake-lsp"));
    assert_eq!(
        a.doc.as_deref(),
        Some("```rust\nfn helper()\n```\n---\nCalls greet.")
    );
    // Nothing to say: ready, no doc.
    let a = run(&pool, &ctx(&d, Query::Hover, "src/main.rs", 1, 4, &[]));
    assert_eq!((a.state.as_str(), a.doc), ("ready", None));
    // Workspace symbols of the Worktree (any case), with kinds.
    let c = ctx(&d, Query::Workspace, ".", 1, 1, &[]);
    let w = workspace(&pool, &c, "GREE");
    assert_eq!((w.state.as_str(), w.source.as_str()), ("ready", "fake-lsp"));
    let got: Vec<_> = w
        .items
        .iter()
        .map(|i| {
            format!(
                "{} {} {}:{}",
                i.name.as_deref().unwrap(),
                i.kind.as_deref().unwrap(),
                nav::rel(&i.path, &d),
                i.line
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            "greet function src/lib.rs:1",
            "Greeting struct src/main.rs:1"
        ]
    );
    assert_eq!(workspace(&pool, &c, " ").state, "no-word");
    // Outline from the server.
    let o = run(&pool, &ctx(&d, Query::Symbols, "src/lib.rs", 1, 1, &[]));
    assert_eq!(at(&o), ["src/lib.rs:1:8 greet"]);
    assert_eq!(o.items[0].kind.as_deref(), Some("function"));
    assert_eq!(o.query.unwrap().title(&o.word), "Outline of lib.rs");
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );

    // Without a server: the declaring line as the hover, a text outline,
    // declarations as workspace symbols — all labelled.
    let off = |q| {
        let mut c = ctx(&d, q, "src/lib.rs", 3, 5, &[]);
        c.cfg = toml::from_str("[server.rust]\ncommand = [\"no-such-ls\"]\n").unwrap();
        c
    };
    let a = run(&pool, &off(Query::Hover));
    assert_eq!((a.state.as_str(), a.text()), ("no-server", true));
    assert_eq!(a.doc.as_deref(), Some("```\nfn helper() { greet(); }\n```"));
    let o = run(&pool, &off(Query::Symbols));
    assert_eq!(
        at(&o),
        [
            "src/lib.rs:1:8 greet",
            "src/lib.rs:3:4 helper",
            "src/lib.rs:4:4 crash_now"
        ]
    );
    let mut c = off(Query::Workspace);
    c.path = d.clone();
    let w = workspace(&pool, &c, "help");
    assert_eq!((w.state.as_str(), w.text()), ("no-server", true));
    assert!(w.hint.as_deref().unwrap().contains("no-such-ls"));
    assert_eq!(at(&w), ["src/lib.rs:3:4 helper"]);
}

/// Real servers (M30, manual): `RADE_TEST_TSLS` + `RADE_TEST_TS_NM` as
/// above, `RADE_TEST_PYRIGHT=<pyright-langserver>`, `RADE_TEST_GOPLS=<gopls>`:
/// definition, hover and workspace symbols on a tiny project each.
#[test]
#[ignore]
fn real_servers_def_hover_workspace() {
    let mk = |name: &str, files: &[(&str, &str)]| {
        let d = std::env::temp_dir().join(format!("rade-real-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (f, t) in files {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), t).unwrap();
        }
        d
    };
    let mut cases = vec![];
    if let (Ok(tsls), Ok(nm)) = (
        std::env::var("RADE_TEST_TSLS"),
        std::env::var("RADE_TEST_TS_NM"),
    ) {
        let d = mk("ts", &[
            ("package.json", "{}"),
            ("tsconfig.json", r#"{"compilerOptions": {"strict": true}, "include": ["src"]}"#),
            ("src/a.ts", "/** Counts them. */\nexport function total(xs: number[]): number {\n  return xs.length;\n}\n"),
            ("src/b.ts", "import { total } from \"./a\";\nexport const n = total([1]);\n"),
        ]);
        std::os::unix::fs::symlink(&nm, d.join("node_modules")).unwrap();
        cases.push((
            d,
            "typescript",
            tsls,
            vec!["--stdio".to_string()],
            "src/b.ts",
            2,
            18,
            "src/a.ts:2:17",
            "Counts them",
        ));
    }
    if let Ok(py) = std::env::var("RADE_TEST_PYRIGHT") {
        let d = mk("py", &[
            ("pyproject.toml", "[project]\nname = \"x\"\n"),
            ("pkg/a.py", "def total(xs: list[int]) -> int:\n    \"\"\"Counts them.\"\"\"\n    return len(xs)\n"),
            ("pkg/b.py", "from pkg.a import total\n\nn = total([1])\n"),
        ]);
        cases.push((
            d,
            "python",
            py,
            vec!["--stdio".to_string()],
            "pkg/b.py",
            3,
            6,
            "pkg/a.py:1:5",
            "Counts them",
        ));
    }
    if let Ok(go) = std::env::var("RADE_TEST_GOPLS") {
        let d = mk("go", &[
            ("go.mod", "module x\n\ngo 1.21\n"),
            ("a.go", "package x\n\n// Total counts them.\nfunc Total(xs []int) int { return len(xs) }\n"),
            ("b.go", "package x\n\nvar N = Total([]int{1})\n"),
        ]);
        cases.push((
            d,
            "go",
            go,
            vec![],
            "b.go",
            3,
            10,
            "a.go:4:6",
            "Total counts them",
        ));
    }
    assert!(
        !cases.is_empty(),
        "set RADE_TEST_TSLS/_TS_NM, RADE_TEST_PYRIGHT or RADE_TEST_GOPLS"
    );
    for (d, lang, cmd, args, file, line, col, def, doc) in cases {
        let mut argv = vec![cmd];
        argv.extend(args);
        let cfg: CodeConfig =
            toml::from_str(&format!("[server.{lang}]\ncommand = {argv:?}\n")).unwrap();
        let pool = Mutex::new(Pool::default());
        let c = |q| Ctx {
            query: q,
            path: d.join(file),
            line,
            col,
            worktree: d.clone(),
            cfg: cfg.clone(),
            enabled: true,
            env: vec![],
            wait: Duration::from_secs(30),
            timeout: Duration::from_secs(10),
            root: None,
            depth: 1,
        };
        let t = std::time::Instant::now();
        let a = run(&pool, &c(Query::Def));
        assert_eq!(at(&a), [def], "{lang}: {:?} {:?}", a.state, a.hint);
        let h = run(&pool, &c(Query::Hover));
        assert!(
            h.doc.as_deref().unwrap_or("").contains(doc),
            "{lang}: {:?}",
            h.doc
        );
        assert!(!h.text(), "{lang}");
        let w = workspace(&pool, &c(Query::Workspace), "tota");
        assert!(
            w.items
                .iter()
                .any(|i| i.name.as_deref() == Some(if lang == "go" { "Total" } else { "total" })),
            "{lang}: {:?} {:?} {:?}",
            w.state,
            w.hint,
            at(&w)
        );
        eprintln!(
            "{lang}: def + hover + workspace in {:?} ({}; {:?})",
            t.elapsed(),
            w.source,
            w.hint
        );
        super::stop(
            pool.lock().unwrap().take_under(None),
            Duration::from_secs(2),
        );
    }
}

#[test]
fn r92_workspace_symbols_in_a_monorepo_package() {
    // Nothing at the Worktree's top: the language is found in a package.
    let top = std::env::temp_dir().join(format!("rade-code-mono-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&top);
    std::fs::create_dir_all(&top).unwrap();
    let d = repo("mono-pkg");
    std::fs::rename(&d, top.join("pkg")).unwrap();
    let pool = Mutex::new(Pool::default());
    let mut c = ctx(&top, Query::Workspace, ".", 1, 1, &[]);
    c.worktree = top.clone();
    c.path = top.clone();
    let w = workspace(&pool, &c, "help");
    assert_eq!(
        (w.state.as_str(), w.source.as_str()),
        ("ready", "fake-lsp"),
        "{:?}",
        w.hint
    );
    assert_eq!(at(&w), ["pkg/src/lib.rs:3:4 helper"]);
    let keys: Vec<_> = pool
        .lock()
        .unwrap()
        .list(std::time::Instant::now())
        .into_iter()
        .map(|(k, _, _)| k.0)
        .collect();
    assert_eq!(
        keys,
        [top.join("pkg")],
        "the package's root, as for navigation"
    );
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
}

/// The owner's `momentWithTZ` (0.7.1): several call sites per caller, the
/// next levels on demand, a recursive caller, a use that is not a call.
#[test]
fn r85_call_hierarchy_sites_levels_and_other_references() {
    let d = repo("calls");
    std::fs::write(
        d.join("src/utils.rs"),
        "pub fn tz() {}\nfn fmt_a() { tz(); tz(); }\nfn fmt_b() {\n    tz();\n}\n",
    )
    .unwrap();
    std::fs::write(
        d.join("src/view.rs"),
        "use crate::utils::tz;\nfn show() { fmt_a(); }\nfn pick() { let f = if x { tz } else { other }; }\nfn rec() { rec(); fmt_a(); }\n",
    )
    .unwrap();
    std::fs::write(
        d.join(".fake-lsp.json"),
        r#"{"tz": {"def": ["src/utils.rs:1:8"],
                   "refs": ["src/utils.rs:2:14", "src/utils.rs:2:20", "src/utils.rs:4:5", "src/view.rs:1:19", "src/view.rs:3:28"],
                   "callers": [{"name": "fmt_a", "def": "src/utils.rs:2:4", "at": ["src/utils.rs:2:14", "src/utils.rs:2:20"]},
                               {"name": "fmt_b", "def": "src/utils.rs:3:4", "at": "src/utils.rs:4:5"}]},
            "fmt_a": {"def": ["src/utils.rs:2:4"],
                      "callers": [{"name": "show", "def": "src/view.rs:2:4", "at": "src/view.rs:2:13"},
                                  {"name": "rec", "def": "src/view.rs:4:4", "at": "src/view.rs:4:19"}]},
            "rec": {"callers": [{"name": "rec", "def": "src/view.rs:4:4", "at": "src/view.rs:4:12"}]},
            "show": {"def": ["src/view.rs:2:4"], "callees": [{"name": "fmt_a", "at": "src/utils.rs:2:4", "sites": ["src/view.rs:2:13"]}]}}"#,
    )
    .unwrap();
    let pool = Mutex::new(Pool::default());
    let a = run(&pool, &ctx(&d, Query::Callers, "src/utils.rs", 1, 8, &[]));
    assert_eq!(a.state, "ready", "{:?}", a.hint);
    assert_eq!(
        at(&a),
        [
            "src/utils.rs:2:14 fmt_a",
            "src/utils.rs:2:20 fmt_a",
            "src/utils.rs:4:5 fmt_b"
        ],
        "every call site, named by its caller"
    );
    let shape: Vec<_> = a
        .calls
        .iter()
        .map(|c| (c.name.as_str(), c.line, c.from_ranges.len(), c.expanded))
        .collect();
    assert_eq!(shape, [("fmt_a", 2, 2, false), ("fmt_b", 3, 1, false)]);
    let others: Vec<_> = a
        .others
        .iter()
        .map(|i| format!("{}:{}:{}", nav::rel(&i.path, &d), i.line, i.col))
        .collect();
    assert_eq!(
        others,
        ["src/view.rs:3:28"],
        "not the calls, not the import"
    );
    assert_eq!(
        a.summary(),
        "3 call sites in 2 callers · 1 other reference · fake-lsp"
    );
    // The next level, as the results popover expands a node.
    let next = expand(
        &pool,
        &ctx(&d, Query::Callers, "src/utils.rs", 1, 8, &[]),
        &a.calls[0].item,
        true,
    )
    .unwrap();
    let names: Vec<_> = next
        .iter()
        .map(|c| (c.name.as_str(), c.from_ranges[0].line))
        .collect();
    assert_eq!(names, [("show", 2), ("rec", 4)]);
    // --depth 4: fmt_a → rec → rec, which is not expanded again.
    let mut c = ctx(&d, Query::Callers, "src/utils.rs", 1, 8, &[]);
    c.depth = 4;
    let a = run(&pool, &c);
    let fmt_a = &a.calls[0];
    assert!(fmt_a.expanded);
    let rec = &fmt_a.children[1];
    assert_eq!((rec.name.as_str(), rec.children.len()), ("rec", 1));
    assert!(rec.children[0].recursive && rec.children[0].children.is_empty());
    assert_eq!(a.items.len(), 3, "items: the first level only");
    let text = nav::render(&a);
    assert!(
        text.contains("src/utils.rs:2:20\tfmt_a\t")
            && text.contains("\n  src/view.rs:2:13\tshow\tfn show() { fmt_a(); }\n")
            && text.contains("\n    src/view.rs:4:12\trec (recursive)\t")
            && text.contains("# other references (not calls): 1\nsrc/view.rs:3:28\t\t"),
        "{text}"
    );
    let j = serde_json::to_value(&a).unwrap();
    assert_eq!(j["calls"][0]["from_ranges"].as_array().unwrap().len(), 2);
    assert_eq!(j["calls"][0]["children"][0]["name"], "show");
    // Callees: each at its definition, the call sites in the asked function.
    let a = run(&pool, &ctx(&d, Query::Callees, "src/view.rs", 2, 5, &[]));
    assert_eq!(at(&a), ["src/utils.rs:2:4 fmt_a"]);
    assert_eq!(a.calls[0].from_ranges[0].path, d.join("src/view.rs"));
    assert_eq!(a.calls[0].from_ranges[0].col, 13);
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
}

/// A file outside the Worktree reached by navigation (the std library) is
/// asked through the server it was reached from: no second server.
#[test]
fn r83_a_file_outside_the_worktree_uses_the_origin_server() {
    let d = repo("origin");
    let std_lib = d.with_extension("std");
    let _ = std::fs::remove_dir_all(&std_lib);
    std::fs::create_dir_all(std_lib.join("core/src")).unwrap();
    std::fs::write(std_lib.join("Cargo.toml"), "[workspace]\n").unwrap();
    std::fs::write(std_lib.join("core/src/fmt.rs"), "pub fn greet() {}\n").unwrap();
    let pool = Mutex::new(Pool::default());
    assert_eq!(
        run(&pool, &ctx(&d, Query::Def, "src/main.rs", 2, 18, &[])).state,
        "ready"
    );
    let mut c = ctx(&d, Query::Def, "src/main.rs", 1, 8, &[]);
    c.path = std_lib.join("core/src/fmt.rs");
    c.root = Some(d.clone());
    let a = run(&pool, &c);
    assert_eq!(a.state, "ready");
    assert_eq!(at(&a), ["src/lib.rs:1:8"]);
    let keys: Vec<_> = pool
        .lock()
        .unwrap()
        .list(std::time::Instant::now())
        .into_iter()
        .map(|(k, _, _)| k.0)
        .collect();
    assert_eq!(keys, std::slice::from_ref(&d));
    super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
    let _ = std::fs::remove_dir_all(&std_lib);
}
