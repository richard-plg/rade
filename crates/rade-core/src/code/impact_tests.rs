//! Impact (R88): the pure parts, then `compute` / `within` against
//! tests/e2e/fake-lsp.py on a git repository with a diff.

use super::*;
use crate::code::nav::Query;
use crate::diff::{DiffLine, Hunk};

fn sym(name: &str, kind: u64, start: u32, end: u32, local: bool) -> Sym {
    Sym {
        name: name.into(),
        kind,
        start,
        end,
        sel_line: start,
        sel_col: 3,
        local,
    }
}

fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, at: u32) -> DiffLine {
    DiffLine {
        kind,
        old,
        new,
        at,
        text: String::new(),
    }
}

#[test]
fn r88_touched_lines_and_innermost_symbols() {
    use LineKind::*;
    let f = FileDiff {
        path: "a.rs".into(),
        old_path: None,
        status: 'M',
        binary: false,
        added: 4,
        removed: 1,
        hunks: vec![Hunk {
            header: "@@".into(),
            lines: vec![
                line(Context, Some(2), Some(2), 2),
                line(Added, None, Some(3), 3),
                line(Removed, Some(3), None, 4),
                line(Added, None, Some(10), 10),
                line(Added, None, Some(11), 11),
                line(Added, None, Some(12), 12),
            ],
        }],
    };
    let t = touched(&f);
    assert_eq!(t.lines, [3, 10, 11, 12]);
    assert!(
        t.gaps.is_empty(),
        "the removal next to added line 3 is its change"
    );
    assert!(t.added.contains(&3) && !t.added.contains(&4));
    // 0-based ranges: S (struct, lines 0..=5) holds method m (1..=4) with a
    // local closure; f (9..=11) is all new; a module never counts.
    let syms = [
        sym("mod", 2, 0, 20, false),
        sym("S", 23, 0, 5, false),
        sym("S.m", 6, 1, 4, false),
        sym("S.m.local", 13, 2, 2, true),
        sym("f", 12, 9, 11, false),
    ];
    let p = pick(&syms, &t, &HashSet::new());
    let got: Vec<(&str, &str)> = p.iter().map(|(s, c)| (s.name.as_str(), *c)).collect();
    assert_eq!(got, [("S.m", "changed"), ("f", "added")]);
    // A new first line whose name a removed line declared: a changed signature.
    let p = pick(&syms, &t, &["f".to_string()].into());
    assert_eq!(p[1].1, "changed");
}

#[test]
fn r88_removals_count_inside_a_symbol_only_and_docs_dont_make_it_new() {
    // helper 0..=2, user_a 4..=6 (0-based). Acceptance H1: a whole function
    // removed between them (gap at line 5) touches neither; a removal
    // inside helper (gap at line 2) is helper's change.
    let syms = [
        sym("helper", 12, 0, 2, false),
        sym("user_a", 12, 4, 6, false),
    ];
    let t = Touched {
        gaps: vec![5],
        ..Default::default()
    };
    assert!(pick(&syms, &t, &HashSet::new()).is_empty());
    let t = Touched {
        gaps: vec![2],
        ..Default::default()
    };
    assert_eq!(pick(&syms, &t, &HashSet::new())[0].0.name, "helper");
    // Acceptance M1: doc comment + attribute added above an existing
    // function (its range starts there, its name is on line 3): changed.
    let mut f = sym("helper", 12, 0, 4, false);
    f.sel_line = 2;
    let t = Touched {
        added: [1, 2].into(),
        lines: vec![1, 2],
        gaps: vec![],
    };
    assert_eq!(pick(&[f], &t, &HashSet::new())[0].1, "changed");
}

#[test]
fn r88_text_declarations_and_enclosing_function() {
    for (l, want) in [
        ("pub fn greet(n: &str) {", Some(("greet", "function"))),
        ("pub(crate) async fn load() {", Some(("load", "function"))),
        (
            "export async function fetchAll() {",
            Some(("fetchAll", "function")),
        ),
        ("export default class Billing {", Some(("Billing", "class"))),
        ("pub struct Invoice {", Some(("Invoice", "struct"))),
        ("def handle(self, x):", Some(("handle", "function"))),
        (
            "export const total = (items: Item[]) => {",
            Some(("total", "function")),
        ),
        (
            "  async verifyOtp(user: User, code: string): Promise<boolean> {",
            Some(("verifyOtp", "method")),
        ),
        ("  if (x) {", None),
        ("// fn not_this() {", None),
        ("    greet(x);", None),
    ] {
        assert_eq!(
            declared(l).as_ref().map(|(n, k)| (n.as_str(), *k)),
            want,
            "{l}"
        );
    }
    let src = "fn a() {\n    let x = 1;\n}\n\nfn b() {\n    x();\n    y();\n}\nconst Z: u8 = 1;\n";
    let lines: Vec<&str> = src.lines().collect();
    let t = Touched {
        added: [5, 7].into(),
        lines: vec![2, 6, 7, 9],
        gaps: vec![],
    };
    let got = text_pick(&lines, &t, &HashSet::new());
    let got: Vec<(&str, u32, u32, &str)> =
        got.iter().map(|g| (g.0.as_str(), g.2, g.3, g.4)).collect();
    // Line 9 (`const Z`, top level) has no enclosing declaration.
    assert_eq!(got, [("a", 1, 4, "changed"), ("b", 5, 4, "added")]);
    // Removals: after a top-level `}` or a blank line nothing; inside b, b.
    let gaps = |g: Vec<u32>| Touched {
        gaps: g,
        ..Default::default()
    };
    assert!(text_pick(&lines, &gaps(vec![4, 5]), &HashSet::new()).is_empty());
    assert_eq!(text_pick(&lines, &gaps(vec![7]), &HashSet::new())[0].0, "b");
}

#[test]
fn r88_test_files() {
    for p in [
        "tests/m.rs",
        "src/a/__tests__/x.ts",
        "src/auth.service.spec.ts",
        "web/a.test.tsx",
        "pkg/x_test.go",
        "test_api.py",
        "crates/x/src/task_tests.rs",
        "src/code/tests.rs",
    ] {
        assert!(is_test(p), "{p}");
    }
    for p in [
        "src/lib.rs",
        "src/testing.rs",
        "latest/x.ts",
        "src/contest.rs",
    ] {
        assert!(!is_test(p), "{p}");
    }
}

fn site(p: &str, line: u32, test: bool, text: bool) -> Site {
    Site {
        path: PathBuf::from("/w").join(p),
        line,
        col: 1,
        name: None,
        preview: "x()".into(),
        test,
        text,
    }
}

#[test]
fn r88_summary_and_bounded_render() {
    let mut i = Impact {
        root: "/w".into(),
        source: "rust-analyzer".into(),
        state: "ready".into(),
        symbols: vec![
            Changed {
                name: "greet".into(),
                kind: "function".into(),
                change: "changed".into(),
                path: "/w/src/lib.rs".into(),
                line: 1,
                col: 8,
                text: false,
                callers: vec![
                    site("src/main.rs", 2, false, false),
                    site("tests/t.rs", 4, true, true),
                ],
            },
            Changed {
                name: "legacy".into(),
                kind: "function".into(),
                change: "removed".into(),
                path: "/w/src/lib.rs".into(),
                line: 9,
                col: 8,
                text: true,
                callers: vec![site("src/x.rs", 4, false, true)],
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        summary(&i),
        "1 changed · 2 call sites in 2 files (1 in tests) · 1 removed, still used"
    );
    assert_eq!(
        render(&i, 60),
        "1 changed · 2 call sites in 2 files (1 in tests) · 1 removed, still used · rust-analyzer\n\
         ~ greet (function) src/lib.rs:1 — 2 call sites\n    src/main.rs:2: x()\n    tests/t.rs:4 [test, text match]: x()\n\
         - legacy (function) src/lib.rs:9 — still used at 1 place (text matches)\n    src/x.rs:4 [text match]: x()\n"
    );
    let r = render(&i, 3);
    assert_eq!(r.lines().count(), 3);
    assert!(
        r.ends_with("… 4 more lines: run `rade code impact` in the Worktree\n"),
        "{r}"
    );
    assert!(prompt_section(&i, " (src)").contains("## Impact (src)\n"));
    i.symbols.clear();
    assert_eq!(summary(&i), "No changed symbols found");
    assert_eq!(prompt_section(&i, ""), "", "no section without symbols");
}

fn fake() -> String {
    format!("{}/../../tests/e2e/fake-lsp.py", env!("CARGO_MANIFEST_DIR"))
}

fn git(d: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(d)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// main: lib.rs with greet / helper / legacy; the branch changes greet's
/// body, removes legacy (still called from src/old.rs), adds fresh; a
/// test calls greet (the fake server does not return it).
fn repo(name: &str, extra_json: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rade-impact-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_file(d.with_extension("log"));
    for f in ["src", "tests"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    let w = |p: &str, t: &str| std::fs::write(d.join(p), t).unwrap();
    w("Cargo.toml", "[package]\nname = \"x\"\n");
    w(
        "src/lib.rs",
        "pub fn greet() -> u8 {\n    1\n}\n\npub fn helper() {\n    greet();\n}\n\npub fn legacy() {}\n",
    );
    w("src/old.rs", "fn old() {\n    legacy();\n}\n");
    w("src/main.rs", "fn main() {\n    greet();\n}\n");
    w("tests/t.rs", "#[test]\nfn t() {\n    x::greet();\n}\n");
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "base"]);
    git(&d, &["checkout", "-qb", "feat"]);
    w(
        "src/lib.rs",
        "pub fn greet() -> u8 {\n    2\n}\n\npub fn helper() {\n    greet();\n}\n\npub fn fresh() {\n    greet();\n}\n",
    );
    w(
        ".fake-lsp.json",
        &format!(
            r#"{{"greet": {{"def": ["src/lib.rs:1:8"], "callers": [{{"name": "main", "at": "src/main.rs:2:5"}}, {{"name": "helper", "at": "src/lib.rs:6:5"}}, {{"name": "fresh", "at": "src/lib.rs:10:5"}}]}},
                "fresh": {{"def": ["src/lib.rs:9:8"]}},
                "@symbols": {{"src/lib.rs": [{{"name": "greet", "at": "src/lib.rs:1:8", "end": "src/lib.rs:3:1"}},
                                            {{"name": "helper", "at": "src/lib.rs:5:8", "end": "src/lib.rs:7:1"}},
                                            {{"name": "fresh", "at": "src/lib.rs:9:8", "end": "src/lib.rs:11:1"}}]}}{extra_json}}}"#
        ),
    );
    std::fs::write(d.join(".git/info/exclude"), ".fake-lsp.json\n").unwrap();
    d
}

fn inputs(d: &Path, budget: u64, env: &[(&str, &str)]) -> Inputs {
    let cfg: CodeConfig = toml::from_str(&format!(
        "[server.rust]\ncommand = [\"python3\", \"{}\"]\n",
        fake()
    ))
    .unwrap();
    let mut e = vec![(
        "FAKE_LSP_LOG".to_string(),
        d.with_extension("log").display().to_string(),
    )];
    e.extend(env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    Inputs {
        worktree: d.to_path_buf(),
        base: Some("main".into()),
        cfg,
        enabled: true,
        env: e,
        budget: Duration::from_secs(budget),
        wait: Duration::from_secs(budget),
    }
}

fn lines(i: &Impact) -> Vec<String> {
    i.symbols
        .iter()
        .map(|s| {
            let c: Vec<String> = s
                .callers
                .iter()
                .map(|c| {
                    format!(
                        "{}:{}{}{}",
                        nav::rel(&c.path, &i.root),
                        c.line,
                        if c.test { " test" } else { "" },
                        if c.text { " text" } else { "" }
                    )
                })
                .collect();
            format!(
                "{} {} {}:{} <- {}",
                s.change,
                s.name,
                nav::rel(&s.path, &i.root),
                s.line,
                c.join(", ")
            )
        })
        .collect()
}

fn stop(pool: &Mutex<Pool<Client>>) {
    super::super::stop(
        pool.lock().unwrap().take_under(None),
        Duration::from_secs(2),
    );
}

#[test]
fn r88_impact_through_a_server() {
    let d = repo("srv", "");
    let pool = Mutex::new(Pool::default());
    let i = compute(&pool, &inputs(&d, 20, &[]), true).unwrap();
    assert_eq!(
        (i.state.as_str(), i.source.as_str()),
        ("ready", "fake-lsp"),
        "{i:?}"
    );
    assert_eq!(i.branch.as_deref(), Some("main"));
    assert_eq!(
        lines(&i),
        [
            // helper's call is not in the diff; fresh's call is (added).
            "changed greet src/lib.rs:1 <- src/lib.rs:6, src/main.rs:2, tests/t.rs:3 test text",
            "added fresh src/lib.rs:9 <- ",
            "removed legacy src/lib.rs:9 <- src/old.rs:2 text",
        ]
    );
    let log = std::fs::read_to_string(d.with_extension("log")).unwrap();
    assert!(log.contains("watched 2:src/lib.rs"), "{log}");
    assert!(
        log.contains("request callHierarchy/incomingCalls greet"),
        "{log}"
    );
    // Text only: the same symbols from declarations, callers by name.
    let t = compute(&pool, &inputs(&d, 20, &[]), false).unwrap();
    assert_eq!((t.state.as_str(), t.source.as_str()), ("text", "text"));
    assert_eq!(
        lines(&t)[0],
        "changed greet src/lib.rs:1 <- src/lib.rs:6 text, src/main.rs:2 text, tests/t.rs:3 test text"
    );
    stop(&pool);
}

#[test]
fn r88_indexing_server_gives_text_within_the_budget() {
    let d = repo("idx", r#", "@index_ms": 60000"#);
    let pool = Mutex::new(Pool::default());
    let t = Instant::now();
    let i = compute(&pool, &inputs(&d, 2, &[]), true).unwrap();
    assert!(t.elapsed() < Duration::from_secs(6), "{:?}", t.elapsed());
    assert_eq!(i.state, "indexing");
    assert!(
        i.hint.as_deref().unwrap().contains("fake-lsp is indexing"),
        "{:?}",
        i.hint
    );
    assert!(i.symbols.iter().all(|s| s.text));
    assert_eq!(i.symbols[0].name, "greet");
    stop(&pool);
}

#[test]
fn r88_a_hung_server_never_holds_past_the_budget() {
    static POOL: LazyLock<Mutex<Pool<Client>>> = LazyLock::new(Mutex::default);
    for (n, on) in ["initialize", "textDocument/documentSymbol"]
        .iter()
        .enumerate()
    {
        let d = repo(&format!("hang{n}"), "");
        let t = Instant::now();
        let i = within(&POOL, inputs(&d, 2, &[("FAKE_LSP_HANG_ON", on)])).unwrap();
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "{on}: {:?}",
            t.elapsed()
        );
        assert_eq!(i.source, "text", "{on}");
        assert!(i.hint.is_some(), "{on}");
        assert_eq!(i.symbols[0].name, "greet", "{on}");
    }
    for s in POOL.lock().unwrap().take_under(None) {
        s.kill();
    }
}

#[test]
fn r87_removed_line_names_current_tree_else_base() {
    let d = repo("rm", "");
    let pool = Mutex::new(Pool::default());
    let i = inputs(&d, 20, &[]);
    let c = |q: Query| super::super::Ctx {
        query: q,
        path: d.join("src/lib.rs"),
        line: 9,
        col: 8,
        worktree: d.clone(),
        cfg: i.cfg.clone(),
        enabled: true,
        env: i.env.clone(),
        wait: Duration::ZERO,
        timeout: Duration::from_secs(5),
        root: None,
        depth: 1,
    };
    let base = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(&d)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let base = base.trim();
    // `greet` still exists: asked from its declaration in the tree.
    let a = super::super::run_removed(&pool, &c(Query::Callers), "greet", base);
    assert_eq!(
        (a.state.as_str(), a.source.as_str()),
        ("ready", "fake-lsp"),
        "{a:?}"
    );
    assert!(a
        .hint
        .as_deref()
        .unwrap()
        .starts_with("Removed line: greet looked up in the current tree (src/lib.rs:1)"));
    assert_eq!(a.items.len(), 3);
    // `legacy` is gone: `git grep` at the base, labelled.
    let a = super::super::run_removed(&pool, &c(Query::Refs), "legacy", base);
    assert_eq!((a.state.as_str(), a.source.as_str()), ("base", "text"));
    let at: Vec<String> = a
        .items
        .iter()
        .map(|i| format!("{}:{}:{}", nav::rel(&i.path, &d), i.line, i.col))
        .collect();
    assert_eq!(at, ["src/lib.rs:9:8", "src/old.rs:2:5"]);
    assert_eq!(
        a.summary(),
        "2 results in 2 files · text matches at the base"
    );
    assert!(a.hint.unwrap().contains(&format!("({})", &base[..7])));
    let a = super::super::run_removed(&pool, &c(Query::Def), "legacy", base);
    assert_eq!(a.items.len(), 1, "the declaring line");
    stop(&pool);
}

#[test]
fn r88_a_huge_diff_stays_within_the_budget_and_the_symbol_cap() {
    // Acceptance H3 / M2: hundreds of new files by text.
    static POOL: LazyLock<Mutex<Pool<Client>>> = LazyLock::new(Mutex::default);
    let d = repo("huge", "");
    for i in 0..300 {
        std::fs::write(
            d.join(format!("src/m{i}.py")),
            format!("def f{i}_a():\n    return f{i}_b()\n\ndef f{i}_b():\n    return 1\n\ndef f{i}_c():\n    return f{i}_a()\n"),
        )
        .unwrap();
    }
    let t = Instant::now();
    let i = within(&POOL, inputs(&d, 2, &[])).unwrap();
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    assert!(i.symbols.len() <= 60, "{}", i.symbols.len());
    assert_eq!(i.state, "partial", "{:?}", i.hint);
    stop(&POOL);
}
