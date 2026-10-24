//! `rade workspace setup | import | clone | doctor` (PRD R25, R26,
//! ADR-0016): local commands run in the caller's terminal. Every file edit
//! is a `setup::Change` (diff, backup, atomic write); every action and
//! probe runs a CLI from PATH (`git`, `gh`, `ssh-keygen`, `ssh`, `$SHELL`),
//! which is what lets the tests swap in fakes.

use crate::local::{on_path, show_diff, stamp};
use crate::{call, clone};
use clap::{Args, Subcommand, ValueEnum};
use rade_core::model::slug;
use rade_core::paths::{self, process_env};
use rade_core::rpc::{Error, ErrorKind};
use rade_core::setup::{self, Change};
use rade_core::shell_env::bounded_output;
use rade_core::workspace::WorkspaceFile;
use rade_core::workspace_setup as wsu;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Subcommand)]
pub enum LocalCmd {
    /// Make a Workspace's identity real on this machine: workspace.toml,
    /// git identity (includeIf), SSH key, gh login, keys on GitHub, hooks.
    /// Idempotent; fills gaps, never overwrites.
    Setup(SetupArgs),
    /// Write workspace.toml from how this machine is already set up (git
    /// config, SSH key, gh account, shell rc), then register it.
    Import {
        root: PathBuf,
        /// Extra `[env]` entries (repeatable).
        #[arg(long = "env", value_name = "K=V")]
        env: Vec<String>,
        /// Show the diff, write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Clone Sources into a Workspace with its identity (skips existing ones).
    Clone {
        #[arg(long)]
        workspace: Option<String>,
        /// `owner/repo`, `repo` (default_owner), or a URL.
        #[arg(required = true)]
        specs: Vec<String>,
    },
    /// Check a Workspace's identity end to end, with the command that fixes each problem.
    Doctor {
        /// A Workspace id / name or a root path; default every registered one.
        target: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum SignArg {
    Ssh,
    None,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
pub enum ClaudeArg {
    Shared,
    Separate,
}

#[derive(Args)]
pub struct SetupArgs {
    root: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    git_name: Option<String>,
    #[arg(long)]
    email: Option<String>,
    /// The GitHub login this Workspace uses.
    #[arg(long)]
    github: Option<String>,
    /// A file name in ~/.ssh (default id_ed25519_<ws>).
    #[arg(long)]
    ssh_key: Option<String>,
    #[arg(long)]
    sign: Option<SignArg>,
    /// `separate`: CLAUDE_CONFIG_DIR=~/.claude-<ws> for this Workspace's Panes.
    #[arg(long)]
    claude: Option<ClaudeArg>,
    #[arg(long)]
    default_owner: Option<String>,
    /// Extra `[env]` entries (repeatable).
    #[arg(long = "env", value_name = "K=V")]
    env: Vec<String>,
    /// Clone the Workspace definition (`<root>/.rade`) from this git URL when missing.
    #[arg(long)]
    from: Option<String>,
    /// Print the diffs and the commands, change nothing.
    #[arg(long)]
    dry_run: bool,
}

fn home() -> PathBuf {
    paths::home_dir(&process_env)
}

fn abs(p: &Path) -> Result<PathBuf, Error> {
    let p = match p.strip_prefix("~") {
        Ok(rest) => home().join(rest),
        Err(_) => p.to_path_buf(),
    };
    let p = if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .map_err(|e| Error::internal(e.to_string()))?
            .join(p)
    };
    Ok(p.canonicalize().unwrap_or(p))
}

/// A root must not be a repository: that is a Source (R25.2, R25.3).
fn check_root(root: &Path) -> Result<(), Error> {
    if root.join(".git").exists() {
        return Err(Error::usage(format!(
            "{} is a git checkout: that's a Source (`rade source add`), not a Workspace root",
            root.display()
        )));
    }
    Ok(())
}

/// How long any probe may take (network ones: `gh api`, `ssh -T`).
const PROBE_LIMIT: Duration = Duration::from_secs(20);

/// Run a probe (null stdin, own session, killed after `PROBE_LIMIT`);
/// `None` when it can't start or hangs, and a hang is said on stderr.
fn run_probe(argv: &[&str], env: &[(String, String)]) -> Option<std::process::Output> {
    let mut c = Command::new(argv[0]);
    c.args(&argv[1..]).envs(env.iter().cloned());
    match bounded_output(&mut c, PROBE_LIMIT) {
        Ok(out) => Some(out),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::TimedOut {
                eprintln!("warning: `{}` {e}", argv.join(" "));
            }
            None
        }
    }
}

/// A probe's success and stdout + stderr.
fn probe(argv: &[&str], env: &[(String, String)]) -> Option<(bool, String)> {
    let out = run_probe(argv, env)?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some((out.status.success(), text))
}

/// A probe's trimmed stdout when it succeeds.
fn stdout_of(argv: &[&str], env: &[(String, String)]) -> Option<String> {
    let out = run_probe(argv, env)?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The login `gh api user` returns under `env` (never `gh auth status`);
/// `Err` when gh did not answer: not the same as logged out.
fn gh_login(env: &[(String, String)]) -> Result<Option<String>, Error> {
    let out = run_probe(&["gh", "api", "user", "--jq", ".login"], env).ok_or_else(|| {
        Error::usage("`gh api user` did not answer: check the network, then run this again")
    })?;
    let login = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((out.status.success() && !login.is_empty()).then_some(login))
}

/// `K=V` flags.
fn parse_env(flags: &[String]) -> Result<Vec<(String, String)>, Error> {
    flags
        .iter()
        .map(|f| match f.split_once('=') {
            Some((k, v)) if !k.is_empty() => Ok((k.to_string(), v.to_string())),
            _ => Err(Error::usage(format!("--env takes K=V, not '{f}'"))),
        })
        .collect()
}

/// Quote an argument for display only when the shell would need it.
fn shell_quote(a: &str) -> String {
    let plain = !a.is_empty()
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:,=@+%".contains(c));
    if plain {
        a.into()
    } else {
        rade_core::preset::shell_quote(a)
    }
}

/// One run: file changes and commands, each printed; nothing done in a
/// dry run.
struct Run {
    dry: bool,
    did: bool,
    stamp: String,
}

impl Run {
    fn files(&mut self, changes: &[Change]) -> Result<(), Error> {
        for c in changes {
            let what = match (c.remove, c.old.is_some()) {
                (true, _) => "remove",
                (_, true) => "update",
                _ => "create",
            };
            println!("== {what} {}", c.path.display());
            if !c.remove {
                show_diff(c);
            }
        }
        if changes.is_empty() || self.dry {
            return Ok(());
        }
        let backups = setup::apply(changes, &self.stamp)
            .map_err(|e| Error::new(ErrorKind::Internal, format!("writing: {e}")))?;
        for b in backups {
            println!("backup: {}", b.display());
        }
        self.did = true;
        Ok(())
    }

    fn mkdir(&mut self, dir: &Path) -> Result<(), Error> {
        if dir.is_dir() {
            return Ok(());
        }
        if self.dry {
            println!("would create: {}", dir.display());
            return Ok(());
        }
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::internal(format!("{}: {e}", dir.display())))?;
        println!("created: {}", dir.display());
        self.did = true;
        Ok(())
    }

    /// Run a command with the terminal (prompts show), unless dry.
    fn cmd(&mut self, argv: &[String], env: &[(String, String)]) -> Result<bool, Error> {
        let shown: Vec<String> = env
            .iter()
            .map(|(k, v)| format!("{k}={}", shell_quote(v)))
            .chain(argv.iter().map(|a| shell_quote(a)))
            .collect();
        if self.dry {
            println!("would run: {}", shown.join(" "));
            return Ok(true);
        }
        println!("run: {}", shown.join(" "));
        self.did = true;
        let status = Command::new(&argv[0])
            .args(&argv[1..])
            .envs(env.iter().cloned())
            .status()
            .map_err(|e| Error::usage(format!("cannot run {}: {e}", argv[0])))?;
        Ok(status.success())
    }
}

fn workspace_toml(root: &Path) -> PathBuf {
    root.join(".rade/workspace.toml")
}

fn read_opt(path: &Path) -> Result<Option<String>, Error> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::internal(format!("{}: {e}", path.display()))),
    }
}

fn change(path: PathBuf, old: Option<String>, new: String) -> Option<Change> {
    (old.as_deref() != Some(new.as_str())).then_some(Change {
        path,
        old,
        new,
        remove: false,
    })
}

/// The new `<root>/.gitconfig` with the identity's missing keys set by
/// `git config --file` on a copy (no hand-written gitconfig parser);
/// keys already set to another value are kept and returned.
fn root_gitconfig(
    path: &Path,
    keys: &[(&str, String)],
) -> Result<(Option<Change>, Vec<String>), Error> {
    let old = read_opt(path)?;
    let tmp_dir = process_env("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let tmp = tmp_dir.join(format!("rade-gitconfig-{}", std::process::id()));
    std::fs::write(&tmp, old.as_deref().unwrap_or(""))
        .map_err(|e| Error::internal(format!("{}: {e}", tmp.display())))?;
    let file = tmp.to_string_lossy().into_owned();
    let mut kept = Vec::new();
    for (k, v) in keys {
        match stdout_of(&["git", "config", "--file", &file, "--get", k], &[]) {
            Some(cur) if &cur == v => {}
            Some(cur) => kept.push(format!("{k} = {cur} (wanted {v})")),
            None => {
                let ok =
                    probe(&["git", "config", "--file", &file, k, v], &[]).is_some_and(|(ok, _)| ok);
                if !ok {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(Error::internal(format!("git config {k} failed")));
                }
            }
        }
    }
    let new = std::fs::read_to_string(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);
    Ok((change(path.into(), old, new), kept))
}

/// The `includeIf` conditions git reads globally, through `[include]`d
/// files too.
fn global_includeif_conditions() -> Vec<String> {
    stdout_of(
        &[
            "git",
            "config",
            "--global",
            "--includes",
            "--get-regexp",
            "^includeif\\.",
        ],
        &[],
    )
    .map(|o| wsu::includeif_conditions_from_regexp(&o))
    .unwrap_or_default()
}

/// `~/.gitconfig` with the Workspace's marked `includeIf` block (R25.2),
/// unless git already includes the root's config (a hand-written
/// `includeIf`, maybe in an `[include]`d file).
fn home_gitconfig(ws: &str, root: &Path) -> Result<Option<Change>, Error> {
    if global_includeif_conditions()
        .iter()
        .any(|c| wsu::condition_matches_root(c, root, &home()))
    {
        return Ok(None);
    }
    let path = home().join(".gitconfig");
    let old = read_opt(&path)?;
    let new = wsu::gitconfig_block(old.as_deref().unwrap_or(""), ws, root, &home());
    Ok(new.and_then(|n| change(path, old, n)))
}

pub fn run(cmd: &LocalCmd, json_out: bool) -> Result<Value, Error> {
    match cmd {
        LocalCmd::Setup(a) => setup_ws(a),
        LocalCmd::Import { root, env, dry_run } => import(root, env, *dry_run),
        LocalCmd::Clone { workspace, specs } => clone_all(workspace.as_deref(), specs),
        LocalCmd::Doctor { target } => doctor(target.as_deref(), json_out),
    }
}

/// Register the root with the app if it runs (R25.2): `workspace.add` is
/// idempotent.
fn register(run: &mut Run, root: &Path, name: Option<&str>) {
    if run.dry {
        println!("would register: rade workspace add {}", root.display());
        return;
    }
    match call("workspace.add", json!({"root": root, "name": name})) {
        Ok(v) if v["existing"] == true => {}
        Ok(v) => {
            println!(
                "registered: workspace '{}'",
                v["workspace"].as_str().unwrap_or("-")
            );
            run.did = true;
        }
        Err(e) if e.kind == ErrorKind::NotRunning => println!(
            "note: RADE is not running; register later with `rade workspace add {}`",
            root.display()
        ),
        Err(e) => eprintln!("rade: registering failed: {e}"),
    }
}

fn setup_ws(a: &SetupArgs) -> Result<Value, Error> {
    let missing: Vec<&str> = ["git", "gh", "ssh-keygen"]
        .into_iter()
        .filter(|t| !on_path(t))
        .collect();
    if !missing.is_empty() {
        return Err(Error::usage(format!(
            "not installed: {} (Ubuntu: sudo apt install git gh openssh-client)",
            missing.join(", ")
        )));
    }
    let root = abs(&a.root)?;
    check_root(&root)?;
    let home = home();
    let mut run = Run {
        dry: a.dry_run,
        did: false,
        stamp: stamp(),
    };
    if let Some(url) = &a.from {
        if !root.join(".rade").exists() {
            run.mkdir(&root)?;
            let dest = root.join(".rade").display().to_string();
            if !run.cmd(&["git".into(), "clone".into(), url.clone(), dest], &[])? {
                return Err(Error::usage(format!("git clone {url} failed")));
            }
        }
    }
    // workspace.toml: flags must agree with the file; defaults fill gaps.
    let toml_path = workspace_toml(&root);
    let old = read_opt(&toml_path)?;
    let file = WorkspaceFile::parse(old.as_deref().unwrap_or(""))
        .map_err(|e| Error::usage(format!("{}: {e}", toml_path.display())))?;
    let name = a
        .name
        .clone()
        .or(file.name.clone())
        .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "workspace".into());
    let id = slug(&name);
    let env_flags = parse_env(&a.env)?;
    let mut flags: Vec<(&str, &str, String)> = Vec::new();
    let mut flag = |t, k, v: &Option<String>| {
        if let Some(v) = v {
            flags.push((t, k, v.clone()));
        }
    };
    flag("", "name", &a.name);
    flag("identity", "git_name", &a.git_name);
    flag("identity", "git_email", &a.email);
    flag("identity", "github", &a.github);
    flag("identity", "ssh_key", &a.ssh_key);
    flag(
        "identity",
        "sign",
        &a.sign.map(|s| match s {
            SignArg::Ssh => "ssh".into(),
            SignArg::None => "none".into(),
        }),
    );
    flag("clone", "default_owner", &a.default_owner);
    for (k, v) in &env_flags {
        flags.push(("env", k.as_str(), v.clone()));
    }
    let (text, conflicts) = wsu::merge_toml(old.as_deref(), &flags).map_err(Error::usage)?;
    if !conflicts.is_empty() {
        return Err(Error::usage(format!(
            "{} already has {}: edit the file or drop the flag",
            toml_path.display(),
            conflicts.join(", ")
        )));
    }
    let mut defaults = vec![
        ("", "name", name.clone()),
        ("identity", "ssh_key", format!("id_ed25519_{id}")),
        ("identity", "sign", "ssh".into()),
        ("env", "GH_CONFIG_DIR", format!("~/.config/gh-{id}")),
    ];
    if a.claude == Some(ClaudeArg::Separate) {
        defaults.push(("env", "CLAUDE_CONFIG_DIR", format!("~/.claude-{id}")));
    }
    let (text, _) = wsu::merge_toml(Some(&text), &defaults).map_err(Error::usage)?;
    let file = WorkspaceFile::parse(&text).map_err(Error::usage)?;
    let idn = &file.identity;
    let need: Vec<&str> = [
        (idn.git_name.is_none(), "--git-name"),
        (idn.git_email.is_none(), "--email"),
        (idn.github.is_none(), "--github"),
    ]
    .into_iter()
    .filter_map(|(m, f)| m.then_some(f))
    .collect();
    if !need.is_empty() {
        return Err(Error::usage(format!(
            "missing {} (or set them in [identity] of {})",
            need.join(", "),
            toml_path.display()
        )));
    }
    run.mkdir(&root.join(".rade"))?;
    run.mkdir(&root.join(".worktrees"))?;
    // Files: workspace.toml, <root>/.gitconfig, the ~/.gitconfig block.
    let mut changes: Vec<Change> = change(toml_path.clone(), old, text).into_iter().collect();
    let (rg, kept) = root_gitconfig(&root.join(".gitconfig"), &wsu::gitconfig_keys(idn, &home))?;
    changes.extend(rg);
    changes.extend(home_gitconfig(&id, &root)?);
    run.files(&changes)?;
    for k in kept {
        println!("note: kept {k} in {}/.gitconfig", root.display());
    }
    // SSH key.
    let key = wsu::key_path(idn.ssh_key.as_deref().unwrap_or_default(), &home);
    let pub_path = PathBuf::from(format!("{}.pub", key.display()));
    if !key.exists() {
        if !run.dry {
            let ssh = home.join(".ssh");
            std::fs::create_dir_all(&ssh).map_err(|e| Error::internal(e.to_string()))?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700));
        }
        let argv: Vec<String> = ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C"]
            .map(String::from)
            .into_iter()
            .chain([idn.git_email.clone().unwrap_or_default(), "-f".into()])
            .chain([key.display().to_string()])
            .collect();
        if !run.cmd(&argv, &[])? {
            return Err(Error::internal("ssh-keygen failed"));
        }
    }
    // gh login in the Workspace's own config dir.
    let env = file.env(&home);
    let gh_env: Vec<(String, String)> = env
        .iter()
        .filter(|(k, _)| k == "GH_CONFIG_DIR")
        .cloned()
        .collect();
    let github = idn.github.clone().unwrap_or_default();
    let mut login = gh_login(&gh_env)?;
    if login.is_none() {
        let skip = stdout_of(&["gh", "auth", "login", "--help"], &[])
            .is_some_and(|h| h.contains("--skip-ssh-key"));
        if !run.cmd(&wsu::login_argv(skip, idn.signs()), &gh_env)? {
            return Err(Error::usage("gh auth login failed"));
        }
        if !run.dry {
            login = gh_login(&gh_env)?;
            if login.is_none() {
                return Err(Error::usage("still not logged in after gh auth login"));
            }
        }
    }
    let other = login.as_ref().filter(|l| **l != github);
    if let Some(l) = other {
        println!("warning: gh is logged in as '{l}', workspace.toml says '{github}': the key is not added to '{l}' (`rade workspace doctor` checks it)");
    }
    // The key on the account: authentication, and signing with sign = ssh.
    let pubkey = std::fs::read_to_string(&pub_path).unwrap_or_default();
    let mut kinds = vec![("authentication", "user/keys")];
    if idn.signs() {
        kinds.push(("signing", "user/ssh_signing_keys"));
    }
    for (kind, api) in kinds.into_iter().filter(|_| other.is_none()) {
        let listed = login
            .as_ref()
            .and_then(|_| stdout_of(&["gh", "api", api, "--jq", ".[].key"], &gh_env));
        if listed.is_some_and(|l| wsu::key_on_account(&pubkey, &l)) {
            continue;
        }
        let argv: Vec<String> = vec![
            "gh".into(),
            "ssh-key".into(),
            "add".into(),
            pub_path.display().to_string(),
            "--title".into(),
            format!("{id} (rade)"),
            "--type".into(),
            kind.into(),
        ];
        if !run.cmd(&argv, &gh_env)? {
            println!("warning: gh ssh-key add ({kind}) failed; `rade workspace doctor` checks it");
        }
    }
    // Hooks into the Workspace's own Agent config dirs.
    let mut hooks = Vec::new();
    for (agent, var) in [("claude", "CLAUDE_CONFIG_DIR"), ("codex", "CODEX_HOME")] {
        if let Some((_, dir)) = env.iter().find(|(k, _)| k == var) {
            hooks.extend(setup::plan_dir(agent, Path::new(dir)).map_err(Error::usage)?);
        }
    }
    run.files(&hooks)?;
    register(&mut run, &root, a.name.as_deref());
    if run.dry {
        println!("\n(dry run: nothing written, nothing run)");
    } else if !run.did {
        println!("nothing to do");
    }
    Ok(Value::Null)
}

/// The variables an import reads from the shell probe (R25.3).
const PROBED: [&str; 3] = ["GH_CONFIG_DIR", "CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// The Sources (git checkouts) right under a root, sorted.
fn sources_under(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(".git").exists())
        .collect();
    v.sort();
    v
}

fn config_z(argv: &[&str]) -> std::collections::BTreeMap<String, String> {
    run_probe(argv, &[])
        .filter(|o| o.status.success())
        .map(|o| wsu::parse_config_z(&o.stdout))
        .unwrap_or_default()
}

/// The effective git config for a root: inside its first Source (which
/// applies `includeIf`), else its own `.gitconfig`.
fn root_config(root: &Path) -> (std::collections::BTreeMap<String, String>, Option<PathBuf>) {
    if let Some(src) = sources_under(root).into_iter().next() {
        let s = src.display().to_string();
        return (
            config_z(&["git", "-C", &s, "config", "--list", "-z"]),
            Some(src),
        );
    }
    let f = root.join(".gitconfig").display().to_string();
    (
        config_z(&["git", "config", "--file", &f, "--list", "-z"]),
        None,
    )
}

/// Variables a login shell started in `root` sets (with them unset first).
fn shell_vars(root: &Path) -> Option<Vec<(String, String)>> {
    let shell = process_env("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let env = rade_core::shell_env::probe(&shell, Some(root), &[], &PROBED)?;
    Some(
        env.into_iter()
            .filter(|(k, v)| PROBED.contains(&k.as_str()) && !v.is_empty())
            .collect(),
    )
}

fn import(root: &Path, env_flags: &[String], dry: bool) -> Result<Value, Error> {
    let root = abs(root)?;
    if !root.is_dir() {
        return Err(Error::usage(format!("not a directory: {}", root.display())));
    }
    check_root(&root)?;
    let home = home();
    let (cfg, _) = root_config(&root);
    let alias_key = wsu::insteadof_host(&cfg)
        .and_then(|h| stdout_of(&["ssh", "-G", &h], &[]))
        .and_then(|o| wsu::ssh_g_identityfile(&o));
    let mut id = wsu::infer_identity(&cfg, alias_key.as_deref(), &home);
    let vars = shell_vars(&root).unwrap_or_else(|| {
        eprintln!("warning: a login shell in the root failed or timed out (5 s): [env] not read from rc files");
        Vec::new()
    });
    let gh_env: Vec<(String, String)> = vars
        .iter()
        .filter(|(k, _)| k == "GH_CONFIG_DIR")
        .cloned()
        .collect();
    id.github = gh_login(&gh_env).ok().flatten();
    let origins: Vec<String> = sources_under(&root)
        .iter()
        .filter_map(|s| {
            let s = s.display().to_string();
            stdout_of(&["git", "-C", &s, "remote", "get-url", "origin"], &[])
        })
        .collect();
    let owner = wsu::most_common_owner(origins.iter().map(String::as_str));
    let mut entries: Vec<(&str, &str, String)> = Vec::new();
    if let Some(n) = root.file_name() {
        entries.push(("", "name", n.to_string_lossy().into_owned()));
    }
    let id_fields = [
        ("git_name", id.git_name.clone()),
        ("git_email", id.git_email.clone()),
        ("github", id.github.clone()),
        ("ssh_key", id.ssh_key.clone()),
        (
            "sign",
            Some(if id.signs() { "ssh" } else { "none" }.to_string()),
        ),
    ];
    for (k, v) in id_fields {
        if let Some(v) = v {
            entries.push(("identity", k, v));
        }
    }
    for k in PROBED {
        if let Some((_, v)) = vars.iter().find(|(x, _)| x == k) {
            entries.push(("env", k, wsu::tilde(Path::new(v), &home)));
        }
    }
    if let Some(o) = owner {
        entries.push(("clone", "default_owner", o));
    }
    let toml_path = workspace_toml(&root);
    let old = read_opt(&toml_path)?;
    let flags: Vec<(String, String)> = parse_env(env_flags)?;
    let flag_entries: Vec<(&str, &str, String)> = flags
        .iter()
        .map(|(k, v)| ("env", k.as_str(), v.clone()))
        .collect();
    let (text, conflicts) = wsu::merge_toml(old.as_deref(), &flag_entries).map_err(Error::usage)?;
    if !conflicts.is_empty() {
        return Err(Error::usage(format!(
            "{} already has {}",
            toml_path.display(),
            conflicts.join(", ")
        )));
    }
    let (text, _) = wsu::merge_toml(Some(&text), &entries).map_err(Error::usage)?;
    let mut run = Run {
        dry,
        did: false,
        stamp: stamp(),
    };
    let changes: Vec<Change> = change(toml_path, old, text).into_iter().collect();
    run.files(&changes)?;
    register(&mut run, &root, None);
    if dry {
        println!("\n(dry run: nothing written)");
    } else if !run.did {
        println!("nothing to do");
    }
    Ok(Value::Null)
}

fn clone_all(workspace: Option<&str>, specs: &[String]) -> Result<Value, Error> {
    let (ws, all) = clone::pick(workspace)?;
    let extra: Vec<(String, String)> = ws["identity"]["ssh_key"]
        .as_str()
        .map(|k| {
            let cmd = wsu::ssh_command(&wsu::key_path(k, &home()));
            vec![("GIT_SSH_COMMAND".to_string(), cmd)]
        })
        .unwrap_or_default();
    let (mut cloned, mut skipped, mut failed) = (0, 0, 0);
    for spec in specs {
        match clone::one(&ws, &all, spec, None, &extra) {
            Ok((v, true)) => {
                cloned += 1;
                println!("cloned: {}", v["source"].as_str().unwrap_or("-"));
            }
            Ok((v, false)) => {
                skipped += 1;
                println!(
                    "skipped: {} (already there)",
                    v["source"].as_str().unwrap_or("-")
                );
            }
            Err(e) => {
                failed += 1;
                eprintln!("rade: {spec}: {e}");
            }
        }
    }
    println!("cloned {cloned}, skipped {skipped}, failed {failed}");
    if failed > 0 {
        return Err(Error::usage(format!("{failed} clone(s) failed")));
    }
    Ok(Value::Null)
}

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Ok,
    Warn,
    Fail,
}

/// One doctor line (R26).
struct Check {
    check: &'static str,
    level: Level,
    detail: String,
    fix: String,
}

fn ok(check: &'static str, detail: impl Into<String>) -> Check {
    Check {
        check,
        level: Level::Ok,
        detail: detail.into(),
        fix: String::new(),
    }
}

fn bad(
    check: &'static str,
    level: Level,
    detail: impl Into<String>,
    fix: impl Into<String>,
) -> Check {
    Check {
        check,
        level,
        detail: detail.into(),
        fix: fix.into(),
    }
}

/// Registered Workspaces `(id, root)` from `state.json`, read as plain
/// JSON (no app needed, nothing written).
fn registered() -> Vec<(String, PathBuf)> {
    let state: Value = std::fs::read_to_string(paths::state_file(&process_env))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    state["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            Some((
                w["id"].as_str()?.to_string(),
                PathBuf::from(w["root"].as_str()?),
            ))
        })
        .collect()
}

fn doctor(target: Option<&str>, json_out: bool) -> Result<Value, Error> {
    let roots: Vec<PathBuf> = match target {
        Some(t) if t.contains('/') || t.starts_with('~') => vec![abs(Path::new(t))?],
        Some(t) => {
            let found: Vec<PathBuf> = registered()
                .into_iter()
                .filter(|(id, root)| {
                    id == t
                        || root.file_name().is_some_and(|n| n == t)
                        || WorkspaceFile::read(root)
                            .ok()
                            .and_then(|f| f.name)
                            .is_some_and(|n| n == t)
                })
                .map(|(_, r)| r)
                .collect();
            if found.is_empty() {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("no registered workspace '{t}' (pass its root path)"),
                ));
            }
            found
        }
        None => registered()
            .into_iter()
            .filter(|(id, _)| id != rade_core::model::DEFAULT_WORKSPACE)
            .map(|(_, r)| r)
            .collect(),
    };
    let mut report = Vec::new();
    let mut failed = 0;
    for root in roots {
        let checks = doctor_one(&root);
        failed += checks.iter().filter(|c| c.level == Level::Fail).count();
        if !json_out {
            println!("== {}", root.display());
            for c in &checks {
                let l = match c.level {
                    Level::Ok => "ok  ",
                    Level::Warn => "warn",
                    Level::Fail => "FAIL",
                };
                println!("{l} {}: {}", c.check, c.detail);
                if !c.fix.is_empty() {
                    println!("  fix: {}", c.fix);
                }
            }
        }
        report.push(json!({
            "root": root,
            "workspace": WorkspaceFile::read(&root).ok().and_then(|f| f.name),
            "checks": checks.iter().map(|c| json!({
                "check": c.check,
                "level": match c.level { Level::Ok => "ok", Level::Warn => "warn", Level::Fail => "fail" },
                "detail": c.detail,
                "fix": c.fix,
            })).collect::<Vec<_>>(),
        }));
    }
    if json_out {
        println!("{}", json!({"workspaces": report, "ok": failed == 0}));
    }
    if failed > 0 {
        return Err(Error::internal(format!("{failed} check(s) failed")));
    }
    Ok(Value::Null)
}

/// The R26 checks for one root, in order.
fn doctor_one(root: &Path) -> Vec<Check> {
    let home = home();
    let r = root.display().to_string();
    let setup_fix = format!("rade workspace setup {}", shell_quote(&r));
    let mut out = Vec::new();
    // 1. root
    let toml = workspace_toml(root).display().to_string();
    let file = if !root.is_dir() {
        Err((format!("{r} does not exist"), setup_fix.clone()))
    } else if root.join(".git").exists() {
        Err((
            format!("{r} is a git checkout: that's a Source"),
            format!("rade source add {}", shell_quote(&r)),
        ))
    } else {
        // One line: TOML errors span several.
        WorkspaceFile::read(root).map_err(|e| {
            let e = e.split_whitespace().collect::<Vec<_>>().join(" ");
            (e, format!("${{EDITOR:-nano}} {}", shell_quote(&toml)))
        })
    };
    let file = match file {
        Ok(f) => {
            let what = if workspace_toml(root).exists() {
                "workspace.toml parses"
            } else {
                "no workspace.toml yet"
            };
            out.push(ok("root", format!("{r}, {what}")));
            f
        }
        Err((e, fix)) => {
            out.push(bad("root", Level::Fail, e, fix));
            return out;
        }
    };
    // 2. tools
    let mut missing = Vec::new();
    for (tool, min) in [("git", (2, 36)), ("gh", (2, 40))] {
        match stdout_of(&[tool, "--version"], &[]) {
            Some(v) if wsu::version_at_least(&v, min) => {}
            Some(v) => missing.push(format!(
                "{tool} too old ({}; need {}.{})",
                v.lines().next().unwrap_or(""),
                min.0,
                min.1
            )),
            None if on_path(tool) => missing.push(format!("`{tool} --version` failed")),
            None => missing.push(format!("{tool} not installed")),
        }
    }
    if !on_path("ssh-keygen") {
        missing.push("ssh-keygen not installed".into());
    }
    out.push(if missing.is_empty() {
        ok("tools", "git, gh, ssh-keygen")
    } else {
        bad(
            "tools",
            Level::Fail,
            missing.join("; "),
            "sudo apt install git gh openssh-client",
        )
    });
    let idn = &file.identity;
    let env = file.env(&home);
    let var = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
    if idn.git_email.is_none() && idn.github.is_none() && idn.ssh_key.is_none() {
        out.push(bad(
            "identity",
            Level::Warn,
            "no [identity] in workspace.toml: git, SSH and GitHub checks skipped",
            format!("rade workspace import {}", shell_quote(&r)),
        ));
    } else {
        identity_checks(root, &file, &env, &setup_fix, &mut out);
    }
    // 10. Claude
    let declared = var("CLAUDE_CONFIG_DIR");
    let dir = declared
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"));
    let lvl = if declared.is_some() {
        Level::Fail
    } else {
        Level::Warn
    };
    let d = dir.display().to_string();
    let logged_in = std::fs::read_to_string(wsu::claude_json(&dir, &home))
        .is_ok_and(|t| wsu::claude_logged_in(&t));
    let hooks_missing = setup::plan_dir("claude", &dir).map_or(true, |c| !c.is_empty());
    out.push(if !dir.is_dir() {
        bad(
            "claude",
            lvl,
            format!("{d} does not exist"),
            format!(
                "CLAUDE_CONFIG_DIR={} claude  # then /login",
                shell_quote(&d)
            ),
        )
    } else if !logged_in {
        bad(
            "claude",
            lvl,
            format!("no account logged in under {d}"),
            format!(
                "CLAUDE_CONFIG_DIR={} claude  # then /login",
                shell_quote(&d)
            ),
        )
    } else if hooks_missing {
        bad(
            "claude",
            lvl,
            format!("RADE hooks or skills missing or outdated in {d}"),
            "rade setup",
        )
    } else {
        ok("claude", format!("{d}: logged in, hooks installed"))
    });
    // 11. shell rc
    out.push(shell_check(root, &env, &home));
    // 12. registered
    out.push(match call("workspace.list", json!({})) {
        Ok(v) => {
            let has = v["workspaces"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|w| w["root"].as_str().map(Path::new) == Some(root));
            if has {
                ok("registered", "the app shows this Workspace")
            } else {
                bad(
                    "registered",
                    Level::Warn,
                    "not registered in the running app",
                    format!("rade workspace add {}", shell_quote(&r)),
                )
            }
        }
        Err(_) => bad(
            "registered",
            Level::Warn,
            "RADE is not running",
            "start RADE, then: rade workspace doctor",
        ),
    });
    out
}

/// Checks 3–9: git identity, signing, key files, gh account, token
/// storage, keys on the account, `ssh -T`.
fn identity_checks(
    root: &Path,
    file: &WorkspaceFile,
    env: &[(String, String)],
    setup_fix: &str,
    out: &mut Vec<Check>,
) {
    let home = home();
    let idn = &file.identity;
    let r = root.display().to_string();
    // 3. git identity inside a Source, else <root>/.gitconfig + includeIf.
    let (cfg, src) = root_config(root);
    let mut problems = Vec::new();
    if src.is_none() {
        let conds = global_includeif_conditions();
        if !conds
            .iter()
            .any(|c| wsu::condition_matches_root(c, root, &home))
        {
            problems.push("no includeIf in ~/.gitconfig points at the root".to_string());
        }
    }
    for (key, want) in [("user.name", &idn.git_name), ("user.email", &idn.git_email)] {
        if let Some(w) = want {
            match cfg.get(key) {
                Some(v) if v == w => {}
                Some(v) => problems.push(format!("{key} = {v}, [identity] says {w}")),
                None => problems.push(format!("{key} unset, [identity] says {w}")),
            }
        }
    }
    let at = src
        .as_ref()
        .map(|s| s.display().to_string())
        .unwrap_or_else(|| format!("{r}/.gitconfig"));
    out.push(if problems.is_empty() {
        ok("git identity", format!("as [identity] in {at}"))
    } else {
        bad(
            "git identity",
            Level::Fail,
            format!("{at}: {}", problems.join("; ")),
            setup_fix,
        )
    });
    // 4. signing
    if idn.signs() {
        let mut p = Vec::new();
        if cfg.get("gpg.format").map(String::as_str) != Some("ssh") {
            p.push("gpg.format is not ssh".to_string());
        }
        match cfg.get("user.signingkey") {
            Some(k) if wsu::key_path_exists(k, &home) => {}
            Some(k) => p.push(format!("signing key {k} missing")),
            None => p.push("user.signingkey unset".into()),
        }
        if cfg.get("commit.gpgsign").map(String::as_str) != Some("true") {
            p.push("commit.gpgsign is not true".into());
        }
        out.push(if p.is_empty() {
            ok("signing", "ssh, commits signed")
        } else {
            bad("signing", Level::Fail, p.join("; "), setup_fix)
        });
    }
    // 5. key files
    let key = idn.ssh_key.as_deref().map(|k| wsu::key_path(k, &home));
    let pub_path = key
        .as_ref()
        .map(|k| PathBuf::from(format!("{}.pub", k.display())));
    match (&key, &pub_path) {
        (Some(k), Some(p)) => {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(k).map(|m| m.permissions().mode() & 0o777);
            let kd = k.display().to_string();
            out.push(match mode {
                Err(_) => bad("ssh key", Level::Fail, format!("{kd} missing"), setup_fix),
                Ok(_) if !p.exists() => bad(
                    "ssh key",
                    Level::Fail,
                    format!("{} missing", p.display()),
                    format!("ssh-keygen -y -f {kd} > {kd}.pub"),
                ),
                Ok(m) if m & 0o077 != 0 => bad(
                    "ssh key",
                    Level::Fail,
                    format!("{kd} is mode {m:o}"),
                    format!("chmod 600 {kd}"),
                ),
                Ok(_) => ok("ssh key", format!("{kd} (600) and .pub")),
            });
        }
        _ => out.push(bad(
            "ssh key",
            Level::Fail,
            "no ssh_key in [identity]",
            setup_fix,
        )),
    }
    // 6. gh account
    let github = idn.github.clone().unwrap_or_default();
    let gh_dir = env
        .iter()
        .find(|(k, _)| k == "GH_CONFIG_DIR")
        .map(|(_, v)| v.clone());
    let Some(gh_dir) = gh_dir else {
        out.push(bad(
            "gh account",
            Level::Fail,
            "no GH_CONFIG_DIR in [env]: gh would use the shared ~/.config/gh",
            setup_fix,
        ));
        return;
    };
    let gh_env = vec![("GH_CONFIG_DIR".to_string(), gh_dir.clone())];
    let login_fix = format!(
        "GH_CONFIG_DIR={} gh auth login --hostname github.com --git-protocol ssh --insecure-storage",
        shell_quote(&gh_dir)
    );
    let login = gh_login(&gh_env);
    let login_fails = login.as_ref().is_err();
    let login = login.ok().flatten();
    out.push(match &login {
        Some(l) if *l == github => ok("gh account", format!("{l} in {gh_dir}")),
        Some(l) => bad(
            "gh account",
            Level::Fail,
            format!("{gh_dir} is logged in as {l}, [identity] github = {github}"),
            login_fix.clone(),
        ),
        None if login_fails => bad(
            "gh account",
            Level::Fail,
            format!("gh api user did not answer in {gh_dir}"),
            "check the network, then: rade workspace doctor",
        ),
        None => bad(
            "gh account",
            Level::Fail,
            format!("gh api user fails in {gh_dir} (not logged in?)"),
            login_fix.clone(),
        ),
    });
    // 7. token storage
    let hosts = std::fs::read_to_string(Path::new(&gh_dir).join("hosts.yml")).unwrap_or_default();
    out.push(if wsu::hosts_has_token(&hosts) {
        ok("gh token", "stored in hosts.yml (not the shared keyring slot)")
    } else {
        bad("gh token", Level::Warn, "no oauth_token in hosts.yml: the token is in the keyring, shared by every gh account on this host", login_fix.clone())
    });
    // 8. keys on the account
    if let (Some(_), Some(p)) = (&login, &pub_path) {
        let pubkey = std::fs::read_to_string(p).unwrap_or_default();
        let mut kinds = vec![("authentication", "user/keys")];
        if idn.signs() {
            kinds.push(("signing", "user/ssh_signing_keys"));
        }
        let mut missing = Vec::new();
        for (kind, api) in &kinds {
            let listed =
                stdout_of(&["gh", "api", api, "--jq", ".[].key"], &gh_env).unwrap_or_default();
            if !wsu::key_on_account(&pubkey, &listed) {
                missing.push(*kind);
            }
        }
        out.push(if missing.is_empty() {
            ok(
                "keys on GitHub",
                kinds.iter().map(|k| k.0).collect::<Vec<_>>().join(" + "),
            )
        } else {
            let fixes: Vec<String> = missing
                .iter()
                .map(|k| {
                    format!(
                        "GH_CONFIG_DIR={} gh ssh-key add {} --type {k}",
                        shell_quote(&gh_dir),
                        p.display()
                    )
                })
                .collect();
            bad(
                "keys on GitHub",
                Level::Fail,
                format!("{} not on {github}: {}", p.display(), missing.join(", ")),
                fixes.join(" && "),
            )
        });
    }
    // 9. ssh -T
    if let Some(k) = &key {
        let kd = k.display().to_string();
        let res = probe(
            &[
                "ssh",
                // Only this key: a `Host github.com` IdentityFile in
                // ~/.ssh/config would otherwise answer first.
                "-F",
                "/dev/null",
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                // Read-only: never add host keys to known_hosts.
                "-o",
                "UpdateHostKeys=no",
                "-o",
                "IdentitiesOnly=yes",
                "-i",
                &kd,
                "git@github.com",
            ],
            &[],
        );
        let want = format!("Hi {github}!");
        out.push(match res {
            Some((_, text)) if text.contains(&want) => {
                ok("ssh", format!("github.com greets {github}"))
            }
            Some((_, text)) => bad(
                "ssh",
                Level::Fail,
                format!(
                    "ssh -T with {kd}: {}",
                    text.trim().lines().last().unwrap_or("no answer")
                ),
                format!("ssh -F /dev/null -T -i {kd} -o IdentitiesOnly=yes git@github.com"),
            ),
            None => bad(
                "ssh",
                Level::Fail,
                "ssh -T timed out",
                format!("ssh -F /dev/null -T -i {kd} -o IdentitiesOnly=yes git@github.com"),
            ),
        });
    }
}

/// Check 11: a login shell started in the root, with `[env]` exported,
/// keeps every `[env]` variable.
fn shell_check(root: &Path, env: &[(String, String)], home: &Path) -> Check {
    if env.is_empty() {
        return ok("shell", "no [env] to compare");
    }
    let shell = process_env("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let Some(got) = rade_core::shell_env::probe(&shell, Some(root), env, &[]) else {
        return bad(
            "shell",
            Level::Warn,
            format!("{shell} -l -i in the root failed or timed out (5 s)"),
            "",
        );
    };
    let rc = rc_files(home);
    let mut changed = Vec::new();
    let mut fixes = Vec::new();
    for (k, v) in env {
        let now = got.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
        if now != Some(v.as_str()) {
            let lines = wsu::rc_lines_setting(k, &rc);
            changed.push(format!(
                "{k} becomes {} (set in {})",
                now.unwrap_or("unset"),
                if lines.is_empty() {
                    "an rc file".to_string()
                } else {
                    lines.join(", ")
                }
            ));
            fixes.push(format!(
                "grep -n {k} {}",
                rc.iter()
                    .map(|(p, _)| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
    }
    if changed.is_empty() {
        ok("shell", format!("{shell} keeps [env] in the root"))
    } else {
        bad("shell", Level::Warn, changed.join("; "), fixes.join(" ; "))
    }
}

/// The shell rc files that exist: bash, zsh, `~/.config/zsh/*`.
fn rc_files(home: &Path) -> Vec<(PathBuf, String)> {
    let mut paths: Vec<PathBuf> = [
        ".bashrc",
        ".bash_profile",
        ".profile",
        ".zshenv",
        ".zprofile",
        ".zshrc",
    ]
    .iter()
    .map(|f| home.join(f))
    .collect();
    let mut zsh: Vec<PathBuf> = std::fs::read_dir(home.join(".config/zsh"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    zsh.sort();
    paths.extend(zsh);
    paths
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
        .collect()
}
