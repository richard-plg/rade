//! All tests use a temp HOME through an injected env; the process env (and
//! the real ~/.claude, ~/.codex, ~/.config) is never read or written.

use super::*;
use std::collections::HashMap;

struct Home {
    dir: PathBuf,
    vars: HashMap<&'static str, String>,
}

impl Home {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rade-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vars = HashMap::from([("HOME", dir.display().to_string())]);
        Self { dir, vars }
    }
    fn env(&self) -> impl Fn(&str) -> Option<String> + '_ {
        |k| self.vars.get(k).cloned()
    }
    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }
    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).unwrap()
    }
    fn plan(&self, agents: &[&str]) -> Vec<Change> {
        let a: Vec<String> = agents.iter().map(|s| s.to_string()).collect();
        plan(&a, &self.env()).unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn all() -> Vec<&'static str> {
    AGENTS.to_vec()
}

#[test]
fn r16_3_fresh_home_gets_every_file_and_second_run_is_a_noop() {
    let h = Home::new("fresh");
    let changes = h.plan(&all());
    let rel: Vec<String> = changes
        .iter()
        .map(|c| c.path.strip_prefix(&h.dir).unwrap().display().to_string())
        .collect();
    assert_eq!(
        rel,
        [
            ".claude/settings.json",
            ".claude/skills/rade/SKILL.md",
            ".claude/skills/rade-workspace/SKILL.md",
            ".claude/skills/rade-config/SKILL.md",
            ".codex/config.toml",
            ".gemini/config/hooks.json",
            ".gemini/antigravity-cli/skills/rade/SKILL.md",
            ".gemini/antigravity-cli/skills/rade-workspace/SKILL.md",
            ".gemini/antigravity-cli/skills/rade-config/SKILL.md",
            ".config/opencode/plugins/rade.js"
        ]
    );
    assert!(changes.iter().all(|c| c.old.is_none()));
    assert!(
        apply(&changes, "1").unwrap().is_empty(),
        "nothing to back up"
    );
    assert!(h.plan(&all()).is_empty(), "idempotent");

    let claude: Value = serde_json::from_str(&h.read(".claude/settings.json")).unwrap();
    for (event, arg) in [
        ("Stop", "status done --hook"),
        ("StopFailure", "status error --hook"),
        ("UserPromptSubmit", "status running --hook"),
        ("SessionStart", "status --agent claude --hook"),
    ] {
        let cmd = claude["hooks"][event][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            cmd.starts_with(r#"[ -z "$RADE_PANE_ID" ] || rade "#),
            "{cmd}"
        );
        assert!(cmd.ends_with(arg), "{event}: {cmd}");
    }
    let n = &claude["hooks"]["Notification"];
    assert_eq!(n[0]["matcher"], "permission_prompt");
    assert_eq!(n[1]["matcher"], "elicitation_dialog");
    let agy: Value = serde_json::from_str(&h.read(".gemini/config/hooks.json")).unwrap();
    let stop = agy["rade"]["Stop"][0]["command"].as_str().unwrap();
    assert!(
        stop.contains("rade status done --hook") && stop.ends_with(r#"echo '{"decision":"stop"}'"#),
        "{stop}"
    );
    assert!(agy["rade"]["PreInvocation"][0]["command"]
        .as_str()
        .unwrap()
        .contains("running --agent agy"));
    assert!(h
        .read(".claude/skills/rade/SKILL.md")
        .starts_with("---\nname: rade\n"));
    // R27-a: the Workspace skill edits through `rade` only.
    let ws = h.read(".claude/skills/rade-workspace/SKILL.md");
    assert!(ws.starts_with("---\nname: rade-workspace\n"));
    for cmd in ["rade workspace setup", "rade workspace doctor", "--dry-run"] {
        assert!(ws.contains(cmd), "{cmd}");
    }
    // R32-a: the config skill edits `.rade` files and runs `rade`, never state.json.
    let cfg = h.read(".claude/skills/rade-config/SKILL.md");
    assert!(cfg.starts_with("---\nname: rade-config\n"));
    for s in [
        "project.toml",
        ".rade.toml",
        "rade project show",
        "rade config explain",
        "Never",
    ] {
        assert!(cfg.contains(s), "{s}");
    }
    assert!(h
        .read(".config/opencode/plugins/rade.js")
        .contains("process.env.RADE_PANE_ID"));
}

#[test]
fn r16_3_claude_merge_keeps_user_entries_and_backs_up() {
    let h = Home::new("claude");
    let original = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [{"hooks": [{"type": "command", "command": "notify-send done"}]}],
    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "guard.sh"}]}]
  },
  "permissions": {"allow": ["Bash(ls)"]}
}"#;
    h.write(".claude/settings.json", original);
    let changes = h.plan(&["claude"]);
    let backups = apply(&changes, "20260930").unwrap();
    let bak = h.dir.join(".claude/settings.json.rade-bak-20260930");
    assert_eq!(backups, std::slice::from_ref(&bak));
    assert_eq!(std::fs::read_to_string(bak).unwrap(), original);

    let v: Value = serde_json::from_str(&h.read(".claude/settings.json")).unwrap();
    assert_eq!(v["model"], "opus");
    assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "guard.sh"
    );
    let stop = v["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(
        stop[0]["hooks"][0]["command"], "notify-send done",
        "user hook first, kept"
    );
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["model", "hooks", "permissions", "statusLine"],
        "key order preserved; the statusline wrapper appended (R109.1)"
    );
    assert!(h.plan(&["claude"]).is_empty(), "idempotent after merge");
}

#[test]
fn agy_merge_keeps_other_hook_groups() {
    let h = Home::new("agy");
    let original = r#"{"mine": {"Stop": [{"hooks": [{"type": "command", "command": "say hi"}]}]}}"#;
    h.write(".gemini/config/hooks.json", original);
    let backups = apply(&h.plan(&["agy"]), "3").unwrap();
    assert_eq!(backups.len(), 1);
    let v: Value = serde_json::from_str(&h.read(".gemini/config/hooks.json")).unwrap();
    assert_eq!(v["mine"]["Stop"][0]["hooks"][0]["command"], "say hi");
    assert!(v["rade"]["Stop"].is_array());
    assert!(h.plan(&["agy"]).is_empty(), "idempotent");
}

#[test]
fn stale_rade_entries_are_replaced_not_duplicated() {
    let h = Home::new("stale");
    h.write(
        ".claude/settings.json",
        r#"{"hooks": {"Stop": [{"hooks": [
            {"type": "command", "command": "rade status done --hook --old"},
            {"type": "command", "command": "mine.sh"}]}]}}"#,
    );
    apply(&h.plan(&["claude"]), "1").unwrap();
    let v: Value = serde_json::from_str(&h.read(".claude/settings.json")).unwrap();
    let stop = v["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(
        stop[0]["hooks"].as_array().unwrap().len(),
        1,
        "only mine.sh left"
    );
    assert_eq!(stop[0]["hooks"][0]["command"], "mine.sh");
    assert!(!h.read(".claude/settings.json").contains("--old"));
}

#[test]
fn r16_3_codex_merge_keeps_comments_user_hooks_and_notify() {
    let h = Home::new("codex");
    let original = "# my settings\nmodel = \"gpt-5\"\nnotify = [\"my-notifier\"]\n\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = \"say done\"\n\n[tui]\nnotifications = true\n";
    h.write(".codex/config.toml", original);
    let changes = h.plan(&["codex"]);
    apply(&changes, "7").unwrap();
    let text = h.read(".codex/config.toml");
    assert!(
        text.starts_with("# my settings\nmodel = \"gpt-5\"\nnotify = [\"my-notifier\"]"),
        "{text}"
    );
    let doc: toml::Value = toml::from_str(&text).unwrap();
    let stop = doc["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0]["hooks"][0]["command"].as_str(), Some("say done"));
    assert!(stop[1]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .ends_with("rade status done --hook"));
    for e in ["SessionStart", "UserPromptSubmit", "PermissionRequest"] {
        assert_eq!(doc["hooks"][e].as_array().unwrap().len(), 1, "{e}");
    }
    assert_eq!(doc["tui"]["notifications"].as_bool(), Some(true));
    assert_eq!(
        std::fs::read_to_string(h.dir.join(".codex/config.toml.rade-bak-7")).unwrap(),
        original
    );
    assert!(h.plan(&["codex"]).is_empty(), "idempotent");
    let a = vec!["codex".to_string()];
    assert!(
        notes(&a, &h.env()).iter().any(|n| n.contains("/hooks")),
        "trust notice"
    );
    assert!(notes(&a, &h.env())
        .iter()
        .any(|n| n.contains("own `notify`")));
}

#[test]
fn codex_gets_notify_fallback_when_it_has_none() {
    let h = Home::new("notify");
    apply(&h.plan(&["codex"]), "1").unwrap();
    let doc: toml::Value = toml::from_str(&h.read(".codex/config.toml")).unwrap();
    let notify: Vec<&str> = doc["notify"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(notify[..2], ["sh", "-c"]);
    assert!(notify[2].contains("rade status done --hook"));
    assert!(
        h.read(".codex/config.toml").starts_with("notify = "),
        "root key before tables"
    );
}

#[test]
fn dry_run_plan_writes_nothing_and_bad_files_are_refused() {
    let h = Home::new("dry");
    let _ = h.plan(&all());
    assert!(!h.dir.join(".claude").exists());
    h.write(".claude/settings.json", "{ not json");
    let e = plan(&["claude".to_string()], &h.env()).unwrap_err();
    assert!(
        e.contains("invalid JSON") && e.contains("left unchanged"),
        "{e}"
    );
    assert_eq!(h.read(".claude/settings.json"), "{ not json");
    assert!(plan(&["cursor".to_string()], &h.env())
        .unwrap_err()
        .contains("unknown agent"));
}

#[test]
fn honors_claude_config_dir_codex_home_and_xdg() {
    let mut h = Home::new("dirs");
    let d = h.dir.display().to_string();
    h.vars.insert("CLAUDE_CONFIG_DIR", format!("{d}/cc"));
    h.vars.insert("CODEX_HOME", format!("{d}/cx"));
    h.vars.insert("XDG_CONFIG_HOME", format!("{d}/xdg"));
    let paths: Vec<PathBuf> = h
        .plan(&["claude", "codex", "opencode"])
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert_eq!(
        paths,
        [
            h.dir.join("cc/settings.json"),
            h.dir.join("cc/skills/rade/SKILL.md"),
            h.dir.join("cc/skills/rade-workspace/SKILL.md"),
            h.dir.join("cc/skills/rade-config/SKILL.md"),
            h.dir.join(".claude/settings.json"),
            h.dir.join(".claude/skills/rade/SKILL.md"),
            h.dir.join(".claude/skills/rade-workspace/SKILL.md"),
            h.dir.join(".claude/skills/rade-config/SKILL.md"),
            h.dir.join("cx/config.toml"),
            h.dir.join("xdg/opencode/plugins/rade.js")
        ]
    );
}

#[test]
fn r16_3_claude_hooks_go_into_every_claude_config_dir() {
    let mut h = Home::new("multi");
    h.write(".claude/settings.json", "{}");
    h.write(".claude-work/settings.json", r#"{"model":"opus"}"#);
    h.write(
        ".claude-old/notes.txt",
        "no settings.json: not a config dir",
    );
    let set =
        |h: &Home| -> Vec<PathBuf> { h.plan(&["claude"]).into_iter().map(|c| c.path).collect() };
    let want = [
        h.dir.join(".claude/settings.json"),
        h.dir.join(".claude/skills/rade/SKILL.md"),
        h.dir.join(".claude/skills/rade-workspace/SKILL.md"),
        h.dir.join(".claude/skills/rade-config/SKILL.md"),
        h.dir.join(".claude-work/settings.json"),
        h.dir.join(".claude-work/skills/rade/SKILL.md"),
        h.dir.join(".claude-work/skills/rade-workspace/SKILL.md"),
        h.dir.join(".claude-work/skills/rade-config/SKILL.md"),
    ];
    assert_eq!(set(&h), want);
    // CLAUDE_CONFIG_DIR pointing at one of them is not listed twice.
    let work = h.dir.join(".claude-work").display().to_string();
    h.vars.insert("CLAUDE_CONFIG_DIR", work);
    assert_eq!(set(&h), [&want[4..], &want[..4]].concat());
    apply(&h.plan(&["claude"]), "1").unwrap();
    assert!(h.read(".claude-work/settings.json").contains("\"opus\""));
    assert!(h
        .read(".claude-work/settings.json")
        .contains("--agent claude"));
    assert!(h.plan(&["claude"]).is_empty(), "idempotent");
}

#[test]
fn codex_shell_rc_block_is_appended_once_and_only_to_existing_rc_files() {
    let once = merge_shell_rc(Some("export A=1"), "codex").unwrap();
    assert!(once.starts_with("export A=1\n\n# >>> rade:"));
    assert!(once.contains(r#"command codex --no-daemon "$@""#));
    assert_eq!(
        merge_shell_rc(Some(&once), "codex").unwrap(),
        once,
        "idempotent"
    );
}

#[test]
fn r25_apply_edits_a_symlink_target_and_keeps_the_mode() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::new("symlink");
    let target = h.write("dotfiles/gitconfig", "[user]\n");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = h.dir.join(".gitconfig");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let c = Change {
        path: link.clone(),
        old: Some("[user]\n".into()),
        new: "[user]\n\tname = A\n".into(),
        remove: false,
    };
    let backups = apply(&[c], "9").unwrap();
    assert_eq!(backups, [h.dir.join(".gitconfig.rade-bak-9")]);
    assert_eq!(h.read(".gitconfig.rade-bak-9"), "[user]\n");
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(h.read("dotfiles/gitconfig"), "[user]\n\tname = A\n");
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn r25_marked_block_appends_replaces_in_place_and_is_idempotent() {
    let (b, e) = ("# >>> rade workspace a >>>", "# <<< rade workspace a <<<");
    let once = marked_block("[core]\n\tx = 1", b, e, "[includeIf \"gitdir:~/a/\"]\n");
    assert_eq!(
        once,
        "[core]\n\tx = 1\n\n# >>> rade workspace a >>>\n[includeIf \"gitdir:~/a/\"]\n# <<< rade workspace a <<<\n"
    );
    assert_eq!(
        marked_block(&once, b, e, "[includeIf \"gitdir:~/a/\"]\n"),
        once
    );
    let moved = marked_block(
        &format!("{once}[tail]\n"),
        b,
        e,
        "[includeIf \"gitdir:~/b/\"]\n",
    );
    assert_eq!(
        moved,
        "[core]\n\tx = 1\n\n# >>> rade workspace a >>>\n[includeIf \"gitdir:~/b/\"]\n# <<< rade workspace a <<<\n[tail]\n",
        "replaced in place, the rest kept"
    );
    assert_eq!(marked_block("", b, e, "x\n"), format!("{b}\nx\n{e}\n"));
}

#[test]
fn r25_5_hooks_go_into_each_registered_workspace_dirs() {
    let mut h = Home::new("wsdirs");
    let data = h.dir.join("data");
    h.vars.insert("XDG_DATA_HOME", data.display().to_string());
    let root = h.dir.join("ws/acme");
    h.write(
        "ws/acme/.rade/workspace.toml",
        "[env]\nCLAUDE_CONFIG_DIR = \"~/elsewhere/claude\"\nCODEX_HOME = \"~/elsewhere/codex\"\n",
    );
    let state = crate::paths::state_file(&h.env());
    std::fs::create_dir_all(state.parent().unwrap()).unwrap();
    std::fs::write(
        &state,
        serde_json::json!({"version": 2, "workspaces": [{"id": "acme", "root": root}]}).to_string(),
    )
    .unwrap();
    let paths: Vec<PathBuf> = h
        .plan(&["claude", "codex"])
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert!(paths.contains(&h.dir.join("elsewhere/claude/settings.json")));
    assert!(paths.contains(&h.dir.join("elsewhere/claude/skills/rade/SKILL.md")));
    assert!(paths.contains(&h.dir.join("elsewhere/codex/config.toml")));
    assert!(
        paths.contains(&h.dir.join(".codex/config.toml")),
        "still the default"
    );
    // One dir only.
    let one = plan_dir("claude", &h.dir.join("only")).unwrap();
    assert!(one.iter().all(|c| c.path.starts_with(h.dir.join("only"))));
    assert!(!one.is_empty());
    apply(&one, "1").unwrap();
    assert!(plan_dir("claude", &h.dir.join("only")).unwrap().is_empty());
    assert!(plan_dir("agy", &h.dir).is_err());
}

#[test]
fn setup_dirs_include_every_declared_config_dir() {
    // R108.5: every `[[account]]` `config_dir` of every Workspace, by agent.
    let mut h = Home::new("accounts");
    let data = h.dir.join("data");
    h.vars.insert("XDG_DATA_HOME", data.display().to_string());
    let (acme, personal) = (h.dir.join("ws/acme"), h.dir.join("ws/personal"));
    h.write(
        "ws/acme/.rade/workspace.toml",
        "[[account]]\nagent = \"claude\"\nname = \"acme-work\"\nconfig_dir = \"~/.claude-work\"\n\n\
         [[account]]\nagent = \"claude\"\nname = \"acme-team\"\nconfig_dir = \"~/.claude-team\"\n\n\
         [[account]]\nagent = \"codex\"\nname = \"acme\"\nconfig_dir = \"~/.codex-acme\"\n",
    );
    h.write(
        "ws/personal/.rade/workspace.toml",
        "[[account]]\nagent = \"claude\"\nname = \"personal\"\nconfig_dir = \"~/.claude-work\"\n",
    );
    let state = crate::paths::state_file(&h.env());
    std::fs::create_dir_all(state.parent().unwrap()).unwrap();
    std::fs::write(
        &state,
        serde_json::json!({"version": 3, "workspaces": [
            {"id": "acme", "root": acme}, {"id": "personal", "root": personal}
        ]})
        .to_string(),
    )
    .unwrap();
    let paths: Vec<PathBuf> = h
        .plan(&["claude", "codex"])
        .into_iter()
        .map(|c| c.path)
        .collect();
    // `~/.claude-team` has no settings.json, so only its account brings it.
    for d in [".claude-work", ".claude-team"] {
        assert!(paths.contains(&h.dir.join(d).join("settings.json")), "{d}");
    }
    assert!(paths.contains(&h.dir.join(".codex-acme/config.toml")));
    assert!(!paths.contains(&h.dir.join(".codex-acme/settings.json")));
    assert!(!paths.contains(&h.dir.join(".claude-team/config.toml")));
    // One dir declared twice is planned once.
    let n = paths
        .iter()
        .filter(|p| **p == h.dir.join(".claude-work/settings.json"))
        .count();
    assert_eq!(n, 1);
}

#[test]
fn r125_f_setup_dirs_include_user_accounts() {
    // R125.3: a user account's config dir in accounts.toml is planned too.
    let mut h = Home::new("user-accounts");
    let cfg = h.dir.join("cfg");
    h.vars.insert("XDG_CONFIG_HOME", cfg.display().to_string());
    h.write(
        "cfg/rade/accounts.toml",
        "[[account]]\nagent = \"claude\"\nname = \"clw\"\nconfig_dir = \"~/.claude-work2\"\n",
    );
    let paths: Vec<PathBuf> = h.plan(&["claude"]).into_iter().map(|c| c.path).collect();
    assert!(
        paths.contains(&h.dir.join(".claude-work2/settings.json")),
        "{paths:?}"
    );
}

/// The five skills 0.7.1 installed and 1.0.0 removed (R101.1).
const REMOVED: [&str; 5] = [
    "rade-task",
    "rade-batch",
    "rade-initiative",
    "rade-design",
    "rade-report",
];

#[test]
fn r101_i_setup_removes_the_five_old_skills_and_installs_three() {
    let h = Home::new("r101i");
    for base in [".claude/skills", ".gemini/antigravity-cli/skills"] {
        for s in ["rade", "rade-config"].iter().chain(REMOVED.iter()) {
            h.write(
                &format!("{base}/{s}/SKILL.md"),
                &format!("---\nname: {s}\n---\nold\n"),
            );
        }
        // A backup an earlier `rade setup` left next to the skill it replaced.
        h.write(&format!("{base}/rade-task/SKILL.md.rade-bak-1"), "older\n");
    }
    // A file the user put in a removed skill's folder stays.
    h.write(".claude/skills/rade-design/notes.md", "mine\n");
    let changes = h.plan(&["claude", "agy"]);
    let removed: Vec<String> = changes
        .iter()
        .filter(|c| c.remove)
        .map(|c| c.path.strip_prefix(&h.dir).unwrap().display().to_string())
        .collect();
    let want: Vec<String> = [".claude/skills", ".gemini/antigravity-cli/skills"]
        .iter()
        .flat_map(|b| REMOVED.iter().map(move |s| format!("{b}/{s}/SKILL.md")))
        .collect();
    let mut sorted = removed.clone();
    sorted.sort();
    let mut want_sorted = want.clone();
    want_sorted.sort();
    assert_eq!(sorted, want_sorted);
    assert!(changes
        .iter()
        .filter(|c| c.remove)
        .all(|c| c.old.is_some() && c.new.is_empty()));
    apply(&changes, "2").unwrap();
    for base in [".claude/skills", ".gemini/antigravity-cli/skills"] {
        for s in REMOVED {
            let d = h.dir.join(base).join(s);
            if base == ".claude/skills" && s == "rade-design" {
                assert_eq!(h.read(&format!("{base}/{s}/notes.md")), "mine\n");
                assert!(!d.join("SKILL.md").exists());
            } else {
                assert!(!d.exists(), "{} still there", d.display());
            }
        }
        for s in ["rade", "rade-workspace", "rade-config"] {
            assert!(h
                .read(&format!("{base}/{s}/SKILL.md"))
                .starts_with(&format!("---\nname: {s}\n")));
        }
    }
    assert!(h.plan(&["claude", "agy"]).is_empty(), "idempotent");
}

#[test]
fn r109_j_statusline_rewrite_keeps_old_command_and_is_idempotent() {
    let h = Home::new("r109j");
    let old = r#"bash -c "echo 'hi'""#;
    h.write(
        ".claude-work/settings.json",
        &format!(
            "{{\"statusLine\": {{\"type\": \"command\", \"command\": {}, \"padding\": 0}}}}\n",
            serde_json::to_string(old).unwrap()
        ),
    );
    h.write(".claude-team/settings.json", "{}\n");
    let changes = h.plan(&["claude"]);
    apply(&changes, "1").unwrap();
    let work: Value = serde_json::from_str(&h.read(".claude-work/settings.json")).unwrap();
    let cmd = work["statusLine"]["command"].as_str().unwrap();
    assert_eq!(
        cmd,
        r#"rade status --statusline --then 'bash -c "echo '\''hi'\''"'"#
    );
    assert_eq!(work["statusLine"]["padding"], 0);
    assert_eq!(work["statusLine"]["type"], "command");
    // The `--then` argument, as `sh` parses it, is the old command byte for byte.
    let arg = cmd
        .strip_prefix("rade status --statusline --then ")
        .unwrap();
    let out = std::process::Command::new("sh")
        .args(["-c", &format!("printf '%s' {arg}")])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), old);
    let team: Value = serde_json::from_str(&h.read(".claude-team/settings.json")).unwrap();
    assert_eq!(team["statusLine"]["command"], "rade status --statusline");
    // Hooks are still installed next to it.
    assert!(team["hooks"]["Stop"].is_array());

    // A second run: both files byte-identical, so no change and no backup.
    let before = (
        h.read(".claude-work/settings.json"),
        h.read(".claude-team/settings.json"),
    );
    let again = h.plan(&["claude"]);
    assert!(
        again.iter().all(|c| !c.path.ends_with("settings.json")),
        "{again:?}"
    );
    assert_eq!(
        apply(&again, "2")
            .unwrap()
            .iter()
            .filter(|b| b.to_string_lossy().contains("settings.json"))
            .count(),
        0
    );
    assert_eq!(
        before,
        (
            h.read(".claude-work/settings.json"),
            h.read(".claude-team/settings.json")
        )
    );
    assert!(!h.dir.join(".claude-work/settings.json.rade-bak-2").exists());
}

#[test]
fn statusline_of_another_type_is_left_alone_with_a_note() {
    let h = Home::new("r109j-note");
    let p = h.write(
        ".claude/settings.json",
        "{\"statusLine\": {\"type\": \"static\", \"text\": \"hi\"}}\n",
    );
    let changes = h.plan(&["claude"]);
    let c = changes.iter().find(|c| c.path == p).unwrap();
    let v: Value = serde_json::from_str(&c.new).unwrap();
    assert_eq!(
        v["statusLine"],
        serde_json::json!({"type": "static", "text": "hi"})
    );
    let notes = notes(&["claude".to_string()], &h.env());
    assert!(
        notes
            .iter()
            .any(|n| n.contains("`statusLine` is not a command")),
        "{notes:?}"
    );
    assert_eq!(
        statusline_command(Some("  rade status --statusline --then 'x'")),
        "  rade status --statusline --then 'x'"
    );
    assert_eq!(statusline_command(Some("")), STATUSLINE);
    assert_eq!(sh_quote("it's"), r"'it'\''s'");
}

#[test]
fn r138_e_skill_names_every_schedule_command() {
    let at = SKILL
        .find("\n## Start an agent later or on a timetable: `rade schedule`")
        .expect("a Schedules section");
    let section = &SKILL[at..];
    let section = &section[..section[4..].find("\n## ").map_or(section.len(), |i| i + 4)];
    for sub in [
        "add", "list", "show", "edit", "on", "off", "remove", "run", "runs", "resume", "next",
    ] {
        assert!(
            section.contains(&format!("rade schedule {sub} ")),
            "the Schedules section lacks `rade schedule {sub}`"
        );
    }
    for outcome in ["started", "skipped", "missed", "failed"] {
        assert!(section.contains(outcome), "{outcome}");
    }
}
