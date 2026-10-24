//! Agent Accounts (PRD R108, ADR-0035): an Agent kind + a config dir + a
//! name, declared per Workspace in `<root>/.rade/workspace.toml`
//! `[[account]]`. Which account a new Pane runs under, the Workspace lock
//! (`FORBIDDEN`, R108.3), the Pane's env, the `VAR=value` prefix check
//! (R108.7) and the `toml_edit` writes of `rade account add|default`.

use crate::agents::PresetsFor;
use crate::model::State;
use crate::preset::{shell_words, HookKind, Invocation};
use crate::rpc::{Error, ErrorKind, Result};
use crate::workspace::{expand_tilde, WorkspaceFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The Agents an account can be declared for: each separates its logins
/// by one config-dir variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    pub const ALL: [AgentKind; 2] = [AgentKind::Claude, AgentKind::Codex];

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The variable that selects the login (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`).
    pub fn var(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CONFIG_DIR",
            Self::Codex => "CODEX_HOME",
        }
    }

    /// Shown name (`+` menu sections, spec F.5).
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    /// The account kind of an Agent Preset: its hook kind (a user Preset
    /// `hooks = "claude"` runs Claude Code); others have no account.
    pub fn of_hooks(h: HookKind) -> Option<Self> {
        match h {
            HookKind::Claude => Some(Self::Claude),
            HookKind::Codex => Some(Self::Codex),
            _ => None,
        }
    }
}

/// One `[[account]]` table as written (R108.1); checked by [`accounts`],
/// so a broken entry never breaks the rest of `workspace.toml`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AccountDecl {
    pub agent: String,
    pub name: String,
    pub config_dir: String,
    pub default: bool,
    /// Detailed limits opt-in (ADR-0035 decision 9; used from M39).
    pub detailed_limits: bool,
}

/// A validated Agent Account of one Workspace.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Account {
    pub workspace: String,
    pub agent: AgentKind,
    pub name: String,
    /// `config_dir` with `~` expanded.
    pub config_dir: PathBuf,
    /// `config_dir` as written (messages).
    pub declared_dir: String,
    /// The agent's default: `default = true`, else the first listed.
    pub default: bool,
    pub detailed_limits: bool,
}

/// The accounts of Workspace `workspace` from its `[[account]]` tables
/// (R108.1): agent `claude` / `codex`, a name and a dir; at most one
/// `default = true` per agent (none → the first listed); a name unique per
/// agent. Any violation → `USAGE` naming `file` (every spawn that needs an
/// account in this Workspace fails until it is fixed).
pub fn accounts(
    workspace: &str,
    decls: &[AccountDecl],
    file: &Path,
    home: &Path,
) -> Result<Vec<Account>> {
    let f = file.display();
    let mut errors = Vec::new();
    let mut out: Vec<Account> = Vec::new();
    for (i, d) in decls.iter().enumerate() {
        let name = d.name.trim();
        let label = if name.is_empty() {
            format!("[[account]] #{}", i + 1)
        } else {
            format!("account '{name}'")
        };
        let Some(agent) = AgentKind::parse(&d.agent) else {
            errors.push(format!(
                "{label}: agent '{}' is not claude or codex",
                d.agent
            ));
            continue;
        };
        if name.is_empty() {
            errors.push(format!("{label}: name is empty"));
            continue;
        }
        if d.config_dir.trim().is_empty() {
            errors.push(format!("{label}: config_dir is empty"));
            continue;
        }
        if out.iter().any(|a| a.agent == agent && a.name == name) {
            errors.push(format!(
                "two {} accounts are named '{name}' (a name is unique per agent)",
                agent.as_str()
            ));
            continue;
        }
        out.push(Account {
            workspace: workspace.into(),
            agent,
            name: name.into(),
            config_dir: PathBuf::from(expand_tilde(d.config_dir.trim(), home)),
            declared_dir: d.config_dir.trim().into(),
            default: d.default,
            detailed_limits: d.detailed_limits,
        });
    }
    for kind in AgentKind::ALL {
        let defaults: Vec<&str> = out
            .iter()
            .filter(|a| a.agent == kind && a.default)
            .map(|a| a.name.as_str())
            .collect();
        if defaults.len() > 1 {
            errors.push(format!(
                "{} accounts {} all have default = true (one default per agent)",
                kind.as_str(),
                defaults
                    .iter()
                    .map(|n| format!("'{n}'"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ));
        } else if defaults.is_empty() {
            if let Some(first) = out.iter_mut().find(|a| a.agent == kind) {
                first.default = true;
            }
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(Error::usage(format!("{f}: {}", errors.join("; "))))
    }
}

/// `list` with the in-memory defaults of `next` (`<workspace>/<agent>` →
/// name) applied: that account becomes its agent's only default (a name
/// no longer declared changes nothing).
pub fn with_next_defaults(
    mut list: Vec<Account>,
    workspace: &str,
    next: &BTreeMap<String, String>,
) -> Vec<Account> {
    for agent in AgentKind::ALL {
        let Some(name) = next.get(&format!("{workspace}/{}", agent.as_str())) else {
            continue;
        };
        if !list.iter().any(|a| a.agent == agent && a.name == *name) {
            continue;
        }
        for a in list.iter_mut().filter(|a| a.agent == agent) {
            a.default = a.name == *name;
        }
    }
    list
}

/// The agent's default account.
pub fn default_for(accounts: &[Account], agent: AgentKind) -> Option<&Account> {
    accounts.iter().find(|a| a.agent == agent && a.default)
}

/// An account by name (`rade account default|usage`): with `agent`, that
/// agent's; without, `AMBIGUOUS` when a claude and a codex account share
/// the name; none → `NOT_FOUND`.
pub fn find<'a>(
    accounts: &'a [Account],
    name: &str,
    agent: Option<AgentKind>,
    workspace: &str,
) -> Result<&'a Account> {
    let hits: Vec<&Account> = accounts
        .iter()
        .filter(|a| a.name == name && agent.is_none_or(|k| a.agent == k))
        .collect();
    match hits.as_slice() {
        [a] => Ok(a),
        [] => Err(Error::not_found(format!(
            "no {}account '{name}' in workspace '{workspace}'",
            agent
                .map(|k| format!("{} ", k.as_str()))
                .unwrap_or_default()
        ))),
        _ => Err(Error::new(
            ErrorKind::Ambiguous,
            format!(
                "account '{name}' of workspace '{workspace}' is both a {} account: pass --agent {}",
                hits.iter()
                    .map(|a| a.agent.as_str())
                    .collect::<Vec<_>>()
                    .join(" and a "),
                hits.iter()
                    .map(|a| a.agent.as_str())
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
        )),
    }
}

/// The `FORBIDDEN` answer for an account of another Workspace (R108.3).
pub fn foreign(name: &str, owner: &str, workspace: &str) -> Error {
    Error::forbidden(format!(
        "account '{name}' belongs to workspace '{owner}', not '{workspace}': RADE never runs an Agent under another Workspace's account (ADR-0035)"
    ))
}

/// What a spawn asks for (R108.2).
#[derive(Clone, Copy, Debug)]
pub struct SpawnAsk<'a> {
    /// The Workspace of the Pane being created.
    pub workspace: &'a str,
    /// The Preset's account kind; `None` for a shell, a `--cmd` without
    /// `--agent` or an Agent without accounts.
    pub agent: Option<AgentKind>,
    /// `--account`.
    pub explicit: Option<&'a str>,
    /// The calling Pane's account (a child inherits it).
    pub parent: Option<&'a str>,
}

/// The account a new Pane runs under (R108.2, R108.3): `--account`, else
/// the parent Pane's (when the target Workspace declares it for this
/// agent), else the Workspace's default for the agent, else none.
/// `lists` = every Workspace's [`accounts`]. An explicit name declared
/// only in another Workspace → `FORBIDDEN`; unknown, or another agent's →
/// `USAGE`; the target Workspace's list broken → its `USAGE` error.
pub fn resolve_spawn(
    ask: &SpawnAsk<'_>,
    lists: &BTreeMap<String, Result<Vec<Account>>>,
) -> Result<Option<Account>> {
    let Some(agent) = ask.agent else {
        return match ask.explicit {
            Some(n) => Err(Error::usage(format!(
                "--account '{n}' needs --agent with a claude or codex Preset"
            ))),
            None => Ok(None),
        };
    };
    let own: &[Account] = match lists.get(ask.workspace) {
        Some(Ok(l)) => l,
        Some(Err(e)) => return Err(e.clone()),
        None => &[],
    };
    let mine = |n: &str| own.iter().find(|a| a.agent == agent && a.name == n);
    if let Some(n) = ask.explicit {
        if let Some(a) = mine(n) {
            return Ok(Some(a.clone()));
        }
        if let Some(a) = own.iter().find(|a| a.name == n) {
            return Err(Error::usage(format!(
                "account '{n}' of workspace '{}' is a {} account, not {}",
                ask.workspace,
                a.agent.as_str(),
                agent.as_str()
            )));
        }
        let owner = lists
            .iter()
            .filter(|(w, _)| w.as_str() != ask.workspace)
            .find(|(_, l)| l.as_ref().is_ok_and(|l| l.iter().any(|a| a.name == n)));
        if let Some((w, _)) = owner {
            return Err(foreign(n, w, ask.workspace));
        }
        let names: Vec<&str> = own
            .iter()
            .filter(|a| a.agent == agent)
            .map(|a| a.name.as_str())
            .collect();
        return Err(Error::usage(format!(
            "no {} account '{n}' in workspace '{}' (accounts: {})",
            agent.as_str(),
            ask.workspace,
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        )));
    }
    if let Some(a) = ask.parent.and_then(mine) {
        return Ok(Some(a.clone()));
    }
    Ok(default_for(own, agent).cloned())
}

/// Put the account's env on a Pane's env (R108.2, R108.7): its variable
/// replaces any value from the Workspace's `[env]`, plus `RADE_ACCOUNT`.
pub fn apply_env(env: &mut Vec<(String, String)>, account: &Account) {
    let var = account.agent.var();
    env.retain(|(k, _)| k != var && k != "RADE_ACCOUNT");
    env.push((var.into(), account.config_dir.display().to_string()));
    env.push(("RADE_ACCOUNT".into(), account.name.clone()));
}

/// Refuse a command whose `VAR=value` prefix (also after a leading `env`)
/// sets the account's variable to another directory (R108.7): `FORBIDDEN`
/// `preset '<preset>' sets <VAR>=<dir> but account '<name>' uses <dir>
/// (ADR-0035)`. The same directory (after `~` / `$HOME` expansion, by
/// `realpath`) is allowed.
pub fn check_command(preset: &str, command: &str, account: &Account, home: &Path) -> Result<()> {
    let var = account.agent.var();
    let mut words = shell_words(command).into_iter().peekable();
    if words.peek() == Some(&"env") {
        words.next();
    }
    for w in words {
        if !crate::preset::is_assignment(w) {
            break;
        }
        let (k, v) = w.split_once('=').unwrap();
        if k == var {
            check_dir(preset, var, unquote(v), account, home)?;
        }
    }
    Ok(())
}

/// [`check_command`] for a recorded invocation's env (R108.7, R11.2).
pub fn check_invocation(
    preset: &str,
    inv: &Invocation,
    account: &Account,
    home: &Path,
) -> Result<()> {
    match inv.env.get(account.agent.var()) {
        Some(v) => check_dir(preset, account.agent.var(), v, account, home),
        None => Ok(()),
    }
}

fn check_dir(preset: &str, var: &str, raw: &str, account: &Account, home: &Path) -> Result<()> {
    let h = home.display().to_string();
    let expanded = expand_tilde(raw, home)
        .replacen("${HOME}", &h, 1)
        .replacen("$HOME", &h, 1);
    if same_dir(Path::new(&expanded), &account.config_dir) {
        return Ok(());
    }
    Err(Error::forbidden(format!(
        "preset '{preset}' sets {var}={raw} but account '{}' uses {} (ADR-0035)",
        account.name, account.declared_dir
    )))
}

fn unquote(v: &str) -> &str {
    for q in ['\'', '"'] {
        if let Some(inner) = v.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

/// Same directory: by `realpath` when both exist, else by their text
/// without trailing slashes.
fn same_dir(a: &Path, b: &Path) -> bool {
    if let (Ok(x), Ok(y)) = (a.canonicalize(), b.canonicalize()) {
        return x == y;
    }
    let t = |p: &Path| p.display().to_string().trim_end_matches('/').to_string();
    t(a) == t(b)
}

fn document(text: &str) -> Result<toml_edit::DocumentMut> {
    text.parse()
        .map_err(|e: toml_edit::TomlError| Error::usage(format!("workspace.toml: {e}")))
}

fn decls_of(text: &str) -> Result<Vec<AccountDecl>> {
    WorkspaceFile::parse(text)
        .map(|f| f.account)
        .map_err(|e| Error::usage(format!("workspace.toml: {e}")))
}

/// Remove `default` from every `agent` table but `keep` (by index).
fn clear_defaults(doc: &mut toml_edit::DocumentMut, agent: AgentKind, keep: Option<usize>) {
    if let Some(tables) = doc
        .get_mut("account")
        .and_then(|i| i.as_array_of_tables_mut())
    {
        for (i, t) in tables.iter_mut().enumerate() {
            let same = t.get("agent").and_then(|v| v.as_str()) == Some(agent.as_str());
            if same && Some(i) != keep {
                t.remove("default");
            }
        }
    }
}

/// `rade account add` (R108.4): append one `[[account]]` table, every
/// other byte kept; with `default`, the agent's other tables lose their
/// `default` line. A used name for that agent, an unknown agent or an
/// empty name / dir → `USAGE`.
pub fn add_to_toml(text: &str, decl: &AccountDecl) -> Result<String> {
    let agent = AgentKind::parse(&decl.agent).ok_or_else(|| {
        Error::usage(format!(
            "agent must be claude or codex, not '{}'",
            decl.agent
        ))
    })?;
    let (name, dir) = (decl.name.trim(), decl.config_dir.trim());
    if name.is_empty() || dir.is_empty() {
        return Err(Error::usage("an account needs a name and a config dir"));
    }
    if decls_of(text)?
        .iter()
        .any(|d| AgentKind::parse(&d.agent) == Some(agent) && d.name.trim() == name)
    {
        return Err(Error::usage(format!(
            "workspace.toml already has a {} account named '{name}'",
            agent.as_str()
        )));
    }
    let mut out = if decl.default {
        let mut doc = document(text)?;
        clear_defaults(&mut doc, agent, None);
        doc.to_string()
    } else {
        text.to_string()
    };
    let mut table = toml_edit::Table::new();
    table.insert("agent", toml_edit::value(agent.as_str()));
    table.insert("name", toml_edit::value(name));
    table.insert("config_dir", toml_edit::value(dir));
    if decl.default {
        table.insert("default", toml_edit::value(true));
    }
    if decl.detailed_limits {
        table.insert("detailed_limits", toml_edit::value(true));
    }
    let mut tables = toml_edit::ArrayOfTables::new();
    tables.push(table);
    let mut doc = toml_edit::DocumentMut::new();
    doc.insert("account", toml_edit::Item::ArrayOfTables(tables));
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&doc.to_string());
    decls_of(&out)?;
    Ok(out)
}

/// `rade account default` (R108.4): `default = true` on the account, its
/// agent's other tables lose their `default` line. Without `agent`, a name
/// a claude and a codex account share → `AMBIGUOUS`; none → `NOT_FOUND`.
pub fn set_default_in_toml(text: &str, agent: Option<AgentKind>, name: &str) -> Result<String> {
    let hits: Vec<(usize, AgentKind)> = decls_of(text)?
        .iter()
        .enumerate()
        .filter_map(|(i, d)| Some((i, AgentKind::parse(&d.agent)?, d.name.trim() == name)))
        .filter(|(_, k, same)| *same && agent.is_none_or(|a| a == *k))
        .map(|(i, k, _)| (i, k))
        .collect();
    let (index, kind) = match hits.as_slice() {
        [one] => *one,
        [] => {
            return Err(Error::not_found(format!(
                "no {}account '{name}' in workspace.toml",
                agent
                    .map(|k| format!("{} ", k.as_str()))
                    .unwrap_or_default()
            )))
        }
        _ => {
            return Err(Error::new(
                ErrorKind::Ambiguous,
                format!(
                    "account '{name}' is both a {} account: pass --agent {}",
                    hits.iter()
                        .map(|(_, k)| k.as_str())
                        .collect::<Vec<_>>()
                        .join(" and a "),
                    hits.iter()
                        .map(|(_, k)| k.as_str())
                        .collect::<Vec<_>>()
                        .join("|")
                ),
            ))
        }
    };
    let mut doc = document(text)?;
    clear_defaults(&mut doc, kind, Some(index));
    let t = doc
        .get_mut("account")
        .and_then(|i| i.as_array_of_tables_mut())
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| Error::internal("account table not found"))?;
    if t.get("default").and_then(|v| v.as_bool()) != Some(true) {
        match t.get_mut("default").and_then(|i| i.as_value_mut()) {
            Some(v) => *v = toml_edit::Value::from(true).decorated(" ", ""),
            None => {
                t.insert("default", toml_edit::value(true));
            }
        }
    }
    Ok(doc.to_string())
}

/// Detailed limits on or off for one account (R109.5: an unexpected
/// response turns it off): `detailed_limits = <on>` on its table, every
/// other byte kept; off with no key → unchanged. No such account → `NOT_FOUND`.
pub fn set_detailed_in_toml(text: &str, agent: AgentKind, name: &str, on: bool) -> Result<String> {
    let index = decls_of(text)?
        .iter()
        .position(|d| AgentKind::parse(&d.agent) == Some(agent) && d.name.trim() == name)
        .ok_or_else(|| {
            Error::not_found(format!(
                "no {} account '{name}' in workspace.toml",
                agent.as_str()
            ))
        })?;
    let mut doc = document(text)?;
    let t = doc
        .get_mut("account")
        .and_then(|i| i.as_array_of_tables_mut())
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| Error::internal("account table not found"))?;
    match t.get_mut("detailed_limits").and_then(|i| i.as_value_mut()) {
        Some(v) if v.as_bool() == Some(on) => return Ok(text.to_string()),
        Some(v) => {
            let decor = v.decor().clone();
            *v = toml_edit::Value::from(on);
            *v.decor_mut() = decor;
        }
        None if !on => return Ok(text.to_string()),
        None => {
            t.insert("detailed_limits", toml_edit::value(on));
        }
    }
    Ok(doc.to_string())
}

/// [`set_detailed_in_toml`] on a Workspace root, written atomically.
pub fn set_detailed_file(root: &Path, agent: AgentKind, name: &str, on: bool) -> Result<PathBuf> {
    let new = set_detailed_in_toml(&read_workspace_text(root)?, agent, name, on)?;
    write_workspace_file(root, &new)
}

/// Write `<root>/.rade/workspace.toml` (creating `.rade/`) through a temp
/// file and a rename, so a reader never sees half a file.
pub fn write_workspace_file(root: &Path, text: &str) -> Result<PathBuf> {
    let dir = root.join(".rade");
    let file = dir.join("workspace.toml");
    let fail = |e: std::io::Error| Error::internal(format!("{}: {e}", file.display()));
    std::fs::create_dir_all(&dir).map_err(fail)?;
    let tmp = dir.join(format!(".workspace.toml.{}.tmp", std::process::id()));
    let res = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &file));
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(e));
    }
    Ok(file)
}

fn read_workspace_text(root: &Path) -> Result<String> {
    let file = root.join(".rade/workspace.toml");
    match std::fs::read_to_string(&file) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(Error::internal(format!("{}: {e}", file.display()))),
    }
}

/// `account.add` on a Workspace root: [`add_to_toml`] on its
/// `workspace.toml` (missing = empty), written atomically. Returns the file.
pub fn add_account_file(root: &Path, decl: &AccountDecl) -> Result<PathBuf> {
    let new = add_to_toml(&read_workspace_text(root)?, decl)?;
    write_workspace_file(root, &new)
}

/// `account.set_default` on a Workspace root ([`set_default_in_toml`]).
pub fn set_default_file(root: &Path, agent: Option<AgentKind>, name: &str) -> Result<PathBuf> {
    let new = set_default_in_toml(&read_workspace_text(root)?, agent, name)?;
    write_workspace_file(root, &new)
}

/// What a restored plain shell is given to type (R108.7): a `printf` of
/// the notice (a leading space keeps it out of most shells' history).
pub fn notice_input(notice: &str) -> String {
    format!(" printf '%s\\n' {}\n", crate::preset::shell_quote(notice))
}

impl crate::Pane {
    /// Record the account a new Pane runs under (R108.2): its name and agent.
    pub fn set_account(&mut self, account: Option<Account>) {
        self.account_agent = account.as_ref().map(|a| a.agent);
        self.account = account.map(|a| a.name);
    }
}

impl State {
    /// The Workspace lock (R108.3): an account for a Pane of `source` must
    /// be declared in that Source's Workspace, else `FORBIDDEN`.
    pub fn check_account(&self, source: &str, account: Option<&Account>) -> Result<()> {
        let (Some(a), Some(s)) = (account, self.source(source)) else {
            return Ok(());
        };
        if a.workspace != s.workspace {
            return Err(foreign(&a.name, &a.workspace, &s.workspace));
        }
        Ok(())
    }

    /// The account of an Agent Pane RADE starts itself in `source` (a
    /// Fan-out Member or Judge, R108.2): the `caller` Pane's when the
    /// Workspace declares it for `agent`, else the Workspace default; its
    /// Preset `command` checked against it (R108.7).
    pub fn agent_account(
        &self,
        source: &str,
        preset: &str,
        agent: Option<AgentKind>,
        command: &str,
        caller: Option<&str>,
        home: &Path,
    ) -> Result<Option<Account>> {
        let workspace = self
            .source(source)
            .map(|s| s.workspace.clone())
            .unwrap_or_else(|| self.active_workspace().to_string());
        let parent = caller.and_then(|c| self.pane(c)?.account.clone());
        let ask = SpawnAsk {
            workspace: &workspace,
            agent,
            explicit: None,
            parent: parent.as_deref(),
        };
        let account = resolve_spawn(&ask, &self.account_lists(home))?;
        if let Some(a) = &account {
            check_command(preset, command, a, home)?;
        }
        Ok(account)
    }

    /// Every Workspace's accounts, read from its `workspace.toml` now
    /// (R108.1); a broken file or account list is that Workspace's error.
    /// A default chosen for later Agents ([`State::set_next_default`])
    /// replaces the file's `default` of that agent.
    pub fn account_lists(&self, home: &Path) -> BTreeMap<String, Result<Vec<Account>>> {
        self.workspaces
            .iter()
            .map(|w| {
                let file = w.root.join(".rade/workspace.toml");
                let list = WorkspaceFile::read(&w.root)
                    .map_err(Error::usage)
                    .and_then(|f| accounts(&w.id, &f.account, &file, home))
                    .map(|l| with_next_defaults(l, &w.id, &self.next_defaults));
                (w.id.clone(), list)
            })
            .collect()
    }

    /// The forecast Toast's action (R110.4, spec F.20): Agents of `agent`
    /// opened later in `workspace` default to `name`. In memory only:
    /// running Panes keep their account; `workspace.toml` is untouched.
    pub fn set_next_default(&mut self, workspace: &str, agent: AgentKind, name: &str) {
        self.next_defaults
            .insert(format!("{workspace}/{}", agent.as_str()), name.to_string());
    }

    /// An explicit default (`rade account default`, R108) replaces the
    /// forecast Toast's choice for `agent` in `workspace`.
    pub fn clear_next_default(&mut self, workspace: &str, agent: AgentKind) {
        self.next_defaults
            .remove(&format!("{workspace}/{}", agent.as_str()));
    }

    /// The account of an existing Pane (env of a Terminal being created or
    /// restored): its recorded name in its Workspace, for its recorded
    /// agent (else `agent`, for a Pane saved without one). `None` = the
    /// Pane has no account; `Err` = it is no longer declared there (or the
    /// list is broken).
    pub fn pane_account(
        &self,
        pane: &str,
        agent: Option<AgentKind>,
        home: &Path,
    ) -> Option<Result<Account>> {
        let p = self.pane(pane)?;
        let name = p.account.clone()?;
        let agent = p.account_agent.or(agent);
        let (source, _) = self.pane_worktree(pane)?;
        let ws = self.source(&source)?.workspace.clone();
        let mut lists = self.account_lists(home);
        Some(match lists.remove(&ws) {
            Some(Ok(l)) => find(&l, &name, agent, &ws).cloned(),
            Some(Err(e)) => Err(e),
            None => Err(Error::not_found(format!("no workspace '{ws}'"))),
        })
    }

    /// Session Restore (R108.7, after [`State::prepare_restore`]): a Pane
    /// with an account whose restart command sets the account's variable
    /// to another directory (a recorded invocation, or the Preset's own
    /// command) — or whose account is no longer declared — comes back as a
    /// plain shell in its cwd that prints why (`Pane.notice`), keeping its
    /// account. Returns the refused Panes.
    pub fn refuse_restores(&mut self, presets: &PresetsFor<'_>, home: &Path) -> Vec<String> {
        self.refuse_restores_in(presets, home, None)
    }

    /// [`State::refuse_restores`] for the Panes of one Source (pausing it
    /// restarts them, R19.3), or of all with `None`.
    pub fn refuse_restores_in(
        &mut self,
        presets: &PresetsFor<'_>,
        home: &Path,
        source: Option<&str>,
    ) -> Vec<String> {
        let candidates: Vec<(String, String, Option<AgentKind>, String)> = self
            .all_panes()
            .filter(|p| p.account.is_some())
            .filter_map(|p| {
                let cmd = p.command()?.to_string();
                let preset = p.agent.clone()?;
                let (s, w) = self.pane_worktree(&p.id)?;
                if source.is_some_and(|x| x != s) {
                    return None;
                }
                let kind = presets(&s, &w)
                    .get(&preset)
                    .and_then(|x| AgentKind::of_hooks(x.hooks));
                Some((p.id.clone(), preset, kind, cmd))
            })
            .collect();
        let mut refused = Vec::new();
        for (id, preset, kind, cmd) in candidates {
            let verdict = match self.pane_account(&id, kind, home) {
                Some(Ok(a)) => check_command(&preset, &cmd, &a, home).err(),
                Some(Err(e)) => Some(e),
                None => None,
            };
            let Some(e) = verdict else { continue };
            let Some(loc) = self.locate(&id) else {
                continue;
            };
            let Ok(t) = self.tab_mut(&loc) else { continue };
            let p = t.pane_mut(&id).unwrap();
            if let crate::PaneKind::Terminal { command, .. } = &mut p.kind {
                *command = None;
            }
            p.agent = None;
            p.session_id = None;
            p.invocation = None;
            p.had_turn = false;
            p.status = None;
            p.status_since = None;
            p.notice = Some(format!("rade: not resumed: {}", e.message));
            refused.push(id);
        }
        refused
    }
}

#[cfg(test)]
#[path = "account_tests.rs"]
mod tests;
