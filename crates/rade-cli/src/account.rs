//! `rade account …` (PRD R108, R110; docs/API.md).

use clap::Subcommand;
use serde_json::{json, Value};

#[derive(Subcommand)]
pub enum AccountCmd {
    /// The Workspace's Agent Accounts (`[[account]]` in its workspace.toml).
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
        #[arg(long)]
        config_dir: String,
        /// Make it the agent's default (moves `default = true`).
        #[arg(long)]
        default: bool,
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

pub fn request(c: &AccountCmd) -> (&'static str, Value) {
    match c {
        AccountCmd::List { workspace } => ("account.list", json!({"workspace": workspace})),
        AccountCmd::Add {
            agent,
            name,
            config_dir,
            default,
            workspace,
        } => (
            "account.add",
            json!({"agent": agent, "name": name, "config_dir": config_dir,
                   "default": default, "workspace": workspace}),
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
    }
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
    if a["default"] == true {
        l.push_str("  ✓");
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

pub fn print(c: &AccountCmd, r: &Value) {
    match c {
        AccountCmd::List { .. } => {
            for a in r["accounts"].as_array().into_iter().flatten() {
                println!("{}", line(a));
            }
            for e in r["errors"].as_array().into_iter().flatten() {
                eprintln!("rade: error: {}", s(e));
            }
        }
        AccountCmd::Add { .. } | AccountCmd::Default { .. } => println!("{}", s(&r["file"])),
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
    }

    #[test]
    fn r108_usage_with_a_name_is_usage_get() {
        let c = AccountCmd::Usage {
            name: Some("personal".into()),
            agent: Some("codex".into()),
            workspace: None,
        };
        let (m, p) = request(&c);
        assert_eq!(m, "usage.get");
        assert_eq!(p["account"], "personal");
        assert_eq!(p["agent"], "codex");
        let (m, _) = request(&AccountCmd::Usage {
            name: None,
            agent: None,
            workspace: None,
        });
        assert_eq!(m, "usage.list");
    }
}
