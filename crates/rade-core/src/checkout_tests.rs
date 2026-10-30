use super::*;
use crate::drift::{drift, Head};

const ROOT: &str = "/w";
const FOLDER: &str = "/w/.worktrees";

fn id(p: &Path) -> PathBuf {
    p.to_path_buf()
}

fn co(path: &str, branch: &str, dirty: usize) -> Co {
    Co {
        path: path.into(),
        branch: Some(branch.into()),
        commit: "abc1234".into(),
        dirty,
    }
}

/// A repository whose main checkout `<ROOT>/<source>` is on `branch`.
fn repo(source: &str, branch: &str, dirty: usize, local: &[&str], remote: &[&str]) -> Repo {
    let main = format!("{ROOT}/{source}");
    let mut local: BTreeSet<String> = local.iter().map(|s| s.to_string()).collect();
    local.insert(branch.into());
    Repo {
        main: main.clone().into(),
        git: true,
        checkouts: vec![co(&main, branch, dirty)],
        local,
        remote: remote
            .iter()
            .map(|b| (b.to_string(), format!("origin/{b}")))
            .collect(),
        unborn: None,
    }
}

fn link(project: &str, source: &str, base: &str) -> Link {
    Link {
        project: project.into(),
        source: source.into(),
        base: base.into(),
        checkout: Checkout::Auto,
    }
}

fn with(mut l: Link, c: Checkout) -> Link {
    l.checkout = c;
    l
}

fn record(project: &str, source: &str, base: &str, path: &str) -> LinkCheckout {
    LinkCheckout {
        project: project.into(),
        source: source.into(),
        base: base.into(),
        checkout: "auto".into(),
        path: path.into(),
    }
}

fn resolve(
    repos: Vec<(&str, Repo)>,
    links: &[Link],
    records: &[LinkCheckout],
    exists: &[&str],
) -> Vec<Resolved> {
    let exists: BTreeSet<PathBuf> = exists.iter().map(PathBuf::from).collect();
    let ex = move |p: &Path| exists.contains(p);
    let world = World {
        repos: repos.into_iter().map(|(s, r)| (s.to_string(), r)).collect(),
        folder: FOLDER.into(),
        home: "/home/u".into(),
        canon: &id,
        exists: &ex,
    };
    resolve_checkout(links, &world, records)
}

fn by<'a>(out: &'a [Resolved], project: &str, source: &str) -> &'a Resolved {
    out.iter()
        .find(|r| r.project == project && r.source == source)
        .unwrap()
}

fn path(r: &Resolved) -> &str {
    r.path.as_deref().unwrap().to_str().unwrap()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn r98_a_two_projects_two_bases_get_two_project_checkouts() {
    let out = resolve(
        vec![(
            "shared-ui",
            repo("shared-ui", "main", 0, &["develop", "release/2.4"], &[]),
        )],
        &[
            link("shop", "shared-ui", "develop"),
            link("billing", "shared-ui", "release/2.4"),
        ],
        &[],
        &[],
    );
    let shop = by(&out, "shop", "shared-ui");
    let billing = by(&out, "billing", "shared-ui");
    assert_eq!(path(shop), "/w/.worktrees/.projects/shop/shared-ui");
    assert_eq!(path(billing), "/w/.worktrees/.projects/billing/shared-ui");
    assert_eq!(shop.kind, Some(Kind::Project));
    assert!(shop.create && billing.create);
    for (r, base) in [(shop, "develop"), (billing, "release/2.4")] {
        assert_eq!(
            r.commands,
            [Command::AddWorktree {
                repo: "/w/shared-ui".into(),
                dir: r.path.clone().unwrap(),
                branch: base.into(),
                track: None,
            }]
        );
    }
    let on_main = out
        .iter()
        .flat_map(|r| &r.commands)
        .filter(|Command::AddWorktree { dir, .. }| dir == Path::new("/w/shared-ui"))
        .count();
    assert_eq!(on_main, 0, "nothing runs on the main checkout");
}

#[test]
fn r98_b_main_checkout_on_base_is_used_and_nothing_created() {
    let out = resolve(
        vec![("shop-web", repo("shop-web", "develop", 0, &[], &[]))],
        &[link("shop", "shop-web", "develop")],
        &[],
        &[],
    );
    let r = &out[0];
    assert_eq!(r.kind, Some(Kind::Main));
    assert_eq!(path(r), "/w/shop-web");
    assert!(!r.create && r.commands.is_empty() && r.refused.is_none());
    assert_eq!(r.record.as_ref().unwrap().path, Path::new("/w/shop-web"));
    assert_eq!(r.text(Some("develop"), Path::new(ROOT)), "main checkout");
}

#[test]
fn r98_c_same_source_same_base_share_one_checkout() {
    let out = resolve(
        vec![("shared-ui", repo("shared-ui", "main", 0, &["develop"], &[]))],
        &[
            link("shop", "shared-ui", "develop"),
            link("checkout-revamp", "shared-ui", "develop"),
        ],
        &[],
        &[],
    );
    let (a, b) = (
        by(&out, "shop", "shared-ui"),
        by(&out, "checkout-revamp", "shared-ui"),
    );
    assert_eq!(a.path, b.path);
    assert_eq!(a.kind, Some(Kind::Project));
    assert_eq!(a.shared_with, pairs(&[("checkout-revamp", "develop")]));
    assert_eq!(b.shared_with, pairs(&[("shop", "develop")]));
    assert!(a
        .text(Some("main"), Path::new(ROOT))
        .ends_with("shared with checkout-revamp"));
}

#[test]
fn r98_e_checkout_main_held_at_other_base_is_refused() {
    let out = resolve(
        vec![(
            "shop-api",
            repo("shop-api", "release/2.4", 0, &["develop"], &[]),
        )],
        &[
            with(link("shop", "shop-api", "develop"), Checkout::Main),
            link("billing", "shop-api", "release/2.4"),
        ],
        &[record("billing", "shop-api", "release/2.4", "/w/shop-api")],
        &[],
    );
    let shop = by(&out, "shop", "shop-api");
    let e = shop.error().unwrap();
    assert_eq!(e.kind, crate::rpc::ErrorKind::Usage);
    assert!(
        e.message.contains("billing") && e.message.contains("release/2.4"),
        "{e}"
    );
    assert!(shop.commands.is_empty() && shop.record.is_none());
    assert_eq!(by(&out, "billing", "shop-api").kind, Some(Kind::Main));
}

#[test]
fn r98_f_checkout_main_dirty_off_base_refused_without_commands() {
    let out = resolve(
        vec![(
            "shop-api",
            repo("shop-api", "feat/checkout-v2", 3, &["develop"], &[]),
        )],
        &[with(link("shop", "shop-api", "develop"), Checkout::Main)],
        &[],
        &[],
    );
    let r = &out[0];
    let why = r.refused.as_deref().unwrap();
    assert!(why.contains("3 uncommitted changes"), "{why}");
    assert!(
        why.contains("feat/checkout-v2") && why.contains("develop"),
        "{why}"
    );
    assert!(r.commands.is_empty() && r.record.is_none() && r.path.is_none());
}

#[test]
fn r98_g_apply_creates_project_checkout_tracking_remote_only_base() {
    use crate::git::tests::{repo as git_repo, sh};
    let main = git_repo("r98g");
    let remote = main.with_file_name(format!("rade-r98g-remote-{}.git", std::process::id()));
    let _ = std::fs::remove_dir_all(&remote);
    sh(&main, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    sh(
        &main,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    sh(&main, &["switch", "-qc", "develop"]);
    std::fs::write(main.join("d.txt"), "develop\n").unwrap();
    sh(&main, &["add", "-A"]);
    sh(&main, &["commit", "-qm", "develop"]);
    sh(&main, &["push", "-q", "origin", "develop"]);
    sh(&main, &["switch", "-q", "main"]);
    sh(&main, &["branch", "-qD", "develop"]);
    sh(&main, &["fetch", "-q", "origin"]);
    std::fs::write(main.join("a.txt"), "edited\n").unwrap();
    let before_head = crate::git::branch(&main);
    let before_status = crate::git::dirty(&main).unwrap();

    let root = main.with_file_name(format!("rade-r98g-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join(".worktrees");
    let repos = observe(&[("shared-ui".to_string(), main.clone())]);
    let r = &repos["shared-ui"];
    assert!(r.git && !r.local.contains("develop"));
    assert_eq!(
        r.remote.get("develop").map(String::as_str),
        Some("origin/develop")
    );
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let ex = |p: &Path| p.exists();
    let world = World {
        repos,
        folder: folder.clone(),
        home: root.clone(),
        canon: &canon,
        exists: &ex,
    };
    let plan = resolve_checkout(&[link("shop", "shared-ui", "develop")], &world, &[]);
    let dir = project_checkout_dir(&canon(&folder), "shop", "shared-ui");
    assert_eq!(plan[0].path.as_deref(), Some(dir.as_path()));
    assert_eq!(plan[0].commands.len(), 1);
    for c in &plan[0].commands {
        apply(c).unwrap();
        apply(c).unwrap(); // a second run finds it there
    }
    assert_eq!(crate::git::branch(&dir).as_deref(), Some("develop"));
    let upstream = crate::git::run(&dir, &["rev-parse", "--abbrev-ref", "@{upstream}"]).unwrap();
    assert_eq!(upstream.trim(), "origin/develop");
    assert_eq!(crate::git::branch(&main), before_head);
    assert_eq!(crate::git::dirty(&main).unwrap(), before_status);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&remote);
    let _ = std::fs::remove_dir_all(&main);
}

#[test]
fn r98_i_recorded_main_checkout_kept_when_drifted() {
    let shop_api = repo("shop-api", "feat/checkout-v2", 3, &["develop"], &[]);
    let co = shop_api.checkouts[0].clone();
    let out = resolve(
        vec![("shop-api", shop_api)],
        &[link("shop", "shop-api", "develop")],
        &[record("shop", "shop-api", "develop", "/w/shop-api")],
        &[],
    );
    let r = &out[0];
    assert_eq!(r.kind, Some(Kind::Main));
    assert_eq!(path(r), "/w/shop-api");
    assert!(r.commands.is_empty() && r.refused.is_none());
    let d = drift(&r.base, &Head::Branch(co.branch.unwrap()), co.dirty).unwrap();
    assert_eq!(
        (d.head.as_str(), d.base.as_str(), d.dirty),
        ("feat/checkout-v2", "develop", 3)
    );
}

#[test]
fn r98_j_drifted_record_not_shared_and_base_change_re_resolves() {
    let repos = || {
        vec![(
            "shop-api",
            repo(
                "shop-api",
                "feat/checkout-v2",
                3,
                &["develop", "release/2.5"],
                &[],
            ),
        )]
    };
    let recs = [record("shop", "shop-api", "develop", "/w/shop-api")];
    // A new link of Checkout revamp while Shop's record is drifted.
    let out = resolve(
        repos(),
        &[
            link("shop", "shop-api", "develop"),
            link("checkout-revamp", "shop-api", "develop"),
        ],
        &recs,
        &[],
    );
    let r = by(&out, "checkout-revamp", "shop-api");
    assert_eq!(
        path(r),
        "/w/.worktrees/.projects/checkout-revamp/shop-api",
        "not the drifted main checkout"
    );
    assert_eq!(r.kind, Some(Kind::Project));
    assert_eq!(by(&out, "shop", "shop-api").kind, Some(Kind::Main));
    // Shop's base changes: its record no longer applies.
    let out = resolve(
        repos(),
        &[
            link("shop", "shop-api", "release/2.5"),
            link("checkout-revamp", "shop-api", "develop"),
        ],
        &recs,
        &[],
    );
    let s = by(&out, "shop", "shop-api");
    assert_eq!(path(s), "/w/.worktrees/.projects/shop/shop-api");
    assert!(
        s.create && s.record.is_none(),
        "record dropped until created"
    );
    assert!(matches!(
        &s.commands[..],
        [Command::AddWorktree { branch, .. }] if branch == "release/2.5"
    ));
}

#[test]
fn r98_l_shared_with_and_also_in_labels() {
    let out = resolve(
        vec![(
            "shared-ui",
            repo("shared-ui", "main", 0, &["develop", "release/2.4"], &[]),
        )],
        &[
            link("shop", "shared-ui", "develop"),
            link("billing", "shared-ui", "release/2.4"),
            link("checkout-revamp", "shared-ui", "develop"),
        ],
        &[],
        &[],
    );
    let shop = by(&out, "shop", "shared-ui");
    let billing = by(&out, "billing", "shared-ui");
    assert_eq!(shop.shared_with, pairs(&[("checkout-revamp", "develop")]));
    assert_eq!(
        shop.also_in,
        pairs(&[("billing", "release/2.4"), ("checkout-revamp", "develop")])
    );
    assert!(billing.shared_with.is_empty());
    assert_eq!(
        billing.also_in,
        pairs(&[("checkout-revamp", "develop"), ("shop", "develop")])
    );
    assert_ne!(billing.path, shop.path);
}

#[test]
fn r98_m_project_checkout_and_worktree_paths_per_flavor_and_foreign_folder_refused() {
    use crate::paths::Flavor;
    let d = crate::paths::outside_temp_dir().join(format!("rade-r98m-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let root = d.join("acme");
    for s in ["shared-ui", "billing"] {
        std::fs::create_dir_all(root.join(s)).unwrap();
    }
    let mut st = crate::model::State::new(&d.join("home"));
    let (ws, _) = st.add_workspace(&root, "acme").unwrap();
    st.add_source_in(&root.join("shared-ui"), None, Some(&ws))
        .unwrap();
    st.add_source_in(&root.join("billing"), None, Some(&ws))
        .unwrap();
    let data = d.join("data");
    for (flavor, folder) in [
        (Flavor::Installed, ".worktrees"),
        (Flavor::Devel, ".worktrees-dev"),
    ] {
        assert_eq!(
            st.project_checkout_dir(&ws, "billing", "shared-ui", &data, flavor),
            root.join(folder).join(".projects/billing/shared-ui")
        );
        assert_eq!(
            st.worktree_dir("billing", "shared-ui", &data, flavor),
            root.join(folder).join("billing/shared-ui")
        );
    }
    // A plain folder already where the Project checkout goes.
    let out = resolve(
        vec![(
            "shared-ui",
            repo("shared-ui", "main", 0, &["release/2.4"], &[]),
        )],
        &[link("billing", "shared-ui", "release/2.4")],
        &[],
        &["/w/.worktrees/.projects/billing/shared-ui"],
    );
    let e = out[0].error().unwrap();
    assert_eq!(e.kind, crate::rpc::ErrorKind::Usage);
    assert!(
        e.message
            .contains("/w/.worktrees/.projects/billing/shared-ui"),
        "{e}"
    );
    assert!(out[0].commands.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r98_o_checkout_main_clean_off_base_refused_then_resolves_on_base() {
    let l = [with(link("shop", "shop-api", "develop"), Checkout::Main)];
    let out = resolve(
        vec![(
            "shop-api",
            repo("shop-api", "feat/checkout-v2", 0, &["develop"], &[]),
        )],
        &l,
        &[],
        &[],
    );
    let why = out[0].refused.as_deref().unwrap();
    assert!(
        why.contains("feat/checkout-v2") && why.contains("develop"),
        "{why}"
    );
    assert!(out[0].commands.is_empty() && out[0].record.is_none());
    let out = resolve(
        vec![("shop-api", repo("shop-api", "develop", 0, &[], &[]))],
        &l,
        &[],
        &[],
    );
    assert_eq!(out[0].kind, Some(Kind::Main));
    assert!(out[0].commands.is_empty());
    let rec = out[0].record.as_ref().unwrap();
    assert_eq!(
        (rec.checkout.as_str(), rec.path.as_path()),
        ("main", Path::new("/w/shop-api"))
    );
}

#[test]
fn r98_p_non_git_source_refuses_only_its_link() {
    let notes = Repo {
        main: "/w/notes".into(),
        ..Repo::default()
    };
    let out = resolve(
        vec![
            ("shop-web", repo("shop-web", "develop", 0, &[], &[])),
            ("notes", notes),
        ],
        &[
            link("shop", "shop-web", "develop"),
            link("shop", "notes", "main"),
        ],
        &[],
        &[],
    );
    let n = by(&out, "shop", "notes");
    assert_eq!(
        n.error().unwrap().message,
        "/w/notes: not a git repository (run: git init -b main)"
    );
    assert!(n.commands.is_empty() && n.record.is_none());
    assert_eq!(by(&out, "shop", "shop-web").kind, Some(Kind::Main));
}

#[test]
fn r98_q_symlinked_path_is_the_same_checkout() {
    let d = crate::paths::outside_temp_dir().join(format!("rade-r98q-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let main = d.join("acme/shop-web");
    std::fs::create_dir_all(&main).unwrap();
    std::fs::create_dir_all(d.join("home/src")).unwrap();
    std::os::unix::fs::symlink(&main, d.join("home/src/shop-web")).unwrap();
    let main = main.canonicalize().unwrap();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let ex = |p: &Path| p.exists();
    let mut r = repo("shop-web", "develop", 0, &[], &[]);
    r.main = main.clone();
    r.checkouts[0].path = main.clone();
    let world = World {
        repos: BTreeMap::from([("shop-web".to_string(), r)]),
        folder: d.join("acme/.worktrees"),
        home: d.join("home"),
        canon: &canon,
        exists: &ex,
    };
    let links = [
        with(
            link("shop", "shop-web", "develop"),
            Checkout::Path("~/src/shop-web".into()),
        ),
        link("checkout-revamp", "shop-web", "develop"),
    ];
    let first = resolve_checkout(&links, &world, &[]);
    for r in &first {
        assert_eq!(r.path.as_deref(), Some(main.as_path()), "{}", r.project);
        assert!(r.commands.is_empty());
    }
    assert_eq!(
        first[0].shared_with,
        pairs(&[("checkout-revamp", "develop")])
    );
    assert_eq!(first[1].shared_with, pairs(&[("shop", "develop")]));
    assert_eq!(resolve_checkout(&links, &world, &[]), first);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r99_h_comparison_base_recorded_then_link_then_main() {
    assert_eq!(
        comparison_base(Some("main"), Some("develop")).as_deref(),
        Some("main")
    );
    assert_eq!(
        comparison_base(Some("develop"), Some("release/2.5")).as_deref(),
        Some("develop")
    );
    assert_eq!(
        comparison_base(None, Some("develop")).as_deref(),
        Some("develop")
    );
    assert_eq!(comparison_base(None, None), None, "the main branch");
    // Through the model, as Review / counts / Create PR call it.
    let d = crate::paths::outside_temp_dir().join(format!("rade-r99h-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let root = d.join("acme");
    for s in ["shop-web", "shared-ui"] {
        std::fs::create_dir_all(root.join(s)).unwrap();
    }
    let f = crate::project::Project::path(&root, "shop");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(
        &f,
        "name = \"Shop\"\n[[source]]\nid = \"shop-web\"\nbase = \"develop\"\n[[source]]\nid = \"shared-ui\"\nbase = \"release/2.5\"\n",
    )
    .unwrap();
    let mut st = crate::model::State::new(&d.join("home"));
    let (ws, _) = st.add_workspace(&root, "acme").unwrap();
    let web = st
        .add_source_in(&root.join("shop-web"), None, Some(&ws))
        .unwrap();
    let ui = st
        .add_source_in(&root.join("shared-ui"), None, Some(&ws))
        .unwrap();
    let wt = |st: &mut crate::model::State, src: &str, name: &str, base: Option<&str>| {
        let p = d.join(name);
        let (s, w) = st.add_worktree(src, name, &p, None).unwrap();
        st.set_worktree_origin(&s, &w, Some("shop".into()), base.map(String::from))
            .unwrap();
        p
    };
    let hotfix = wt(&mut st, &web, "hotfix", Some("main"));
    let tokens = wt(&mut st, &ui, "tokens", Some("develop"));
    let legacy = wt(&mut st, &web, "legacy", None);
    assert_eq!(st.comparison_base(&hotfix).as_deref(), Some("main"));
    assert_eq!(st.comparison_base(&tokens).as_deref(), Some("develop"));
    assert_eq!(st.comparison_base(&legacy).as_deref(), Some("develop"));
    // A link's recorded checkout compares with the link's base.
    st.workspaces
        .iter_mut()
        .find(|w| w.id == ws)
        .unwrap()
        .link_checkouts
        .push(record(
            "shop",
            "shop-web",
            "develop",
            root.join("shop-web").to_str().unwrap(),
        ));
    assert_eq!(
        st.comparison_base(&root.join("shop-web")).as_deref(),
        Some("develop")
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r99_d_counts_and_pr_base_use_the_comparison_base() {
    use crate::git::tests::{repo as git_repo, sh};
    let main = git_repo("r99d");
    // `develop` is 2 commits ahead of `main`.
    sh(&main, &["switch", "-qc", "develop"]);
    for n in 0..2 {
        std::fs::write(main.join(format!("dev{n}.txt")), "x\n".repeat(30)).unwrap();
        sh(&main, &["add", "-A"]);
        sh(&main, &["commit", "-qm", &format!("dev {n}")]);
    }
    let base_file: String = (0..100).map(|i| format!("line {i}\n")).collect();
    std::fs::write(main.join("big.txt"), &base_file).unwrap();
    sh(&main, &["add", "-A"]);
    sh(&main, &["commit", "-qm", "big"]);
    sh(&main, &["switch", "-q", "main"]);
    let wt = main.with_file_name(format!("rade-r99d-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&wt);
    sh(
        &main,
        &[
            "worktree",
            "add",
            "-qb",
            "checkout-v2",
            wt.to_str().unwrap(),
            "develop",
        ],
    );
    // +162 −58 of its own: 58 lines of big.txt removed, 100 + 62 added.
    let kept: String = (58..100).map(|i| format!("line {i}\n")).collect();
    std::fs::write(wt.join("big.txt"), kept).unwrap();
    std::fs::write(wt.join("new.txt"), "n\n".repeat(100)).unwrap();
    sh(&wt, &["add", "-A"]);
    sh(&wt, &["commit", "-qm", "own"]);
    std::fs::write(wt.join("more.txt"), "m\n".repeat(62)).unwrap();
    let base = comparison_base(None, Some("develop"));
    let c = crate::diff::changes(&wt, base.as_deref()).unwrap();
    let added: u32 = c.files.iter().map(|f| f.added).sum();
    let removed: u32 = c.files.iter().map(|f| f.removed).sum();
    assert_eq!((added, removed), (162, 58));
    assert!(
        c.files.iter().all(|f| !f.path.starts_with("dev")),
        "develop's own commits"
    );
    assert_eq!(crate::scm::pr_base(base.as_deref()), "develop");
    // Against the main branch (the 0.7.1 default) develop's commits leak in.
    let wrong = crate::diff::changes(&wt, None).unwrap();
    assert!(wrong.files.iter().any(|f| f.path.starts_with("dev")));
    let _ = std::fs::remove_dir_all(&wt);
    let _ = std::fs::remove_dir_all(&main);
}

#[test]
fn r98_1_link_checkouts_kept_in_state_json() {
    let d = crate::paths::outside_temp_dir().join(format!("rade-r98-1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("acme")).unwrap();
    let mut st = crate::model::State::new(&d.join("home"));
    let (ws, _) = st.add_workspace(&d.join("acme"), "acme").unwrap();
    let json = serde_json::to_string(&st).unwrap();
    assert!(!json.contains("link_checkouts"), "empty: not written");
    st.workspaces
        .iter_mut()
        .find(|w| w.id == ws)
        .unwrap()
        .link_checkouts
        .push(record("shop", "shop-api", "develop", "/w/shop-api"));
    let back: crate::model::State =
        serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
    assert_eq!(back, st);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r97_1_links_of_fill_the_main_branch_and_skip_broken_or_unregistered() {
    let shop = crate::project::Project::parse(
        "shop",
        Path::new("/w/.rade/projects/shop/project.toml"),
        "[[source]]\nid = \"shop-web\"\nbase = \"develop\"\n[[source]]\nid = \"gone\"\n\
         [[source]]\nid = \"shared-ui\"\ncheckout = \"main\"\n",
    );
    let broken = crate::project::Project::parse("x", Path::new("/x"), "[[source]]\nnope = 1\n");
    assert!(broken.error.is_some());
    let links = links_of(&[shop, broken], &|s| s != "gone", &|s| {
        if s == "shared-ui" {
            "trunk".into()
        } else {
            "main".into()
        }
    });
    assert_eq!(
        links,
        [
            link("shop", "shop-web", "develop"),
            with(link("shop", "shared-ui", "trunk"), Checkout::Main),
        ]
    );
}

#[test]
fn r98_1_merge_records_keeps_one_per_link_and_only_chosen_checkouts() {
    let old = vec![
        record("shop", "shop-api", "develop", "/w/shop-api"),
        record("billing", "billing", "main", "/w/billing"),
    ];
    let out = resolve(
        vec![
            ("shop-api", repo("shop-api", "feat/x", 0, &["develop"], &[])),
            ("shop-web", repo("shop-web", "develop", 0, &[], &[])),
            ("shared-ui", repo("shared-ui", "main", 0, &["develop"], &[])),
        ],
        &[
            link("shop", "shop-api", "develop"),
            link("shop", "shop-web", "develop"),
            link("shop", "shared-ui", "develop"),
        ],
        &old,
        &[],
    );
    let new = merge_records(&old, &out).expect("shop-web is new");
    assert_eq!(
        new,
        [
            record("shop", "shop-api", "develop", "/w/shop-api"),
            record("billing", "billing", "main", "/w/billing"),
            record("shop", "shop-web", "develop", "/w/shop-web"),
        ],
        "a planned (not created) Project checkout is not recorded"
    );
    assert_eq!(merge_records(&new, &out), None, "nothing new");
    // A link re-resolved elsewhere replaces its record.
    let moved = resolve(
        vec![("shop-web", repo("shop-web", "main", 0, &["develop"], &[]))],
        &[link("shop", "shop-web", "develop")],
        &[],
        &[],
    );
    let mut moved = moved;
    moved[0].record = Some(record("shop", "shop-web", "develop", "/w/other"));
    let again = merge_records(&new, &moved).unwrap();
    assert_eq!(again.iter().filter(|r| r.source == "shop-web").count(), 1);
    assert_eq!(again.last().unwrap().path, PathBuf::from("/w/other"));
}

#[test]
fn r98_n_project_checkouts_are_never_imported() {
    let recs = [record(
        "shop",
        "shared-ui",
        "develop",
        "/elsewhere/ui-develop",
    )];
    let folder = Path::new(FOLDER);
    assert!(is_project_checkout(
        Path::new("/w/.worktrees/.projects/billing/shared-ui"),
        folder,
        &recs,
        &id
    ));
    assert!(is_project_checkout(
        Path::new("/elsewhere/ui-develop"),
        folder,
        &recs,
        &id
    ));
    assert!(!is_project_checkout(
        Path::new("/w/.worktrees/billing/shared-ui"),
        folder,
        &recs,
        &id
    ));
    assert!(!is_project_checkout(
        Path::new("/w/shared-ui"),
        folder,
        &[],
        &id
    ));
}

#[test]
fn r100_3_show_json_has_checkout_counts_and_drift() {
    let out = resolve(
        vec![(
            "shop-api",
            repo("shop-api", "feat/checkout-v2", 3, &["develop"], &[]),
        )],
        &[link("shop", "shop-api", "develop")],
        &[record("shop", "shop-api", "develop", "/w/shop-api")],
        &[],
    );
    let facts = Facts {
        exists: true,
        head: Some(Head::Branch("feat/checkout-v2".into())),
        dirty: 3,
        ahead: Some(1),
        behind: Some(0),
    };
    let v = show_json(&out[0], &facts);
    assert_eq!(v["resolved_base"], "develop");
    assert_eq!(v["checkout_kind"], "main");
    assert_eq!(v["path"], "/w/shop-api");
    assert_eq!(v["exists"], true);
    assert_eq!(v["head"], "feat/checkout-v2");
    assert_eq!(
        (v["ahead"].as_u64(), v["behind"].as_u64()),
        (Some(1), Some(0))
    );
    assert_eq!(v["dirty"], 3);
    assert_eq!(
        v["drift"],
        serde_json::json!({"head": "feat/checkout-v2", "base": "develop", "dirty": 3})
    );
    assert_eq!(v["shared_with"], serde_json::json!([]));
    // Not created yet: no HEAD, no drift.
    let planned = resolve(
        vec![("shared-ui", repo("shared-ui", "main", 0, &["develop"], &[]))],
        &[
            link("shop", "shared-ui", "develop"),
            link("billing", "shared-ui", "release/2.4"),
        ],
        &[],
        &[],
    );
    let v = show_json(by(&planned, "shop", "shared-ui"), &Facts::default());
    assert_eq!(v["checkout_kind"], "project");
    assert_eq!(v["exists"], false);
    assert_eq!(v["drift"], serde_json::Value::Null);
    assert_eq!(v["head"], serde_json::Value::Null);
    assert_eq!(
        v["also_in"],
        serde_json::json!([{"project": "billing", "base": "release/2.4"}])
    );
    // Refused: the reason, no path.
    let refused = resolve(
        vec![("shop-api", repo("shop-api", "feat/x", 2, &["develop"], &[]))],
        &[with(link("shop", "shop-api", "develop"), Checkout::Main)],
        &[],
        &[],
    );
    let v = show_json(&refused[0], &Facts::default());
    assert_eq!(v["checkout_kind"], serde_json::Value::Null);
    assert!(v["refused"].as_str().unwrap().contains("never switches"));
}

#[test]
fn r100_2_elsewhere_names_another_checkout_on_the_base() {
    let mut r = repo("shop-api", "feat/x", 0, &["develop"], &[]);
    r.checkouts
        .push(co("/w/.worktrees/shop-api/d", "develop", 0));
    assert_eq!(
        elsewhere(&r, "develop", Path::new("/w/shop-api"), &id),
        Some(PathBuf::from("/w/.worktrees/shop-api/d"))
    );
    assert_eq!(elsewhere(&r, "main", Path::new("/w/shop-api"), &id), None);
    assert_eq!(
        elsewhere(&r, "develop", Path::new("/w/.worktrees/shop-api/d"), &id),
        None,
        "itself"
    );
}

// ---- R98-d: the invariants on generated Workspaces ------------------------

/// xorshift64*: a fixed-seed generator (no new crate).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
    fn pick<'a>(&mut self, v: &'a [&'a str]) -> &'a str {
        v[self.below(v.len())]
    }
}

const BRANCHES: [&str; 5] = ["main", "develop", "release/2.4", "feat/x", "hotfix"];
const PROJECTS: [&str; 4] = ["billing", "checkout-revamp", "shop", "zeta"];

struct Scene {
    repos: BTreeMap<String, Repo>,
    exists: BTreeSet<PathBuf>,
    links: Vec<Link>,
}

fn generate(rng: &mut Rng) -> Scene {
    let mut repos = BTreeMap::new();
    let mut exists = BTreeSet::new();
    let n_src = 1 + rng.below(3);
    for s in 0..n_src {
        let id = format!("src{s}");
        let main = format!("{ROOT}/{id}");
        let mut r = Repo {
            main: main.clone().into(),
            git: !rng.chance(5),
            ..Repo::default()
        };
        if r.git {
            let head = rng.pick(&BRANCHES).to_string();
            let mut c = co(
                &main,
                &head,
                if rng.chance(40) { 1 + rng.below(4) } else { 0 },
            );
            if rng.chance(10) {
                c.branch = None;
            }
            r.checkouts.push(c);
            if rng.chance(30) {
                let b = rng.pick(&BRANCHES).to_string();
                if !r.checkouts.iter().any(|c| c.branch.as_deref() == Some(&b)) {
                    r.checkouts
                        .push(co(&format!("{FOLDER}/{id}/wt"), &b, rng.below(2)));
                }
            }
            for b in BRANCHES {
                if rng.chance(60) {
                    r.local.insert(b.into());
                } else if rng.chance(50) {
                    r.remote.insert(b.into(), format!("origin/{b}"));
                }
            }
            for c in &r.checkouts {
                if let Some(b) = &c.branch {
                    r.local.insert(b.clone());
                }
            }
        }
        repos.insert(id, r);
    }
    let mut links = Vec::new();
    for p in PROJECTS.iter().take(1 + rng.below(4)) {
        for s in 0..n_src {
            if !rng.chance(60) {
                continue;
            }
            let source = format!("src{s}");
            let checkout = match rng.below(10) {
                0 | 1 => Checkout::Main,
                2 => match repos[&source].checkouts.get(rng.below(2)) {
                    Some(c) => Checkout::Path(c.path.display().to_string()),
                    None => Checkout::Path("/nowhere".into()),
                },
                _ => Checkout::Auto,
            };
            links.push(Link {
                project: p.to_string(),
                source: source.clone(),
                base: rng.pick(&BRANCHES).into(),
                checkout,
            });
            if rng.chance(5) {
                exists.insert(project_checkout_dir(Path::new(FOLDER), p, &source));
            }
        }
    }
    Scene {
        repos,
        exists,
        links,
    }
}

fn run(scene: &Scene, records: &[LinkCheckout]) -> Vec<Resolved> {
    let ex = |p: &Path| scene.exists.contains(p);
    let world = World {
        repos: scene.repos.clone(),
        folder: FOLDER.into(),
        home: "/home/u".into(),
        canon: &id,
        exists: &ex,
    };
    resolve_checkout(&scene.links, &world, records)
}

/// Whether the checkout at `p` is on `base` (or is still to be created on it).
fn on_base(scene: &Scene, r: &Resolved) -> bool {
    r.create
        || scene.repos[&r.source].checkouts.iter().any(|c| {
            Some(c.path.as_path()) == r.path.as_deref() && c.branch.as_deref() == Some(&r.base)
        })
}

fn violations(scene: &Scene, out: &[Resolved], records: &[LinkCheckout]) -> Vec<String> {
    let mut v = Vec::new();
    let ok: Vec<&Resolved> = out.iter().filter(|r| r.refused.is_none()).collect();
    for (i, a) in ok.iter().enumerate() {
        for b in &ok[i + 1..] {
            if a.path == b.path && a.base != b.base {
                v.push(format!(
                    "1: {}/{} and {}/{} share {:?} at two bases",
                    a.project, a.source, b.project, b.source, a.path
                ));
            }
            if a.source == b.source
                && a.base == b.base
                && a.path != b.path
                && on_base(scene, a)
                && on_base(scene, b)
            {
                v.push(format!(
                    "2: {}/{} and {}/{} got two checkouts of {}",
                    a.project, a.source, b.project, b.source, a.base
                ));
            }
        }
    }
    for r in out {
        if r.refused.is_some() && (!r.commands.is_empty() || r.record.is_some()) {
            v.push(format!(
                "refused {}/{} has commands or a record",
                r.project, r.source
            ));
        }
        for Command::AddWorktree { dir, .. } in &r.commands {
            let repo = &scene.repos[&r.source];
            if repo.checkouts.iter().any(|c| c.path == *dir) {
                v.push(format!(
                    "3: a command touches the existing checkout {}",
                    dir.display()
                ));
            }
        }
    }
    for rec in records {
        let Some(l) = scene.links.iter().find(|l| {
            l.project == rec.project
                && l.source == rec.source
                && l.base == rec.base
                && l.checkout.as_str() == rec.checkout
        }) else {
            continue;
        };
        let alive = scene.repos[&l.source]
            .checkouts
            .iter()
            .any(|c| c.path == rec.path);
        let got = out
            .iter()
            .find(|r| r.project == l.project && r.source == l.source)
            .unwrap();
        if alive && got.path.as_deref() != Some(rec.path.as_path()) {
            v.push(format!(
                "5: record {}/{} → {} not kept (got {:?})",
                rec.project,
                rec.source,
                rec.path.display(),
                got.path
            ));
        }
    }
    v
}

#[test]
fn r98_d_invariants_hold_on_500_generated_workspaces() {
    let mut rng = Rng(0x5EED_0036);
    let mut checked = 0;
    // (main, project created, path, refused, shared, drifted kept): the
    // generator must reach every branch of the rules.
    let mut seen = [0usize; 6];
    for n in 0..500 {
        let mut scene = generate(&mut rng);
        // Without records.
        let first = run(&scene, &[]);
        let mut v = violations(&scene, &first, &[]);
        if run(&scene, &[]) != first {
            v.push("4: resolving twice differs".into());
        }
        // Apply the plan, record every checkout, then move some HEADs.
        let mut records: Vec<LinkCheckout> = Vec::new();
        for r in &first {
            for Command::AddWorktree { dir, branch, .. } in &r.commands {
                let repo = scene.repos.get_mut(&r.source).unwrap();
                if !repo.checkouts.iter().any(|c| c.path == *dir) {
                    repo.checkouts.push(co(dir.to_str().unwrap(), branch, 0));
                }
                scene.exists.insert(dir.clone());
            }
            if r.refused.is_none() {
                let l = scene
                    .links
                    .iter()
                    .find(|l| l.project == r.project && l.source == r.source)
                    .unwrap();
                records.push(LinkCheckout {
                    project: r.project.clone(),
                    source: r.source.clone(),
                    base: r.base.clone(),
                    checkout: l.checkout.as_str().into(),
                    path: r.path.clone().unwrap(),
                });
            }
        }
        // Git keeps one branch in one checkout: a move only to a free branch.
        for repo in scene.repos.values_mut() {
            for i in 0..repo.checkouts.len() {
                let b = rng.pick(&BRANCHES).to_string();
                let free = !repo
                    .checkouts
                    .iter()
                    .any(|c| c.branch.as_deref() == Some(&b));
                if rng.chance(25) && free {
                    repo.checkouts[i].branch = Some(b.clone());
                    repo.checkouts[i].dirty = rng.below(3);
                    repo.local.insert(b);
                }
            }
        }
        if rng.chance(30) && !scene.links.is_empty() {
            let i = rng.below(scene.links.len());
            scene.links[i].base = rng.pick(&BRANCHES).into();
        }
        let second = run(&scene, &records);
        for r in first.iter().chain(&second) {
            match (r.kind, r.create) {
                (Some(Kind::Main), _) => seen[0] += 1,
                (Some(Kind::Project), true) => seen[1] += 1,
                (Some(_), _) => seen[2] += 1,
                (None, _) => seen[3] += 1,
            }
            seen[4] += usize::from(!r.shared_with.is_empty());
        }
        seen[5] += second
            .iter()
            .filter(|r| r.record.is_some() && !on_base(&scene, r))
            .count();
        v.extend(violations(&scene, &second, &records));
        if run(&scene, &records) != second {
            v.push("4: resolving twice with records differs".into());
        }
        assert!(v.is_empty(), "workspace {n}: {v:#?}");
        checked += 1;
    }
    assert_eq!(checked, 500);
    assert!(seen.iter().all(|n| *n >= 20), "coverage {seen:?}");
}

/// Spec C.19: the New Project dialog's plan texts for `Checkout revamp`
/// over scene A (Shop at develop, `shop-api` drifted, `shared-ui` in Shop
/// and Billing at two bases).
#[test]
fn plan_line_texts_of_scene_c() {
    let pc = |p: &str, s: &str| format!("{FOLDER}/.projects/{p}/{s}");
    let mut ui = repo(
        "shared-ui",
        "main",
        0,
        &["develop", "release/2.4", "release/2.5"],
        &[],
    );
    ui.checkouts
        .push(co(&pc("shop", "shared-ui"), "develop", 0));
    ui.checkouts
        .push(co(&pc("billing", "shared-ui"), "release/2.4", 0));
    let repos = vec![
        ("shop-web", repo("shop-web", "develop", 0, &[], &[])),
        (
            "shop-api",
            repo("shop-api", "feat/checkout-v2", 3, &["develop"], &[]),
        ),
        ("shared-ui", ui),
        ("billing", repo("billing", "main", 0, &[], &[])),
    ];
    let links = [
        link("shop", "shop-web", "develop"),
        link("shop", "shop-api", "develop"),
        link("shop", "shared-ui", "develop"),
        link("billing", "billing", "main"),
        link("billing", "shared-ui", "release/2.4"),
        link("checkout-revamp", "shop-web", "develop"),
        link("checkout-revamp", "shop-api", "develop"),
        link("checkout-revamp", "shared-ui", "release/2.5"),
    ];
    let records = [
        record("shop", "shop-web", "develop", "/w/shop-web"),
        record("shop", "shop-api", "develop", "/w/shop-api"),
        record("shop", "shared-ui", "develop", &pc("shop", "shared-ui")),
        record(
            "billing",
            "shared-ui",
            "release/2.4",
            &pc("billing", "shared-ui"),
        ),
    ];
    let exists = [
        "/w/shop-web",
        "/w/shop-api",
        "/w/shared-ui",
        "/w/billing",
        &pc("shop", "shared-ui"),
        &pc("billing", "shared-ui"),
    ];
    let out = resolve(repos, &links, &records, &exists);
    let names = crate::linkview::Names(vec![
        ("shop".into(), "Shop".into()),
        ("billing".into(), "Billing".into()),
        ("checkout-revamp".into(), "Checkout revamp".into()),
    ]);
    let line = |s: &str, head: &str| {
        let (k, d) = by(&out, "checkout-revamp", s).plan_line(Some(head), &names);
        format!("{k} · {d}")
    };
    assert_eq!(
        line("shop-web", "develop"),
        "main checkout · on develop · shared with Shop"
    );
    assert_eq!(
        line("shop-api", "feat/checkout-v2"),
        "new project checkout · main checkout is on feat/checkout-v2"
    );
    assert_eq!(
        line("shared-ui", "main"),
        "new project checkout · Shop uses develop, Billing release/2.4"
    );
    // Nothing planned beyond new folders; a refusal reads as one.
    let mut r = by(&out, "checkout-revamp", "shop-web").clone();
    r.refused = Some("main checkout is used by Shop at develop".into());
    assert_eq!(
        r.plan_line(None, &names),
        (
            "refused".into(),
            "main checkout is used by Shop at develop".into()
        )
    );
}

#[test]
fn r105_b_vs_base_counts_against_the_bases_upstream_when_it_has_one() {
    use crate::git::tests::{repo as git_repo, sh};
    // The remote's `develop` is 2 commits ahead of the local one, which
    // tracks it: the main checkout on `develop` reads `↓2` (scene B).
    let main = git_repo("r105b");
    sh(&main, &["switch", "-qc", "develop"]);
    let remote = main.with_file_name(format!("rade-r105b-remote-{}.git", std::process::id()));
    let _ = std::fs::remove_dir_all(&remote);
    sh(&main, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    sh(
        &main,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    sh(&main, &["push", "-qu", "origin", "develop"]);
    for n in 0..2 {
        std::fs::write(main.join(format!("up{n}.txt")), "x\n").unwrap();
        sh(&main, &["add", "-A"]);
        sh(&main, &["commit", "-qm", &format!("up {n}")]);
    }
    sh(&main, &["push", "-q", "origin", "develop"]);
    sh(&main, &["reset", "-q", "--hard", "HEAD~2"]);
    let repos = observe(&[("shop-web".into(), main.clone())]);
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let f = facts(&repos["shop-web"], &main, "develop", &canon);
    assert_eq!((f.ahead, f.behind), (Some(0), Some(2)));
    // A local base with no upstream: against itself.
    sh(&main, &["branch", "--unset-upstream", "develop"]);
    let f = facts(&repos["shop-web"], &main, "develop", &canon);
    assert_eq!((f.ahead, f.behind), (Some(0), Some(0)));
    let _ = std::fs::remove_dir_all(&remote);
    let _ = std::fs::remove_dir_all(&main);
}

#[test]
fn r131_a_plain_folder_and_unborn_repository() {
    use crate::git::tests::sh;
    let base = std::env::temp_dir().join(format!("rade-r131a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let plain = base.join("plain");
    let fresh = base.join("fresh");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::create_dir_all(&fresh).unwrap();
    sh(&fresh, &["init", "-q", "-b", "main"]);
    std::fs::write(fresh.join("a.txt"), "untracked\n").unwrap();

    let repos = observe(&[
        ("plain".to_string(), plain.clone()),
        ("fresh".to_string(), fresh.clone()),
    ]);
    assert!(!repos["plain"].git);
    assert!(repos["fresh"].git);
    assert_eq!(repos["fresh"].unborn.as_deref(), Some("main"));

    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let ex = |p: &Path| p.exists();
    let world = World {
        repos,
        folder: base.join(".worktrees"),
        home: base.clone(),
        canon: &canon,
        exists: &ex,
    };
    let out = resolve_checkout(
        &[
            link("a", "plain", "main"),
            link("a", "fresh", "main"),
            link("b", "fresh", "develop"),
        ],
        &world,
        &[],
    );
    let p = by(&out, "a", "plain");
    assert_eq!(
        p.error().unwrap().message,
        format!(
            "{}: not a git repository (run: git init -b main)",
            canon(&plain).display()
        )
    );
    assert!(p.commands.is_empty());
    // The unborn branch is the base: the main checkout, nothing to run.
    let f = by(&out, "a", "fresh");
    assert_eq!(f.kind, Some(Kind::Main), "{f:?}");
    assert!(f.commands.is_empty() && f.refused.is_none());
    // Another base needs a Project checkout, which needs a commit.
    let d = by(&out, "b", "fresh");
    assert!(d.commands.is_empty());
    assert_eq!(
        d.error().unwrap().message,
        format!(
            "{} has no commits yet: commit once, then try again",
            canon(&fresh).display()
        )
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn r133_a_orphans_are_project_checkouts_no_link_uses() {
    let mut web = repo("shop-web", "develop", 0, &["main"], &[]);
    web.checkouts.push(co(
        "/w/.worktrees/.projects/shop24/shop-web",
        "release/2.4",
        0,
    ));
    web.checkouts
        .push(co("/w/.worktrees/.projects/old/shop-web", "main", 2));
    // An ordinary Worktree is never a Project checkout.
    web.checkouts
        .push(co("/w/.worktrees/shop-web/feature", "feature", 0));
    let mut ui = repo("shared-ui", "main", 0, &[], &[]);
    ui.checkouts
        .push(co("/w/.worktrees/.projects/shop/shared-ui", "develop", 0));
    let repos: BTreeMap<String, Repo> =
        [("shop-web".to_string(), web), ("shared-ui".to_string(), ui)]
            .into_iter()
            .collect();
    // Shop24 is gone; Shop still uses shared-ui's checkout.
    let used = [PathBuf::from("/w/.worktrees/.projects/shop/shared-ui")];
    let found = orphans(&repos, Path::new(FOLDER), &used, &id);
    assert_eq!(
        found,
        vec![
            Orphan {
                source: "shop-web".into(),
                path: "/w/.worktrees/.projects/old/shop-web".into(),
                branch: Some("main".into()),
                dirty: 2,
            },
            Orphan {
                source: "shop-web".into(),
                path: "/w/.worktrees/.projects/shop24/shop-web".into(),
                branch: Some("release/2.4".into()),
                dirty: 0,
            },
        ]
    );
}
