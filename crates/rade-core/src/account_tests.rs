use super::*;
use crate::agents::SpawnSpec;
use crate::config::Limits;
use crate::preset::{builtins, Invocation};
use crate::workspace::WorkspaceFile;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const HOME: &str = "/home/u";

const ACME: &str = r#"name = "acme"

[env]
GH_CONFIG_DIR = "~/.config/gh-acme"

[[account]]
agent = "claude"
name = "acme-work"
config_dir = "~/.claude-work"
default = true

[[account]]
agent = "claude"
name = "acme-team"
config_dir = "~/.claude-team"

[[account]]
agent = "codex"
name = "acme"
config_dir = "~/.codex"
default = true
"#;

const PERSONAL: &str = r#"[[account]]
agent = "claude"
name = "personal"
config_dir = "~/.claude"

[[account]]
agent = "codex"
name = "personal"
config_dir = "~/.codex"
"#;

fn parse(ws: &str, text: &str) -> Result<Vec<Account>> {
    let f = WorkspaceFile::parse(text).unwrap();
    accounts(
        ws,
        &f.account,
        Path::new("/w/acme/.rade/workspace.toml"),
        Path::new(HOME),
    )
}

fn lists() -> BTreeMap<String, Result<Vec<Account>>> {
    BTreeMap::from([
        ("acme".to_string(), parse("acme", ACME)),
        ("personal".to_string(), parse("personal", PERSONAL)),
        ("default".to_string(), Ok(vec![])),
    ])
}

fn ask<'a>(agent: Option<AgentKind>, explicit: Option<&'a str>) -> SpawnAsk<'a> {
    SpawnAsk {
        workspace: "acme",
        agent,
        explicit,
        parent: None,
    }
}

#[test]
fn r108_a_defaults_per_agent_and_tilde_expansion() {
    let all = parse("acme", ACME).unwrap();
    assert_eq!(all.len(), 3);
    let claude = default_for(&all, AgentKind::Claude).unwrap();
    assert_eq!(claude.name, "acme-work");
    assert_eq!(default_for(&all, AgentKind::Codex).unwrap().name, "acme");
    let team = all.iter().find(|a| a.name == "acme-team").unwrap();
    assert_eq!(team.config_dir, PathBuf::from("/home/u/.claude-team"));
    assert_eq!(team.declared_dir, "~/.claude-team");
    assert!(!team.default);
    assert_eq!(team.workspace, "acme");
    assert_eq!(AgentKind::Claude.var(), "CLAUDE_CONFIG_DIR");
    assert_eq!(AgentKind::Codex.var(), "CODEX_HOME");
}

#[test]
fn r108_b_two_defaults_is_an_error_none_takes_the_first() {
    let two = ACME.replacen(
        "name = \"acme-team\"\n",
        "name = \"acme-team\"\ndefault = true\n",
        1,
    );
    let e = parse("acme", &two).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    for want in ["workspace.toml", "acme-work", "acme-team"] {
        assert!(e.message.contains(want), "{want}: {}", e.message);
    }
    let none = ACME.replace("default = true\n", "");
    let all = parse("acme", &none).unwrap();
    assert_eq!(
        default_for(&all, AgentKind::Claude).unwrap().name,
        "acme-work"
    );
    assert_eq!(default_for(&all, AgentKind::Codex).unwrap().name, "acme");
    assert_eq!(all.iter().filter(|a| a.default).count(), 2);
}

#[test]
fn bad_agent_or_empty_fields_are_validation_errors() {
    let e = parse(
        "acme",
        "[[account]]\nagent = \"opencode\"\nname = \"x\"\nconfig_dir = \"~/.x\"\n",
    )
    .unwrap_err();
    assert!(e.message.contains("opencode") && e.message.contains("workspace.toml"));
    let e = parse("acme", "[[account]]\nagent = \"claude\"\nname = \"x\"\n").unwrap_err();
    assert!(e.message.contains("config_dir"), "{}", e.message);
    let e = parse(
        "acme",
        "[[account]]\nagent = \"claude\"\nconfig_dir = \"~/.x\"\n",
    )
    .unwrap_err();
    assert!(e.message.contains("name"), "{}", e.message);
    // A file without accounts has none, not an error.
    assert_eq!(parse("acme", "name = \"acme\"\n").unwrap(), vec![]);
}

#[test]
fn r108_c_account_of_another_workspace_is_forbidden() {
    let e = resolve_spawn(&ask(Some(AgentKind::Claude), Some("personal")), &lists()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert_eq!(e.kind.exit_code(), 7);
    assert!(
        e.message.contains("'personal'") && e.message.contains("'acme'"),
        "{}",
        e.message
    );
    // The same name declared in the Pane's own Workspace is allowed.
    let mut l = lists();
    l.insert("acme".into(), parse("acme", &format!("{ACME}{PERSONAL}")));
    let a = resolve_spawn(&ask(Some(AgentKind::Claude), Some("personal")), &l)
        .unwrap()
        .unwrap();
    assert_eq!(
        (a.workspace.as_str(), a.name.as_str()),
        ("acme", "personal")
    );
    // Unknown names and another agent's account are bad arguments.
    let e = resolve_spawn(&ask(Some(AgentKind::Claude), Some("nope")), &lists()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    let e = resolve_spawn(&ask(Some(AgentKind::Claude), Some("acme")), &lists()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(e.message.contains("codex"), "{}", e.message);
    // `--account` with a Preset that has no account kind.
    let e = resolve_spawn(&ask(None, Some("acme-team")), &lists()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
}

#[test]
fn r108_c_state_spawn_refuses_an_account_of_another_workspace() {
    let mut st = State::new(Path::new(HOME));
    st.add_workspace(Path::new("/w/acme"), "acme").unwrap();
    st.add_workspace(Path::new("/w/personal"), "personal")
        .unwrap();
    st.add_source(Path::new("/w/acme/api"), None).unwrap();
    let personal = parse("personal", PERSONAL).unwrap().remove(0);
    let before = st.all_panes().count();
    let e = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                agent: Some("claude".into()),
                account: Some(personal),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert_eq!(st.all_panes().count(), before);
    let team = parse("acme", ACME).unwrap().remove(1);
    let p = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                agent: Some("claude".into()),
                account: Some(team),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    assert_eq!(st.pane(&p).unwrap().account.as_deref(), Some("acme-team"));
}

#[test]
fn child_inherits_parent_account_else_default() {
    let l = lists();
    let child = SpawnAsk {
        parent: Some("acme-team"),
        ..ask(Some(AgentKind::Claude), None)
    };
    assert_eq!(
        resolve_spawn(&child, &l).unwrap().unwrap().name,
        "acme-team"
    );
    // A root spawn: the Workspace default.
    assert_eq!(
        resolve_spawn(&ask(Some(AgentKind::Claude), None), &l)
            .unwrap()
            .unwrap()
            .name,
        "acme-work"
    );
    // A parent's claude account doesn't carry over to a codex child.
    let codex_child = SpawnAsk {
        parent: Some("acme-team"),
        ..ask(Some(AgentKind::Codex), None)
    };
    assert_eq!(
        resolve_spawn(&codex_child, &l).unwrap().unwrap().name,
        "acme"
    );
    // A parent account the target Workspace doesn't declare: its default.
    let elsewhere = SpawnAsk {
        parent: Some("personal"),
        ..ask(Some(AgentKind::Claude), None)
    };
    assert_eq!(
        resolve_spawn(&elsewhere, &l).unwrap().unwrap().name,
        "acme-work"
    );
    // An explicit account beats the parent's.
    let both = SpawnAsk {
        parent: Some("acme-team"),
        ..ask(Some(AgentKind::Claude), Some("acme-work"))
    };
    assert_eq!(resolve_spawn(&both, &l).unwrap().unwrap().name, "acme-work");
    // No account kind (a shell, `--cmd`): no account, whatever the parent.
    let shell = SpawnAsk {
        parent: Some("acme-team"),
        ..ask(None, None)
    };
    assert_eq!(resolve_spawn(&shell, &l).unwrap(), None);
    // A Workspace with no accounts: none.
    let bare = SpawnAsk {
        workspace: "default",
        ..ask(Some(AgentKind::Claude), None)
    };
    assert_eq!(resolve_spawn(&bare, &l).unwrap(), None);
}

#[test]
fn a_broken_account_list_fails_only_spawns_that_need_an_account() {
    let mut l = lists();
    l.insert(
        "acme".into(),
        parse("acme", &ACME.replace("acme-team", "acme-work")),
    );
    let e = resolve_spawn(&ask(Some(AgentKind::Claude), None), &l).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(e.message.contains("workspace.toml"));
    assert_eq!(resolve_spawn(&ask(None, None), &l).unwrap(), None);
}

#[test]
fn account_env_replaces_the_workspace_variable() {
    let team = parse("acme", ACME).unwrap().remove(1);
    let mut env = vec![
        (
            "CLAUDE_CONFIG_DIR".to_string(),
            "/home/u/.claude".to_string(),
        ),
        ("RADE_WORKSPACE".to_string(), "acme".to_string()),
        ("RADE_ACCOUNT".to_string(), "old".to_string()),
    ];
    apply_env(&mut env, &team).unwrap();
    let get = |k: &str| {
        env.iter()
            .filter(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(get("CLAUDE_CONFIG_DIR"), ["/home/u/.claude-team"]);
    assert_eq!(get("RADE_ACCOUNT"), ["acme-team"]);
    assert_eq!(get("RADE_WORKSPACE"), ["acme"]);
    assert!(get("CODEX_HOME").is_empty());
}

#[test]
fn r108_j_preset_prefix_with_another_dir_is_forbidden() {
    let home = Path::new(HOME);
    let team = parse("acme", ACME).unwrap().remove(1);
    let e = check_command("claude", "CLAUDE_CONFIG_DIR=~/.claude claude", &team, home).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert_eq!(
        e.message,
        "preset 'claude' sets CLAUDE_CONFIG_DIR=~/.claude but account 'acme-team' uses ~/.claude-team (ADR-0035)"
    );
    // The same directory, however it is written, is allowed.
    for ok in [
        "CLAUDE_CONFIG_DIR=~/.claude-team claude",
        "CLAUDE_CONFIG_DIR='/home/u/.claude-team/' claude --x",
        "CLAUDE_CONFIG_DIR=$HOME/.claude-team claude",
        "CLAUDE_CONFIG_DIR=\"${HOME}/.claude-team\" claude",
        "claude --dangerously-skip-permissions",
        "CODEX_HOME=/elsewhere claude",
    ] {
        assert!(check_command("claude", ok, &team, home).is_ok(), "{ok}");
    }
    // `env VAR=… cmd` and a later prefix word are prefixes too.
    for bad in [
        "env CLAUDE_CONFIG_DIR=~/.claude claude",
        "FOO=1 CLAUDE_CONFIG_DIR=/home/u/.claude claude",
        "CLAUDE_CONFIG_DIR='/home/u/.claude' 'claude' --resume x",
    ] {
        let e = check_command("claude", bad, &team, home).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Forbidden, "{bad}");
    }
    // A word after the program is not a prefix.
    assert!(check_command("claude", "claude CLAUDE_CONFIG_DIR=/x", &team, home).is_ok());
}

#[test]
fn restore_refuses_invocation_with_another_dir() {
    let home = Path::new(HOME);
    let team = parse("acme", ACME).unwrap().remove(1);
    let inv = |dir: &str| Invocation {
        argv: vec!["claude".into()],
        env: BTreeMap::from([("CLAUDE_CONFIG_DIR".to_string(), dir.to_string())]),
    };
    let e = check_invocation("claude", &inv("/home/u/.claude"), &team, home).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert!(e.message.contains("'claude'") && e.message.contains("'acme-team'"));
    assert!(check_invocation("claude", &inv("/home/u/.claude-team"), &team, home).is_ok());

    // Session Restore: the refused Pane comes back as a plain shell that
    // prints the message, keeping its account.
    let root = crate::paths::outside_temp_dir().join(format!("rade-acct-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".rade")).unwrap();
    std::fs::write(root.join(".rade/workspace.toml"), ACME).unwrap();
    let mut st = State::new(Path::new(HOME));
    st.add_workspace(&root, "acme").unwrap();
    st.add_source(&root.join("api"), None).unwrap();
    let spawn = |st: &mut State| {
        st.spawn(
            SpawnSpec {
                source: Some("api".into()),
                agent: Some("claude".into()),
                command: Some("claude".into()),
                account: Some(team.clone()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap()
    };
    let bad = spawn(&mut st);
    let good = spawn(&mut st);
    st.set_invocation(&bad, inv("/home/u/.claude")).unwrap();
    st.set_invocation(&good, inv("/home/u/.claude-team"))
        .unwrap();
    let mut back: State = serde_json::from_value(serde_json::to_value(&st).unwrap()).unwrap();
    back.prepare_restore(&builtins(), 0);
    let refused = back.refuse_restores(&|_: &str, _: &str| builtins(), home);
    assert_eq!(refused, vec![bad.clone()]);
    let p = back.pane(&bad).unwrap();
    assert_eq!(p.command(), None);
    assert_eq!(p.account.as_deref(), Some("acme-team"));
    assert_eq!(p.agent, None);
    assert!(p.notice.as_deref().unwrap().contains("acme-team"));
    let g = back.pane(&good).unwrap();
    assert!(g.command().unwrap().contains(".claude-team"));
    assert_eq!(g.notice, None);
    // The account's env for the restored Pane.
    let a = back
        .pane_account(&bad, Some(AgentKind::Claude), home)
        .unwrap()
        .unwrap();
    assert_eq!(a.config_dir, PathBuf::from("/home/u/.claude-team"));
    // Pausing a Source restarts its Panes the same way (R19.3): only that
    // Source's Panes are checked.
    let presets = |_: &str, _: &str| builtins();
    let mut paused = st.clone();
    let api = paused.find_source("api").unwrap().id.clone();
    paused.pause_source(&api, &presets, true, 1).unwrap();
    assert!(paused
        .refuse_restores_in(&presets, home, Some("general"))
        .is_empty());
    assert_eq!(
        paused.refuse_restores_in(&presets, home, Some(&api)),
        vec![bad.clone()]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pane_account_round_trips_through_state_json() {
    let mut st = State::new(Path::new(HOME));
    st.add_workspace(Path::new("/w/acme"), "acme").unwrap();
    st.add_source(Path::new("/w/acme/api"), None).unwrap();
    let team = parse("acme", ACME).unwrap().remove(1);
    let p = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                agent: Some("claude".into()),
                account: Some(team),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    let v = serde_json::to_value(&st).unwrap();
    let text = v.to_string();
    assert!(text.contains(r#""account":"acme-team""#), "{text}");
    let back: State = serde_json::from_value(v).unwrap();
    assert_eq!(back.pane(&p).unwrap().account.as_deref(), Some("acme-team"));
    // A Pane without one writes no key.
    let q = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    let pane = serde_json::to_value(st.pane(&q).unwrap()).unwrap();
    assert!(pane.get("account").is_none());
}

#[test]
fn r108_l_shared_name_is_ambiguous_without_agent() {
    let all = parse("personal", PERSONAL).unwrap();
    let e = find(&all, "personal", None, "personal").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Ambiguous);
    assert_eq!(e.kind.exit_code(), 4);
    assert!(e.message.contains("claude") && e.message.contains("codex"));
    let c = find(&all, "personal", Some(AgentKind::Codex), "personal").unwrap();
    assert_eq!(c.config_dir, PathBuf::from("/home/u/.codex"));
    assert_eq!(
        find(&all, "nope", None, "personal").unwrap_err().kind,
        ErrorKind::NotFound
    );
    // A spawn looks the name up for the Preset's agent: never ambiguous.
    let l = lists();
    let a = resolve_spawn(
        &SpawnAsk {
            workspace: "personal",
            agent: Some(AgentKind::Claude),
            explicit: Some("personal"),
            parent: None,
        },
        &l,
    )
    .unwrap()
    .unwrap();
    assert_eq!(a.config_dir, PathBuf::from("/home/u/.claude"));
    // `set_default` refuses the shared name without an agent, file unchanged.
    let e = set_default_in_toml(PERSONAL, None, "personal").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Ambiguous);
    let out = set_default_in_toml(PERSONAL, Some(AgentKind::Codex), "personal").unwrap();
    assert_eq!(
        out,
        PERSONAL.replace(
            "name = \"personal\"\nconfig_dir = \"~/.codex\"\n",
            "name = \"personal\"\nconfig_dir = \"~/.codex\"\ndefault = true\n"
        )
    );
    // Two claude accounts with one name: a validation error.
    let dup = format!("{PERSONAL}\n[[account]]\nagent = \"claude\"\nname = \"personal\"\nconfig_dir = \"~/.claude-2\"\n");
    let e = parse("personal", &dup).unwrap_err();
    assert!(e.message.contains("workspace.toml") && e.message.contains("'personal'"));
}

#[test]
fn add_account_keeps_other_bytes_and_moves_default() {
    let two: String = ACME
        .split("[[account]]")
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join("[[account]]");
    assert!(!two.contains("acme-team"));
    let decl = AccountDecl {
        agent: "claude".into(),
        name: "acme-team".into(),
        config_dir: "~/.claude-team".into(),
        default: false,
        detailed_limits: false,
        ..Default::default()
    };
    let added = add_to_toml(&two, &decl).unwrap();
    assert!(added.starts_with(&two), "{added}");
    assert_eq!(
        &added[two.len()..],
        "\n[[account]]\nagent = \"claude\"\nname = \"acme-team\"\nconfig_dir = \"~/.claude-team\"\n"
    );
    let all = parse("acme", &added).unwrap();
    assert_eq!(all.len(), 3);
    // `default` moves the flag: one line removed, one added.
    let moved = set_default_in_toml(&added, None, "acme-team").unwrap();
    let all = parse("acme", &moved).unwrap();
    assert_eq!(
        default_for(&all, AgentKind::Claude).unwrap().name,
        "acme-team"
    );
    assert_eq!(default_for(&all, AgentKind::Codex).unwrap().name, "acme");
    let removed: Vec<&str> = added.lines().filter(|l| !moved.contains(l)).collect();
    assert!(removed.is_empty(), "{removed:?}");
    assert_eq!(moved.lines().count(), added.lines().count());
    assert_eq!(moved.matches("default = true").count(), 2);
    // `add --default` takes the flag from the agent's default.
    let d = AccountDecl {
        name: "acme-3".into(),
        config_dir: "~/.claude-3".into(),
        default: true,
        ..decl.clone()
    };
    let out = add_to_toml(&two, &d).unwrap();
    let all = parse("acme", &out).unwrap();
    assert_eq!(default_for(&all, AgentKind::Claude).unwrap().name, "acme-3");
    // Refusals: a used name, an unknown agent, an empty dir.
    for bad in [
        AccountDecl {
            name: "acme-work".into(),
            ..decl.clone()
        },
        AccountDecl {
            agent: "agy".into(),
            ..decl.clone()
        },
        AccountDecl {
            config_dir: " ".into(),
            ..decl.clone()
        },
    ] {
        assert_eq!(add_to_toml(&two, &bad).unwrap_err().kind, ErrorKind::Usage);
    }
    // An explicit `default = false` becomes `true` in place.
    let f = two.replacen(
        "name = \"acme\"\nconfig_dir = \"~/.codex\"\ndefault = true",
        "name = \"acme\"\nconfig_dir = \"~/.codex\"\ndefault = false",
        1,
    );
    assert!(f.contains("default = false"));
    let out = set_default_in_toml(&f, Some(AgentKind::Codex), "acme").unwrap();
    assert_eq!(out, f.replace("default = false", "default = true"));
    // An empty or missing file gets just the table.
    assert_eq!(
        add_to_toml("", &decl).unwrap(),
        "[[account]]\nagent = \"claude\"\nname = \"acme-team\"\nconfig_dir = \"~/.claude-team\"\n"
    );
    // A file without a final newline.
    let out = add_to_toml("name = \"acme\"", &decl).unwrap();
    assert!(out.starts_with("name = \"acme\"\n\n[[account]]\n"), "{out}");
}

fn tmp_root(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rade-account-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn account_files_are_written_atomically_creating_dot_rade() {
    let root = tmp_root("files");
    let decl = AccountDecl {
        agent: "claude".into(),
        name: "acme-work".into(),
        config_dir: "~/.claude-work".into(),
        default: false,
        detailed_limits: false,
        ..Default::default()
    };
    // No `.rade/` yet: created, the file holds just the table.
    let file = add_account_file(&root, &decl).unwrap();
    assert_eq!(file, root.join(".rade/workspace.toml"));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("[[account]]\n"), "{text}");
    // A second account, then the default moves to it.
    let team = AccountDecl {
        name: "acme-team".into(),
        config_dir: "~/.claude-team".into(),
        ..decl.clone()
    };
    add_account_file(&root, &team).unwrap();
    set_default_file(&root, None, "acme-team").unwrap();
    let f = WorkspaceFile::read(&root).unwrap();
    let all = accounts("acme", &f.account, &file, Path::new(HOME)).unwrap();
    assert_eq!(
        default_for(&all, AgentKind::Claude).unwrap().name,
        "acme-team"
    );
    // A refusal leaves the file as it was, and no temp file behind.
    let before = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        add_account_file(&root, &team).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(
        set_default_file(&root, None, "nobody").unwrap_err().kind,
        ErrorKind::NotFound
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    let names: Vec<_> = std::fs::read_dir(root.join(".rade"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["workspace.toml"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn notice_is_typed_into_the_shell_as_a_printf() {
    let input = notice_input("rade: not resumed: preset 'claude' sets X=~/a b's");
    assert_eq!(
        input,
        " printf '%s\\n' 'rade: not resumed: preset '\\''claude'\\'' sets X=~/a b'\\''s'\n"
    );
}

/// A temp Workspace `ws` whose `workspace.toml` is `text`, with Source `api`.
fn ws_state(tag: &str, ws: &str, text: &str) -> (State, PathBuf) {
    let root =
        crate::paths::outside_temp_dir().join(format!("rade-acct-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".rade")).unwrap();
    std::fs::write(root.join(".rade/workspace.toml"), text).unwrap();
    let mut st = State::new(Path::new(HOME));
    st.add_workspace(&root, ws).unwrap();
    st.add_source(&root.join("api"), None).unwrap();
    (st, root)
}

#[test]
fn refused_restore_keeps_the_account_env_when_a_codex_account_shares_its_name() {
    // R108.7 / R108-k with the R108-l fixture: `personal` is both a claude
    // and a codex account. The refused Pane is no longer an Agent Pane, so
    // its env must come from the account's recorded agent.
    let home = Path::new(HOME);
    let (mut st, root) = ws_state("shared", "personal", PERSONAL);
    let claude = parse("personal", PERSONAL).unwrap().remove(0);
    let bad = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                agent: Some("claude".into()),
                command: Some("claude".into()),
                account: Some(claude),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    st.set_invocation(
        &bad,
        Invocation {
            argv: vec!["claude".into()],
            env: BTreeMap::from([("CLAUDE_CONFIG_DIR".to_string(), "/home/u/.other".into())]),
        },
    )
    .unwrap();
    let mut back: State = serde_json::from_value(serde_json::to_value(&st).unwrap()).unwrap();
    back.prepare_restore(&builtins(), 0);
    assert_eq!(
        back.refuse_restores(&|_: &str, _: &str| builtins(), home),
        vec![bad.clone()]
    );
    assert_eq!(back.pane(&bad).unwrap().agent, None);
    // No agent kind known from the (cleared) Preset: the recorded one decides.
    let a = back.pane_account(&bad, None, home).unwrap().unwrap();
    assert_eq!(a.agent, AgentKind::Claude);
    assert_eq!(a.config_dir, PathBuf::from("/home/u/.claude"));
    // And on the next restart too (state.json keeps it).
    let again: State = serde_json::from_value(serde_json::to_value(&back).unwrap()).unwrap();
    let a = again.pane_account(&bad, None, home).unwrap().unwrap();
    assert_eq!(a.agent, AgentKind::Claude);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn fanout_members_and_judge_run_under_the_workspace_account() {
    // R108.2 / R108.3 / R108.7 for the Agents RADE starts itself (Fan-out
    // Members, the Judge): parent's account, else the Workspace default;
    // the lock and the Preset prefix check hold as for `rade spawn`.
    use crate::fanout::{JudgeSpawn, MemberSpawn, Start};
    let home = Path::new(HOME);
    let (mut st, root) = ws_state("fanout", "acme", ACME);
    let api = st.find_source("api").unwrap().id.clone();
    let claude = Some(AgentKind::Claude);
    let acct = |st: &State, kind, cmd: &str, caller: Option<&str>| {
        st.agent_account(&api, "claude", kind, cmd, caller, home)
    };
    assert_eq!(
        acct(&st, claude, "claude", None).unwrap().unwrap().name,
        "acme-work"
    );
    assert_eq!(
        acct(&st, Some(AgentKind::Codex), "codex", None)
            .unwrap()
            .unwrap()
            .name,
        "acme"
    );
    assert_eq!(acct(&st, None, "fake", None).unwrap(), None);
    let team = parse("acme", ACME).unwrap().remove(1);
    let parent = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                agent: Some("claude".into()),
                command: Some("claude".into()),
                account: Some(team.clone()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    assert_eq!(
        acct(&st, claude, "claude", Some(&parent))
            .unwrap()
            .unwrap()
            .name,
        "acme-team"
    );
    let e = acct(&st, claude, "CLAUDE_CONFIG_DIR=~/.claude claude", None).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert!(e.message.contains("'claude'") && e.message.contains("acme-work"));

    // The Panes record the account; another Workspace's is refused.
    let main = st.source(&api).unwrap().worktrees[0].id.clone();
    let member = |account: Option<Account>| MemberSpawn {
        label: "claude".into(),
        agent: "claude".into(),
        worktree: main.clone(),
        command: "claude".into(),
        session_id: None,
        account,
    };
    let start = |members| Start {
        source: api.clone(),
        id: "fanout-1".into(),
        prompt: "p".into(),
        base: "main".into(),
        base_commit: "abc".into(),
        members,
        caller: None,
        now: 1,
    };
    let foreign = parse("personal", PERSONAL).unwrap().remove(0);
    let n = st.all_panes().count();
    let e = st
        .start_fanout(
            start(vec![
                member(Some(team.clone())),
                member(Some(foreign.clone())),
            ]),
            &Limits::default(),
        )
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert_eq!(st.all_panes().count(), n);
    let (_, panes) = st
        .start_fanout(
            start(vec![member(Some(team.clone())), member(None)]),
            &Limits::default(),
        )
        .unwrap();
    assert_eq!(
        st.pane(&panes[0]).unwrap().account.as_deref(),
        Some("acme-team")
    );
    assert_eq!(st.pane(&panes[1]).unwrap().account, None);
    let a = st.pane_account(&panes[0], None, home).unwrap().unwrap();
    assert_eq!(a.name, "acme-team");
    let judge = |account| JudgeSpawn {
        agent: "claude".into(),
        command: "claude".into(),
        session_id: None,
        dir: "/j".into(),
        read_only: true,
        now: 2,
        account,
    };
    for p in &panes {
        st.set_status(p, Some(crate::AgentStatus::Done), None, None, 2)
            .unwrap();
    }
    let e = st.add_judge("fanout-1", judge(Some(foreign))).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    let work = parse("acme", ACME).unwrap().remove(0);
    let jp = st.add_judge("fanout-1", judge(Some(work))).unwrap();
    assert_eq!(st.pane(&jp).unwrap().account.as_deref(), Some("acme-work"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn r109_h_detailed_limits_off_keeps_every_other_byte() {
    let on = ACME.replace(
        "name = \"acme-work\"\nconfig_dir = \"~/.claude-work\"\ndefault = true\n",
        "name = \"acme-work\"\nconfig_dir = \"~/.claude-work\"\ndefault = true\ndetailed_limits = true  # opt-in\n",
    );
    let off = set_detailed_in_toml(&on, AgentKind::Claude, "acme-work", false).unwrap();
    assert_eq!(
        off,
        on.replace(
            "detailed_limits = true  # opt-in",
            "detailed_limits = false  # opt-in"
        )
    );
    let parsed = parse("acme", &off).unwrap();
    assert!(parsed.iter().all(|a| !a.detailed_limits));
    // Already off (no key): unchanged text; on again adds the key.
    assert_eq!(
        set_detailed_in_toml(ACME, AgentKind::Claude, "acme-work", false).unwrap(),
        ACME
    );
    let again = set_detailed_in_toml(ACME, AgentKind::Claude, "acme-team", true).unwrap();
    assert!(parse("acme", &again)
        .unwrap()
        .iter()
        .any(|a| a.name == "acme-team" && a.detailed_limits));
    assert!(set_detailed_in_toml(ACME, AgentKind::Codex, "acme-work", false).is_err());
}

#[test]
fn r110_g_next_default_changes_later_agents_only_in_memory() {
    // The forecast Toast's action (R110.4, F.20): Agents opened later
    // default to acme-team; workspace.toml and running Panes are untouched;
    // the choice is not saved in state.json.
    let home = Path::new(HOME);
    let (mut st, root) = ws_state("nextdef", "acme", ACME);
    let api = st.find_source("api").unwrap().id.clone();
    let before = std::fs::read(root.join(".rade/workspace.toml")).unwrap();
    let running = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                agent: Some("claude".into()),
                command: Some("claude".into()),
                account: st
                    .agent_account(
                        &api,
                        "claude",
                        Some(AgentKind::Claude),
                        "claude",
                        None,
                        home,
                    )
                    .unwrap(),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    assert_eq!(
        st.pane(&running).unwrap().account.as_deref(),
        Some("acme-work")
    );
    st.set_next_default("acme", AgentKind::Claude, "acme-team");
    let later = st
        .agent_account(
            &api,
            "claude",
            Some(AgentKind::Claude),
            "claude",
            None,
            home,
        )
        .unwrap()
        .unwrap();
    assert_eq!(later.name, "acme-team");
    let l = st.account_lists(home).remove("acme").unwrap().unwrap();
    let defaults: Vec<(&str, &str)> = l
        .iter()
        .filter(|a| a.default)
        .map(|a| (a.agent.as_str(), a.name.as_str()))
        .collect();
    assert_eq!(defaults, vec![("claude", "acme-team"), ("codex", "acme")]);
    assert_eq!(
        st.pane(&running).unwrap().account.as_deref(),
        Some("acme-work")
    );
    assert_eq!(
        std::fs::read(root.join(".rade/workspace.toml")).unwrap(),
        before
    );
    let saved = serde_json::to_string(&st).unwrap();
    assert!(!saved.contains("next_defaults") && !saved.contains("acme-team"));
    // An explicit `rade account default` later wins over the Toast's choice.
    st.clear_next_default("acme", AgentKind::Claude);
    let l = st.account_lists(home).remove("acme").unwrap().unwrap();
    assert!(l.iter().any(|a| a.name == "acme-work" && a.default));
    assert!(!l.iter().any(|a| a.name == "acme-team" && a.default));
    // An undeclared name changes nothing.
    st.set_next_default("acme", AgentKind::Codex, "gone");
    let l = st.account_lists(home).remove("acme").unwrap().unwrap();
    assert!(l.iter().any(|a| a.name == "acme" && a.default));
    let _ = std::fs::remove_dir_all(&root);
}

// ---- M47 (R125, R126, ADR-0047) ----

const USER: &str = r#"# my logins
[[account]]
agent = "claude"
name = "cl"
config_dir = "~/.claude"
default = true

[[account]]
agent = "claude"
name = "clw"
config_dir = "~/.claude-work"
"#;

fn tmp_file(tag: &str, name: &str, text: &str) -> PathBuf {
    let d = crate::paths::outside_temp_dir().join(format!("rade-m47-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join(name);
    std::fs::write(&f, text).unwrap();
    f
}

fn get<'a>(env: &'a [(String, String)], k: &str) -> Vec<&'a str> {
    env.iter()
        .filter(|(n, _)| n == k)
        .map(|(_, v)| v.as_str())
        .collect()
}

#[test]
fn r125_b_env_order_and_reserved_keys() {
    let file = tmp_file("order", "clw.env", "B=file\nC=file\n");
    let text = format!(
        "[[account]]\nagent = \"claude\"\nname = \"clw\"\nconfig_dir = \"~/.claude-work\"\nenv_file = \"{}\"\n[account.env]\nC = \"inline\"\n",
        file.display()
    );
    let a = parse("acme", &text).unwrap().remove(0);
    let mut env = vec![
        ("A".to_string(), "ws".to_string()),
        ("B".to_string(), "ws".to_string()),
    ];
    apply_env(&mut env, &a).unwrap();
    assert_eq!(get(&env, "A"), ["ws"]);
    assert_eq!(get(&env, "B"), ["file"]);
    assert_eq!(get(&env, "C"), ["inline"]);
    let n = env.len();
    assert_eq!(
        env[n - 2],
        ("CLAUDE_CONFIG_DIR".into(), "/home/u/.claude-work".into())
    );
    assert_eq!(env[n - 1], ("RADE_ACCOUNT".into(), "clw".into()));

    // A reserved key in [account.env] is a validation error naming the file.
    let bad = "[[account]]\nagent = \"claude\"\nname = \"x\"\nconfig_dir = \"~/.x\"\n[account.env]\nCLAUDE_CONFIG_DIR = \"~/.y\"\n";
    let e = parse("acme", bad).unwrap_err().message;
    assert!(
        e.contains("workspace.toml") && e.contains("CLAUDE_CONFIG_DIR"),
        "{e}"
    );
    // A codex account may set CLAUDE_CONFIG_DIR (not its variable).
    let other = "[[account]]\nagent = \"codex\"\nname = \"x\"\nconfig_dir = \"~/.x\"\n[account.env]\nCLAUDE_CONFIG_DIR = \"~/.y\"\n";
    assert!(parse("acme", other).is_ok());
    // A reserved key in the env file: the file is named; the login is still set.
    let rf = tmp_file("order", "rade.env", "RADE_X=1\n");
    let text = format!(
        "[[account]]\nagent = \"claude\"\nname = \"r\"\nconfig_dir = \"~/.r\"\nenv_file = \"{}\"\n",
        rf.display()
    );
    let a = parse("acme", &text).unwrap().remove(0);
    let mut env = vec![];
    let e = apply_env(&mut env, &a).unwrap_err().message;
    assert!(e.contains("rade.env") && e.contains("RADE_X"), "{e}");
    assert_eq!(get(&env, "RADE_ACCOUNT"), ["r"]);
    assert!(get(&env, "RADE_X").is_empty());
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn r125_d_missing_env_file_names_the_file() {
    let text = "[[account]]\nagent = \"claude\"\nname = \"clw\"\nconfig_dir = \"~/.c\"\nenv_file = \"/nonexistent/clw.env\"\n";
    let a = parse("acme", text).unwrap().remove(0);
    let e = account_env(&a).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(e.message.contains("/nonexistent/clw.env"), "{}", e.message);
}

#[test]
fn r125_e_user_accounts_merge_own_only_and_defaults() {
    let home = Path::new(HOME);
    let user = tmp_file("merge", "accounts.toml", USER);
    let (mut st, acme) = ws_state("m47-acme", "acme", "[[account]]\nagent = \"claude\"\nname = \"acme-work\"\nconfig_dir = \"~/.claude-acme\"\ndefault = true\n");
    st.user_accounts_file = Some(user.clone());
    let corp =
        crate::paths::outside_temp_dir().join(format!("rade-acct-m47-corp-{}", std::process::id()));
    std::fs::create_dir_all(corp.join(".rade")).unwrap();
    std::fs::write(corp.join(".rade/workspace.toml"), "accounts = \"own\"\n").unwrap();
    st.add_workspace(&corp, "corp").unwrap();
    let lists = st.account_lists(home);
    let names = |ws: &str| -> Vec<(String, bool, Scope)> {
        lists[ws]
            .as_ref()
            .unwrap()
            .iter()
            .map(|a| (a.name.clone(), a.default, a.scope))
            .collect()
    };
    assert_eq!(
        names("acme"),
        [
            ("acme-work".into(), true, Scope::Workspace),
            ("cl".into(), false, Scope::User),
            ("clw".into(), false, Scope::User)
        ]
    );
    assert_eq!(
        names("default"),
        [
            ("cl".into(), true, Scope::User),
            ("clw".into(), false, Scope::User)
        ]
    );
    assert!(names("corp").is_empty());
    // The user account carries the Workspace it is used in (the lock).
    assert_eq!(lists["acme"].as_ref().unwrap()[2].workspace, "acme");
    let spawn = |ws: &str| {
        resolve_spawn(
            &SpawnAsk {
                workspace: ws,
                agent: Some(AgentKind::Claude),
                explicit: Some("clw"),
                parent: None,
            },
            &lists,
        )
    };
    assert_eq!(spawn("acme").unwrap().unwrap().name, "clw");
    let e = spawn("corp").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert!(
        e.message.contains("clw") && e.message.contains("corp"),
        "{}",
        e.message
    );
    // The same name in both files: an error naming both.
    std::fs::write(
        acme.join(".rade/workspace.toml"),
        "[[account]]\nagent = \"claude\"\nname = \"clw\"\nconfig_dir = \"~/.x\"\n",
    )
    .unwrap();
    let e = st
        .account_lists(home)
        .remove("acme")
        .unwrap()
        .unwrap_err()
        .message;
    assert!(
        e.contains("workspace.toml") && e.contains("accounts.toml") && e.contains("clw"),
        "{e}"
    );
    // A bad `accounts` value.
    std::fs::write(corp.join(".rade/workspace.toml"), "accounts = \"mine\"\n").unwrap();
    assert!(st.account_lists(home).remove("corp").unwrap().is_err());
    for d in [acme, corp, user.parent().unwrap().to_path_buf()] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn user_default_is_used_when_the_workspace_names_none() {
    let home = Path::new(HOME);
    let user = tmp_file(
        "udef",
        "accounts.toml",
        USER.replace("default = true\n", "")
            .replace(
                "\"~/.claude-work\"\n",
                "\"~/.claude-work\"\ndefault = true\n",
            )
            .as_str(),
    );
    let (mut st, root) = ws_state(
        "m47-udef",
        "acme",
        "[[account]]\nagent = \"claude\"\nname = \"acme-work\"\nconfig_dir = \"~/.claude-acme\"\n",
    );
    st.user_accounts_file = Some(user.clone());
    let l = st.account_lists(home).remove("acme").unwrap().unwrap();
    assert_eq!(default_for(&l, AgentKind::Claude).unwrap().name, "clw");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(user.parent().unwrap());
}

#[test]
fn r125_g_hand_started_agent_gets_its_account_detected() {
    let home = Path::new(HOME);
    let user = tmp_file("detect", "accounts.toml", USER);
    let (mut st, root) = ws_state("m47-detect", "acme", "");
    st.user_accounts_file = Some(user.clone());
    let shell = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    let inv = |dir: Option<&str>| Invocation {
        argv: vec!["claude".into()],
        env: dir
            .map(|d| BTreeMap::from([("CLAUDE_CONFIG_DIR".to_string(), d.to_string())]))
            .unwrap_or_default(),
    };
    st.set_status(&shell, None, None, Some("claude".into()), 1)
        .unwrap();
    assert_eq!(
        st.detect_account(
            &shell,
            AgentKind::Claude,
            &inv(Some("~/.claude-work")),
            home
        ),
        Some("clw".into())
    );
    let p = st.pane(&shell).unwrap();
    assert_eq!(p.account.as_deref(), Some("clw"));
    assert!(p.account_detected);
    // Recorded in state.json.
    let back: State = serde_json::from_value(serde_json::to_value(&st).unwrap()).unwrap();
    assert!(back.pane(&shell).unwrap().account_detected);
    // Never re-detected while it has one.
    assert_eq!(
        st.detect_account(&shell, AgentKind::Claude, &inv(None), home),
        None
    );
    // The Agent ends: cleared.
    st.set_status(
        &shell,
        Some(crate::agents::AgentStatus::Exited),
        None,
        None,
        2,
    )
    .unwrap();
    let p = st.pane(&shell).unwrap();
    assert_eq!(p.account, None);
    assert!(!p.account_detected);
    // No CLAUDE_CONFIG_DIR: ~/.claude → cl.
    assert_eq!(
        st.detect_account(&shell, AgentKind::Claude, &inv(None), home),
        Some("cl".into())
    );
    // A chosen account is never cleared by an exit.
    let mut p = st.pane(&shell).unwrap().clone();
    p.account_detected = false;
    p.clear_detected_account();
    assert_eq!(p.account.as_deref(), Some("cl"));
    // Unknown dir: none.
    let (mut st2, root2) = ws_state("m47-detect2", "acme", "");
    st2.user_accounts_file = Some(user.clone());
    let s2 = st2
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    assert_eq!(
        st2.detect_account(&s2, AgentKind::Claude, &inv(Some("/elsewhere")), home),
        None
    );
    for d in [root, root2, user.parent().unwrap().to_path_buf()] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn detection_with_two_accounts_on_one_dir_takes_the_default() {
    let home = Path::new(HOME);
    let text = "[[account]]\nagent = \"claude\"\nname = \"a\"\nconfig_dir = \"~/.c\"\n\n[[account]]\nagent = \"claude\"\nname = \"b\"\nconfig_dir = \"~/.c\"\ndefault = true\n";
    let (mut st, root) = ws_state("m47-two", "acme", text);
    let shell = st
        .spawn(
            SpawnSpec {
                source: Some("api".into()),
                ..Default::default()
            },
            &Limits::default(),
        )
        .unwrap();
    let inv = Invocation {
        argv: vec!["claude".into()],
        env: BTreeMap::from([("CLAUDE_CONFIG_DIR".to_string(), "$HOME/.c".to_string())]),
    };
    assert_eq!(
        st.detect_account(&shell, AgentKind::Claude, &inv, home),
        Some("b".into())
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn r125_h_accounts_toml_is_its_own_file() {
    // config.toml gains no key: an older Config still parses it.
    let cfg: crate::config::Config = toml::from_str("idle_seconds = 2\n").unwrap();
    assert_eq!(cfg.idle_seconds, 2);
    // Unknown top-level keys in accounts.toml are ignored.
    let f = tmp_file("unknown", "accounts.toml", &format!("version = 3\n{USER}"));
    assert_eq!(user_decls(&f).unwrap().len(), 2);
    assert_eq!(
        crate::paths::accounts_file(&|k| (k == "HOME").then(|| "/home/u".to_string())),
        PathBuf::from("/home/u/.config/rade/accounts.toml")
    );
    let _ = std::fs::remove_dir_all(f.parent().unwrap());
}

#[test]
fn r126_a_accounts_toml_edits_keep_other_bytes() {
    let ed = |t: &str, name: &str, e: Edit| edit_in_toml(t, AgentKind::Claude, name, &e);
    // Add with env.
    let decl = AccountDecl {
        agent: "claude".into(),
        name: "x".into(),
        config_dir: "~/.x".into(),
        env_file: "~/.x.env".into(),
        env: BTreeMap::from([("A".into(), "1".into())]),
        ..Default::default()
    };
    let t1 = add_to_toml(USER, &decl).unwrap();
    assert!(t1.starts_with(USER), "{t1}");
    assert!(
        t1.contains("env_file = \"~/.x.env\"\n\n[account.env]\nA = \"1\"\n"),
        "{t1}"
    );
    // Config dir.
    let t2 = ed(&t1, "clw", Edit::ConfigDir("~/.cw".into())).unwrap();
    assert_eq!(t2, t1.replace("\"~/.claude-work\"", "\"~/.cw\""));
    // Set, change, unset a variable (the last removes [account.env]).
    let t3 = ed(&t2, "clw", Edit::SetEnv("FOO".into(), "bar".into())).unwrap();
    assert!(t3.contains("[account.env]\nFOO = \"bar\""), "{t3}");
    assert!(t3.starts_with("# my logins\n"));
    let t4 = ed(&t3, "clw", Edit::SetEnv("FOO".into(), "baz".into())).unwrap();
    assert_eq!(t4, t3.replace("\"bar\"", "\"baz\""));
    let t5 = ed(&t4, "clw", Edit::UnsetEnv("FOO".into())).unwrap();
    assert_eq!(t5, t2);
    // Env file set and cleared.
    let t6 = ed(&t5, "cl", Edit::EnvFile(Some("~/.t.env".into()))).unwrap();
    assert!(
        t6.contains("default = true\nenv_file = \"~/.t.env\"\n"),
        "{t6}"
    );
    assert_eq!(ed(&t6, "cl", Edit::EnvFile(None)).unwrap(), t5);
    // Default moved.
    let t7 = set_default_in_toml(&t5, Some(AgentKind::Claude), "clw").unwrap();
    let decls = WorkspaceFile::parse(&t7).unwrap().account;
    assert!(!decls[0].default && decls[1].default);
    // Delete.
    let t8 = ed(&t5, "x", Edit::Remove).unwrap();
    assert_eq!(
        t8.trim_end(),
        t2.split("\n[[account]]\nagent = \"claude\"\nname = \"x\"")
            .next()
            .unwrap()
            .trim_end()
    );
    // Refused: reserved key, bad name, rename onto a used name; unknown account.
    for (n, e) in [
        ("clw", Edit::SetEnv("RADE_X".into(), "1".into())),
        (
            "clw",
            Edit::SetEnv("CLAUDE_CONFIG_DIR".into(), "~/.y".into()),
        ),
        ("clw", Edit::SetEnv("1A".into(), "1".into())),
        ("clw", Edit::Name("cl".into())),
    ] {
        assert_eq!(ed(&t5, n, e).unwrap_err().kind, ErrorKind::Usage);
    }
    assert_eq!(
        ed(&t5, "nope", Edit::Remove).unwrap_err().kind,
        ErrorKind::NotFound
    );
    assert!(ed("not toml [", "cl", Edit::Remove).is_err());
}

#[test]
fn r125_i_token_logins_get_no_detailed_limits() {
    let f = tmp_file("token", "clw.env", "export CLAUDE_CODE_OAUTH_TOKEN=tok-1\n");
    let inline = "[[account]]\nagent = \"claude\"\nname = \"cl\"\nconfig_dir = \"~/.c\"\ndetailed_limits = true\n[account.env]\nCLAUDE_CODE_OAUTH_TOKEN = \"tok-2\"\n";
    let file = format!(
        "[[account]]\nagent = \"claude\"\nname = \"clw\"\nconfig_dir = \"~/.cw\"\ndetailed_limits = true\nenv_file = \"{}\"\n",
        f.display()
    );
    assert!(token_login(&parse("acme", inline).unwrap()[0]));
    assert!(token_login(&parse("acme", &file).unwrap()[0]));
    assert!(!token_login(&parse("acme", ACME).unwrap()[0]));
    let _ = std::fs::remove_dir_all(f.parent().unwrap());
}

#[test]
fn edit_default_on_and_off() {
    let on = edit_in_toml(USER, AgentKind::Claude, "clw", &Edit::Default(true)).unwrap();
    let d = WorkspaceFile::parse(&on).unwrap().account;
    assert!(!d[0].default && d[1].default);
    let off = edit_in_toml(&on, AgentKind::Claude, "clw", &Edit::Default(false)).unwrap();
    assert!(!WorkspaceFile::parse(&off)
        .unwrap()
        .account
        .iter()
        .any(|a| a.default));
    assert!(off.starts_with("# my logins\n"));
}
