//! `rade source add owner/repo | repo | URL` (PRD R24.2, ADR-0015): clone
//! with `gh repo clone` into the Workspace root under the Workspace's env,
//! here in the caller's terminal (prompts and progress show there; the app
//! never blocks), then register the folder like a path.

use crate::call;
use rade_core::rpc::Error;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

/// The repository's folder name: the spec's last part without `.git`.
pub fn repo_name(spec: &str) -> Option<String> {
    let last = spec
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()?
        .trim_end_matches(".git");
    (!last.is_empty()).then(|| last.to_string())
}

/// `repo` alone gets the Workspace's `[clone] default_owner/` in front.
pub fn full_spec(spec: &str, owner: Option<&str>) -> Result<String, Error> {
    if spec.contains(['/', ':']) {
        return Ok(spec.to_string());
    }
    owner.map(|o| format!("{o}/{spec}")).ok_or_else(|| {
        Error::usage(format!(
            "'{spec}' needs an owner: use owner/{spec}, or set [clone] default_owner in the Workspace's .rade/workspace.toml"
        ))
    })
}

/// The env `gh` runs with: the target Workspace's `[env]` set, and every
/// variable another Workspace's `[env]` sets removed (the caller may be a
/// Pane of that other identity, ADR-0015); `PATH` is never removed.
pub fn clone_env(ws: &Value, all: &[Value]) -> (Vec<(String, String)>, Vec<String>) {
    let vars = |w: &Value| w["env"].as_object().cloned().unwrap_or_default();
    let own = vars(ws);
    let set = own
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect();
    let mut unset: Vec<String> = all
        .iter()
        .flat_map(|w| vars(w).into_iter().map(|(k, _)| k))
        .filter(|k| k != "PATH" && !own.contains_key(k))
        .collect();
    unset.sort();
    unset.dedup();
    (set, unset)
}

pub fn add(spec: &str, name: Option<&str>, workspace: Option<&str>) -> Result<Value, Error> {
    let (ws, all) = pick(workspace)?;
    one(&ws, &all, spec, name, &[]).map(|(v, _)| v)
}

/// The Workspace (a `workspace.list` row) — `workspace` (id or name), else
/// the caller's / shown one — and every row.
pub fn pick(workspace: Option<&str>) -> Result<(Value, Vec<Value>), Error> {
    let list = call("workspace.list", json!({}))?;
    let all = list["workspaces"].as_array().cloned().unwrap_or_default();
    let ws = match workspace {
        Some(q) => all
            .iter()
            .find(|w| w["workspace"] == q || w["name"] == q)
            .ok_or_else(|| Error::not_found(format!("no workspace '{q}'")))?,
        None => all
            .iter()
            .find(|w| w["current"] == true)
            .ok_or_else(|| Error::internal("no current workspace"))?,
    }
    .clone();
    Ok((ws, all))
}

/// Clone one spec into `ws` with its env plus `extra` (R25.4:
/// `GIT_SSH_COMMAND`), unless its folder exists, then register it.
/// Returns the `source.add` result and whether it cloned.
pub fn one(
    ws: &Value,
    all: &[Value],
    spec: &str,
    name: Option<&str>,
    extra: &[(String, String)],
) -> Result<(Value, bool), Error> {
    let full = full_spec(spec, ws["default_owner"].as_str())?;
    let repo =
        repo_name(&full).ok_or_else(|| Error::usage(format!("no repository in '{spec}'")))?;
    let dest = PathBuf::from(ws["root"].as_str().unwrap_or("/")).join(&repo);
    let cloned = !dest.exists();
    if !cloned {
        eprintln!("rade: {} exists, not cloning again", dest.display());
    } else {
        let (env, unset) = clone_env(ws, all);
        let mut gh = Command::new("gh");
        for k in unset {
            gh.env_remove(k);
        }
        let status = gh
            .args(["repo", "clone", &full])
            .arg(&dest)
            .envs(env)
            .envs(extra.iter().cloned())
            .status()
            .map_err(|e| {
                Error::usage(if e.kind() == std::io::ErrorKind::NotFound {
                    "GitHub CLI (gh) is not installed: see https://cli.github.com".to_string()
                } else {
                    format!("cannot run gh: {e}")
                })
            })?;
        if !status.success() {
            return Err(Error::usage(format!(
                "gh repo clone {full} failed ({status})"
            )));
        }
    }
    let v = call(
        "source.add",
        json!({"path": dest, "name": name, "workspace": ws["workspace"]}),
    )?;
    Ok((v, cloned))
}
