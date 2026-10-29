//! Settings → Accounts (PRD R126, ADR-0047): one card per Agent Account
//! the shown Workspace may use. User accounts (`accounts.toml`) are
//! edited here through [`crate::account::edit_in_toml`]; a Workspace's own
//! are shown read-only (R112.6). GTK-free. Never holds an env file's values.

use crate::account::{Account, AgentKind, Scope};
use std::path::{Path, PathBuf};

/// The category's name on the page.
pub const CATEGORY: &str = "Accounts";

/// What a card's `.env` file row says.
#[derive(Clone, Debug, PartialEq)]
pub enum EnvFileState {
    None,
    /// Its keys, in file order.
    Loaded(Vec<String>),
    /// The file's error (`path:line: …`).
    Error(String),
}

impl EnvFileState {
    /// `2 variables` / `1 variable` / the error / empty.
    pub fn text(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Loaded(k) if k.len() == 1 => "1 variable".into(),
            Self::Loaded(k) => format!("{} variables", k.len()),
            Self::Error(e) => e.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub scope: Scope,
    pub agent: AgentKind,
    pub name: String,
    /// `config_dir` as written.
    pub config_dir: String,
    pub default: bool,
    /// The default for this agent in the shown Workspace (its own
    /// `default = true` wins over the user file's, R125.3): the list's ✓.
    pub effective_default: bool,
    /// The declaring file.
    pub file: PathBuf,
    /// `env_file` as shown (`~/…`), empty without one.
    pub env_file: String,
    pub env_file_state: EnvFileState,
    /// `[account.env]`: key, value, whether the value is hidden.
    pub variables: Vec<(String, String, bool)>,
    /// A user account, but the Workspace sets `accounts = "own"`.
    pub unused_here: bool,
    /// Other accounts of this agent on the same login (`cl` = `~/.claude`
    /// and `clp` = the default login): one quota, shown as a warning.
    pub same_login: Vec<String>,
}

impl Card {
    pub fn editable(&self) -> bool {
        self.scope == Scope::User
    }

    /// The list row's badge (R126.1): `.env · 2 vars`, `3 vars`, or empty.
    pub fn badge(&self) -> String {
        let file = !matches!(self.env_file_state, EnvFileState::None);
        let mut keys: Vec<&str> = match &self.env_file_state {
            EnvFileState::Loaded(k) => k.iter().map(String::as_str).collect(),
            _ => vec![],
        };
        for (k, _, _) in &self.variables {
            if !keys.contains(&k.as_str()) {
                keys.push(k);
            }
        }
        let vars = match keys.len() {
            0 => String::new(),
            1 => "1 var".into(),
            n => format!("{n} vars"),
        };
        match (file, vars.is_empty()) {
            (true, true) => ".env".into(),
            (true, false) => format!(".env · {vars}"),
            (false, _) => vars,
        }
    }

    /// The list row's muted line: the config dir, or `default login`.
    pub fn dir_text(&self) -> String {
        if self.config_dir.is_empty() {
            "default login".into()
        } else {
            self.config_dir.clone()
        }
    }

    /// The Environment table (R126.2), in the order an Agent gets it.
    pub fn env_rows(&self) -> Vec<EnvRow> {
        let mut rows = vec![EnvRow {
            key: self.agent.var().into(),
            source: EnvSource::Login,
            value: Some(self.config_dir.clone()),
            overridden: false,
            secret: false,
        }];
        if let EnvFileState::Loaded(keys) = &self.env_file_state {
            rows.extend(keys.iter().map(|k| EnvRow {
                key: k.clone(),
                source: EnvSource::File,
                value: None,
                overridden: self.variables.iter().any(|(x, _, _)| x == k),
                secret: true,
            }));
        }
        rows.extend(self.variables.iter().map(|(k, v, s)| EnvRow {
            key: k.clone(),
            source: EnvSource::Own,
            value: Some(v.clone()),
            overridden: false,
            secret: *s,
        }));
        rows
    }
}

/// Where an Environment row comes from (R126.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvSource {
    /// `CLAUDE_CONFIG_DIR` / `CODEX_HOME` = `config_dir` (pinned).
    Login,
    /// A key of the `.env` file: never a value.
    File,
    /// `[account.env]`.
    Own,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvRow {
    pub key: String,
    pub source: EnvSource,
    /// `None` for a `.env` key: its value is never read into the UI.
    pub value: Option<String>,
    /// A `.env` key the account's own variables set again.
    pub overridden: bool,
    pub secret: bool,
}

/// The default login's path, shown as the empty login row's placeholder.
pub fn default_login_text(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "default login ~/.claude",
        AgentKind::Codex => "default login ~/.codex",
    }
}

/// A value shown as a password entry: its key names a secret.
pub fn secret_key(key: &str) -> bool {
    let k = key.to_uppercase();
    ["TOKEN", "SECRET", "KEY", "PASSWORD"]
        .iter()
        .any(|w| k.contains(w))
}

fn tilde(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn card(a: &Account, home: &Path, unused_here: bool) -> Card {
    let env_file_state = match &a.env_file {
        None => EnvFileState::None,
        Some(f) => match crate::dotenv::read(f) {
            Ok(v) => EnvFileState::Loaded(v.into_iter().map(|(k, _)| k).collect()),
            Err(e) => EnvFileState::Error(e),
        },
    };
    Card {
        scope: a.scope,
        agent: a.agent,
        name: a.name.clone(),
        config_dir: a.declared_dir.clone(),
        default: a.default,
        effective_default: a.default,
        file: a.file.clone(),
        env_file: a
            .env_file
            .as_deref()
            .map(|f| tilde(f, home))
            .unwrap_or_default(),
        env_file_state,
        variables: a
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone(), secret_key(k)))
            .collect(),
        unused_here,
        same_login: vec![],
    }
}

/// The cards: every user account (editable; `own_only` marks them unused
/// in this Workspace), then the Workspace's own accounts (read-only).
pub fn cards(user: &[Account], own: &[Account], own_only: bool, home: &Path) -> Vec<Card> {
    let all: Vec<(&Account, bool)> = user
        .iter()
        .map(|a| (a, own_only))
        .chain(own.iter().map(|a| (a, false)))
        .collect();
    all.iter()
        .map(|(a, unused)| {
            let mut c = card(a, home, *unused);
            c.same_login = all
                .iter()
                .filter(|(o, _)| {
                    o.agent == a.agent
                        && o.name != a.name
                        && crate::account::same_dir(&o.config_dir, &a.config_dir)
                })
                .map(|(o, _)| o.name.clone())
                .collect();
            c
        })
        .collect()
}

/// The card matches every word of `query` (name, agent, scope, env keys,
/// the category's name).
pub fn matches(c: &Card, query: &str) -> bool {
    let mut hay = format!(
        "{} {} {} {} {}",
        CATEGORY,
        c.name,
        c.agent.as_str(),
        c.agent.label(),
        c.scope.as_str()
    );
    for (k, _, _) in &c.variables {
        hay.push(' ');
        hay.push_str(k);
    }
    if let EnvFileState::Loaded(keys) = &c.env_file_state {
        for k in keys {
            hay.push(' ');
            hay.push_str(k);
        }
    }
    let hay = hay.to_lowercase();
    query
        .split_whitespace()
        .all(|w| hay.contains(&w.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::user_accounts;

    #[test]
    fn r126_a_cards_hide_file_values_and_mark_secrets() {
        let d = crate::paths::outside_temp_dir()
            .join(format!("rade-settings-acct-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let env = d.join("clw.env");
        std::fs::write(&env, "CLAUDE_CODE_OAUTH_TOKEN=tok-secret\nB=2\n").unwrap();
        let bad = d.join("bad.env");
        std::fs::write(&bad, "nope\n").unwrap();
        let file = d.join("accounts.toml");
        std::fs::write(
            &file,
            format!(
                "[[account]]\nagent = \"claude\"\nname = \"clw\"\nconfig_dir = \"~/.claude-work\"\nenv_file = \"{}\"\n[account.env]\nFOO = \"bar\"\nAPI_KEY = \"k\"\n\n[[account]]\nagent = \"codex\"\nname = \"cx\"\nconfig_dir = \"~/.codex\"\nenv_file = \"{}\"\n",
                env.display(),
                bad.display()
            ),
        )
        .unwrap();
        let home = Path::new("/home/u");
        let user = user_accounts(&file, home).unwrap();
        let c = cards(&user, &[], false, home);
        assert_eq!(c.len(), 2);
        assert!(c[0].editable());
        assert_eq!(c[0].env_file_state.text(), "2 variables");
        assert_eq!(
            c[0].env_file_state,
            EnvFileState::Loaded(vec!["CLAUDE_CODE_OAUTH_TOKEN".into(), "B".into()])
        );
        assert!(!format!("{c:?}").contains("tok-secret"));
        assert_eq!(
            c[0].variables,
            vec![
                ("API_KEY".to_string(), "k".to_string(), true),
                ("FOO".to_string(), "bar".to_string(), false)
            ]
        );
        assert!(c[1].env_file_state.text().contains("bad.env:1"));
        assert!(matches(&c[0], "accounts FOO"));
        assert!(matches(&c[0], "oauth"));
        assert!(matches(&c[1], "codex"));
        assert!(!matches(&c[1], "FOO"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn r126_c_env_rows_login_file_overridden_own() {
        let d = crate::paths::outside_temp_dir()
            .join(format!("rade-settings-rows-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let env = d.join("a.env");
        std::fs::write(&env, "A=file-a\nB=file-b\n").unwrap();
        let file = d.join("accounts.toml");
        std::fs::write(
            &file,
            format!(
                "[[account]]\nagent = \"claude\"\nname = \"x\"\nenv_file = \"{}\"\n[account.env]\nB = \"own-b\"\nC = \"own-c\"\n",
                env.display()
            ),
        )
        .unwrap();
        let home = Path::new("/home/u");
        let c = cards(&user_accounts(&file, home).unwrap(), &[], false, home).remove(0);
        let rows = c.env_rows();
        let brief: Vec<(String, EnvSource, bool)> = rows
            .iter()
            .map(|r| (r.key.clone(), r.source, r.overridden))
            .collect();
        assert_eq!(
            brief,
            vec![
                ("CLAUDE_CONFIG_DIR".into(), EnvSource::Login, false),
                ("A".into(), EnvSource::File, false),
                ("B".into(), EnvSource::File, true),
                ("B".into(), EnvSource::Own, false),
                ("C".into(), EnvSource::Own, false),
            ]
        );
        assert_eq!(rows[0].value.as_deref(), Some(""));
        assert_eq!(c.dir_text(), "default login");
        assert_eq!(default_login_text(c.agent), "default login ~/.claude");
        assert!(rows
            .iter()
            .filter(|r| r.source == EnvSource::File)
            .all(|r| r.value.is_none()));
        assert!(!format!("{rows:?}").contains("file-"));
        assert_eq!(c.badge(), ".env · 3 vars");
        assert!(c.same_login.is_empty());
        // R125.4: `cl` written as ~/.claude and the default-login `x` share a login.
        let f2 = d.join("two.toml");
        std::fs::write(&f2, "[[account]]\nagent = \"claude\"\nname = \"x\"\n\n[[account]]\nagent = \"claude\"\nname = \"cl\"\nconfig_dir = \"~/.claude\"\n\n[[account]]\nagent = \"codex\"\nname = \"cx\"\n").unwrap();
        let c = cards(&user_accounts(&f2, home).unwrap(), &[], false, home);
        assert_eq!(c[0].same_login, vec!["cl".to_string()]);
        assert_eq!(c[1].same_login, vec!["x".to_string()]);
        assert!(c[2].same_login.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn secrets_by_key() {
        for k in [
            "CLAUDE_CODE_OAUTH_TOKEN",
            "api_key",
            "DB_PASSWORD",
            "MY_SECRET",
        ] {
            assert!(secret_key(k), "{k}");
        }
        assert!(!secret_key("ANTHROPIC_MODEL"));
    }
}
