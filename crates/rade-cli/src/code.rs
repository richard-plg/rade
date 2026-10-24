//! `rade code …` (PRD R86, docs/API.md `code.*`): code navigation for
//! Agents through the app's language servers (one warm server per
//! Worktree, shared with the owner's Viewer) and a diff's Impact (R88.3).
//! Text output by default,
//! `--json` for machines; every answer says where it came from.

use clap::Subcommand;
use rade_core::code::nav::Answer;
use rade_core::rpc::Error;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum CodeCmd {
    /// Where the name at file:line:col is defined.
    Def(At),
    /// Every use of the name at file:line:col.
    Refs(At),
    /// Every call of the function at file:line:col, named by the calling function (call hierarchy), then references that are not calls.
    Callers(Calls),
    /// Functions the function at file:line:col calls.
    Callees(Calls),
    /// Implementations of the trait / interface / method at file:line:col.
    Impl(At),
    /// The type and docs of the name at file:line:col (hover).
    Hover(At),
    /// A file's symbols (its outline), or with --query the symbols of the Worktree whose name contains the text.
    Symbols {
        /// A file (the outline); with --query, a file or folder in the Worktree (default: the current directory).
        file: Option<String>,
        /// Search the Worktree's symbols (workspace symbols) for this text.
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 20.0)]
        wait: f64,
    },
    /// The diff's changed symbols and their callers outside it (the Review's Impact): server callers, text matches in tests, removed symbols still used.
    Impact {
        /// A Worktree's name (in the current Source first); default: the one holding the current directory.
        #[arg(long)]
        worktree: Option<String>,
        /// Compare with this branch or commit (default: the Worktree's base).
        #[arg(long)]
        base: Option<String>,
        /// Seconds for the whole answer (waiting for an indexing server included); past it, text matches.
        #[arg(long, default_value_t = 60.0)]
        wait: f64,
    },
    /// Running language servers.
    Status,
    /// Stop the server for a file and forget its crashes; the next query starts it.
    Restart { file: String },
}

#[derive(clap::Args)]
pub struct At {
    /// `path:line:col` (1-based; col counts characters), relative to the current directory.
    target: String,
    /// Seconds to wait for an indexing server before answering with text matches.
    #[arg(long, default_value_t = 20.0)]
    wait: f64,
}

#[derive(clap::Args)]
pub struct Calls {
    #[command(flatten)]
    at: At,
    /// Levels of the call hierarchy: 2 adds the callers of each caller (indented), …
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=10))]
    depth: u32,
}

/// `path:line[:col]` → (absolute path, line, col).
fn at(target: &str) -> Result<(PathBuf, u32, u32), Error> {
    let bad = || Error::usage(format!("expected path:line:col, got {target}"));
    let mut parts = target.rsplitn(3, ':');
    let (a, b, c) = (parts.next(), parts.next(), parts.next());
    let (path, line, col) = match (a, b, c) {
        (Some(col), Some(line), Some(path)) if line.parse::<u32>().is_ok() => (path, line, col),
        (Some(line), Some(path), None) => (path, line, "1"),
        _ => return Err(bad()),
    };
    let (line, col) = (
        line.parse().map_err(|_| bad())?,
        col.parse().map_err(|_| bad())?,
    );
    Ok((crate::abs(path.into())?, line, col))
}

pub fn request(c: &CodeCmd) -> Result<(&'static str, Value), Error> {
    let cmd = c;
    let q = |name: &str, a: &At| -> Result<(&'static str, Value), Error> {
        let (path, line, col) = at(&a.target)?;
        Ok((
            "code.query",
            json!({"query": name, "path": path, "line": line, "col": col, "wait": a.wait}),
        ))
    };
    match c {
        CodeCmd::Def(a) => q("def", a),
        CodeCmd::Refs(a) => q("refs", a),
        CodeCmd::Callers(c) | CodeCmd::Callees(c) => {
            let name = if matches!(cmd, CodeCmd::Callers(_)) {
                "callers"
            } else {
                "callees"
            };
            let (m, mut p) = q(name, &c.at)?;
            p["depth"] = json!(c.depth);
            Ok((m, p))
        }
        CodeCmd::Impl(a) => q("impl", a),
        CodeCmd::Hover(a) => q("hover", a),
        CodeCmd::Symbols {
            file,
            query: Some(text),
            wait,
        } => Ok((
            "code.query",
            json!({"query": "workspace", "path": crate::abs(file.as_deref().unwrap_or(".").into())?, "text": text, "wait": wait}),
        )),
        CodeCmd::Symbols {
            file: Some(file),
            query: None,
            wait,
        } => Ok((
            "code.query",
            json!({"query": "symbols", "path": crate::abs(file.into())?, "line": 1, "col": 1, "wait": wait}),
        )),
        CodeCmd::Symbols { file: None, .. } => Err(Error::usage(
            "give a file (its outline) or --query <text> (the Worktree's symbols)",
        )),
        CodeCmd::Impact {
            worktree,
            base,
            wait,
        } => Ok((
            "code.impact",
            json!({"path": crate::abs(".".into())?, "worktree": worktree, "base": base, "wait": wait}),
        )),
        CodeCmd::Status => Ok(("code.status", json!({}))),
        CodeCmd::Restart { file } => {
            Ok(("code.restart", json!({"path": crate::abs(file.into())?})))
        }
    }
}

pub fn print(c: &CodeCmd, r: &Value) {
    match c {
        CodeCmd::Status => {
            let servers = r["servers"].as_array().cloned().unwrap_or_default();
            if servers.is_empty() {
                println!("no language servers running");
            }
            for s in servers {
                println!(
                    "{}\t{}\tpid {}\tidle {}s{}",
                    s["language"].as_str().unwrap_or("-"),
                    s["root"].as_str().unwrap_or("-"),
                    s["pid"],
                    s["idle_secs"],
                    if s["busy"] == true { "\tindexing" } else { "" }
                );
            }
        }
        CodeCmd::Impact { .. } => {
            match serde_json::from_value::<rade_core::code::impact::Impact>(r.clone()) {
                Ok(i) => print!("{}", rade_core::code::impact::render(&i, usize::MAX)),
                Err(_) => println!("{r}"),
            }
        }
        CodeCmd::Restart { .. } => println!(
            "{}",
            if r["stopped"] == true {
                "stopped; the next query starts it"
            } else {
                "not running (its crashes and pause are forgotten); the next query starts it"
            }
        ),
        _ => match serde_json::from_value::<Answer>(r.clone()) {
            Ok(a) => print!("{}", rade_core::code::nav::render(&a)),
            Err(_) => println!("{r}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r86_targets() {
        let (p, l, c) = at("/a/b.rs:12:5").unwrap();
        assert_eq!((p, l, c), ("/a/b.rs".into(), 12, 5));
        let (p, l, c) = at("/a/b.rs:12").unwrap();
        assert_eq!((p, l, c), ("/a/b.rs".into(), 12, 1));
        assert!(at("/a/b.rs").is_err());
        assert!(at("/a/b.rs:x:1").is_err());
    }
}
