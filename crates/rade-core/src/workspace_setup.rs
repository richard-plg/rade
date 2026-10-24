//! Workspace setup, import and doctor (PRD R25, R26, ADR-0016): the pure
//! parts. The CLI (`rade-cli/src/workspace.rs`) runs git / gh / ssh and
//! feeds their output here; every file edit becomes a `setup::Change`.

use crate::setup::marked_block;
use crate::workspace::{Identity, Sign};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The marker lines of a Workspace's block in `~/.gitconfig`.
pub fn markers(ws: &str) -> (String, String) {
    (
        format!("# >>> rade workspace {ws} >>>"),
        format!("# <<< rade workspace {ws} <<<"),
    )
}

/// `~/rest` for a path under `home` (portable in a file that may move to
/// another machine), else the path as is.
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.into(),
        None => path.into(),
    }
}

/// The conditions of every `[includeIf "<condition>"]` header in a
/// gitconfig text (section names are case-insensitive).
pub fn includeif_conditions(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let head = l.get(..11)?;
            if !head.eq_ignore_ascii_case("[includeIf ") {
                return None;
            }
            let rest = l[11..].trim_start().strip_prefix('"')?;
            Some(rest[..rest.find('"')?].to_string())
        })
        .collect()
}

/// Whether an `includeIf` condition (`gitdir:~/ws/`, `gitdir:/abs/ws/`,
/// `gitdir/i:…`, ending in `/` or `/**`) covers exactly the repos under
/// `root`. Without the trailing `/` git matches only that one gitdir, not
/// the repos under it.
pub fn condition_matches_root(cond: &str, root: &Path, home: &Path) -> bool {
    let (pattern, fold) = if let Some(p) = cond.strip_prefix("gitdir/i:") {
        (p, true)
    } else if let Some(p) = cond.strip_prefix("gitdir:") {
        (p, false)
    } else {
        return false;
    };
    let pattern = pattern.strip_suffix("**").unwrap_or(pattern);
    let Some(pattern) = pattern.strip_suffix('/') else {
        return false;
    };
    let path = expand(pattern, home);
    let root = root.to_string_lossy();
    let root = root.trim_end_matches('/');
    let path = path.to_string_lossy();
    if fold {
        path.to_lowercase() == root.to_lowercase()
    } else {
        path == root
    }
}

/// The `[includeIf]` block for a Workspace root.
pub fn includeif_body(root: &Path, home: &Path) -> String {
    let r = tilde(root, home);
    format!("[includeIf \"gitdir:{r}/\"]\n\tpath = {r}/.gitconfig\n")
}

/// New `~/.gitconfig` text with the Workspace's marked block, or `None`
/// when nothing changes: the block is already right, or a hand-written
/// `includeIf` (outside the block) already points at the root (R25.2).
pub fn gitconfig_block(text: &str, ws: &str, root: &Path, home: &Path) -> Option<String> {
    let (begin, end) = markers(ws);
    let outside = marked_block(text, &begin, &end, "").replace(&format!("{begin}\n{end}\n"), "");
    if includeif_conditions(&outside)
        .iter()
        .any(|c| condition_matches_root(c, root, home))
    {
        return None;
    }
    let new = marked_block(text, &begin, &end, &includeif_body(root, home));
    (new != text).then_some(new)
}

/// `~/.ssh/<key>`.
pub fn key_path(key: &str, home: &Path) -> PathBuf {
    home.join(".ssh").join(key)
}

/// Whether a key path from git config (`~/…` or absolute) exists.
pub fn key_path_exists(path: &str, home: &Path) -> bool {
    expand(path.trim(), home).exists()
}

/// `core.sshCommand` / `GIT_SSH_COMMAND` for a key.
pub fn ssh_command(key: &Path) -> String {
    format!("ssh -i {} -o IdentitiesOnly=yes", key.display())
}

/// The keys `<root>/.gitconfig` should hold for an identity (R25.2), in
/// order. Setup sets only the ones missing.
pub fn gitconfig_keys(id: &Identity, home: &Path) -> Vec<(&'static str, String)> {
    let mut keys = Vec::new();
    if let Some(n) = &id.git_name {
        keys.push(("user.name", n.clone()));
    }
    if let Some(e) = &id.git_email {
        keys.push(("user.email", e.clone()));
    }
    if let Some(k) = &id.ssh_key {
        let path = key_path(k, home);
        keys.push(("core.sshCommand", ssh_command(&path)));
        if id.signs() {
            keys.push(("gpg.format", "ssh".into()));
            keys.push(("user.signingkey", format!("{}.pub", path.display())));
            keys.push(("commit.gpgsign", "true".into()));
            keys.push(("tag.gpgsign", "true".into()));
        }
    }
    keys
}

/// Set `entries` (`(table, key, value)`, table `""` = top level) in a
/// `workspace.toml` text where missing. Returns the new text and the
/// entries that already hold another value (`table.key = "old"`; left).
pub fn merge_toml(
    old: Option<&str>,
    entries: &[(&str, &str, String)],
) -> Result<(String, Vec<String>), String> {
    let mut doc: toml_edit::DocumentMut = old
        .unwrap_or("")
        .parse()
        .map_err(|e| format!("invalid TOML: {e}"))?;
    let mut conflicts = Vec::new();
    for (table, key, value) in entries {
        let t = if table.is_empty() {
            doc.as_table_mut()
        } else {
            doc.entry(table)
                .or_insert(toml_edit::table())
                .as_table_mut()
                .ok_or(format!("`{table}` is not a table"))?
        };
        match t.get(key) {
            Some(v) if v.as_str() == Some(value) => {}
            Some(v) => {
                let name = if table.is_empty() {
                    key.to_string()
                } else {
                    format!("{table}.{key}")
                };
                conflicts.push(format!("{name} = {}", v.to_string().trim()));
            }
            None => t[key] = toml_edit::value(value.as_str()),
        }
    }
    Ok((doc.to_string(), conflicts))
}

/// `gh auth login` for a Workspace (R25.2): its own `hosts.yml` through
/// `--insecure-storage` (ADR-0016), the scopes `gh ssh-key add` needs, and
/// `--skip-ssh-key` when this `gh` has it (2.46 does not: it then asks).
pub fn login_argv(skip_ssh_key: bool, sign: bool) -> Vec<String> {
    let scopes = if sign {
        "admin:public_key,admin:ssh_signing_key"
    } else {
        "admin:public_key"
    };
    let mut a: Vec<String> = [
        "gh",
        "auth",
        "login",
        "--hostname",
        "github.com",
        "--git-protocol",
        "ssh",
        "--web",
        "--insecure-storage",
        "--scopes",
        scopes,
    ]
    .map(String::from)
    .into();
    if skip_ssh_key {
        a.push("--skip-ssh-key".into());
    }
    a
}

/// Whether a public key (`type base64 comment`) is among the keys an
/// account lists (one `type base64` per line, as `gh api user/keys --jq
/// '.[].key'` prints them).
pub fn key_on_account(pubkey: &str, listed: &str) -> bool {
    let id = |l: &str| l.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    let k = id(pubkey);
    !k.is_empty() && listed.lines().any(|l| id(l) == k)
}

/// `git version 2.53.0` / `gh version 2.46.0 (…)` ≥ `min` (major, minor).
pub fn version_at_least(text: &str, min: (u32, u32)) -> bool {
    let Some(v) = text
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))
    else {
        return false;
    };
    let mut n = v.split('.').map(|p| {
        p.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    });
    (n.next().unwrap_or(0), n.next().unwrap_or(0)) >= min
}

/// `git config --list -z` → key → value (the last one wins, as git reads).
pub fn parse_config_z(bytes: &[u8]) -> BTreeMap<String, String> {
    bytes
        .split(|b| *b == 0)
        .filter_map(|e| std::str::from_utf8(e).ok())
        .filter(|e| !e.is_empty())
        .map(|e| match e.split_once('\n') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (e.to_string(), "true".into()),
        })
        .collect()
}

/// A key's file name when it lives in `~/.ssh` (`.pub` dropped).
pub fn ssh_key_name(path: &str, home: &Path) -> Option<String> {
    let p = expand(path.trim(), home);
    if p.parent()? != home.join(".ssh") {
        return None;
    }
    let name = p.file_name()?.to_string_lossy();
    Some(name.trim_end_matches(".pub").to_string())
}

/// The `-i <key>` of an ssh command line.
pub fn ssh_key_from_command(cmd: &str) -> Option<String> {
    let mut w = cmd.split_whitespace();
    while let Some(a) = w.next() {
        if a == "-i" {
            return w.next().map(String::from);
        }
        if let Some(k) = a.strip_prefix("-i") {
            return Some(k.to_string());
        }
    }
    None
}

/// The ssh host of a `url.<alias>.insteadof` rewriting GitHub URLs
/// (`github-globex:` → `github-globex`, `git@github.com:` → `github.com`).
pub fn insteadof_host(cfg: &BTreeMap<String, String>) -> Option<String> {
    cfg.iter().find_map(|(k, v)| {
        let alias = k.strip_prefix("url.")?.strip_suffix(".insteadof")?;
        if !v.contains("github.com") && !alias.contains("github") {
            return None;
        }
        let host = alias.trim_end_matches(':').trim_end_matches('/');
        let host = host.rsplit('@').next()?;
        let host = host.strip_prefix("ssh://").unwrap_or(host);
        (!host.is_empty() && !host.contains("://")).then(|| host.to_string())
    })
}

/// The first `identityfile` of `ssh -G <host>` output.
pub fn ssh_g_identityfile(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.strip_prefix("identityfile "))
        .map(|s| s.trim().to_string())
}

/// The owner of a GitHub remote URL (`git@github.com:o/r.git`,
/// `alias:o/r`, `https://github.com/o/r`, `ssh://git@github.com/o/r`).
pub fn owner_of(url: &str) -> Option<String> {
    let url = url.trim();
    let path = if let Some((_, rest)) = url.split_once("://") {
        rest.split_once('/')?.1
    } else {
        url.split_once(':')?.1
    };
    let mut parts = path.trim_start_matches('/').split('/');
    let owner = parts.next()?;
    parts.next()?;
    (!owner.is_empty()).then(|| owner.to_string())
}

/// The most common owner among remote URLs (ties: the first seen).
pub fn most_common_owner<'a>(urls: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for o in urls.into_iter().filter_map(owner_of) {
        match counts.iter_mut().find(|(x, _)| *x == o) {
            Some((_, n)) => *n += 1,
            None => counts.push((o, 1)),
        }
    }
    let max = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == max).map(|(o, _)| o)
}

/// The identity a Source's effective git config shows (R25.3): name,
/// email, the SSH key from `user.signingkey`, else `core.sshCommand`'s
/// `-i`, else `alias_key` (`ssh -G` of the `insteadOf` alias), and `sign`.
pub fn infer_identity(
    cfg: &BTreeMap<String, String>,
    alias_key: Option<&str>,
    home: &Path,
) -> Identity {
    let get = |k: &str| cfg.get(k).filter(|v| !v.trim().is_empty()).cloned();
    let ssh_key = get("user.signingkey")
        .and_then(|k| ssh_key_name(&k, home))
        .or_else(|| {
            get("core.sshcommand")
                .and_then(|c| ssh_key_from_command(&c))
                .and_then(|k| ssh_key_name(&k, home))
        })
        .or_else(|| alias_key.and_then(|k| ssh_key_name(k, home)));
    let signs = get("gpg.format").as_deref() == Some("ssh")
        && get("commit.gpgsign").is_some_and(|v| v == "true");
    Identity {
        git_name: get("user.name"),
        git_email: get("user.email"),
        github: None,
        ssh_key,
        sign: Some(if signs { Sign::Ssh } else { Sign::None }),
    }
}

/// Whether a gh `hosts.yml` holds an `oauth_token` key: the token is in
/// the file, not the shared keyring slot (R26 check 7). The value is
/// never returned.
pub fn hosts_has_token(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim_start().starts_with("oauth_token:"))
}

/// `file:line` of every line that assigns `var` (`var=` not in a comment)
/// in the given rc files (R26 check 11 hint).
pub fn rc_lines_setting(var: &str, files: &[(PathBuf, String)]) -> Vec<String> {
    let pat = format!("{var}=");
    let mut out = Vec::new();
    for (path, text) in files {
        for (n, line) in text.lines().enumerate() {
            let code = line.split('#').next().unwrap_or("");
            let hit = code.match_indices(&pat).any(|(i, _)| {
                !code[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            });
            if hit {
                out.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
    out
}

/// Claude Code's account file for a config dir: `~/.claude.json` for
/// `~/.claude`, else `<dir>/.claude.json` (verified on Claude Code 2.x).
pub fn claude_json(dir: &Path, home: &Path) -> PathBuf {
    if dir == home.join(".claude") {
        home.join(".claude.json")
    } else {
        dir.join(".claude.json")
    }
}

/// Whether a `.claude.json` has a logged-in account
/// (`oauthAccount.emailAddress`).
pub fn claude_logged_in(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .is_ok_and(|v| v["oauthAccount"]["emailAddress"].as_str().is_some())
}

/// The conditions in `git config --get-regexp '^includeif\.'` output
/// (`includeif.<condition>.path <value>` per line).
pub fn includeif_conditions_from_regexp(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            let key = l.split_whitespace().next()?;
            let rest = key.strip_prefix("includeif.")?;
            Some(rest.strip_suffix(".path")?.to_string())
        })
        .collect()
}

#[cfg(test)]
#[path = "workspace_setup_tests.rs"]
mod tests;
