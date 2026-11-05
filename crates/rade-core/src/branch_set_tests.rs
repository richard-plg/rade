use super::*;
use crate::model::State;
use crate::preset;
use crate::rpc::ErrorKind;
use std::path::Path;

const SHOP: &str = "name = \"Shop\"\n\
[[source]]\nid = \"web\"\nbase = \"develop\"\n\
[[source]]\nid = \"api\"\nbase = \"main\"\n\
[[source]]\nid = \"infra\"\nbase = \"release/2.4\"\n";

fn shop() -> Project {
    let p = Project::parse(
        "shop",
        Path::new("/w/.rade/projects/shop/project.toml"),
        SHOP,
    );
    assert_eq!(p.error, None);
    p
}

fn facts(src: &str) -> LinkFacts {
    LinkFacts {
        source: src.into(),
        repo: format!("/w/{src}").into(),
        dir: format!("/w/.worktrees/{src}/feat-x").into(),
        branch_exists: false,
        worktree_exists: false,
        main_branch: Some("main".into()),
        start_point: None,
        checkout: Ok(()),
    }
}

fn all_facts() -> Vec<LinkFacts> {
    ["web", "api", "infra"].map(facts).to_vec()
}

fn members() -> Vec<Member> {
    vec![
        Member {
            link: "web".into(),
            path: "/w/.worktrees/web/feat-x".into(),
            base: "develop".into(),
        },
        Member {
            link: "api".into(),
            path: "/w/.worktrees/api/feat-x".into(),
            base: "main".into(),
        },
        Member {
            link: "infra".into(),
            path: "/w/.worktrees/infra/it's".into(),
            base: "release/2.4".into(),
        },
    ]
}

fn usage_naming(r: Result<Plan>, link: &str) {
    let e = r.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(e.message.contains(&format!("'{link}'")), "{}", e.message);
}

#[test]
fn r149_a_a_taken_name_is_usage_naming_the_link_with_no_commands() {
    let mut f = all_facts();
    f[1].branch_exists = true;
    usage_naming(plan(&shop(), &Sources::All, "feat-x", &f), "api");
    let mut f = all_facts();
    f[1].worktree_exists = true;
    usage_naming(plan(&shop(), &Sources::All, "feat-x", &f), "api");
}

#[test]
fn r149_a_an_unresolvable_checkout_or_a_bad_name_is_usage() {
    let mut f = all_facts();
    f[2].checkout = Err("main checkout is on x and dirty".into());
    usage_naming(plan(&shop(), &Sources::All, "feat-x", &f), "infra");
    usage_naming(
        plan(&shop(), &Sources::All, "bad name", &all_facts()),
        "web",
    );
    usage_naming(
        plan(&shop(), &Sources::Named(vec!["nope".into()]), "feat-x", &[]),
        "nope",
    );
}

#[test]
fn r149_a_a_valid_plan_has_one_command_per_link_in_project_order() {
    let p = plan(
        &shop(),
        &Sources::Named(vec!["infra".into(), "web".into()]),
        "feat-x",
        &all_facts(),
    )
    .unwrap();
    let order: Vec<&str> = p.steps.iter().map(|s| s.source.as_str()).collect();
    assert_eq!(order, ["web", "infra"]);
}

#[test]
fn r149_b_each_worktree_starts_from_its_own_links_base() {
    let p = plan(&shop(), &Sources::All, "feat-x", &all_facts()).unwrap();
    let starts: Vec<(&str, &str)> = p
        .steps
        .iter()
        .map(|s| (s.base.as_str(), s.argv.last().unwrap().as_str()))
        .collect();
    assert_eq!(
        starts,
        [
            ("develop", "develop"),
            ("main", "main"),
            ("release/2.4", "release/2.4")
        ]
    );
    assert_eq!(
        p.steps[0].argv,
        [
            "worktree",
            "add",
            "--no-track",
            "-b",
            "feat-x",
            "/w/.worktrees/web/feat-x",
            "develop"
        ]
    );
    // A remote-only base is cut from `origin/<base>`; the base recorded stays.
    let mut f = all_facts();
    f[2].start_point = Some("origin/release/2.4".into());
    let p = plan(&shop(), &Sources::All, "feat-x", &f).unwrap();
    assert_eq!(p.steps[2].base, "release/2.4");
    assert_eq!(p.steps[2].argv.last().unwrap(), "origin/release/2.4");
}

#[test]
fn a_link_without_a_base_is_cut_from_the_main_branch() {
    let proj = Project::parse(
        "p",
        Path::new("/f"),
        "[[source]]\nid = \"web\"\n[[source]]\nid = \"api\"\nbase = \"dev\"\n",
    );
    let mut f = vec![facts("web"), facts("api")];
    f[0].main_branch = Some("trunk".into());
    let p = plan(&proj, &Sources::All, "n", &f).unwrap();
    assert_eq!(p.steps[0].base, "trunk");
    f[0].main_branch = None;
    usage_naming(plan(&proj, &Sources::All, "n", &f), "web");
}

#[test]
fn r149_c_a_failure_at_link_2_removes_only_link_1() {
    let p = plan(&shop(), &Sources::All, "feat-x", &all_facts()).unwrap();
    let r = rollback(&p, 1, false, "fatal: boom\n");
    assert_eq!(r.undo.len(), 1);
    assert_eq!(r.undo[0].source, "web");
    assert_eq!(
        r.undo[0].commands,
        [
            vec!["worktree", "remove", "--force", "/w/.worktrees/web/feat-x"],
            vec!["branch", "-D", "feat-x"]
        ]
    );
    assert!(r.message.contains("'api'") && r.message.contains("fatal: boom"));
    assert!(r.message.contains("undone") && r.message.contains("web"));
    assert!(!r.message.contains("infra"));
}

#[test]
fn r149_c_a_failure_at_link_1_has_nothing_to_undo_and_a_left_behind_one_is_removed() {
    let p = plan(&shop(), &Sources::All, "feat-x", &all_facts()).unwrap();
    let r = rollback(&p, 0, false, "x");
    assert!(r.undo.is_empty());
    assert!(r.message.contains("nothing to undo"));
    // post-checkout hook failing in link 2: links 2 and 1 go, newest first.
    let r = rollback(&p, 1, true, "hook failed");
    let order: Vec<&str> = r.undo.iter().map(|u| u.source.as_str()).collect();
    assert_eq!(order, ["api", "web"]);
}

fn rf(src: &str) -> RemoveFacts {
    RemoveFacts {
        source: src.into(),
        worktree: "feat-x".into(),
        repo: format!("/w/{src}").into(),
        path: format!("/w/.worktrees/{src}/feat-x").into(),
        missing: false,
        branch: Some("feat-x".into()),
        dirty: vec![],
        submodules: false,
        locked: None,
    }
}

#[test]
fn r149_d_a_dirty_third_worktree_refuses_the_whole_removal() {
    let mut f = vec![rf("web"), rf("api"), rf("infra")];
    f[2].dirty = vec!["src/a.rs".into()];
    let e = plan_remove("feat-x", &f, false, false).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(e.message.contains("'infra'") && e.message.contains("uncommitted"));
    // With force all three go, in order.
    let steps = plan_remove("feat-x", &f, true, true).unwrap();
    assert_eq!(steps.len(), 3);
    assert!(steps.iter().all(|s| s.force && !s.prune));
    assert_eq!(steps[0].delete_branch.as_deref(), Some("feat-x"));
}

#[test]
fn removal_refuses_submodules_and_locks_even_with_force() {
    let mut f = vec![rf("web"), rf("api")];
    f[0].submodules = true;
    assert!(plan_remove("s", &f, false, false).is_err());
    assert!(plan_remove("s", &f, true, false).is_ok());
    f[1].locked = Some("keep".into());
    let e = plan_remove("s", &f, true, false).unwrap_err();
    assert!(e.message.contains("'api'") && e.message.contains("locked (keep)"));
}

#[test]
fn removal_prunes_a_missing_folder_and_keeps_branches_by_default() {
    let mut f = vec![rf("web")];
    f[0].missing = true;
    f[0].dirty = vec!["x".into()];
    let s = plan_remove("s", &f, false, false).unwrap();
    assert!(s[0].prune && s[0].delete_branch.is_none());
    assert_eq!(
        plan_remove("s", &[], false, false).unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn r151_a_claude_and_codex_get_add_dirs_the_other_none_and_the_prompt() {
    let presets = preset::builtins();
    let m = members();
    let claude = agent_launch(&presets["claude"], Some("sid"), "feat-x", &m, "go").unwrap();
    assert_eq!(claude.cwd, m[0].path);
    assert_eq!(claude.command.matches("--add-dir").count(), 2);
    assert!(claude
        .command
        .contains(" --add-dir '/w/.worktrees/api/feat-x'"));
    assert!(claude
        .command
        .ends_with(r" --add-dir '/w/.worktrees/infra/it'\''s'"));
    assert!(claude
        .command
        .starts_with("claude --session-id 'sid' 'This task spans 3"));
    let codex = agent_launch(&presets["codex"], None, "feat-x", &m, "go").unwrap();
    assert_eq!(codex.command.matches("--add-dir").count(), 2);
    let mut plain = presets["claude"].clone();
    plain.add_dir = None;
    let none = agent_launch(&plain, None, "feat-x", &m, "go").unwrap();
    assert_eq!(none.command.matches("--add-dir").count(), 0);
    assert_eq!(
        claude.prompt,
        "This task spans 3 repositories, one Worktree each on branch feat-x: \
         web /w/.worktrees/web/feat-x (base develop), \
         api /w/.worktrees/api/feat-x (base main), \
         infra /w/.worktrees/infra/it's (base release/2.4). \
         Commit in each one; do not push unless asked.\n\ngo"
    );
    assert!(agent_launch(&plain, None, "n", &[], "go").is_none());
}

#[test]
fn the_add_dir_key_is_layered_like_prompt_flag() {
    let mut o = std::collections::BTreeMap::new();
    o.insert(
        "claude".to_string(),
        toml::from_str::<preset::PresetOverride>("add_dir = \"--dir\"").unwrap(),
    );
    o.insert(
        "mine".to_string(),
        toml::from_str::<preset::PresetOverride>("command = \"mine\"").unwrap(),
    );
    let r = preset::registry(&o).unwrap();
    assert_eq!(r["claude"].add_dir.as_deref(), Some("--dir"));
    assert_eq!(r["mine"].add_dir, None);
    assert_eq!(r["codex"].add_dir.as_deref(), Some("--add-dir"));
    assert_eq!(r["agy"].add_dir, None);
}

#[test]
fn the_prompt_without_a_user_text_is_the_paragraph_alone() {
    assert_eq!(
        prompt("n", &members()[..1], "  "),
        paragraph("n", &members()[..1])
    );
    assert!(paragraph("n", &members()[..1]).starts_with("This task spans 1 repositories"));
}

#[test]
fn lead_is_first_in_project_order() {
    let mut m = members();
    m.reverse();
    in_project_order(&shop(), &mut m);
    assert_eq!(lead(&m).unwrap().link, "web");
    assert_eq!(m[2].link, "infra");
}

fn s(x: &str) -> String {
    x.into()
}

#[test]
fn deliver_prefers_the_worktrees_own_agent_then_the_sets() {
    assert_eq!(deliver(&[s("p1")], &[s("p9")]), Deliver::Agent(s("p1")));
    assert_eq!(
        deliver(&[s("p1"), s("p2")], &[s("p9")]),
        Deliver::Choose(vec![s("p1"), s("p2")])
    );
    assert_eq!(deliver(&[], &[s("p9")]), Deliver::Agent(s("p9")));
    assert_eq!(
        deliver(&[], &[s("p8"), s("p9")]),
        Deliver::Choose(vec![s("p8"), s("p9")])
    );
    assert_eq!(deliver(&[], &[]), Deliver::Spawn);
}

fn row(src: &str, ahead: u32) -> PrRow {
    PrRow {
        source: src.into(),
        worktree: "feat-x".into(),
        ahead,
        base: "main".into(),
    }
}

fn pr(src: &str, repo: Option<&str>, n: u64) -> PrRef {
    PrRef {
        source: src.into(),
        repo: repo.map(Into::into),
        number: n,
    }
}

#[test]
fn rows_are_the_sources_with_commits() {
    let rows = pr_rows(&[row("web", 2), row("api", 0), row("infra", 1)]);
    let names: Vec<&str> = rows.iter().map(|r| r.source.as_str()).collect();
    assert_eq!(names, ["web", "infra"]);
}

#[test]
fn r151_b_the_cross_link_body_and_a_retry_after_a_failure_at_pr_2() {
    let made = [
        pr("web", Some("acme/web"), 11),
        pr("api", Some("acme/api"), 7),
        pr("infra", None, 3),
    ];
    let suffix = cross_link("feat-x", &made);
    assert_eq!(suffix, "Part of feat-x: acme/web#11, acme/api#7, infra#3");
    assert_eq!(
        linked_body("Adds x.", "feat-x", &suffix),
        format!("Adds x.\n\n{suffix}")
    );
    assert_eq!(linked_body("", "feat-x", &suffix), suffix);
    assert_eq!(
        linked_body(&format!("a\n\n{suffix}"), "feat-x", &suffix),
        format!("a\n\n{suffix}")
    );
    assert_eq!(edit_argv(11, "b"), ["pr", "edit", "11", "--body", "b"]);
    // PR 1 made, gh failed on 2: Retry creates 2 and 3 only.
    let rows = [row("web", 1), row("api", 1), row("infra", 1)];
    let r = retry_plan(&rows, &made[..1], &[]);
    let c: Vec<&str> = r.create.iter().map(|r| r.source.as_str()).collect();
    assert_eq!(c, ["api", "infra"]);
    // The first run creates all; after all exist the edit list is all.
    assert_eq!(retry_plan(&rows, &[], &[]).create.len(), 3);
    assert_eq!(retry_plan(&rows, &made, &[]).edit, [11, 7, 3]);
    // Nothing twice: all made, 11 edited already.
    let done = retry_plan(&rows, &made, &[11]);
    assert!(done.create.is_empty());
    assert_eq!(done.edit, [7, 3]);
}

#[test]
fn a_lone_pr_gets_no_cross_link() {
    assert_eq!(cross_link("n", &[pr("web", None, 1)]), "");
    assert!(retry_plan(&[row("web", 1)], &[pr("web", None, 1)], &[])
        .edit
        .is_empty());
}

#[test]
fn row_text_status_and_pr_chip() {
    assert_eq!(row_text("feat-x", 3), "feat-x · 3 sources");
    assert_eq!(row_text("feat-x", 1), "feat-x · 1 source");
    assert_eq!(
        row_status([None, Some(AgentStatus::Done), Some(AgentStatus::Waiting)]),
        Some(AgentStatus::Waiting)
    );
    assert_eq!(row_status([None]), None);
    use Checks::*;
    assert_eq!(
        pr_chip(&[Some(Passing), Some(Failing), None, Some(Failing)]),
        Some(("3 PRs · 2 failing".into(), Failing))
    );
    assert_eq!(
        pr_chip(&[Some(Passing), Some(Pending)]),
        Some(("2 PRs · 0 failing".into(), Pending))
    );
    assert_eq!(
        pr_chip(&[Some(Passing)]),
        Some(("1 PR · 0 failing".into(), Passing))
    );
    assert_eq!(pr_chip(&[None, None]), None);
}

#[test]
fn result_json_lists_each_worktree_with_its_base() {
    let p = plan(&shop(), &Sources::All, "feat-x", &all_facts()).unwrap();
    let made: Vec<(String, String)> = ["web", "api", "infra"]
        .iter()
        .enumerate()
        .map(|(i, s)| (s.to_string(), format!("w_{i}")))
        .collect();
    let j = p.result_json(&made);
    assert_eq!(j["set"], "feat-x");
    assert_eq!(j["project"], "shop");
    assert_eq!(j["worktrees"][1]["base"], "main");
    assert_eq!(j["worktrees"][2]["worktree"], "w_2");
}

#[test]
fn a_worktree_without_set_loads_from_an_older_state_and_set_round_trips() {
    let mut st = State::new(Path::new("/tmp"));
    let id = st.add_source(Path::new("/tmp/x"), None).unwrap();
    let (s, w) = st
        .add_worktree(&id, "feat-x", Path::new("/tmp/x-wt"), None)
        .unwrap();
    let json = serde_json::to_string(&st).unwrap();
    assert!(!json.contains("\"set\""));
    let back: State = serde_json::from_str(&json).unwrap();
    assert_eq!(back.worktree(&s, &w).unwrap().set, None);
    st.set_worktree_set(&s, &w, Some("feat-x".into())).unwrap();
    let json = serde_json::to_string(&st).unwrap();
    assert!(json.contains("\"set\":\"feat-x\""));
    let back: State = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.worktree(&s, &w).unwrap().set.as_deref(),
        Some("feat-x")
    );
}

#[test]
fn moving_a_set_worktree_out_of_its_project_drops_the_set() {
    let mut st = State::new(Path::new("/tmp"));
    let id = st.add_source(Path::new("/tmp/x"), None).unwrap();
    let (s, w) = st
        .add_worktree(&id, "feat-x", Path::new("/tmp/x-wt"), None)
        .unwrap();
    st.set_worktree_origin(&s, &w, Some("shop".into()), Some("main".into()))
        .unwrap();
    st.set_worktree_set(&s, &w, Some("feat-x".into())).unwrap();
    st.move_worktree(&s, &w, None, None).unwrap();
    assert_eq!(st.worktree(&s, &w).unwrap().set, None);
}

#[test]
fn r150_sets_group_by_project_in_link_order_and_name_the_other_links() {
    let mut sc = crate::navigator::tests::scene_a();
    crate::navigator::tests::add_set(&mut sc);
    let sets = sets(&sc.state, &sc.projects);
    assert_eq!(sets.len(), 1);
    let g = &sets[0];
    assert_eq!((g.project.as_str(), g.name.as_str()), ("shop", "feat-x"));
    let order: Vec<&str> = g.members.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(order, ["shop-web", "shop-api", "shared-ui"], "link order");
    let lead = g.lead().unwrap();
    assert_eq!(lead, &sc.worktrees["shop-web/feat-x"]);
    let (s, w) = &sc.worktrees["shop-api/feat-x"];
    assert_eq!(
        also_in(&sc.state, &sets, s, w).as_deref(),
        Some("also in shop-web, shared-ui")
    );
    let (s, w) = &sc.worktrees["checkout-v2"];
    assert_eq!(also_in(&sc.state, &sets, s, w), None);
    // A Worktree of the same name made by hand is not in the set.
    assert!(set_of(&sets, s, w).is_none());
}

#[test]
fn r150_a_the_plan_text_names_each_links_base() {
    let p = plan(&shop(), &Sources::All, "feat-x", &all_facts()).unwrap();
    let lines: Vec<String> = p
        .steps
        .iter()
        .map(|s| step_text(s, &p.name, Path::new("/w")))
        .collect();
    assert_eq!(
        lines,
        [
            "new branch feat-x from develop in ~/.worktrees/web/feat-x",
            "new branch feat-x from main in ~/.worktrees/api/feat-x",
            "new branch feat-x from release/2.4 in ~/.worktrees/infra/feat-x",
        ]
    );
    let mut f = all_facts();
    f[1].branch_exists = true;
    let e = plan(&shop(), &Sources::All, "feat-x", &f).unwrap_err();
    assert_eq!(refused_source(&e.message), Some("api"));
    assert_eq!(refused_source("--sources needs at least one source"), None);
}

#[test]
fn r151_c_a_pr_url_gives_its_repo_and_number() {
    assert_eq!(
        pr_ref("web", "https://github.com/acme/web/pull/11\n"),
        Some(pr("web", Some("acme/web"), 11))
    );
    assert_eq!(pr_ref("web", "nothing"), None);
    assert_eq!(pr_ref("web", "https://github.com/acme/web/pull/x"), None);
}

#[test]
fn r151_c_generate_drafts_once_from_the_combined_diff() {
    let part = |src: &str, base: &str| {
        (
            src.to_string(),
            crate::scm::DraftInput {
                diff: format!("diff --git a/{src}.rs b/{src}.rs\n"),
                rules: vec![(format!("{src}.md"), "rules".into())],
                log: vec![format!("{src} log")],
                branch: Some("feat-x".into()),
                base: Some(base.into()),
                commits: vec![format!("add {src}")],
            },
        )
    };
    let d = combined_draft("feat-x", &[part("web", "develop"), part("api", "main")]);
    assert_eq!(d.branch.as_deref(), Some("feat-x"));
    assert_eq!(d.base.as_deref(), Some("web ← develop, api ← main"));
    assert_eq!(d.commits, ["web: add web", "api: add api"]);
    assert_eq!(
        d.diff,
        "# web\ndiff --git a/web.rs b/web.rs\n# api\ndiff --git a/api.rs b/api.rs\n"
    );
    assert_eq!(d.rules.len(), 1, "the lead's rules");
    assert_eq!(d.log, ["web log"]);
}
