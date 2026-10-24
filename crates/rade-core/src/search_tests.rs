use super::*;
use std::path::PathBuf;

fn opts(q: &str) -> Options {
    Options {
        query: q.into(),
        ..Default::default()
    }
}

fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rade-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src/deep")).unwrap();
    std::fs::create_dir_all(d.join("target")).unwrap();
    std::fs::create_dir_all(d.join(".git")).unwrap(); // a repo: .gitignore applies
    let w = |p: &str, t: &[u8]| std::fs::write(d.join(p), t).unwrap();
    w(".gitignore", b"target/\n*.log\n");
    w(
        "src/main.rs",
        b"fn main() {\n    let foo = Foo::new();\n    foobar();\n}\n",
    );
    w("src/deep/lib.rs", b"pub struct Foo;\n// foo again, foo\n");
    w("README.md", b"Foo docs\n");
    w(".hidden.toml", b"foo = 1\n");
    w("target/out.rs", b"foo in build output\n");
    w("app.log", b"foo logged\n");
    w("bin.dat", b"foo\0binary\n");
    w(".git/config", b"foo\n");
    d
}

fn found(d: &Path, o: &Options) -> Vec<String> {
    search(d, o, MAX_MATCHES, &|| false)
        .unwrap()
        .files
        .iter()
        .flat_map(|f| f.hits.iter().map(move |h| format!("{}:{}", f.path, h.line)))
        .collect()
}

#[test]
fn r22_1_options_build_the_matcher() {
    assert!(matcher(&opts("")).unwrap().is_none());
    let m = |o: &Options, s: &str| matcher(o).unwrap().unwrap().is_match(s.as_bytes());
    // Literal by default, case-insensitive.
    assert!(m(&opts("a.b"), "A.B"));
    assert!(!m(&opts("a.b"), "axb"));
    let case = Options {
        case: true,
        ..opts("Foo")
    };
    assert!(m(&case, "Foo") && !m(&case, "foo"));
    let word = Options {
        word: true,
        ..opts("foo")
    };
    assert!(m(&word, "a foo b") && !m(&word, "foobar"));
    // Non-word edges keep matching ("foo(" in "foo(x)").
    let w2 = Options {
        word: true,
        ..opts("foo(")
    };
    assert!(m(&w2, "foo(x)") && !m(&w2, "xfoo(x)"));
    let re = Options {
        regex: true,
        ..opts(r"fo+\d")
    };
    assert!(m(&re, "fooo7") && !m(&re, "fo"));
    let anchored = Options {
        regex: true,
        ..opts("^b")
    };
    assert!(m(&anchored, "a\nb"), "multi-line anchors");
    let bad = Options {
        regex: true,
        ..opts("(")
    };
    assert!(matcher(&bad).unwrap_err().contains("unclosed"));
    assert_eq!(globs(" a, *.rs ,,b/** "), vec!["a", "*.rs", "b/**"]);
}

#[test]
fn r22_2_walk_respects_gitignore_hidden_binary_and_globs() {
    let d = tree("walk");
    assert_eq!(
        found(&d, &opts("foo")),
        vec![
            ".hidden.toml:1",
            "README.md:1",
            "src/deep/lib.rs:1",
            "src/deep/lib.rs:2",
            "src/main.rs:2",
            "src/main.rs:3",
        ]
    );
    let inc = Options {
        include: "*.rs".into(),
        ..opts("foo")
    };
    assert_eq!(
        found(&d, &inc),
        vec![
            "src/deep/lib.rs:1",
            "src/deep/lib.rs:2",
            "src/main.rs:2",
            "src/main.rs:3"
        ],
        "an include never un-ignores target/"
    );
    let exc = Options {
        exclude: "src/deep/**, *.md".into(),
        ..opts("foo")
    };
    assert_eq!(
        found(&d, &exc),
        vec![".hidden.toml:1", "src/main.rs:2", "src/main.rs:3"]
    );
    let both = Options {
        include: "src/**".into(),
        exclude: "lib.rs".into(),
        word: true,
        case: true,
        ..opts("foo")
    };
    assert_eq!(found(&d, &both), vec!["src/main.rs:2"]);
}

#[test]
fn r22_3_hits_count_matches_and_highlight_them() {
    let d = tree("hits");
    let r = search(&d, &opts("foo"), MAX_MATCHES, &|| false).unwrap();
    let lib = r
        .files
        .iter()
        .find(|f| f.path == "src/deep/lib.rs")
        .unwrap();
    assert_eq!(lib.matches, 3);
    assert_eq!(lib.hits[1].preview, "// foo again, foo");
    assert_eq!(lib.hits[1].ranges, vec![(3, 6), (14, 17)]);
    let main = r.files.iter().find(|f| f.path == "src/main.rs").unwrap();
    assert_eq!(
        main.hits[0].preview, "let foo = Foo::new();",
        "indent trimmed"
    );
    assert_eq!(main.hits[0].ranges, vec![(4, 7), (10, 13)]);
    assert_eq!(r.matches, 8);
    assert_eq!(r.summary(), "8 results in 4 files");
}

#[test]
fn r22_4_capped_and_cancelled() {
    let d = tree("cap");
    let many: String = (0..50).map(|i| format!("hit {i}\n")).collect();
    std::fs::write(d.join("many.txt"), many).unwrap();
    let r = search(&d, &opts("hit"), 20, &|| false).unwrap();
    assert_eq!(r.matches, 20);
    assert!(r.capped);
    assert_eq!(r.files[0].hits.len(), 20);
    assert!(r.summary().ends_with("— showing the first 2000"));
    let r = search(&d, &opts("hit"), MAX_MATCHES, &|| true).unwrap();
    assert!(r.cancelled);
    assert_eq!(r.matches, 0);
}

#[test]
fn r22_3_preview_cuts_long_lines_around_the_match() {
    let line = format!("{}needle{}", "a".repeat(300), "b".repeat(300));
    let (p, r) = preview(line.as_bytes(), &[(300, 306)]);
    assert!(p.starts_with('…') && p.ends_with('…'));
    assert_eq!(
        &p.chars().collect::<Vec<_>>()[r[0].0..r[0].1]
            .iter()
            .collect::<String>(),
        "needle"
    );
    // Multi-byte text: char offsets.
    let (p, r) = preview("  Tiếng Việt\r\n".as_bytes(), &[(10, 16)]);
    assert_eq!(p, "Tiếng Việt");
    assert_eq!(r, vec![(6, 10)]);
    // Binary skipped.
    let re = matcher(&opts("x")).unwrap().unwrap();
    assert_eq!(search_bytes(&re, b"x\0", 10), None);
    let (h, n) = search_bytes(&re, b"a\nxx\n\nx", 10).unwrap();
    assert_eq!((n, h.len(), h[0].line, h[1].line), (3, 2, 2, 4));
    let (_, n) = search_bytes(&re, b"xxxx", 2).unwrap();
    assert_eq!(n, 2, "budget");
}

#[test]
fn r22_3_rows_group_by_file_and_fold() {
    let d = tree("rows");
    let r = search(&d, &opts("foo"), MAX_MATCHES, &|| false).unwrap();
    let all = rows(&r, &HashSet::new());
    assert_eq!(all.len(), 4 + 6);
    assert_eq!(all[0], Row::File(0));
    assert_eq!(all[1], Row::Hit(0, 0));
    let folded: HashSet<String> = ["src/deep/lib.rs".to_string()].into();
    assert_eq!(rows(&r, &folded).len(), 4 + 4);
}

#[test]
fn search_scope_roots_link_and_project() {
    let base = std::env::temp_dir().join(format!("rade-scope-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let web = base.join("shop-web");
    let ui = base.join("shared-ui");
    let api = base.join("shop-api");
    for (d, f, t) in [
        (&web, "src/theme.css", "body { theme: dark }\n"),
        (&ui, "shared/theme.ts", "export const theme = 1;\n"),
        (&api, "src/main.rs", "fn main() {}\n"),
    ] {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), t).unwrap();
    }
    let group = |s: &str, p: &PathBuf| crate::home::Group {
        project: "shop".into(),
        source: s.into(),
        path: p.clone(),
        worktree: None,
        label: "develop".into(),
        base: "develop".into(),
    };
    let groups = [
        group("shop-web", &web),
        group("shop-api", &api),
        group("shared-ui", &ui),
    ];
    let link = scope_roots(Scope::Link, &groups, "shop-web", &web);
    assert_eq!(link, [("shop-web".to_string(), web.clone())]);
    let got = search_roots(&link, &opts("theme"), MAX_MATCHES, &|| false).unwrap();
    let files: Vec<(String, Vec<String>)> = got
        .iter()
        .map(|(l, r)| (l.clone(), r.files.iter().map(|f| f.path.clone()).collect()))
        .collect();
    assert_eq!(
        files,
        [("shop-web".to_string(), vec!["src/theme.css".to_string()])]
    );
    let project = scope_roots(Scope::Project, &groups, "shop-web", &web);
    assert_eq!(project.len(), 3);
    let got = search_roots(&project, &opts("theme"), MAX_MATCHES, &|| false).unwrap();
    let files: Vec<(String, Vec<String>)> = got
        .iter()
        .map(|(l, r)| (l.clone(), r.files.iter().map(|f| f.path.clone()).collect()))
        .collect();
    assert_eq!(
        files,
        [
            ("shop-web".to_string(), vec!["src/theme.css".to_string()]),
            ("shared-ui".to_string(), vec!["shared/theme.ts".to_string()]),
        ]
    );
    // One budget across folders.
    let capped = search_roots(&project, &opts("theme"), 1, &|| false).unwrap();
    assert_eq!(capped.len(), 1);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn search_scope_link_is_the_links_checkout_not_the_focused_worktree() {
    // A Terminal on Shop's shared-ui Project checkout lives in the Source's
    // main Worktree (on `main`): scope `link` searches the Project checkout.
    let main = PathBuf::from("/w/shared-ui");
    let pc = PathBuf::from("/w/.worktrees/.projects/shop/shared-ui");
    let groups = [crate::home::Group {
        project: "shop".into(),
        source: "shared-ui".into(),
        path: pc.clone(),
        worktree: None,
        label: "develop".into(),
        base: "develop".into(),
    }];
    assert_eq!(
        scope_roots(Scope::Link, &groups, "shared-ui", &main),
        [("shared-ui".to_string(), pc)]
    );
    // Outside every group (or no Project): the focused checkout alone.
    assert_eq!(
        scope_roots(Scope::Link, &groups, "billing", &main),
        [(String::new(), main.clone())]
    );
    assert_eq!(
        scope_roots(Scope::Project, &[], "shared-ui", &main),
        [(String::new(), main)]
    );
}
