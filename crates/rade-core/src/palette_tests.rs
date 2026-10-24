use super::*;
use crate::agents::AgentStatus;
use crate::config::Config;

fn titles(items: &[Item], hits: &[Hit]) -> Vec<String> {
    hits.iter().map(|h| items[h.index].title.clone()).collect()
}

fn go(items: &[Item], q: &str) -> Vec<String> {
    titles(items, &rank(items, q, &[], 50, &|| false).unwrap().hits)
}

/// General + Source "webapp" (2 Worktrees) with a named Agent Pane.
fn workspace() -> (State, String) {
    let mut ws = State::new(Path::new("/home/u"));
    let p = ws.add_source(Path::new("/code/webapp"), None).unwrap();
    ws.add_worktree(&p, "feat-login", Path::new("/wt/feat-login"), None)
        .unwrap();
    let main = ws.source(&p).unwrap().worktrees[0].id.clone();
    let (tab, pane) = ws.new_tab(&p, &main, None, None).unwrap();
    ws.rename_tab(&tab, "reviewers").unwrap();
    ws.rename_pane(&pane, Some("rev")).unwrap();
    ws.set_status(
        &pane,
        Some(AgentStatus::Waiting),
        None,
        Some("codex".into()),
        1,
    )
    .unwrap();
    (ws, pane)
}

fn items() -> Vec<Item> {
    let (ws, _) = workspace();
    let kb = Config::default().keybindings();
    model_items(&ws, &kb, &["claude".into(), "codex".into()])
}

#[test]
fn r14_3_query_prefix_picks_the_category() {
    assert_eq!(parse_query(">  split "), (Filter::Actions, "split"));
    assert_eq!(parse_query("/web"), (Filter::Sources, "web"));
    assert_eq!(parse_query("@rev"), (Filter::Panes, "rev"));
    assert_eq!(parse_query("#main.rs"), (Filter::Files, "main.rs"));
    assert_eq!(parse_query(" rev"), (Filter::All, "rev"));
    assert_eq!(parse_query(""), (Filter::All, ""));
    assert_eq!(with_filter("@rev", Filter::Files), "#rev");
    assert_eq!(with_filter("#rev", Filter::All), "rev");
    assert_eq!(Filter::All.cycle(1), Filter::Actions);
    assert_eq!(Filter::All.cycle(-1), Filter::Symbols);
}

#[test]
fn r14_1_lists_panes_tabs_worktrees_sources_actions() {
    let items = items();
    let kinds = |k: Kind| items.iter().filter(|i| i.kind == k).count();
    assert_eq!(
        (kinds(Kind::Pane), kinds(Kind::Tab)),
        (1, 1),
        "rev in reviewers"
    );
    // R107.3: a main Worktree is its Source's row; General is its root.
    assert_eq!(kinds(Kind::Worktree), 1, "feat-login of webapp");
    assert_eq!(kinds(Kind::Source), 2, "webapp + the Workspace root");
    let rev = items.iter().find(|i| i.title == "rev").unwrap();
    assert_eq!(rev.kind, Kind::Pane);
    assert!(
        rev.detail.contains("codex · waiting · reviewers · webapp"),
        "{}",
        rev.detail
    );
    let web = items.iter().find(|i| i.title == "webapp").unwrap();
    assert_eq!(web.keys.as_deref(), Some("Alt+1"), "Source N key");
    let tree = items
        .iter()
        .find(|i| i.title == "Toggle Right Panel")
        .unwrap();
    assert_eq!(tree.keys.as_deref(), Some("Ctrl+Shift+E"));
    assert!(items.iter().any(|i| i.title == "Spawn codex"
        && i.target
            == Target::Action {
                id: "spawn:codex".into()
            }));
    assert!(items
        .iter()
        .any(|i| i.title == "Mark All Notifications Read" && i.keys.is_none()));
    assert!(
        !items.iter().any(|i| i.title.contains("Palette")),
        "not itself"
    );
}

#[test]
fn r14_1_every_keybinding_action_is_offered() {
    for a in bound_actions() {
        assert!(
            ACTIONS.iter().any(|(id, _)| *id == a),
            "{a} missing from ACTIONS"
        );
    }
}

#[test]
fn r14_1_panes_match_by_name_agent_and_status() {
    let items = items();
    assert_eq!(go(&items, "@rev").first().map(String::as_str), Some("rev"));
    assert_eq!(
        go(&items, "@codex").first().map(String::as_str),
        Some("rev")
    );
    assert_eq!(
        go(&items, "@waiting").first().map(String::as_str),
        Some("rev")
    );
    // The filter keeps other kinds out.
    assert!(go(&items, "@split").is_empty());
    assert_eq!(
        go(&items, ">sr").first().map(String::as_str),
        Some("Split Right")
    );
    assert_eq!(
        go(&items, "/feat").first().map(String::as_str),
        Some("feat-login")
    );
}

#[test]
fn r14_3_ranking_prefers_word_starts_and_highlights_the_title() {
    let items = items();
    let hits = rank(&items, ">rig panel", &[], 10, &|| false).unwrap().hits;
    assert_eq!(items[hits[0].index].title, "Toggle Right Panel");
    // "Rig" of Right and "Panel", as title char positions.
    assert_eq!(hits[0].positions, vec![7, 8, 9, 13, 14, 15, 16, 17]);
    assert!(go(&items, ">zzzq").is_empty());
}

#[test]
fn r14_3_empty_query_lists_recent_first_then_by_kind_without_files() {
    let mut items = items();
    items.push(Item::file(Path::new("/w"), "/w/src/main.rs".into()));
    let mut recent = vec![];
    remember(&mut recent, "action:zoom".into());
    remember(&mut recent, "source:webapp".into());
    let hits = rank(&items, "", &recent, 500, &|| false).unwrap().hits;
    let t = titles(&items, &hits);
    assert_eq!(&t[..2], ["webapp", "Toggle Zoom"], "most recent first");
    assert_eq!(items[hits[2].index].kind, Kind::Pane, "then Panes");
    assert!(!hits.iter().any(|h| items[h.index].kind == Kind::File));
    assert_eq!(go(&items, "#"), ["main.rs"], "files listed under #");
    // Filtered empty query keeps recent-first within the category.
    assert_eq!(go(&items, ">")[0], "New Tab");
    let t = titles(
        &items,
        &rank(&items, ">", &recent, 5, &|| false).unwrap().hits,
    );
    assert_eq!(t[0], "Toggle Zoom");
}

#[test]
fn r14_3_recent_items_win_ties() {
    let items = items();
    let q = ">focus pane";
    let plain = titles(&items, &rank(&items, q, &[], 4, &|| false).unwrap().hits);
    let recent = vec!["action:focus_down".to_string()];
    let boosted = titles(
        &items,
        &rank(&items, q, &recent, 4, &|| false).unwrap().hits,
    );
    assert_ne!(plain[0], "Focus Pane Down");
    assert_eq!(boosted[0], "Focus Pane Down");
}

#[test]
fn r14_3_remember_moves_to_front_and_caps() {
    let mut r = vec![];
    for i in 0..40 {
        remember(&mut r, format!("k{i}"));
    }
    remember(&mut r, "k35".into());
    assert_eq!(r.len(), RECENT_MAX);
    assert_eq!(&r[..2], ["k35", "k39"]);
}

#[test]
fn r14_1_files_match_on_the_relative_path() {
    let root = Path::new("/w");
    let items: Vec<Item> = [
        "src/main.rs",
        "src/model/main_view.rs",
        "docs/PRD.md",
        "README.md",
    ]
    .iter()
    .map(|p| Item::file(root, root.join(p)))
    .collect();
    let hits = rank(&items, "#main", &[], 10, &|| false).unwrap().hits;
    assert_eq!(titles(&items, &hits), ["main.rs", "main_view.rs"]);
    assert_eq!(
        hits[0].positions,
        vec![0, 1, 2, 3],
        "highlight in the file name"
    );
    assert_eq!(items[hits[1].index].detail, "src/model");
    assert_eq!(go(&items, "#src/mo mv")[0], "main_view.rs");
    assert_eq!(go(&items, "#prd")[0], "PRD.md", "smart case");
    assert!(go(&items, "#Prd").is_empty(), "upper case is exact");
}

#[test]
fn r14_1_vietnamese_marks_fold_both_ways() {
    assert_eq!(fold("Tiếng Việt đã"), "Tieng Viet da");
    let items = vec![
        Item::new(
            Kind::Pane,
            Target::Pane { pane: "p_1".into() },
            "Tiếng Việt",
            "",
            None,
        ),
        Item::new(
            Kind::Pane,
            Target::Pane { pane: "p_2".into() },
            "Tieng Anh",
            "",
            None,
        ),
    ];
    assert_eq!(go(&items, "viet"), ["Tiếng Việt"]);
    assert_eq!(go(&items, "Việt"), ["Tiếng Việt"]);
    assert_eq!(go(&items, "tiếng")[..2], ["Tieng Anh", "Tiếng Việt"]);
}

#[test]
fn r14_5_cancelled_ranking_stops() {
    let items: Vec<Item> = (0..10_000)
        .map(|i| Item::file(Path::new("/w"), format!("/w/f{i}.rs").into()))
        .collect();
    assert!(rank(&items, "#f1", &[], 10, &|| true).is_none());
    let r = rank(&items, "#f1", &[], 10, &|| false).unwrap();
    assert_eq!(r.hits.len(), 10);
    assert!(r.total > 1000, "every f…1… name matches");
}

#[test]
fn r14_1_file_items_respect_gitignore_and_cap() {
    let d = std::env::temp_dir().join(format!("rade-palette-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for f in ["src/a.rs", "src/b.rs", "target/x.o", ".hidden", ".git/HEAD"] {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    std::fs::write(d.join(".gitignore"), "target/\n").unwrap();
    let (files, cut) = file_items(&d, 100);
    let names: Vec<String> = files.iter().map(|i| i.target.key()).collect();
    let rel: Vec<String> = names
        .iter()
        .map(|k| {
            k.trim_start_matches(&format!("file:{}/", d.display()))
                .to_string()
        })
        .collect();
    assert_eq!(rel, [".gitignore", ".hidden", "src/a.rs", "src/b.rs"]);
    assert!(!cut);
    let (files, cut) = file_items(&d, 2);
    assert_eq!((files.len(), cut), (2, true));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn r14_1_accelerator_labels() {
    assert_eq!(accel_label("<Ctrl><Shift>p"), "Ctrl+Shift+P");
    assert_eq!(accel_label("<Ctrl><Shift>Return"), "Ctrl+Shift+Enter");
    assert_eq!(accel_label("<Ctrl><Shift>Page_Down"), "Ctrl+Shift+PgDn");
    assert_eq!(accel_label("<Alt>1"), "Alt+1");
    assert_eq!(accel_label("<Primary>F5"), "Ctrl+F5");
}

#[test]
fn r14_3_title_matches_rank_above_detail_matches() {
    let pane = |id: &str, title: &str, detail: &str| {
        Item::new(
            Kind::Pane,
            Target::Pane { pane: id.into() },
            title,
            detail,
            None,
        )
    };
    let items = vec![
        pane("p_1", "rev", "codex · waiting · Tab 1 · webapp"),
        pane("p_2", "codex", "Tab 2 · webapp"),
    ];
    assert_eq!(go(&items, "codex"), ["codex", "rev"]);
    // Kind words are not in the details: "/proj" is not every Source.
    let items = items_for_general_only();
    assert!(go(&items, "/proj").is_empty(), "{:?}", go(&items, "/proj"));
}

fn items_for_general_only() -> Vec<Item> {
    let ws = State::new(Path::new("/home/u"));
    model_items(&ws, &Config::default().keybindings(), &[])
}

#[test]
fn r14_3_special_characters_are_plain_text() {
    let items = vec![
        Item::new(
            Kind::Action,
            Target::Action { id: "a".into() },
            "fix! it",
            "",
            None,
        ),
        Item::new(
            Kind::Action,
            Target::Action { id: "b".into() },
            "fix it",
            "",
            None,
        ),
    ];
    assert_eq!(go(&items, "fix!"), ["fix! it"], "! is not negation");
    assert_eq!(
        go(&items, "^it"),
        Vec::<String>::new(),
        "^ is not an anchor"
    );
}

#[test]
fn r14_3_long_detail_matches_never_outrank_title_matches() {
    let long = "alpha beta gamma delta epsilon zeta eta theta";
    let items = vec![
        Item::new(
            Kind::Pane,
            Target::Pane { pane: "p_1".into() },
            "x",
            long,
            None,
        ),
        Item::new(
            Kind::Pane,
            Target::Pane { pane: "p_2".into() },
            long,
            "",
            None,
        ),
    ];
    let r = rank(&items, long, &[], 5, &|| false).unwrap();
    assert_eq!(r.hits[0].index, 1);
    assert!(r.hits[0].title && !r.hits[1].title);
}

#[test]
fn r14_3_recent_keys_of_closed_things_are_pruned() {
    let (ws, pane) = workspace();
    let mut recent = vec![
        format!("pane:{pane}"),
        "pane:p_999".into(),
        "tab:t_999".into(),
        "source:webapp".into(),
        "source:gone".into(),
        "worktree:webapp/w_999".into(),
        "action:zoom".into(),
        "file:/x/y.rs".into(),
    ];
    prune_recent(&ws, &mut recent);
    assert_eq!(
        recent,
        [
            format!("pane:{pane}"),
            "source:webapp".into(),
            "action:zoom".into(),
            "file:/x/y.rs".into()
        ]
    );
    assert!(!key_exists(&ws, "garbage"));
}

#[test]
fn r14_3_status_line() {
    assert_eq!(
        status_line(0, 0, "zz", Files::Indexing),
        "No matches · indexing files…"
    );
    assert_eq!(status_line(1, 1, ">x", Files::Indexing), "1 result");
    assert_eq!(
        status_line(60, 812, "#f", Files::Listed(1001, false)),
        "60 of 812 results · 1001 files"
    );
    assert_eq!(
        status_line(3, 3, "", Files::Listed(4, false)),
        "3 results",
        "no file note without text"
    );
    assert_eq!(
        status_line(2, 2, "rev", Files::Off),
        "2 results · no file search in General"
    );
    assert_eq!(
        status_line(5, 5, "#a", Files::Listed(100_000, true)),
        "5 results · first 100000 files searched"
    );
}

#[test]
fn r14_4_keyboard_navigation_wraps_and_pages_clamp() {
    assert_eq!(step(0, 5, "up", 10), Some(4));
    assert_eq!(step(4, 5, "down", 10), Some(0));
    assert_eq!(step(3, 50, "page_up", 10), Some(0));
    assert_eq!(step(45, 50, "page_down", 10), Some(49));
    assert_eq!(step(49, 50, "page_down", 10), Some(49));
    assert_eq!(step(0, 0, "down", 10), None);
    assert_eq!(step(0, 5, "enter", 10), None);
}

#[test]
fn r14_1_file_items_include_symlinked_files() {
    let d = std::env::temp_dir().join(format!("rade-palette-ln-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("AGENTS.md"), "x").unwrap();
    std::os::unix::fs::symlink("AGENTS.md", d.join("CLAUDE.md")).unwrap();
    let (files, _) = file_items(&d, 10);
    let names: Vec<&str> = files.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(names, ["AGENTS.md", "CLAUDE.md"]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn r92_dollar_is_the_symbols_filter() {
    assert_eq!(parse_query("$ greet"), (Filter::Symbols, "greet"));
    assert_eq!(Filter::Symbols.label(), "Symbols");
    assert_eq!(Filter::Files.cycle(1), Filter::Symbols);
    assert_eq!(Filter::Symbols.cycle(1), Filter::All);
    assert_eq!(with_filter("#lib", Filter::Symbols), "$lib");
    let root = Path::new("/w");
    let sym = |name: &str, kind: &str, path: &str, line: u32| {
        Item::symbol(
            root,
            &crate::code::nav::Item {
                path: root.join(path),
                line,
                col: 4,
                name: Some(name.into()),
                kind: Some(kind.into()),
                ..Default::default()
            },
        )
    };
    let items = vec![
        sym("greet", "function", "src/lib.rs", 1),
        sym("Greeter", "struct", "src/lib.rs", 9),
        sym("regenerate", "function", "src/gen.rs", 3),
        Item::new(
            Kind::Action,
            Target::Action { id: "x".into() },
            "greet all",
            "",
            None,
        ),
    ];
    assert_eq!(items[0].detail, "function · src/lib.rs:1");
    assert_eq!(
        items[0].target,
        Target::Symbol {
            path: "/w/src/lib.rs".into(),
            line: 1,
            col: 4
        }
    );
    assert_eq!(items[0].target.key(), "symbol:/w/src/lib.rs:1:4");
    // Only symbols, matched on their names.
    assert_eq!(go(&items, "$gree"), ["greet", "Greeter"]);
    assert_eq!(go(&items, "$"), ["greet", "Greeter", "regenerate"]);
    // Never in All or other filters' results.
    assert_eq!(go(&items, "greet"), ["greet all"]);
    assert_eq!(
        status_line(2, 2, "$gree", Files::Listed(3, false)),
        "2 results"
    );
}

/// Scene D (spec d-palette.md): `shared-ui` in Shop and Billing.
fn scene_d_items() -> Vec<Item> {
    let sc = crate::navigator::tests::scene_a();
    let ahead = |p: &Path| match p.file_name().and_then(|n| n.to_str()) {
        Some("checkout-v2") => 3,
        Some("tokens") => 1,
        _ => 0,
    };
    let canon = |p: &Path| p.to_path_buf();
    let ctx = Links {
        projects: &sc.projects,
        links: &sc.links,
        ahead: &ahead,
        canon: &canon,
        home: Path::new("/home/u"),
    };
    super::items(
        &sc.state,
        &Config::default().keybindings(),
        &["claude".into()],
        &ctx,
    )
}

#[test]
fn palette_link_items_and_actions_have_no_paths_or_ids() {
    let items = scene_d_items();
    let r = rank(&items, "shared", &[], 50, &|| false).unwrap();
    let got: Vec<(Kind, String, String)> = r
        .hits
        .iter()
        .map(|h| {
            let it = &items[h.index];
            (it.kind, it.title.clone(), it.detail.clone())
        })
        .collect();
    assert_eq!(
        got[..2],
        [
            (Kind::Link, "shared-ui".into(), "Shop · develop".into()),
            (
                Kind::Link,
                "shared-ui".into(),
                "Billing · release/2.4 · 1 agent".into()
            ),
        ],
        "{got:?}"
    );
    assert_eq!(
        items[r.hits[0].index].target,
        Target::Link {
            project: "shop".into(),
            source: "shared-ui".into()
        }
    );
    let actions: Vec<&str> = got
        .iter()
        .filter(|(k, _, _)| *k == Kind::Action)
        .map(|(_, t, _)| t.as_str())
        .collect();
    for a in [
        "Add shared-ui to Project…",
        "Change base of shared-ui in Shop…",
        "Change base of shared-ui in Billing…",
        "Fan-out on shared-ui…",
    ] {
        assert!(actions.contains(&a), "{a} in {actions:?}");
    }
    let fan = got
        .iter()
        .find(|(_, t, _)| t == "Fan-out on shared-ui…")
        .unwrap();
    assert_eq!(fan.2, "one prompt, several agents");
    // Sections: links, then the Worktree, then actions.
    let kinds: Vec<Kind> = got.iter().map(|(k, _, _)| *k).collect();
    let wt = kinds.iter().position(|k| *k == Kind::Worktree).unwrap();
    let act = kinds.iter().position(|k| *k == Kind::Action).unwrap();
    assert!(wt < act, "{kinds:?}");
    // The Worktree of a Project link.
    let tokens = items.iter().find(|i| i.title == "tokens").unwrap();
    assert_eq!(tokens.kind, Kind::Worktree);
    assert_eq!(tokens.detail, "Billing · shared-ui ← release/2.4 · ↑1");
    let cv = items.iter().find(|i| i.title == "checkout-v2").unwrap();
    assert_eq!(cv.detail, "Shop · shop-web ← develop · ↑3");
    // Projects.
    let shop = items
        .iter()
        .find(|i| i.kind == Kind::Project && i.title == "Shop");
    assert_eq!(
        shop.map(|i| i.target.clone()),
        Some(Target::Project {
            project: "shop".into()
        })
    );
    // A linked Source has no Source row of its own; Panes by name.
    assert!(!items
        .iter()
        .any(|i| i.kind == Kind::Source && i.title == "shared-ui"));
    let tests = items.iter().find(|i| i.title == "tests").unwrap();
    assert_eq!(tests.kind, Kind::Pane);
    assert!(
        tests.detail.contains("Shop / checkout-v2"),
        "{}",
        tests.detail
    );
    // The Workspace root as `~/…`.
    assert!(items
        .iter()
        .any(|i| i.kind == Kind::Source && i.title == "~/workspaces/acme"));
    // No absolute path, Pane id or General in any title or detail.
    for i in &items {
        for t in [&i.title, &i.detail] {
            assert!(
                !t.contains("/home") && !t.contains("p_") && !t.contains("General"),
                "{:?} {t:?}",
                i.kind
            );
        }
    }
}

#[test]
fn palette_scope_chips_and_sections() {
    let labels: Vec<String> = FILTERS.iter().map(|f| f.chip()).collect();
    assert_eq!(
        labels,
        [
            "All",
            "> Actions",
            "@ Agents",
            "/ Sources",
            "# Files",
            "$ Symbols"
        ]
    );
    assert_eq!(section(Kind::Link), "SOURCES");
    assert_eq!(section(Kind::Worktree), "WORKTREES");
    assert_eq!(section(Kind::Action), "ACTIONS");
    assert_eq!(section(Kind::File), "FILES");
    assert!(Filter::Sources.admits(Kind::Link) && Filter::Sources.admits(Kind::Project));
}

#[test]
fn enter_hint_names_what_enter_does() {
    // Spec D.15: the selected link item reads `Enter open terminal`.
    let link = Target::Link {
        project: "shop".into(),
        source: "shared-ui".into(),
    };
    assert_eq!(link.enter_hint(), "open terminal");
    assert_eq!(
        Target::Project {
            project: "shop".into()
        }
        .enter_hint(),
        "open"
    );
    assert_eq!(Target::Pane { pane: "p".into() }.enter_hint(), "focus");
    assert_eq!(Target::Action { id: "x".into() }.enter_hint(), "run");
    assert_eq!(
        Target::File {
            path: "a.ts".into()
        }
        .enter_hint(),
        "open"
    );
}
