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
    /// The declaring file.
    pub file: PathBuf,
    /// `env_file` as shown (`~/…`), empty without one.
    pub env_file: String,
    pub env_file_state: EnvFileState,
    /// `[account.env]`: key, value, whether the value is hidden.
    pub variables: Vec<(String, String, bool)>,
    /// A user account, but the Workspace sets `accounts = "own"`.
    pub unused_here: bool,
}

impl Card {
    pub fn editable(&self) -> bool {
        self.scope == Scope::User
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
    }
}

/// The cards: every user account (editable; `own_only` marks them unused
/// in this Workspace), then the Workspace's own accounts (read-only).
pub fn cards(user: &[Account], own: &[Account], own_only: bool, home: &Path) -> Vec<Card> {
    user.iter()
        .map(|a| card(a, home, own_only))
        .chain(own.iter().map(|a| card(a, home, false)))
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
