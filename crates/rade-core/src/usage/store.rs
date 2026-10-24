//! The Usage store (R109.3, docs/API.md "Usage store"):
//! `$XDG_STATE_HOME/rade/usage/<agent>-<sha8>.jsonl` (`rade-dev` for the
//! Devel flavor), one file per login keyed by the canonical `config_dir`,
//! holding limit samples and token rows; lines past 90 days are pruned.
//! Never holds a token or a credential.

use super::{Limit, TokenRow};
use crate::account::AgentKind;
use crate::paths::{home_dir, Env, Flavor};
use serde::{Deserialize, Serialize};
// The hash through `Update` / `FixedOutput` (the umbrella trait's name is a
// word removed in 1.0.0, R101.5).
use sha2::digest::{FixedOutput, Update};
use sha2::Sha256;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Samples older than this are pruned.
pub const RETENTION_DAYS: i64 = 90;

/// `$XDG_STATE_HOME/<flavor>/usage` or `~/.local/state/<flavor>/usage`.
pub fn usage_dir(env: Env) -> PathBuf {
    usage_dir_of(env, Flavor::current())
}

pub fn usage_dir_of(env: Env, flavor: Flavor) -> PathBuf {
    env("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir(env).join(".local/state"))
        .join(flavor.name())
        .join("usage")
}

/// The first 8 hex digits of the SHA-256 of `config_dir`'s `realpath` (the
/// path as given when it does not exist).
pub fn sha8(config_dir: &Path) -> String {
    let canon = std::fs::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
    let mut hasher = Sha256::default();
    hasher.update(canon.as_os_str().as_encoded_bytes());
    let digest = hasher.finalize_fixed();
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// `<dir>/<agent>-<sha8>.jsonl`: two Workspaces declaring one login (or a
/// symlink to it) share one file.
pub fn store_path(dir: &Path, agent: AgentKind, config_dir: &Path) -> PathBuf {
    dir.join(format!("{}-{}.jsonl", agent.as_str(), sha8(config_dir)))
}

/// The tail positions of the same login, beside its store file.
pub fn book_path(store: &Path) -> PathBuf {
    store.with_extension("tail.json")
}

/// One store line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Record {
    Limit {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        #[serde(flatten)]
        limit: Limit,
    },
    Tokens {
        t: i64,
        #[serde(flatten)]
        row: TokenRow,
    },
}

impl Record {
    pub fn t(&self) -> i64 {
        match self {
            Record::Limit { limit, .. } => limit.t,
            Record::Tokens { t, .. } => *t,
        }
    }
}

/// Append records (one JSON line each), creating the file and its folder.
pub fn append(path: &Path, records: &[Record]) -> std::io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut text = String::new();
    for r in records {
        text.push_str(&serde_json::to_string(r).unwrap_or_default());
        text.push('\n');
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}

/// Every readable record; broken lines are skipped.
pub fn read(path: &Path) -> Vec<Record> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Drop lines older than [`RETENTION_DAYS`] before `now` (and broken
/// ones); rewritten atomically only when something goes. Returns how many
/// lines went.
pub fn prune(path: &Path, now: i64) -> std::io::Result<usize> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let cutoff = now - RETENTION_DAYS * 86_400;
    let mut kept = String::new();
    let mut gone = 0;
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Record>(l) {
            Ok(r) if r.t() >= cutoff => {
                kept.push_str(l);
                kept.push('\n');
            }
            _ => gone += 1,
        }
    }
    if gone > 0 {
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, kept)?;
        std::fs::rename(tmp, path)?;
    }
    Ok(gone)
}

/// The newest sample of each limit (by [`Limit::key`]).
pub fn latest(records: &[Record]) -> Vec<Limit> {
    // By reference first, cloned once at the end (a 90-day store holds
    // tens of thousands of samples, R111-d).
    let mut at: std::collections::HashMap<(&str, &str), usize> = Default::default();
    let mut out: Vec<&Limit> = Vec::new();
    for r in records {
        if let Record::Limit { limit, .. } = r {
            match at.entry((limit.limit_id.as_str(), limit.name.as_str())) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    let x = &mut out[*e.get()];
                    if limit.t >= x.t {
                        *x = limit;
                    }
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(out.len());
                    out.push(limit);
                }
            }
        }
    }
    out.into_iter().cloned().collect()
}

/// Whether `l`'s [`Limit::key`] is `key` (without building it).
fn has_key(l: &Limit, key: &str) -> bool {
    let n = l.limit_id.len();
    key.len() == n + 1 + l.name.len()
        && key.starts_with(l.limit_id.as_str())
        && key.as_bytes()[n] == b'/'
        && key.ends_with(l.name.as_str())
}

/// Every sample of one limit, oldest first (for the forecast).
pub fn series<'a>(records: &'a [Record], key: &str) -> Vec<&'a Limit> {
    let mut v: Vec<&Limit> = records
        .iter()
        .filter_map(|r| match r {
            Record::Limit { limit, .. } if has_key(limit, key) => Some(limit),
            _ => None,
        })
        .collect();
    v.sort_by_key(|l| l.t);
    v
}

/// The records of a store that the Usage page, the `+` menu and the usage
/// cell need at `built` or any later time (R111-d): every token row, the
/// newest sample of each limit, and the samples of each limit's current
/// window (by [`limits::current_window`]'s rule; every sample of a limit
/// that has one stamped after `built` + [`limits::SKEW_S`], since its window
/// depends on `now` then). Store order is kept, so every answer computed
/// from them equals the answer from the whole store.
pub fn for_view(records: &[Record], built: i64) -> Vec<Record> {
    use std::collections::{HashMap, HashSet};
    let mut by_key: HashMap<(&str, &str), Vec<(usize, &Limit)>> = HashMap::new();
    let mut keep: HashSet<usize> = HashSet::new();
    for (i, r) in records.iter().enumerate() {
        match r {
            Record::Tokens { .. } => {
                keep.insert(i);
            }
            Record::Limit { limit, .. } => by_key
                .entry((limit.limit_id.as_str(), limit.name.as_str()))
                .or_default()
                .push((i, limit)),
        }
    }
    for samples in by_key.values() {
        let all = || samples.iter().map(|(i, _)| *i);
        if samples
            .iter()
            .any(|(_, l)| l.t > built + super::limits::SKEW_S)
        {
            keep.extend(all());
            continue;
        }
        // The first (the order of `latest`), the newest (the last of the
        // newest time in store order) and the current window.
        let mut mine: HashSet<usize> = HashSet::new();
        mine.extend(samples.first().map(|(i, _)| *i));
        mine.extend(
            samples
                .iter()
                .rev()
                .max_by_key(|(_, l)| l.t)
                .map(|(i, _)| *i),
        );
        let refs: Vec<&Limit> = samples.iter().map(|(_, l)| *l).collect();
        let window = super::limits::current_window(&refs, built);
        mine.extend(
            samples
                .iter()
                .filter(|(_, l)| window.iter().any(|w| std::ptr::eq(*w, *l)))
                .map(|(i, _)| *i),
        );
        // Kept alone they must make the same window (no sample is ever
        // stamped later than `built` + skew here, so a later `now` keeps it).
        let kept: Vec<&Limit> = samples
            .iter()
            .filter(|(i, _)| mine.contains(i))
            .map(|(_, l)| *l)
            .collect();
        if super::limits::current_window(&kept, built) == window {
            keep.extend(mine);
        } else {
            keep.extend(all());
        }
    }
    records
        .iter()
        .enumerate()
        .filter(|(i, _)| keep.contains(i))
        .map(|(_, r)| r.clone())
        .collect()
}

/// Whether a new sample is worth a line: the statusline runs many times a
/// minute, so an unchanged value is written at most once a minute.
pub fn worth_recording(prev: Option<&Limit>, new: &Limit) -> bool {
    match prev {
        None => true,
        Some(p) => {
            p.used_percent != new.used_percent
                || p.resets_at != new.resets_at
                || p.blocked != new.blocked
                || p.used_usd != new.used_usd
                || p.balance != new.balance
                || new.t - p.t >= 60
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rade-ustore-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(t: i64, used: f64) -> Record {
        Record::Limit {
            account: Some("acme-work".into()),
            limit: Limit {
                limit_id: "five_hour".into(),
                group: "Session".into(),
                name: "5-hour".into(),
                used_percent: Some(used),
                source: "statusline".into(),
                t,
                ..Limit::default()
            },
        }
    }

    #[test]
    fn r109_g_store_path_by_realpath_and_90_day_prune() {
        let home = tmp("g");
        let work = home.join(".claude-work");
        std::fs::create_dir_all(&work).unwrap();
        std::os::unix::fs::symlink(&work, home.join("link")).unwrap();
        let state = home.join("state");
        let env_map = |k: &str| match k {
            "XDG_STATE_HOME" => Some(state.display().to_string()),
            "HOME" => Some(home.display().to_string()),
            _ => None,
        };
        let dir = usage_dir_of(&env_map, Flavor::Installed);
        assert_eq!(dir, state.join("rade/usage"));
        assert_eq!(
            usage_dir_of(&env_map, Flavor::Devel),
            state.join("rade-dev/usage")
        );
        let a = store_path(&dir, AgentKind::Claude, &work);
        let b = store_path(&dir, AgentKind::Claude, &home.join("link"));
        assert_eq!(a, b);
        let name = a.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("claude-") && name.ends_with(".jsonl"));
        assert_eq!(name.len(), "claude-".len() + 8 + ".jsonl".len());
        assert_ne!(a, store_path(&dir, AgentKind::Codex, &work));
        // Without XDG_STATE_HOME: ~/.local/state.
        let no_xdg = |k: &str| (k == "HOME").then(|| home.display().to_string());
        assert_eq!(
            usage_dir_of(&no_xdg, Flavor::Installed),
            home.join(".local/state/rade/usage")
        );

        let now = 1_800_000_000;
        let day = 86_400;
        append(
            &a,
            &[sample(now - 91 * day, 10.0), sample(now - 89 * day, 20.0)],
        )
        .unwrap();
        assert_eq!(prune(&a, now).unwrap(), 1);
        let left = read(&a);
        assert_eq!(left, vec![sample(now - 89 * day, 20.0)]);
        // Nothing more to prune: the file is not rewritten.
        assert_eq!(prune(&a, now).unwrap(), 0);
        assert_eq!(prune(&dir.join("missing.jsonl"), now).unwrap(), 0);
    }

    #[test]
    fn records_round_trip_latest_and_dedupe() {
        let d = tmp("rt");
        let p = d.join("codex-00000000.jsonl");
        let tok = Record::Tokens {
            t: 5,
            row: TokenRow {
                day: "2026-10-23".into(),
                project: "Shop".into(),
                model: "gpt".into(),
                input: 1,
                output: 2,
                cache_creation: 0,
                cache_read: 9,
            },
        };
        append(&p, &[sample(1, 10.0), tok.clone(), sample(3, 30.0)]).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"{broken\n")
            .unwrap();
        let r = read(&p);
        assert_eq!(r.len(), 3);
        assert_eq!(r[1], tok);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains(r#""kind":"tokens""#) && text.contains(r#""kind":"limit""#));
        let l = latest(&r);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].used_percent, Some(30.0));
        assert_eq!(series(&r, "five_hour/5-hour").len(), 2);

        let Record::Limit { limit: a, .. } = sample(100, 50.0) else {
            unreachable!()
        };
        let mut b = a.clone();
        b.t = 130;
        assert!(!worth_recording(Some(&a), &b));
        b.t = 160;
        assert!(worth_recording(Some(&a), &b));
        b.t = 101;
        b.used_percent = Some(51.0);
        assert!(worth_recording(Some(&a), &b));
        assert!(worth_recording(None, &b));
    }
}
