//! The titlebar `+` menu (PRD R108.6, spec F rows F.4, F.5, F.11–F.14 and
//! its empty state): which rows it shows, in which order, with which
//! details. GTK-free; `rade-app/src/new_menu.rs` draws them.
//!
//! Order: Shell · `CLAUDE CODE` · `CODEX` · `OTHER` (Fan-out…, Browser).
//! Per agent: the target Workspace's default account first, then its other
//! accounts by name, then the accounts of other Workspaces, locked (shown
//! dimmed, never activatable: the lock itself is [`crate::account::resolve_spawn`]).

use crate::account::{Account, AgentKind};
use crate::rpc::Result;
use serde::Serialize;
use std::collections::BTreeMap;

/// What a row does when activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    /// A shell in a new Tab.
    Shell,
    /// An Agent under one account (`pane.spawn` `agent` + `account`).
    Account,
    /// An Agent with no account declared: the Workspace env's login.
    Agent,
    /// The Fan-out form.
    FanOut,
    /// A Browser Pane.
    Browser,
}

/// One row of the menu.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub kind: RowKind,
    /// The section label above it (`""` for Shell).
    pub section: &'static str,
    /// The text shown (`Shell`, the account name, `Claude Code`).
    pub name: String,
    /// The detail after the name (`no data yet`, `other workspace`).
    pub detail: Option<String>,
    /// Key hints at the right (`Ctrl ⇧ T`).
    pub keys: Option<&'static str>,
    /// Phosphor icon name (`terminal-window`); account rows show a badge.
    pub icon: Option<&'static str>,
    pub agent: Option<AgentKind>,
    /// The Workspace that declares the account.
    pub workspace: Option<String>,
    /// The agent's default in the target Workspace (`check`).
    pub default: bool,
    /// An account of another Workspace (`lock-simple`, opacity 0.45).
    pub locked: bool,
    /// Activatable.
    pub sensitive: bool,
    pub tooltip: Option<String>,
    /// The detail is a Tightest limit at or above 80 % (F.10: `wait`
    /// color with `warning-diamond`).
    pub hot: bool,
}

impl Row {
    fn plain(kind: RowKind, section: &'static str, name: &str) -> Row {
        Row {
            kind,
            section,
            name: name.into(),
            detail: None,
            keys: None,
            icon: None,
            agent: None,
            workspace: None,
            default: false,
            locked: false,
            sensitive: true,
            tooltip: None,
            hot: false,
        }
    }
}

/// The detail of an account row without usage figures (spec F empty
/// state: never `0%`).
pub const NO_DATA: &str = "no data yet";
/// The detail of a locked row (F.11).
pub const OTHER_WORKSPACE: &str = "other workspace";
/// The detail of an agent with no account declared (spec F empty state).
pub const WORKSPACE_DEFAULT: &str = "workspace default";

/// The section label of an agent (F.5, upper case as drawn).
pub fn section_of(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "CLAUDE CODE",
        AgentKind::Codex => "CODEX",
    }
}

/// The tooltip of a locked row (spec F, States).
pub fn locked_tooltip(owner: &str) -> String {
    format!(
        "Declared in Workspace {owner}; RADE never runs an Agent under another Workspace's account (ADR-0015)"
    )
}

/// The header (F.4): `New in <Project> / <source> / <worktree>`, leaving
/// out what there is not (no Project, a main checkout).
pub fn header(project: Option<&str>, source: &str, worktree: Option<&str>) -> String {
    let parts: Vec<&str> = project
        .into_iter()
        .chain([source])
        .chain(worktree)
        .collect();
    format!("New in {}", parts.join(" / "))
}

/// The rows for a new Pane in Workspace `workspace`, from every
/// Workspace's account list (`State::account_lists`). A broken list of the
/// target Workspace shows the agent's row with the error as its tooltip
/// (activating it reports the error); other Workspaces' broken lists are
/// left out.
pub fn rows(workspace: &str, lists: &BTreeMap<String, Result<Vec<Account>>>) -> Vec<Row> {
    rows_with(workspace, lists, &|_| None)
}

/// An own account's usage detail (R110.2, F.8–F.10): its short figures and
/// whether they are hot (`crate::usage::view::menu_detail`); `None` →
/// [`NO_DATA`].
pub type DetailOf<'a> = &'a dyn Fn(&Account) -> Option<(String, bool)>;

/// [`rows`] with each own account's usage detail from `detail`.
pub fn rows_with(
    workspace: &str,
    lists: &BTreeMap<String, Result<Vec<Account>>>,
    detail: DetailOf<'_>,
) -> Vec<Row> {
    let mut out = vec![Row {
        keys: Some("Ctrl ⇧ T"),
        icon: Some("terminal-window"),
        ..Row::plain(RowKind::Shell, "", "Shell")
    }];
    for agent in AgentKind::ALL {
        let section = section_of(agent);
        let own: Vec<&Account> = match lists.get(workspace) {
            Some(Ok(l)) => l.iter().filter(|a| a.agent == agent).collect(),
            _ => Vec::new(),
        };
        if own.is_empty() {
            let mut r = Row {
                agent: Some(agent),
                workspace: Some(workspace.to_string()),
                detail: Some(WORKSPACE_DEFAULT.into()),
                ..Row::plain(RowKind::Agent, section, agent.label())
            };
            if let Some(Err(e)) = lists.get(workspace) {
                r.detail = Some("account list has errors".into());
                r.tooltip = Some(e.message.clone());
            }
            out.push(r);
        } else {
            let mut own = own;
            own.sort_by(|a, b| b.default.cmp(&a.default).then(a.name.cmp(&b.name)));
            out.extend(own.into_iter().map(|a| {
                let (text, hot) = detail(a).unwrap_or_else(|| (NO_DATA.into(), false));
                Row {
                    agent: Some(agent),
                    workspace: Some(workspace.to_string()),
                    detail: Some(text),
                    default: a.default,
                    hot,
                    ..Row::plain(RowKind::Account, section, &a.name)
                }
            }));
        }
        let mut foreign: Vec<&Account> = lists
            .iter()
            .filter(|(w, _)| w.as_str() != workspace)
            .filter_map(|(_, l)| l.as_ref().ok())
            .flatten()
            .filter(|a| a.agent == agent)
            .collect();
        foreign.sort_by(|a, b| a.workspace.cmp(&b.workspace).then(a.name.cmp(&b.name)));
        out.extend(foreign.into_iter().map(|a| Row {
            agent: Some(agent),
            workspace: Some(a.workspace.clone()),
            detail: Some(OTHER_WORKSPACE.into()),
            locked: true,
            sensitive: false,
            tooltip: Some(locked_tooltip(&a.workspace)),
            ..Row::plain(RowKind::Account, section, &a.name)
        }));
    }
    out.push(Row {
        detail: Some("one prompt, several agents".into()),
        icon: Some("users-three"),
        ..Row::plain(RowKind::FanOut, "OTHER", "Fan-out…")
    });
    out.push(Row {
        keys: Some("Ctrl ⇧ O"),
        icon: Some("globe-simple"),
        ..Row::plain(RowKind::Browser, "OTHER", "Browser")
    });
    out
}

/// The row the keyboard starts on: the first default account, else the
/// first sensitive row.
pub fn initial(rows: &[Row]) -> Option<usize> {
    rows.iter()
        .position(|r| r.kind == RowKind::Account && r.default && r.sensitive)
        .or_else(|| rows.iter().position(|r| r.sensitive))
}

/// `↑` / `↓` from `at`: the previous / next sensitive row, staying put at
/// either end.
pub fn step(rows: &[Row], at: Option<usize>, down: bool) -> Option<usize> {
    let Some(at) = at else {
        return initial(rows);
    };
    let found = if down {
        (at + 1..rows.len()).find(|&i| rows[i].sensitive)
    } else {
        (0..at).rev().find(|&i| rows[i].sensitive)
    };
    Some(found.unwrap_or(at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{accounts, AccountDecl};
    use crate::rpc::Error;
    use std::path::Path;

    fn decl(agent: &str, name: &str, dir: &str, default: bool) -> AccountDecl {
        AccountDecl {
            agent: agent.into(),
            name: name.into(),
            config_dir: dir.into(),
            default,
            ..Default::default()
        }
    }

    fn lists() -> BTreeMap<String, Result<Vec<Account>>> {
        let f = Path::new("/w/.rade/workspace.toml");
        let home = Path::new("/home/u");
        let mut m = BTreeMap::new();
        m.insert(
            "acme".to_string(),
            accounts(
                "acme",
                &[
                    decl("claude", "acme-work", "~/.claude-work", true),
                    decl("claude", "acme-team", "~/.claude-team", false),
                    decl("codex", "acme", "~/.codex", true),
                ],
                f,
                home,
            ),
        );
        m.insert(
            "personal".to_string(),
            accounts(
                "personal",
                &[decl("claude", "personal", "~/.claude", false)],
                f,
                home,
            ),
        );
        m
    }

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{}:{}", r.section, r.name))
            .collect()
    }

    #[test]
    fn rows_follow_spec_f_order_default_first_then_by_name_locked_last() {
        let r = rows("acme", &lists());
        assert_eq!(
            names(&r),
            [
                ":Shell",
                "CLAUDE CODE:acme-work",
                "CLAUDE CODE:acme-team",
                "CLAUDE CODE:personal",
                "CODEX:acme",
                "OTHER:Fan-out…",
                "OTHER:Browser",
            ]
        );
        assert_eq!(r[0].keys, Some("Ctrl ⇧ T"));
        assert_eq!(r[0].icon, Some("terminal-window"));
        assert_eq!(r[6].keys, Some("Ctrl ⇧ O"));
        assert_eq!(r[5].detail.as_deref(), Some("one prompt, several agents"));
        assert_eq!(r[5].icon, Some("users-three"));
        assert_eq!(r[6].icon, Some("globe-simple"));
    }

    #[test]
    fn default_is_ticked_and_details_say_no_data_yet() {
        let r = rows("acme", &lists());
        let by = |n: &str| r.iter().find(|x| x.name == n).unwrap();
        assert!(by("acme-work").default && !by("acme-team").default && by("acme").default);
        for n in ["acme-work", "acme-team", "acme"] {
            assert_eq!(by(n).detail.as_deref(), Some(NO_DATA));
            assert!(by(n).sensitive && !by(n).locked);
            assert_eq!(by(n).kind, RowKind::Account);
            assert_eq!(by(n).workspace.as_deref(), Some("acme"));
        }
    }

    #[test]
    fn account_of_another_workspace_is_locked_and_insensitive() {
        let r = rows("acme", &lists());
        let p = r.iter().find(|x| x.name == "personal").unwrap();
        assert!(p.locked && !p.sensitive && !p.default);
        assert_eq!(p.detail.as_deref(), Some("other workspace"));
        assert_eq!(p.workspace.as_deref(), Some("personal"));
        assert_eq!(
            p.tooltip.as_deref(),
            Some("Declared in Workspace personal; RADE never runs an Agent under another Workspace's account (ADR-0015)")
        );
    }

    #[test]
    fn an_agent_without_accounts_shows_one_workspace_default_row() {
        let r = rows("personal", &lists());
        assert_eq!(
            names(&r),
            [
                ":Shell",
                "CLAUDE CODE:personal",
                "CLAUDE CODE:acme-team",
                "CLAUDE CODE:acme-work",
                "CODEX:Codex",
                "CODEX:acme",
                "OTHER:Fan-out…",
                "OTHER:Browser",
            ]
        );
        let codex = &r[4];
        assert_eq!(codex.kind, RowKind::Agent);
        assert_eq!(codex.detail.as_deref(), Some("workspace default"));
        assert!(codex.sensitive && !codex.locked);
        // `personal` alone is its claude default (the first listed).
        assert!(r[1].default);
    }

    #[test]
    fn a_broken_target_list_shows_the_agent_row_with_the_error() {
        let mut l = lists();
        l.insert("acme".into(), Err(Error::usage("two defaults")));
        let r = rows("acme", &l);
        let c = &r[1];
        assert_eq!((c.kind, c.name.as_str()), (RowKind::Agent, "Claude Code"));
        assert_eq!(c.tooltip.as_deref(), Some("two defaults"));
        // Another Workspace's accounts are still shown, locked.
        assert!(r.iter().any(|x| x.name == "personal" && x.locked));
    }

    #[test]
    fn header_names_project_source_and_worktree() {
        assert_eq!(
            header(Some("Shop"), "shop-web", Some("checkout-v2")),
            "New in Shop / shop-web / checkout-v2"
        );
        assert_eq!(header(None, "api", None), "New in api");
    }

    #[test]
    fn r110_i_own_account_rows_carry_their_usage_detail() {
        let detail = |a: &Account| match a.name.as_str() {
            "acme-work" => Some(("5h 62% · wk 41%".to_string(), false)),
            "acme" => Some(("Spark 5h 81%".to_string(), true)),
            _ => None,
        };
        let r = rows_with("acme", &lists(), &detail);
        let by = |n: &str, locked: bool| {
            r.iter()
                .find(|x| x.name == n && x.locked == locked)
                .unwrap()
                .clone()
        };
        let w = by("acme-work", false);
        assert_eq!(
            (w.detail.as_deref(), w.hot),
            (Some("5h 62% · wk 41%"), false)
        );
        let x = by("acme", false);
        assert_eq!((x.detail.as_deref(), x.hot), (Some("Spark 5h 81%"), true));
        // No figures: no data yet; a locked row never shows figures.
        assert_eq!(by("acme-team", false).detail.as_deref(), Some(NO_DATA));
        assert_eq!(
            by("personal", true).detail.as_deref(),
            Some(OTHER_WORKSPACE)
        );
    }

    #[test]
    fn keyboard_starts_on_the_default_and_skips_insensitive_rows() {
        let r = rows("acme", &lists());
        assert_eq!(initial(&r), Some(1));
        assert_eq!(step(&r, Some(1), true), Some(2));
        // `personal` (3) is locked: ↓ from acme-team lands on acme.
        assert_eq!(step(&r, Some(2), true), Some(4));
        assert_eq!(step(&r, Some(4), false), Some(2));
        assert_eq!(step(&r, Some(6), true), Some(6));
        assert_eq!(step(&r, Some(0), false), Some(0));
        assert_eq!(step(&r, None, true), Some(1));
    }
}
