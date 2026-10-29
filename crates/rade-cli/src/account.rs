//! `rade account …` (PRD R108, R110; docs/API.md).

use clap::Subcommand;
use serde_json::{json, Value};

#[derive(Subcommand)]
pub enum AccountCmd {
    /// The Agent Accounts the Workspace may use: its own (`[[account]]` in
    /// its workspace.toml), then the user's (accounts.toml) unless it sets
    /// `accounts = "own"`.
    List {
        /// Default: your Workspace, else the shown one.
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Declare an account: one `[[account]]` appended, every other byte kept.
    Add {
        /// claude | codex
        #[arg(long)]
        agent: String,
        #[arg(long)]
        name: String,
        /// The Agent's config dir for this login (`~` allowed); not created.
        /// Left out: the agent's default login (~/.claude, ~/.codex).
        #[arg(long)]
        config_dir: Option<String>,
        /// Make it the agent's default (moves `default = true`).
        #[arg(long)]
        default: bool,
        /// Declare it for every Workspace, in ~/.config/rade/accounts.toml.
        #[arg(long)]
        user: bool,
        /// A dotenv file read when its Agents start (KEY=VALUE lines).
        #[arg(long, value_name = "PATH")]
        env_file: Option<String>,
        /// An extra variable for its Agents; repeat for more.
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Change an account's config dir, env file or variables, in the file
    /// that declares it (every other byte kept).
    Set {
        name: String,
        /// Needed when a claude and a codex account share the name.
        #[arg(long)]
        agent: Option<String>,
        /// The account in ~/.config/rade/accounts.toml, even where the
        /// Workspace doesn't offer it (`accounts = "own"`).
        #[arg(long)]
        user: bool,
        #[arg(long, value_name = "DIR")]
        config_dir: Option<String>,
        #[arg(long, value_name = "PATH", conflicts_with = "no_env_file")]
        env_file: Option<String>,
        /// Remove the env file.
        #[arg(long)]
        no_env_file: bool,
        /// Set a variable; repeat for more.
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// Remove a variable; repeat for more.
        #[arg(long = "unset-env", value_name = "KEY")]
        unset_env: Vec<String>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Make an account its agent's default for later spawns.
    Default {
        name: String,
        /// Needed when a claude and a codex account share the name.
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Usage limits and tokens of one account, or of all.
    Usage {
        name: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
    },
}

/// `KEY=VALUE` arguments → a JSON object; one without `=` is a usage error.
pub fn env_pairs(args: &[String]) -> Result<serde_json::Map<String, Value>, String> {
    args.iter()
        .map(|a| match a.split_once('=') {
            Some((k, v)) if !k.is_empty() => Ok((k.to_string(), json!(v))),
            _ => Err(format!("--env needs KEY=VALUE, not '{a}'")),
        })
        .collect()
}

pub fn request(c: &AccountCmd) -> Result<(&'static str, Value), String> {
    Ok(match c {
        AccountCmd::List { workspace } => ("account.list", json!({"workspace": workspace})),
        AccountCmd::Add {
            agent,
            name,
            config_dir,
            default,
            user,
            env_file,
            env,
            workspace,
        } => (
            "account.add",
            json!({"agent": agent, "name": name,
                   "config_dir": config_dir.clone().unwrap_or_default(),
                   "default": default, "workspace": workspace, "user": user,
                   "env_file": env_file.clone().unwrap_or_default(),
                   "env": env_pairs(env)?}),
        ),
        AccountCmd::Set {
            name,
            agent,
            user,
            config_dir,
            env_file,
            no_env_file,
            env,
            unset_env,
            workspace,
        } => (
            "account.set",
            json!({"name": name, "agent": agent, "user": user, "config_dir": config_dir,
                   "env_file": env_file, "no_env_file": no_env_file,
                   "env": env_pairs(env)?, "unset_env": unset_env,
                   "workspace": workspace}),
        ),
        AccountCmd::Default {
            name,
            agent,
            workspace,
        } => (
            "account.set_default",
            json!({"name": name, "agent": agent, "workspace": workspace}),
        ),
        AccountCmd::Usage {
            name: Some(n),
            agent,
            workspace,
        } => (
            "usage.get",
            json!({"account": n, "agent": agent, "workspace": workspace}),
        ),
        AccountCmd::Usage {
            name: None,
            workspace,
            ..
        } => ("usage.list", json!({"workspace": workspace})),
    })
}

/// `$HOME/x` → `~/x` for display.
fn tilde(p: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match p.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => p.to_string(),
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("-")
}

/// One account line: `claude  acme-work  ~/.claude-work  ✓`.
pub fn line(a: &Value) -> String {
    let mut l = format!(
        "{}  {}  {}",
        s(&a["agent"]),
        s(&a["name"]),
        tilde(s(&a["config_dir"]))
    );
    if a["default_login"] == true {
        l.push_str(" (default login)");
    }
    if a["default"] == true {
        l.push_str("  ✓");
    }
    if a["scope"] == "user" {
        l.push_str("  (user)");
    }
    l
}

/// `acme  codex  81% Spark 5-hour  resets in 2h 30m` (a `usage.list` row
/// or a `usage.get` answer).
fn usage_line(name: &str, agent: &str, r: &Value) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let t = &r["tightest"];
    let tightest = t["used_percent"].as_f64().map(|u| {
        (
            t["name"].as_str().unwrap_or("-"),
            u,
            t["resets_at"].as_i64(),
        )
    });
    rade_core::usage::report::account_line(
        name,
        agent,
        tightest,
        r["state"].as_str().unwrap_or("no-data"),
        now,
        rade_core::date::local_offset(now),
    )
}

/// `--user` answered with another file: the running app predates user
/// accounts (it ignored the flag and wrote workspace.toml).
pub fn older_app(c: &AccountCmd, r: &Value) -> Option<String> {
    let user = matches!(
        c,
        AccountCmd::Add { user: true, .. } | AccountCmd::Set { user: true, .. }
    );
    let file = r["file"].as_str()?;
    (user && !file.ends_with("accounts.toml")).then(|| {
        format!("the running RADE app is older than `--user` and wrote {file}; restart it (Updates → Restart) and run this again")
    })
}

pub fn print(c: &AccountCmd, r: &Value) {
    if let Some(e) = older_app(c, r) {
        eprintln!("rade: error: {e}");
        std::process::exit(2);
    }
    match c {
        AccountCmd::List { .. } => {
            for a in r["accounts"].as_array().into_iter().flatten() {
                println!("{}", line(a));
            }
            for e in r["errors"].as_array().into_iter().flatten() {
                eprintln!("rade: error: {}", s(e));
            }
        }
        AccountCmd::Add { .. } | AccountCmd::Default { .. } | AccountCmd::Set { .. } => {
            println!("{}", s(&r["file"]))
        }
        AccountCmd::Usage { name: Some(_), .. } => {
            println!("{}", usage_line(s(&r["account"]), s(&r["agent"]), r))
        }
        AccountCmd::Usage { name: None, .. } => {
            for a in r["accounts"].as_array().into_iter().flatten() {
                println!("{}", usage_line(s(&a["name"]), s(&a["agent"]), a));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r108_account_line_shows_the_default_tick_and_tilde() {
        let home = std::env::var("HOME").unwrap();
        let a = json!({"agent": "claude", "name": "acme-work",
                       "config_dir": format!("{home}/.claude-work"), "default": true});
        assert_eq!(line(&a), "claude  acme-work  ~/.claude-work  ✓");
        let b =
            json!({"agent": "codex", "name": "acme", "config_dir": "/x/codex", "default": false});
        assert_eq!(line(&b), "codex  acme  /x/codex");
        let c = json!({"agent": "claude", "name": "clw", "config_dir": "/x",
                       "default": false, "scope": "user"});
        assert_eq!(line(&c), "claude  clw  /x  (user)");
    }

    #[test]
    fn r125_older_app_that_ignored_user_is_reported() {
        let c = AccountCmd::Set {
            name: "clw".into(),
            agent: None,
            user: true,
            config_dir: None,
            env_file: None,
            no_env_file: false,
            env: vec![],
            unset_env: vec![],
            workspace: None,
        };
        assert!(older_app(&c, &json!({"file": "/w/.rade/workspace.toml"})).is_some());
        assert!(older_app(&c, &json!({"file": "/h/.config/rade/accounts.toml"})).is_none());
    }

    #[test]
    fn r125_env_args_become_an_object() {
        let c = AccountCmd::Set {
            name: "clw".into(),
            agent: None,
            user: true,
            config_dir: None,
            env_file: Some("~/.clw.env".into()),
            no_env_file: false,
            env: vec!["A=1".into(), "B=x=y".into()],
            unset_env: vec!["C".into()],
            workspace: None,
        };
        let (m, p) = request(&c).unwrap();
        assert_eq!(m, "account.set");
        assert_eq!(p["env"], json!({"A": "1", "B": "x=y"}));
        assert_eq!(p["unset_env"], json!(["C"]));
        assert_eq!(p["env_file"], "~/.clw.env");
        assert_eq!(p["user"], true);
        assert!(env_pairs(&["NOPE".into()]).is_err());
    }

    #[test]
    fn r108_usage_with_a_name_is_usage_get() {
        let c = AccountCmd::Usage {
            name: Some("personal".into()),
            agent: Some("codex".into()),
            workspace: None,
        };
        let (m, p) = request(&c).unwrap();
        assert_eq!(m, "usage.get");
        assert_eq!(p["account"], "personal");
        assert_eq!(p["agent"], "codex");
        let (m, _) = request(&AccountCmd::Usage {
            name: None,
            agent: None,
            workspace: None,
        })
        .unwrap();
        assert_eq!(m, "usage.list");
    }
}
