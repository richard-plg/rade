use super::*;
use crate::checkout::Kind;
use crate::linkview::LinkView;
use crate::SplitDir;
use std::collections::HashMap;

pub(crate) const NOW: u64 = 10_000;
pub(crate) const ROOT: &str = "/home/u/workspaces/acme";

/// The mockup data set of screen A (spec a-workspace.md "Content").
pub(crate) struct Scene {
    pub state: State,
    pub projects: Vec<Project>,
    pub links: Vec<LinkView>,
    /// Pane name → id.
    pub panes: HashMap<String, String>,
    /// Worktree name → (source, id).
    pub worktrees: HashMap<String, (String, String)>,
    pub branches: Branches,
}

pub(crate) fn root() -> PathBuf {
    PathBuf::from(ROOT)
}

fn project(id: &str, name: &str, links: &[(&str, &str)]) -> Project {
    let mut t = format!("name = \"{name}\"\n");
    for (s, b) in links {
        t += &format!("\n[[source]]\nid = \"{s}\"\nbase = \"{b}\"\n");
    }
    let p = Project::parse(id, &Project::path(&root(), id), &t);
    assert!(p.error.is_none(), "{p:?}");
    p
}

pub(crate) fn link(
    project: (&str, &str),
    source: &str,
    base: &str,
    kind: Kind,
    path: PathBuf,
) -> LinkView {
    LinkView {
        project: project.0.into(),
        project_name: project.1.into(),
        source: source.into(),
        base: base.into(),
        kind: Some(kind),
        path: Some(path),
        create: false,
        refused: None,
        head: Some(base.into()),
        ahead: Some(0),
        behind: Some(0),
        dirty: 0,
        drift: None,
        also_in: vec![],
        shared_with: vec![],
    }
}

pub(crate) fn scene_a() -> Scene {
    let home = Path::new("/home/u");
    let mut w = State::new(home);
    let (ws, _) = w.add_workspace(&root(), "acme").unwrap();
    w.switch_workspace(&ws).unwrap();
    for s in ["shop-web", "shop-api", "shared-ui", "billing"] {
        w.add_source_in(&root().join(s), None, Some(&ws)).unwrap();
    }
    let mut panes = HashMap::new();
    let mut worktrees = HashMap::new();
    let main = |w: &State, s: &str| w.source(s).unwrap().worktrees[0].id.clone();
    // Scratch: two shells at the Workspace root.
    let general = crate::general_id(&ws);
    let gw = main(&w, &general);
    let (_, a) = w.new_tab(&general, &gw, None, None).unwrap();
    w.split(&a, SplitDir::Right, None, None).unwrap();
    // dev-server on shop-web's main checkout.
    let mw = main(&w, "shop-web");
    let (_, dev) = w
        .new_tab("shop-web", &mw, None, Some("dev-server".into()))
        .unwrap();
    w.set_status(
        &dev,
        Some(AgentStatus::Done),
        None,
        Some("claude".into()),
        NOW - 300,
    )
    .unwrap();
    panes.insert("dev-server".to_string(), dev);
    let mut wt = |w: &mut State, src: &str, name: &str, project: &str, base: &str| {
        let path = root().join(".worktrees-dev").join(src).join(name);
        let (s, id) = w.add_worktree(src, name, &path, None).unwrap();
        w.set_worktree_origin(&s, &id, Some(project.into()), Some(base.into()))
            .unwrap();
        worktrees.insert(name.to_string(), (s.clone(), id.clone()));
        (s, id)
    };
    let (s, id) = wt(&mut w, "shop-web", "checkout-v2", "shop", "develop");
    let (_, planner) = w.new_tab(&s, &id, None, Some("planner".into())).unwrap();
    let tests = w
        .split(&planner, SplitDir::Right, None, Some("tests".into()))
        .unwrap();
    w.set_status(
        &planner,
        Some(AgentStatus::Done),
        None,
        Some("claude".into()),
        NOW - 12,
    )
    .unwrap();
    w.set_status(
        &tests,
        Some(AgentStatus::Waiting),
        None,
        Some("codex".into()),
        NOW - 5,
    )
    .unwrap();
    w.select_worktree(&s, &id).unwrap();
    panes.insert("planner".to_string(), planner);
    panes.insert("tests".to_string(), tests);
    let (s, id) = wt(&mut w, "shared-ui", "tokens", "billing", "release/2.4");
    let (_, ui) = w.new_tab(&s, &id, None, Some("ui".into())).unwrap();
    w.set_status(
        &ui,
        Some(AgentStatus::Running),
        None,
        Some("claude".into()),
        NOW - 134,
    )
    .unwrap();
    panes.insert("ui".to_string(), ui);
    for name in ["shop", "billing", "docs"] {
        w.note_projects(&ws, &[name.to_string()]);
    }
    let shop = ("shop", "Shop");
    let billing = ("billing", "Billing");
    let projects = vec![
        project(
            "shop",
            "Shop",
            &[
                ("shop-web", "develop"),
                ("shop-api", "develop"),
                ("shared-ui", "develop"),
            ],
        ),
        project(
            "billing",
            "Billing",
            &[("billing", "main"), ("shared-ui", "release/2.4")],
        ),
        project("docs", "Docs", &[]),
    ];
    let pc = |p: &str, s: &str| root().join(".worktrees-dev/.projects").join(p).join(s);
    let mut api = link(
        shop,
        "shop-api",
        "develop",
        Kind::Main,
        root().join("shop-api"),
    );
    api.head = Some("feat/checkout-v2".into());
    api.ahead = Some(4);
    api.dirty = 3;
    api.drift = crate::drift::drift(
        "develop",
        &crate::drift::Head::Branch("feat/checkout-v2".into()),
        3,
    );
    let mut ui_shop = link(
        shop,
        "shared-ui",
        "develop",
        Kind::Project,
        pc("shop", "shared-ui"),
    );
    ui_shop.also_in = vec![("Billing".into(), "release/2.4".into())];
    let mut web = link(
        shop,
        "shop-web",
        "develop",
        Kind::Main,
        root().join("shop-web"),
    );
    web.behind = Some(2);
    let mut ui_billing = link(
        billing,
        "shared-ui",
        "release/2.4",
        Kind::Project,
        pc("billing", "shared-ui"),
    );
    ui_billing.also_in = vec![("Shop".into(), "develop".into())];
    let links = vec![
        web,
        api,
        ui_shop,
        link(
            billing,
            "billing",
            "main",
            Kind::Main,
            root().join("billing"),
        ),
        ui_billing,
    ];
    Scene {
        state: w,
        projects,
        links,
        panes,
        worktrees,
        branches: Branches::default(),
    }
}

impl Scene {
    pub(crate) fn rows(&self) -> Vec<Row> {
        let facts = |p: &Path| -> (usize, usize) {
            match p.file_name().and_then(|n| n.to_str()) {
                Some("checkout-v2") => (3, 4),
                Some("tokens") => (1, 0),
                _ => (0, 0),
            }
        };
        let dev = self.panes["dev-server"].clone();
        let port = move |p: &str| (p == dev).then_some(5173);
        let canon = |p: &Path| p.to_path_buf();
        let sel = selection(&self.state, &self.links, None, &canon);
        let input = Input {
            projects: &self.projects,
            links: &self.links,
            worktree_facts: &facts,
            port: &port,
            branches: &self.branches,
            canon: &canon,
            home: Path::new("/home/u"),
            selected: sel.as_ref(),
            now: NOW,
        };
        build(&self.state, &input)
    }

    pub(crate) fn fold(&mut self, key: &str) {
        self.state.collapsed_projects.push(key.into());
    }
}

fn find<'a>(rows: &'a [Row], title: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.title == title)
        .unwrap_or_else(|| panic!("no row {title}: {}", text(rows)))
}

#[test]
fn r104_d_collapsed_parent_carries_worst_led_and_waiting_badge() {
    let mut sc = scene_a();
    let open = sc.rows();
    let shop = find(&open, "Shop");
    assert_eq!(shop.collapsed, Some(false));
    assert_eq!((shop.led, shop.badge), (None, 0), "expanded: no aggregate");
    assert!(open.iter().any(|r| r.title == "tests"));
    sc.fold("acme/shop");
    let folded = sc.rows();
    let shop = find(&folded, "Shop");
    assert_eq!(shop.collapsed, Some(true));
    assert_eq!(shop.led, Some(AgentStatus::Waiting));
    assert_eq!(shop.badge, 1);
    assert!(
        !folded.iter().any(|r| r.title == "tests"),
        "children hidden"
    );
    // A collapsed link aggregates its own subtree only.
    let mut sc = scene_a();
    sc.fold("acme/shop/shop-web");
    let rows = sc.rows();
    let web = rows.iter().find(|r| r.key == "acme/shop/shop-web").unwrap();
    assert_eq!((web.led, web.badge), (Some(AgentStatus::Waiting), 1));
    assert_eq!(find(&rows, "Shop").badge, 0);
}

#[test]
fn r104_d_nested_folds_keep_the_waiting_agent_in_the_aggregate() {
    // Fold the link first, then its Project: the Project still carries the
    // waiting `tests` below the collapsed link.
    let mut sc = scene_a();
    sc.fold("acme/shop/shop-web");
    sc.fold("acme/shop");
    let rows = sc.rows();
    let shop = find(&rows, "Shop");
    assert_eq!((shop.led, shop.badge), (Some(AgentStatus::Waiting), 1));
    // A collapsed link above a collapsed Worktree.
    let mut sc = scene_a();
    let wt = sc
        .rows()
        .into_iter()
        .find(|r| r.title == "checkout-v2")
        .unwrap()
        .key;
    sc.fold(&wt);
    sc.fold("acme/shop/shop-web");
    let rows = sc.rows();
    let web = rows.iter().find(|r| r.key == "acme/shop/shop-web").unwrap();
    assert_eq!((web.led, web.badge), (Some(AgentStatus::Waiting), 1));
    // All three levels folded.
    sc.fold("acme/shop");
    let shop = find(&sc.rows(), "Shop").clone();
    assert_eq!((shop.led, shop.badge), (Some(AgentStatus::Waiting), 1));
}

#[test]
fn r104_e_scene_a_tree_matches_golden_text() {
    let got = text(&scene_a().rows());
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/navigator-a.txt");
    if std::env::var_os("RADE_BLESS").is_some() {
        std::fs::write(&file, &got).unwrap();
    }
    let want = std::fs::read_to_string(&file).unwrap();
    for (i, (g, w)) in got.lines().zip(want.lines()).enumerate() {
        assert_eq!(g, w, "line {}", i + 1);
    }
    assert_eq!(got.lines().count(), want.lines().count(), "{got}");
}

#[test]
fn scene_a_rows_kinds_depths_and_labels() {
    let rows = scene_a().rows();
    let shop: Vec<(RowKind, u8, String)> = rows
        .iter()
        .skip_while(|r| r.title != "Shop")
        .take_while(|r| r.title != "Billing")
        .map(|r| (r.kind, r.depth, r.label()))
        .collect();
    use RowKind::*;
    let want = [
        (Project, 0, "Shop"),
        (Link, 1, "shop-web develop"),
        (Agent, 2, "dev-server"),
        (Worktree, 2, "checkout-v2"),
        (Agent, 3, "planner"),
        (Agent, 3, "tests"),
        (Link, 1, "shop-api"),
        (Link, 1, "shared-ui develop"),
    ];
    let want: Vec<(RowKind, u8, String)> = want
        .iter()
        .map(|(k, d, t)| (*k, *d, t.to_string()))
        .collect();
    assert_eq!(shop, want);
    let api = rows.iter().find(|r| r.key == "acme/shop/shop-api").unwrap();
    assert!(api.drift);
    assert_eq!(api.meta, "feat/checkout-v2");
    let web = rows.iter().find(|r| r.key == "acme/shop/shop-web").unwrap();
    assert_eq!(web.also_in, None, "linked only by Shop: no marker");
    let ui = rows
        .iter()
        .find(|r| r.key == "acme/shop/shared-ui")
        .unwrap();
    assert_eq!(ui.also_in.as_deref(), Some("Also in Billing @ release/2.4"));
    let ui = rows
        .iter()
        .find(|r| r.key == "acme/billing/shared-ui")
        .unwrap();
    assert_eq!(ui.also_in.as_deref(), Some("Also in Shop @ develop"));
    assert_eq!(find(&rows, "dev-server").meta, ":5173");
    assert_eq!(find(&rows, "planner").meta, "claude");
    assert_eq!(find(&rows, "tests").meta, "needs input");
    assert_eq!(find(&rows, "ui").meta, "2m");
    assert_eq!(find(&rows, "checkout-v2").meta, "↑3 ●4");
    // The focused `tests` Agent row, not its Worktree (R104.6).
    assert!(find(&rows, "tests").selected && !find(&rows, "checkout-v2").selected);
    assert!(find(&rows, "Docs").dim);
    assert_eq!(find(&rows, "Docs").collapsed, None);
    let colors: Vec<Option<usize>> = ["Shop", "Billing", "Docs"]
        .iter()
        .map(|t| find(&rows, t).color)
        .collect();
    assert_eq!(colors, [Some(0), Some(1), Some(2)]);
    let scratch = rows.iter().position(|r| r.kind == Section).unwrap();
    let tail: Vec<String> = rows[scratch..].iter().map(|r| r.label()).collect();
    assert_eq!(tail, ["Scratch", "~/workspaces/acme"]);
    assert_eq!(rows[scratch + 1].meta, "2");
}

#[test]
fn scratch_holds_root_unlinked_sources_and_free_worktrees() {
    let mut sc = scene_a();
    // `spike` of shop-web with no recorded Project (imported).
    let (s, id) = sc
        .state
        .add_worktree("shop-web", "spike", &root().join("spike"), None)
        .unwrap();
    sc.branches.current.insert(id.clone(), "spike".into());
    // A Source no Project links.
    sc.state
        .add_source_in(&root().join("notes"), None, Some("acme"))
        .unwrap();
    let rows = sc.rows();
    let scratch = rows
        .iter()
        .position(|r| r.kind == RowKind::Section)
        .unwrap();
    let tail: Vec<String> = rows[scratch..].iter().map(|r| r.label()).collect();
    assert_eq!(tail, ["Scratch", "~/workspaces/acme", "notes", "spike"]);
    let spike = find(&rows, "spike");
    assert_eq!(spike.suggest, Some(("shop".into(), "Shop".into())));
    assert_eq!(
        spike.target,
        Target::Worktree {
            source: s,
            worktree: id
        }
    );
    assert!(rows
        .iter()
        .all(|r| r.title != "General" && r.title != "No Project"));
    assert!(!rows[..scratch].iter().any(|r| r.title == "spike"));
}

#[test]
fn main_checkout_agents_show_under_each_link_using_it_and_project_checkout_panes_under_theirs() {
    let mut sc = scene_a();
    // A second Project linking shop-web at develop shares the main checkout.
    sc.projects
        .push(project("ops", "Ops", &[("shop-web", "develop")]));
    sc.state.note_projects("acme", &["ops".into()]);
    sc.links.push(link(
        ("ops", "Ops"),
        "shop-web",
        "develop",
        Kind::Main,
        root().join("shop-web"),
    ));
    // A Terminal opened on Shop's shared-ui link (its Project checkout).
    let mw = sc.state.source("shared-ui").unwrap().worktrees[0]
        .id
        .clone();
    let (_, p) = sc
        .state
        .new_tab("shared-ui", &mw, None, Some("shop/shared-ui".into()))
        .unwrap();
    sc.state.set_pane_cwd(
        &p,
        &root().join(".worktrees-dev/.projects/shop/shared-ui/src"),
    );
    sc.state
        .set_status(&p, None, None, Some("claude".into()), NOW)
        .unwrap();
    let rows = sc.rows();
    let devs: Vec<&String> = rows
        .iter()
        .filter(|r| r.title == "dev-server")
        .map(|r| &r.key)
        .collect();
    assert_eq!(devs.len(), 2, "{}", text(&rows));
    let i = rows
        .iter()
        .position(|r| r.key == "acme/shop/shared-ui")
        .unwrap();
    assert_eq!(rows[i + 1].title, "shop/shared-ui");
    assert_eq!(rows[i + 1].depth, 2);
    assert_eq!(
        rows.iter().filter(|r| r.title == "shop/shared-ui").count(),
        1
    );
    // shared-ui's main checkout is used by no link; its one Pane is the
    // link's, so it gets no Scratch row; a shell in the main checkout does.
    let scratch = rows
        .iter()
        .position(|r| r.kind == RowKind::Section)
        .unwrap();
    assert!(!rows[scratch..].iter().any(|r| r.title == "shared-ui"));
    sc.state.new_tab("shared-ui", &mw, None, None).unwrap();
    let rows = sc.rows();
    let scratch = rows
        .iter()
        .position(|r| r.kind == RowKind::Section)
        .unwrap();
    let ui = rows[scratch..]
        .iter()
        .find(|r| r.title == "shared-ui")
        .unwrap();
    assert_eq!(ui.meta, "1");
}

#[test]
fn alt_n_follows_project_order() {
    let mut sc = scene_a();
    let ids = sc.state.alt_projects(&sc.projects);
    assert_eq!(ids, ["shop", "billing", "docs"]);
    // Unseen Projects come after, by id; noting is idempotent.
    let mut more = sc.projects.clone();
    more.push(project("api", "API", &[]));
    assert_eq!(
        sc.state.alt_projects(&more),
        ["shop", "billing", "docs", "api"]
    );
    assert!(!sc.state.note_projects("acme", &["shop".into()]));
    assert!(sc.state.note_projects("acme", &["api".into()]));
    let back: State = serde_json::from_str(&serde_json::to_string(&sc.state).unwrap()).unwrap();
    assert_eq!(
        back.workspace("acme").unwrap().project_order,
        ["shop", "billing", "docs", "api"]
    );
}

#[test]
fn no_projects_shows_the_empty_line_and_every_source_in_scratch() {
    let mut sc = scene_a();
    sc.projects.clear();
    sc.links.clear();
    let rows = sc.rows();
    assert_eq!(rows[0].kind, RowKind::Empty);
    assert_eq!(rows[0].title, "No Projects yet");
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    for s in [
        "shop-web",
        "shop-api",
        "shared-ui",
        "billing",
        "checkout-v2",
        "tokens",
    ] {
        assert!(titles.contains(&s), "{s} missing: {titles:?}");
    }
}

#[test]
fn drift_kept_hides_the_marker() {
    let mut sc = scene_a();
    let mut keep = crate::linkview::KeepSet::default();
    keep.keep("shop", "shop-api", "feat/checkout-v2");
    keep.apply(&mut sc.links);
    let rows = sc.rows();
    let api = rows.iter().find(|r| r.key == "acme/shop/shop-api").unwrap();
    assert!(!api.drift);
    assert_eq!(api.label(), "shop-api develop");
}

#[test]
fn r111_c_navigator_splice_replaces_only_changed_rows() {
    let old: Vec<u32> = (0..260).collect();
    // Equal lists: nothing to do.
    assert_eq!(splice(&old, &old), None);
    // One row changed (an Agent's status): only that row.
    let mut one = old.clone();
    one[137] = 9_999;
    assert_eq!(splice(&old, &one), Some((137, 1, 1)));
    // A collapsed parent and its Agent both changed: the rows between them.
    let mut two = old.clone();
    two[40] = 1;
    two[44] = 2;
    assert_eq!(splice(&old, &two), Some((40, 5, 5)));
    // Rows inserted (a Worktree unfolded) and removed (folded again).
    let mut ins = old.clone();
    ins.splice(10..10, [500, 501, 502]);
    assert_eq!(splice(&old, &ins), Some((10, 0, 3)));
    assert_eq!(splice(&ins, &old), Some((10, 3, 0)));
    // Repeated values: the prefix and the suffix never overlap.
    assert_eq!(splice(&[1, 1, 1], &[1, 1]), Some((2, 1, 0)));
    assert_eq!(splice(&[1, 1], &[1, 1, 1]), Some((2, 0, 1)));
    assert_eq!(splice::<u32>(&[], &[1, 2]), Some((0, 0, 2)));
    assert_eq!(splice::<u32>(&[1, 2], &[]), Some((0, 2, 0)));
    // Applying the answer gives the new list.
    for (a, b) in [(&old, &one), (&old, &two), (&old, &ins), (&ins, &old)] {
        let (s, r, i) = splice(a, b).unwrap();
        let mut x = a.clone();
        x.splice(s..s + r, b[s..s + i].iter().copied());
        assert_eq!(&x, b);
    }
}

#[test]
fn r104_j_terminal_in_a_project_checkout_selects_its_link() {
    let mut sc = scene_a();
    // Shop's shared-ui link is on its Project checkout; its Terminal lives
    // in the Source's main Worktree with its cwd there (open_on_link).
    let mw = sc.state.source("shared-ui").unwrap().worktrees[0]
        .id
        .clone();
    let (_, p) = sc
        .state
        .new_tab("shared-ui", &mw, None, Some("shop/shared-ui".into()))
        .unwrap();
    sc.state.set_pane_cwd(
        &p,
        &root().join(".worktrees-dev/.projects/shop/shared-ui/src"),
    );
    // A shell in the main checkout makes shared-ui a Scratch row too.
    let (_, shell) = sc.state.new_tab("shared-ui", &mw, None, None).unwrap();
    sc.state.focus_pane(&p).unwrap();
    let canon = |p: &Path| p.to_path_buf();
    let link = Target::Link {
        project: "shop".into(),
        source: "shared-ui".into(),
    };
    assert_eq!(selection(&sc.state, &sc.links, None, &canon), Some(link));
    let rows = sc.rows();
    let sel: Vec<&str> = rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.key.as_str())
        .collect();
    assert_eq!(sel, ["acme/shop/shared-ui"], "{}", text(&rows));
    // The shell in the main checkout no link uses: its Scratch row.
    sc.state.focus_pane(&shell).unwrap();
    assert_eq!(
        selection(&sc.state, &sc.links, None, &canon),
        Some(Target::Scratch {
            source: "shared-ui".into()
        })
    );
}

#[test]
fn r104_k_remove_from_scratch_closes_only_unclaimed_main_checkout_panes() {
    let mut sc = scene_a();
    let canon = |p: &Path| p.to_path_buf();
    let mw = sc.state.source("shared-ui").unwrap().worktrees[0]
        .id
        .clone();
    let (_, linked) = sc
        .state
        .new_tab("shared-ui", &mw, None, Some("shop/shared-ui".into()))
        .unwrap();
    sc.state.set_pane_cwd(
        &linked,
        &root().join(".worktrees-dev/.projects/shop/shared-ui"),
    );
    let (_, shell) = sc.state.new_tab("shared-ui", &mw, None, None).unwrap();
    let ids = |s: &str, sc: &Scene| -> Vec<String> {
        scratch_panes(sc.state.source(s).unwrap(), &sc.links, &canon)
            .iter()
            .map(|p| p.id.clone())
            .collect()
    };
    assert_eq!(ids("shared-ui", &sc), std::slice::from_ref(&shell));
    // The Workspace root and a link on the main checkout offer nothing.
    assert!(ids(&crate::general_id("acme"), &sc).is_empty());
    assert!(ids("shop-web", &sc).is_empty());
    // A Source no Project links keeps Remove Source… instead.
    sc.links.retain(|l| l.source != "shared-ui");
    assert!(ids("shared-ui", &sc).is_empty());
    sc = scene_a();
    let mw = sc.state.source("shared-ui").unwrap().worktrees[0]
        .id
        .clone();
    let (_, shell) = sc.state.new_tab("shared-ui", &mw, None, None).unwrap();
    // Closing them takes the row out of Scratch.
    for id in ids("shared-ui", &sc) {
        sc.state.close_pane(&id).unwrap();
    }
    let rows = sc.rows();
    let scratch = rows
        .iter()
        .position(|r| r.kind == RowKind::Section)
        .unwrap();
    assert!(
        !rows[scratch..].iter().any(|r| r.title == "shared-ui"),
        "{} still listed after closing {shell}",
        text(&rows)
    );
}

#[test]
fn r115_a_agent_rows_carry_their_agents_glyph_parents_none() {
    let sc = scene_a();
    let rows = sc.rows();
    let mark = |t: &str| find(&rows, t).mark.map(|m| m.as_str());
    assert_eq!(mark("planner"), Some("claude"));
    assert_eq!(mark("tests"), Some("codex"));
    assert_eq!(mark("dev-server"), Some("claude"));
    assert_eq!(mark("Shop"), None);
    assert_eq!(mark("checkout-v2"), None);
}

#[test]
fn r115_e_a_detected_program_makes_a_shell_an_agent_row_until_it_ends() {
    let mut sc = scene_a();
    let wt = sc.worktrees["checkout-v2"].clone();
    let (_, sh) = sc.state.new_tab(&wt.0, &wt.1, None, None).unwrap();
    assert!(!sc.rows().iter().any(|r| r.key == sh), "a shell: no row");
    assert!(sc.state.set_detected(&sh, Some("codex".into())));
    assert!(
        !sc.state.set_detected(&sh, Some("codex".into())),
        "unchanged"
    );
    let rows = sc.rows();
    let r = rows.iter().find(|r| r.key == sh).expect("an Agent row");
    assert_eq!(
        (r.title.as_str(), r.mark.map(|m| m.as_str())),
        ("codex", Some("codex"))
    );
    assert!(sc.state.set_detected(&sh, None));
    assert!(!sc.rows().iter().any(|r| r.key == sh));
}

#[test]
fn r104_o_the_focused_agent_row_is_selected_else_its_checkout() {
    let mut sc = scene_a();
    let planner = sc.panes["planner"].clone();
    sc.state.focus_pane(&planner).unwrap();
    let rows = sc.rows();
    let sel: Vec<&str> = rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.title.as_str())
        .collect();
    assert_eq!(sel, ["planner"], "the Agent row, not checkout-v2");
    // Its Worktree folded: the row is hidden, the checkout is selected.
    let wt = find(&rows, "checkout-v2").key.clone();
    sc.fold(&wt);
    let rows = sc.rows();
    let sel: Vec<&str> = rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.title.as_str())
        .collect();
    assert_eq!(sel, ["checkout-v2"]);
}

#[test]
fn r104_p_an_unnamed_agent_row_shows_its_session_title() {
    let mut sc = scene_a();
    let wt = sc.worktrees["checkout-v2"].clone();
    let (_, p) = sc.state.new_tab(&wt.0, &wt.1, None, None).unwrap();
    sc.state
        .set_status(&p, None, None, Some("codex".into()), NOW)
        .unwrap();
    let title = |sc: &Scene| sc.rows().iter().find(|r| r.key == p).unwrap().title.clone();
    assert_eq!(title(&sc), "codex", "no title yet: the Preset");
    let raw = crate::title::sanitize("⠋ Kiểm tra tiến trình | checkout-v2").unwrap();
    sc.state.set_pane_title(&p, &raw);
    assert_eq!(title(&sc), "Kiểm tra tiến trình");
    sc.state.set_pane_title(&p, "Codex");
    assert_eq!(title(&sc), "codex", "a product name: the Preset");
}
