use super::*;
use crate::agents::{AgentStatus, SpawnSpec};
use crate::config::Limits;
use crate::paths::Flavor;
use std::path::Path;

/// `default` with Source `x` (/tmp/x) and Workspaces `a` (/w/a, Source
/// `api` at /w/a/api added before `a` existed) and `b` (/w/b).
fn two() -> State {
    let mut st = State::new(Path::new("/home/u"));
    st.add_source(Path::new("/tmp/x"), None).unwrap();
    st.add_source(Path::new("/w/a/api"), None).unwrap();
    let (a, moved) = st.add_workspace(Path::new("/w/a"), "A").unwrap();
    assert_eq!((a.as_str(), moved), ("a", vec!["api".to_string()]));
    st.add_workspace(Path::new("/w/b"), "B").unwrap();
    st
}

fn spawn(st: &mut State, source: &str, name: &str) -> String {
    st.spawn(
        SpawnSpec {
            source: Some(source.into()),
            agent: Some("claude".into()),
            name: Some(name.into()),
            ..Default::default()
        },
        &Limits::default(),
    )
    .unwrap()
}

#[test]
fn r23_1_registry_add_moves_sources_and_makes_a_general() {
    let st = two();
    let ids: Vec<&str> = st.workspaces.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["default", "a", "b"]);
    assert_eq!(st.source("api").unwrap().workspace, "a");
    assert_eq!(st.source("x").unwrap().workspace, "default");
    let g = st.source("general@a").unwrap();
    assert!(g.is_general());
    assert_eq!(
        (g.path.as_path(), g.workspace.as_str()),
        (Path::new("/w/a"), "a")
    );
    assert_eq!(g.worktrees[0].path, Path::new("/w/a"));
    // Generals last, in Workspace order.
    let order: Vec<&str> = st.sources.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(order, ["x", "api", "general", "general@a", "general@b"]);
    // A Source added later joins the Workspace whose root holds it.
    let mut st = st;
    let id = st.add_source(Path::new("/w/b/web"), None).unwrap();
    assert_eq!(st.source(&id).unwrap().workspace, "b");
    assert_eq!(st.sources.iter().position(|s| s.id == id), Some(2));
}

#[test]
fn r23_1_add_refuses_bad_roots_and_names_get_unique_ids() {
    let mut st = two();
    assert!(st.add_workspace(Path::new("rel"), "r").is_err());
    assert!(st.add_workspace(Path::new("/w/a"), "again").is_err());
    assert!(st.add_workspace(Path::new("/home/u"), "home").is_err());
    let (id, _) = st.add_workspace(Path::new("/w/c"), "A").unwrap();
    assert_eq!(id, "a-2");
    let (id, _) = st.add_workspace(Path::new("/w/d"), "default").unwrap();
    assert_eq!(id, "default-2");
    assert_eq!(st.find_workspace("B").unwrap().id, "b", "by name");
}

#[test]
fn r23_1_remove_refuses_sources_and_default() {
    let mut st = two();
    assert!(st.remove_workspace("default").is_err());
    let e = st.remove_workspace("a").unwrap_err();
    assert!(e.message.contains("api"), "{}", e.message);
    st.switch_workspace("b").unwrap();
    let (id, general) = st.remove_workspace("b").unwrap();
    assert_eq!((id.as_str(), general.id.as_str()), ("b", "general@b"));
    assert!(st.source("general@b").is_none());
    assert_eq!(st.active_workspace(), "default", "shown one removed");
}

#[test]
fn r23_5_switch_restores_the_last_source_and_focus() {
    let mut st = two();
    let p = spawn(&mut st, "api", "lead");
    st.focus_pane(&p).unwrap();
    assert_eq!(st.active_workspace(), "a");
    // b has never been shown: its General.
    assert_eq!(st.switch_workspace("b").unwrap(), "general@b");
    assert_eq!(st.active_workspace(), "b");
    assert_eq!(st.switch_workspace("a").unwrap(), "api");
    assert_eq!(st.active_tab().unwrap().focused, p);
    // A focus in a hidden Workspace shows it (R23.6).
    st.switch_workspace("default").unwrap();
    st.focus_pane(&p).unwrap();
    assert_eq!(st.active_workspace(), "a");
}

#[test]
fn r23_4_visible_sources_and_summaries_per_workspace() {
    let mut st = two();
    let p = spawn(&mut st, "api", "lead");
    st.switch_workspace("b").unwrap();
    let shown: Vec<&str> = st.visible_sources().map(|s| s.id.as_str()).collect();
    assert_eq!(shown, ["general@b"]);
    st.set_status(&p, Some(AgentStatus::Waiting), None, None, 1)
        .unwrap();
    let a = st.workspace_summary("a");
    assert_eq!((a.sources, a.unread, a.waiting), (1, 1, 1));
    assert_eq!(a.status, Some(AgentStatus::Waiting));
    let b = st.workspace_summary("b");
    assert_eq!((b.sources, b.unread, b.waiting, b.status), (0, 0, 0, None));
    assert_eq!(st.hidden_waiting(), vec![p.clone()]);
    st.switch_workspace("a").unwrap();
    assert!(st.hidden_waiting().is_empty());
}

#[test]
fn r23_names_resolve_in_the_callers_workspace_before_others() {
    let mut st = two();
    let lead = spawn(&mut st, "api", "lead");
    st.add_source(Path::new("/w/a/web"), None).unwrap();
    let rev_a = spawn(&mut st, "web", "rev");
    let _rev_b = spawn(&mut st, "general@b", "rev");
    assert_eq!(st.resolve_pane("rev", Some(&lead)).unwrap(), rev_a);
    assert!(st.resolve_pane("rev", None).is_err(), "ambiguous globally");
    assert_eq!(
        st.pane_infos()
            .iter()
            .find(|i| i.pane == lead)
            .unwrap()
            .workspace,
        "a"
    );
}

#[test]
fn r23_2_workspace_toml_parses_and_ignores_unknown_keys() {
    let f = WorkspaceFile::parse(
        "name = \"Globex\"\ncolor = \"blue\"\n[env]\nGH_CONFIG_DIR = \"~/.config/gh-globex\"\nRADE_PANE_ID = \"x\"\nHOME_ONLY = \"~\"\n[clone]\ndefault_owner = \"globex-ai\"\n[identity]\nemail = \"x@y\"\n",
    )
    .unwrap();
    assert_eq!(f.name.as_deref(), Some("Globex"));
    assert_eq!(f.clone.default_owner.as_deref(), Some("globex-ai"));
    assert_eq!(
        f.env(Path::new("/home/u")),
        [
            (
                "GH_CONFIG_DIR".to_string(),
                "/home/u/.config/gh-globex".to_string()
            ),
            ("HOME_ONLY".into(), "/home/u".into()),
        ],
        "~ expanded, RADE_* dropped"
    );
    assert!(WorkspaceFile::parse("env = 3").is_err());
    assert_eq!(color_hex(Some("blue"), 3), "#3584e4");
    assert_eq!(color_hex(Some("#AbCdEf"), 3), "#abcdef");
    assert_eq!(color_hex(Some("nope"), 1), PALETTE[0].1);
    assert_eq!(color_hex(None, 0), "#77767b", "default is gray");
}

#[test]
fn r23_3_env_for_reads_the_file_each_time() {
    let d = std::env::temp_dir().join(format!("rade-ws-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("a/.rade")).unwrap();
    let mut st = State::new(Path::new("/home/u"));
    st.add_workspace(&d.join("a"), "a").unwrap();
    let src = st.add_source(&d.join("a/api"), None).unwrap();
    let env = |st: &State| st.env_for(&src, Path::new("/home/u")).unwrap();
    assert_eq!(env(&st), [("RADE_WORKSPACE".to_string(), "a".to_string())]);
    std::fs::write(
        d.join("a/.rade/workspace.toml"),
        "[env]\nGH_CONFIG_DIR = \"/g/a\"\n",
    )
    .unwrap();
    assert_eq!(
        env(&st),
        [
            ("GH_CONFIG_DIR".to_string(), "/g/a".to_string()),
            ("RADE_WORKSPACE".into(), "a".into())
        ]
    );
    std::fs::write(d.join("a/.rade/workspace.toml"), "[env\n").unwrap();
    assert!(
        st.env_for(&src, Path::new("/home/u")).is_err(),
        "broken file"
    );
    assert_eq!(
        st.env_for("general", Path::new("/home/u")).unwrap(),
        [("RADE_WORKSPACE".to_string(), "default".to_string())]
    );
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn r23_8_new_worktrees_go_under_the_workspace_root() {
    let st = two();
    let data = Path::new("/d/rade-dev");
    assert_eq!(
        st.worktree_dir("api", "feat", data, Flavor::Installed),
        Path::new("/w/a/.worktrees/api/feat")
    );
    assert_eq!(
        st.worktree_dir("api", "feat", data, Flavor::Devel),
        Path::new("/w/a/.worktrees-dev/api/feat")
    );
    assert_eq!(
        st.worktree_dir("x", "feat", data, Flavor::Devel),
        Path::new("/d/rade-dev/worktrees/x/feat"),
        "default keeps XDG (ADR-0014)"
    );
}

#[test]
fn r23_5_palette_switches_workspaces_and_lists_shown_sources() {
    let mut st = two();
    st.switch_workspace("a").unwrap();
    let kb = crate::config::Config::default().keybindings();
    let items = crate::palette::model_items(&st, &kb, &[]);
    let sw: Vec<(&str, Option<&str>)> = items
        .iter()
        .filter(|i| i.title.starts_with("Switch Workspace"))
        .map(|i| (i.title.as_str(), i.keys.as_deref()))
        .collect();
    assert_eq!(sw.len(), 3);
    assert_eq!(sw[2], ("Switch Workspace: B", Some("Ctrl+Alt+3")));
    let sources: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == crate::palette::Kind::Source)
        .map(|i| i.title.as_str())
        .collect();
    // R107.3: the Workspace root (outside home: as is), not `General`.
    assert_eq!(sources, ["api", "/w/a"], "the shown Workspace's only");
}

#[test]
fn r24_2_new_source_workspace_explicit_then_root_then_caller() {
    let mut st = two();
    let lead = spawn(&mut st, "api", "lead"); // a Pane in `a`
    st.switch_workspace("b").unwrap();
    let w = |st: &State, p: &str, e: Option<&str>, c: Option<&str>| {
        st.workspace_for_new_source(Path::new(p), e, c).unwrap()
    };
    assert_eq!(w(&st, "/w/b/x", Some("A"), Some(&lead)), "a", "explicit");
    assert_eq!(w(&st, "/w/b/x", None, Some(&lead)), "b", "root holds it");
    assert_eq!(w(&st, "/tmp/y", None, Some(&lead)), "a", "the caller's");
    assert_eq!(w(&st, "/tmp/y", None, None), "b", "the shown one");
    assert!(st
        .workspace_for_new_source(Path::new("/x"), Some("nope"), None)
        .is_err());
    let id = st
        .add_source_in(Path::new("/tmp/y"), None, Some("a"))
        .unwrap();
    assert_eq!(st.source(&id).unwrap().workspace, "a");
}

#[test]
fn r25_1_identity_table_parses() {
    let f = WorkspaceFile::parse(
        "[identity]\ngit_name = \"Ann\"\ngit_email = \"ann@acme.test\"\ngithub = \"ann-acme\"\nssh_key = \"id_ed25519_acme\"\nsign = \"ssh\"\nunknown = 1\n",
    )
    .unwrap();
    let i = &f.identity;
    assert_eq!(i.git_name.as_deref(), Some("Ann"));
    assert_eq!(i.git_email.as_deref(), Some("ann@acme.test"));
    assert_eq!(i.github.as_deref(), Some("ann-acme"));
    assert_eq!(i.ssh_key.as_deref(), Some("id_ed25519_acme"));
    assert!(i.signs());
    assert!(!WorkspaceFile::default().identity.signs(), "absent: none");
    assert!(WorkspaceFile::parse("[identity]\nsign = \"gpg\"\n").is_err());
}
