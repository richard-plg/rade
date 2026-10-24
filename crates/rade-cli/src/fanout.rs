//! `rade fanout …` (docs/API.md, PRD R15): start a Fan-out, or list /
//! status / judge / choose / discard / wait on one.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub struct FanoutArgs {
    #[command(subcommand)]
    cmd: Option<FanoutCmd>,
    /// Agent Presets, one Agent (and Worktree) each; repeats allowed: claude,codex,codex.
    #[arg(long, value_delimiter = ',', required = true)]
    agents: Vec<String>,
    #[arg(long)]
    source: Option<String>,
    /// Start every Worktree here (default: the Source's main branch).
    #[arg(long)]
    base: Option<String>,
    /// Fan-out name (default fanout-<n>); Worktrees are <name>-<i>.
    #[arg(long)]
    name: Option<String>,
    /// The task every Agent gets.
    #[arg(required = true)]
    prompt: Option<String>,
}

#[derive(Args)]
struct At {
    /// Fan-out id (e.g. fanout-1).
    fanout: String,
    #[arg(long)]
    source: Option<String>,
}

#[derive(Subcommand)]
enum FanoutCmd {
    /// List Fan-outs (all, or one Source's).
    List {
        #[arg(long)]
        source: Option<String>,
    },
    /// Phase, progress, each Agent's status and diff size, the Judge's pick.
    Status {
        #[command(flatten)]
        at: At,
    },
    /// Start a read-only Judge Agent that compares the diffs and picks a Winner.
    Judge {
        #[command(flatten)]
        at: At,
        /// Judge Preset (default: [fanout] judge in config.toml, else claude).
        #[arg(long)]
        agent: Option<String>,
    },
    /// Apply a Winner (default: the Judge's pick). RADE never commits.
    Choose {
        #[command(flatten)]
        at: At,
        /// Agent label (claude-1) or Worktree name (fanout-1-1).
        winner: Option<String>,
        /// squash: stage its whole result on the Source's current branch; keep: keep its Worktree.
        #[arg(long, default_value = "squash")]
        strategy: String,
        /// Then remove the other Worktrees and branches and close the Fan-out.
        #[arg(long)]
        cleanup: bool,
    },
    /// Close the Fan-out and remove its Worktrees and branches (a kept Winner's excepted).
    Discard {
        #[command(flatten)]
        at: At,
    },
    /// Block until every Agent finished (done) or the Judge answered (judged).
    Wait {
        #[command(flatten)]
        at: At,
        #[arg(long, default_value = "done")]
        until: String,
        /// Seconds; exit 6 when exceeded.
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
    },
}

pub fn request(a: &FanoutArgs) -> (&'static str, Value) {
    let at = |at: &At| json!({"fanout": at.fanout, "source": at.source});
    match &a.cmd {
        None => (
            "fanout.start",
            json!({"agents": a.agents, "source": a.source, "base": a.base, "name": a.name, "prompt": a.prompt}),
        ),
        Some(FanoutCmd::List { source }) => ("fanout.list", json!({"source": source})),
        Some(FanoutCmd::Status { at: x }) => ("fanout.status", at(x)),
        Some(FanoutCmd::Judge { at: x, agent }) => {
            let mut p = at(x);
            p["agent"] = json!(agent);
            ("fanout.judge", p)
        }
        Some(FanoutCmd::Choose {
            at: x,
            winner,
            strategy,
            cleanup,
        }) => {
            let mut p = at(x);
            p["winner"] = json!(winner);
            p["strategy"] = json!(strategy);
            p["cleanup"] = json!(cleanup);
            ("fanout.choose", p)
        }
        Some(FanoutCmd::Discard { at: x }) => ("fanout.discard", at(x)),
        Some(FanoutCmd::Wait {
            at: x,
            until,
            timeout,
        }) => {
            let mut p = at(x);
            p["until"] = json!(until);
            p["timeout"] = json!(timeout);
            ("fanout.wait", p)
        }
    }
}

fn s(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        o => o.to_string(),
    }
}

fn print_status(f: &Value) {
    println!(
        "{}\t{}\t{}/{} done\t{}",
        s(&f["fanout"]),
        s(&f["phase"]),
        s(&f["done"]),
        s(&f["total"]),
        s(&f["prompt"]).lines().next().unwrap_or("")
    );
    for m in f["members"].as_array().into_iter().flatten() {
        let size = match m["files"].as_u64() {
            Some(n) => format!("{n} files +{} -{}", s(&m["added"]), s(&m["removed"])),
            None => "-".into(),
        };
        println!(
            "  {}\t{}\t{}\t{}\t{}",
            s(&m["label"]),
            s(&m["status"]),
            if m["finished"] == true {
                "finished"
            } else {
                "working"
            },
            size,
            s(&m["worktree"])
        );
    }
    if let Some(j) = f["judge"].as_object() {
        println!(
            "  judge {}\t{}\tpick {}",
            s(&j["pane"]),
            s(&j["status"]),
            s(&j["pick"])
        );
    }
    if let Some(w) = f["winner"].as_object() {
        println!("  winner {}\t{}", s(&w["label"]), s(&w["strategy"]));
    }
}

pub fn print(a: &FanoutArgs, r: &Value) {
    match &a.cmd {
        None => {
            println!("{}", s(&r["fanout"]));
            let rows = r["labels"].as_array().into_iter().flatten();
            for (i, l) in rows.enumerate() {
                println!(
                    "  {}\t{}\t{}",
                    s(l),
                    s(&r["panes"][i]),
                    s(&r["worktrees"][i])
                );
            }
        }
        Some(FanoutCmd::List { .. }) => r["fanouts"]
            .as_array()
            .into_iter()
            .flatten()
            .for_each(print_status),
        Some(FanoutCmd::Status { .. } | FanoutCmd::Wait { .. }) => print_status(r),
        Some(FanoutCmd::Judge { .. }) => println!("{}", s(&r["pane"])),
        Some(FanoutCmd::Choose { .. }) => println!("{}", s(&r["next"])),
        Some(FanoutCmd::Discard { .. }) => {
            for w in r["removed"].as_array().into_iter().flatten() {
                println!("removed {}", s(w));
            }
            for e in r["errors"].as_array().into_iter().flatten() {
                eprintln!("rade: {}", s(e));
            }
        }
    }
}
