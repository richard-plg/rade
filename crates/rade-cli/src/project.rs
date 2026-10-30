//! `rade project …` and `rade config …` (PRD R28, R29, R31; docs/API.md).

use clap::Subcommand;
use rade_core::layers::show;
use rade_core::project::parse_source_spec;
use rade_core::rpc::Error;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Projects of your Workspace (default), one Workspace, or all.
    List {
        #[arg(long, conflicts_with = "all")]
        workspace: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// A Project's Source links: base and checkout.
    Show { project: String },
    /// Write `<root>/.rade/projects/<id>/project.toml` (kept if it exists).
    Add {
        name: String,
        /// Default: your Workspace, else the shown one.
        #[arg(long)]
        workspace: Option<String>,
        /// `<source>[:<branch>]`, repeatable (`shop-web:develop`); no branch = its main branch.
        #[arg(long = "source")]
        sources: Vec<String>,
    },
    /// The checkout plan of a Project not written yet (R98.3); writes nothing.
    Plan {
        name: String,
        #[arg(long)]
        workspace: Option<String>,
        /// `<source>[:<branch>]`, repeatable.
        #[arg(long = "source", required = true)]
        sources: Vec<String>,
    },
    /// Add, remove or rebase one Source link, or put its checkout back on its base.
    #[command(subcommand)]
    Link(LinkCmd),
    /// Delete a Project: its folder and project.toml, its unused clean Project
    /// checkouts; its Worktrees go to Scratch (R133.1).
    Remove {
        project: String,
        /// Only print what would happen.
        #[arg(long)]
        dry_run: bool,
        /// Leave its Project checkouts on disk.
        #[arg(long)]
        keep_checkouts: bool,
    },
    /// Remove the clean Project checkouts no link uses any more (R133.2).
    Prune {
        #[arg(long)]
        workspace: Option<String>,
        /// Only list them.
        #[arg(long)]
        dry_run: bool,
    },
    /// Set one key of project.toml, keeping the rest of the file (R97.3):
    /// name, source.<id>.base, source.<id>.checkout; no value removes it.
    Set {
        project: String,
        key: String,
        value: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum LinkCmd {
    /// Link a Source (one `[[source]]` appended; every other byte kept).
    Add {
        project: String,
        source: String,
        /// Default: the Source's main branch.
        #[arg(long)]
        base: Option<String>,
        /// auto (default), main, or the path of an existing checkout.
        #[arg(long)]
        checkout: Option<String>,
    },
    /// Unlink a Source (its checkout and Worktrees are kept).
    Remove { project: String, source: String },
    /// Change a link's base (no checkout is switched; Worktrees keep theirs).
    SetBase {
        project: String,
        source: String,
        branch: String,
    },
    /// Back to base: `git switch <base>` in the link's drifted, clean checkout.
    ToBase { project: String, source: String },
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Which layer sets a key: built-in, user, workspace, project, source.
    Explain {
        /// A key (`scm.draft_command`) or prefix (`agents`); none: every key set in a file.
        key: Option<String>,
        #[arg(long, conflicts_with_all = ["source", "project", "workspace"])]
        pane: Option<String>,
        #[arg(long, conflicts_with_all = ["project", "workspace"])]
        source: Option<String>,
        #[arg(long, requires = "source")]
        worktree: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Trust a Source's `.rade.toml` as it is now (a change untrusts it).
    Trust { source: String },
}

/// What looks like a path (the M15 `rade project add <path>`, R31-a).
fn looks_like_path(arg: &str) -> bool {
    arg.starts_with(['/', '.', '~']) || Path::new(arg).is_dir()
}

pub fn request(c: &ProjectCmd) -> Result<(&'static str, Value), Error> {
    Ok(match c {
        ProjectCmd::List { workspace, all } => {
            ("project.list", json!({"workspace": workspace, "all": all}))
        }
        ProjectCmd::Show { project } => ("project.show", json!({"project": project})),
        ProjectCmd::Remove {
            project,
            dry_run,
            keep_checkouts,
        } => (
            "project.remove",
            json!({"project": project, "dry_run": dry_run, "keep_checkouts": keep_checkouts}),
        ),
        ProjectCmd::Prune { workspace, dry_run } => (
            "project.prune",
            json!({"workspace": workspace, "dry_run": dry_run}),
        ),
        ProjectCmd::Plan {
            name,
            workspace,
            sources,
        } => {
            let mut list = Vec::new();
            for spec in sources {
                let (src, base) = parse_source_spec(spec)?;
                list.push(json!({"source": src, "base": base}));
            }
            (
                "project.plan",
                json!({"name": name, "workspace": workspace, "sources": list}),
            )
        }
        ProjectCmd::Link(l) => match l {
            LinkCmd::Add {
                project,
                source,
                base,
                checkout,
            } => (
                "project.link_add",
                json!({"project": project, "source": source, "base": base, "checkout": checkout}),
            ),
            LinkCmd::Remove { project, source } => (
                "project.link_remove",
                json!({"project": project, "source": source}),
            ),
            LinkCmd::SetBase {
                project,
                source,
                branch,
            } => (
                "project.link_set_base",
                json!({"project": project, "source": source, "base": branch}),
            ),
            LinkCmd::ToBase { project, source } => (
                "project.link_to_base",
                json!({"project": project, "source": source}),
            ),
        },
        ProjectCmd::Set {
            project,
            key,
            value,
        } => (
            "project.set",
            json!({"project": project, "key": key, "value": value.clone().unwrap_or_default()}),
        ),
        ProjectCmd::Add {
            name,
            workspace,
            sources,
        } => {
            if looks_like_path(name) {
                return Err(Error::usage(
                    "'rade project add <path>' is now 'rade source add <path>'",
                ));
            }
            let mut list = Vec::new();
            for spec in sources {
                let (src, base) = parse_source_spec(spec)?;
                list.push(json!({"source": src, "base": base}));
            }
            (
                "project.add",
                json!({"name": name, "workspace": workspace, "sources": list}),
            )
        }
    })
}

pub fn config_request(c: &ConfigCmd) -> (&'static str, Value) {
    match c {
        ConfigCmd::Explain {
            key,
            pane,
            source,
            worktree,
            project,
            workspace,
        } => (
            "config.explain",
            json!({"key": key, "pane": pane, "source": source, "worktree": worktree,
                   "project": project, "workspace": workspace}),
        ),
        ConfigCmd::Trust { source } => ("config.trust", json!({"source": source})),
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("-")
}

pub fn print(c: &ProjectCmd, r: &Value) {
    match c {
        ProjectCmd::Set { .. } => println!("{}", s(&r["file"])),
        ProjectCmd::List { .. } => {
            for p in r["projects"].as_array().into_iter().flatten() {
                let links: Vec<String> = p["links"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|x| match x["base"].as_str() {
                        Some(b) => format!("{}@{b}", s(&x["source"])),
                        None => s(&x["source"]).to_string(),
                    })
                    .collect();
                println!(
                    "{}\t{}\t{}\t{}",
                    s(&p["project"]),
                    s(&p["name"]),
                    s(&p["workspace"]),
                    links.join(",")
                );
                if let Some(e) = p["error"].as_str() {
                    eprintln!("rade: error: {e}");
                }
            }
        }
        ProjectCmd::Show { .. } => {
            println!(
                "{} ({})  {}",
                s(&r["name"]),
                s(&r["project"]),
                s(&r["file"])
            );
            if let Some(e) = r["error"].as_str() {
                println!("error: {e}");
            }
            for l in r["links"].as_array().into_iter().flatten() {
                let id = s(&l["source"]);
                if l["registered"] != true {
                    println!(
                        "  {id}\tnot added: rade source add <path> --workspace {}",
                        s(&r["workspace"])
                    );
                    continue;
                }
                println!("  {}", link_line(l, &root_of(r)));
            }
            for w in r["warnings"].as_array().into_iter().flatten() {
                println!("warning: {}", s(w));
            }
        }
        ProjectCmd::Add { .. } => println!("{}", s(&r["file"])),
        ProjectCmd::Remove { .. } | ProjectCmd::Prune { .. } => {
            let dry = r["dry_run"] == true;
            if r["file"].is_string() {
                println!(
                    "{} {}",
                    if dry { "would delete" } else { "deleted" },
                    s(&r["file"])
                );
            }
            let list = |k: &str| r[k].as_array().cloned().unwrap_or_default();
            for o in list(if dry { "remove" } else { "removed" }) {
                println!(
                    "{} {}",
                    if dry { "would remove" } else { "removed" },
                    s(&o["path"])
                );
            }
            for o in list(if dry { "keep" } else { "kept" }) {
                let why = o["why"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| format!("{} uncommitted changes", o["dirty"]));
                println!("kept {} ({why})", s(&o["path"]));
            }
            for p in list("panes") {
                println!(
                    "{} Pane {}",
                    if dry { "would close" } else { "closed" },
                    s(&p["name"])
                );
            }
            for w in list("worktrees") {
                println!("{}/{} → Scratch", s(&w["source"]), s(&w["worktree"]));
            }
        }
        ProjectCmd::Plan { .. } => {
            for l in r["links"].as_array().into_iter().flatten() {
                println!("{}  {}", s(&l["source"]), s(&l["text"]));
            }
        }
        ProjectCmd::Link(LinkCmd::ToBase { .. }) => println!(
            "{} {}",
            s(&r["head"]),
            if r["switched"] == true {
                "(switched)"
            } else {
                "(not drifted: nothing to do)"
            }
        ),
        ProjectCmd::Link(_) => {
            println!("{}", s(&r["file"]));
            for o in r["unused"].as_array().into_iter().flatten() {
                println!(
                    "1 Project checkout no longer used: {} (rade project prune)",
                    s(&o["path"])
                );
            }
        }
    }
}

/// `rade project plan` exits 2 when a link is refused (API.md).
pub fn refused(c: &ProjectCmd, r: &Value) -> bool {
    matches!(c, ProjectCmd::Plan { .. })
        && r["links"]
            .as_array()
            .is_some_and(|l| l.iter().any(|x| x["refused"].is_string()))
}

/// The Workspace root of a `project.show` result: `<root>/.rade/projects/<id>/project.toml`.
fn root_of(r: &Value) -> String {
    let f = Path::new(r["file"].as_str().unwrap_or(""));
    f.ancestors()
        .nth(4)
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// One link of `rade project show` (API.md): source, base, checkout kind +
/// folder (under `root` shortened), HEAD, `=` / `↑a ↓b`, clean / n changed,
/// then `also in`, `shared`, and a drift last.
pub fn link_line(l: &Value, root: &str) -> String {
    let id = s(&l["source"]);
    let base = l["resolved_base"]
        .as_str()
        .or(l["base"].as_str())
        .unwrap_or("(main branch)");
    if let Some(why) = l["refused"].as_str() {
        return format!("{id}  {base}  refused: {why}");
    }
    let Some(kind) = l["checkout_kind"].as_str() else {
        return format!("{id}  {base}  {}", s(&l["checkout"]));
    };
    let path = s(&l["path"]);
    let rel = path
        .strip_prefix(root)
        .and_then(|p| p.strip_prefix('/'))
        .unwrap_or(path);
    let mut out = format!("{id}  {base}  {kind} {rel}");
    if l["exists"] == false {
        out += "  not created yet";
    } else {
        let counts = match (l["ahead"].as_u64(), l["behind"].as_u64()) {
            (Some(0), Some(0)) => "=".to_string(),
            (Some(a), Some(b)) => format!("↑{a} ↓{b}"),
            _ => "?".into(),
        };
        let dirty = match l["dirty"].as_u64().unwrap_or(0) {
            0 => "clean".to_string(),
            n => format!("{n} changed"),
        };
        out += &format!("  HEAD {}  {counts}  {dirty}", s(&l["head"]));
    }
    let pairs = |k: &str| -> Vec<String> {
        l[k].as_array()
            .into_iter()
            .flatten()
            .map(|x| format!("{}@{}", s(&x["project"]), s(&x["base"])))
            .collect()
    };
    let also = pairs("also_in");
    if !also.is_empty() {
        out += &format!("  also in: {}", also.join(", "));
    }
    let shared: Vec<String> = l["shared_with"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|x| s(&x["project"]).to_string())
        .collect();
    if !shared.is_empty() {
        out += &format!("  shared: {}", shared.join(", "));
    }
    if let Some(d) = l["drift"].as_object() {
        out += &format!(
            "  drift {} (base {})",
            d.get("head").and_then(Value::as_str).unwrap_or("-"),
            d.get("base").and_then(Value::as_str).unwrap_or("-")
        );
    }
    out
}

fn home_short(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && path.starts_with(&format!("{h}/")) => {
            format!("~{}", &path[h.len()..])
        }
        _ => path.to_string(),
    }
}

/// One key's lines (R29.3).
fn explain_lines(v: &Value) -> Vec<String> {
    let mut out = vec![format!("{} = {}", s(&v["key"]), show(&v["value"]))];
    let layers = v["layers"].as_array().cloned().unwrap_or_default();
    let width = layers
        .iter()
        .map(|l| {
            l["file"]
                .as_str()
                .map_or(0, |f| home_short(f).chars().count())
        })
        .max()
        .unwrap_or(0);
    for l in layers.iter().rev() {
        let mark = if v["from"] == l["layer"] { "✓" } else { " " };
        let file = l["file"].as_str().map(home_short).unwrap_or_default();
        let what = if l["error"].is_string() {
            "(error)".to_string()
        } else if l["ignored"].is_string() {
            format!("{}  ignored ({})", show(&l["value"]), s(&l["ignored"]))
        } else if l["untrusted"] == true && !l["value"].is_null() {
            format!("{}  untrusted", show(&l["value"]))
        } else {
            show(&l["value"])
        };
        out.push(format!(
            "  {mark} {:<10} {:<width$}   {what}",
            s(&l["layer"]),
            file
        ));
    }
    out
}

pub fn print_config(c: &ConfigCmd, r: &Value) {
    match c {
        ConfigCmd::Explain { .. } => {
            if r.get("values").is_some() {
                for v in r["values"].as_array().into_iter().flatten() {
                    explain_lines(v).iter().for_each(|l| println!("{l}"));
                }
                for i in r["ignored"].as_array().into_iter().flatten() {
                    println!(
                        "{}  ignored ({})  {} {}",
                        s(&i["key"]),
                        i["why"].as_str().unwrap_or("app-wide"),
                        s(&i["layer"]),
                        i["file"].as_str().map(home_short).unwrap_or_default()
                    );
                }
                for e in r["errors"].as_array().into_iter().flatten() {
                    println!("error: {}", s(e));
                }
            } else {
                explain_lines(r).iter().for_each(|l| println!("{l}"));
            }
            println!("{}", s(&r["context_line"]));
        }
        ConfigCmd::Trust { .. } => println!("trusted {}", s(&r["file"])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r31_a_path_is_a_source_hint_and_specs_are_one_base_per_link() {
        for p in ["./x", "/tmp", "~/x"] {
            let e = request(&ProjectCmd::Add {
                name: p.into(),
                workspace: None,
                sources: vec![],
            })
            .unwrap_err();
            assert_eq!(
                e.message,
                "'rade project add <path>' is now 'rade source add <path>'"
            );
        }
        let (m, p) = request(&ProjectCmd::Add {
            name: "BEACON".into(),
            workspace: None,
            sources: vec!["api:develop".into(), "app".into()],
        })
        .unwrap();
        assert_eq!(m, "project.add");
        assert_eq!(p["sources"][0], json!({"source": "api", "base": "develop"}));
        assert_eq!(p["sources"][1], json!({"source": "app", "base": null}));
        let e = request(&ProjectCmd::Add {
            name: "Shop".into(),
            workspace: None,
            sources: vec!["shop-web:dev=develop".into()],
        })
        .unwrap_err();
        assert!(e.message.contains("--source <src>[:<branch>]"), "{e}");
    }

    #[test]
    fn r100_3_show_line_per_link() {
        let l = json!({"source": "shared-ui", "base": "develop", "resolved_base": "develop",
            "checkout_kind": "project", "path": "/w/acme/.worktrees/.projects/shop/shared-ui",
            "exists": true, "head": "develop", "ahead": 0, "behind": 0, "dirty": 0,
            "also_in": [{"project": "billing", "base": "release/2.4"}], "shared_with": [],
            "drift": null});
        assert_eq!(
            link_line(&l, "/w/acme"),
            "shared-ui  develop  project .worktrees/.projects/shop/shared-ui  HEAD develop  =  clean  also in: billing@release/2.4"
        );
        let d = json!({"source": "shop-api", "resolved_base": "develop", "checkout_kind": "main",
            "path": "/w/acme/shop-api", "exists": true, "head": "feat/checkout-v2", "ahead": 1,
            "behind": 0, "dirty": 3, "also_in": [], "shared_with": [{"project": "x", "base": "develop"}],
            "drift": {"head": "feat/checkout-v2", "base": "develop", "dirty": 3}});
        assert_eq!(
            link_line(&d, "/w/acme"),
            "shop-api  develop  main shop-api  HEAD feat/checkout-v2  ↑1 ↓0  3 changed  shared: x  drift feat/checkout-v2 (base develop)"
        );
        let r = json!({"file": "/w/acme/.rade/projects/shop/project.toml"});
        assert_eq!(root_of(&r), "/w/acme");
        let plan = json!({"links": [{"source": "a", "text": "main checkout"},
            {"source": "b", "text": "refused: x", "refused": "x"}]});
        let c = ProjectCmd::Plan {
            name: "S".into(),
            workspace: None,
            sources: vec![],
        };
        assert!(refused(&c, &plan));
    }

    #[test]
    fn r29_3_explain_lines() {
        let v = json!({"key": "scm.draft_command", "value": "p", "from": "project", "layers": [
            {"layer": "built-in", "file": null, "value": "claude"},
            {"layer": "user", "file": "/c/config.toml", "value": null},
            {"layer": "project", "file": "/w/p.toml", "value": "p"},
        ]});
        assert_eq!(
            explain_lines(&v),
            [
                "scm.draft_command = \"p\"",
                "  ✓ project    /w/p.toml        \"p\"",
                "    user       /c/config.toml   (not set)",
                "    built-in                    \"claude\"",
            ]
        );
    }
}
