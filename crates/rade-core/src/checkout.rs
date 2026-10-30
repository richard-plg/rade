//! Which checkout a Source link uses (PRD R98, ADR-0034 decisions 3–4).
//!
//! [`resolve_checkout`] is pure: it takes the links of a Workspace, the git
//! facts of its Sources ([`World`], gathered by [`observe`] off the UI
//! thread) and the checkouts recorded in `state.json` ([`LinkCheckout`]),
//! and returns one [`Resolved`] per link. The only command it ever plans is
//! `git worktree add` of a new Project checkout folder ([`Command`]);
//! applying it is [`apply`]. It never plans a switch, stash, reset or
//! checkout of an existing checkout, so it can't touch a dirty one.
//!
//! Rules, in order (records first, then `checkout = "main"` / `"<path>"`,
//! then `auto`; each phase by project then source id, so the result never
//! depends on input order):
//! - a recorded checkout that still exists is kept while the link's `base`
//!   and `checkout` keys are those recorded, whatever its HEAD (that is
//!   Base drift, [`crate::drift`]);
//! - `main`: the main checkout when it is on the base and no link holds it
//!   at another base; else refused (never switched);
//! - `<path>`: that existing checkout of the Source when it is on the base;
//! - `auto`: the main checkout when it is on the base and free, else a
//!   Project checkout `<worktrees folder>/.projects/<project>/<source>` on
//!   the base, shared by every link with the same Source and base.

use crate::project::Checkout;
use crate::rpc::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The folder of Project checkouts under the Worktrees folder (R98.1).
pub const PROJECTS: &str = ".projects";

/// A link to resolve: its base is already the branch (the Source's main
/// branch filled in when the file names none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub project: String,
    pub source: String,
    pub base: String,
    pub checkout: Checkout,
}

/// The checkout a link was first resolved to (`state.json`
/// `link_checkouts` of its Workspace, R98.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkCheckout {
    pub project: String,
    pub source: String,
    pub base: String,
    /// The `checkout` key as written (`auto`, `main`, a path).
    pub checkout: String,
    pub path: PathBuf,
}

/// One checkout of a repository (`git worktree list`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Co {
    pub path: PathBuf,
    /// `None` when detached.
    pub branch: Option<String>,
    /// Short commit of HEAD.
    pub commit: String,
    /// Changed or untracked files.
    pub dirty: usize,
}

/// Git facts of one Source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Repo {
    /// The Source folder (its main checkout).
    pub main: PathBuf,
    /// The folder is the top of a git checkout.
    pub git: bool,
    /// Every checkout of the repository, the main one included.
    pub checkouts: Vec<Co>,
    /// Local branches.
    pub local: BTreeSet<String>,
    /// Remote-only candidates: branch → `<remote>/<branch>`.
    pub remote: BTreeMap<String, String>,
    /// The branch of a repository with no commit yet (`git init`, R131).
    pub unborn: Option<String>,
}

impl Repo {
    pub fn checkout(&self, path: &Path, canon: &dyn Fn(&Path) -> PathBuf) -> Option<&Co> {
        let p = canon(path);
        self.checkouts.iter().find(|c| canon(&c.path) == p)
    }
}

/// Everything [`resolve_checkout`] looks at besides the links and records.
pub struct World<'a> {
    /// Source id → its git facts.
    pub repos: BTreeMap<String, Repo>,
    /// The Workspace's Worktrees folder (`<root>/.worktrees`,
    /// `.worktrees-dev`, or `<data dir>/worktrees` for `default`).
    pub folder: PathBuf,
    /// `~` in a `checkout = "<path>"`.
    pub home: PathBuf,
    /// `realpath` (a symlink to a checkout is that checkout).
    pub canon: &'a dyn Fn(&Path) -> PathBuf,
    /// Whether a folder exists.
    pub exists: &'a dyn Fn(&Path) -> bool,
}

/// What a link's checkout is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The Source's main checkout.
    Main,
    /// A Project checkout under `.projects`.
    Project,
    /// Another existing checkout named by `checkout = "<path>"`.
    Path,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Main => "main",
            Kind::Project => "project",
            Kind::Path => "path",
        }
    }
}

/// The only thing a plan may run: a new Project checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `git -C <repo> worktree add <dir> <branch>`, or with `track`
    /// (`origin/<branch>`, the branch only on a remote)
    /// `git worktree add --track -b <branch> <dir> <track>`.
    AddWorktree {
        repo: PathBuf,
        dir: PathBuf,
        branch: String,
        track: Option<String>,
    },
}

/// A link's resolved checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub project: String,
    pub source: String,
    pub base: String,
    /// `None` when refused.
    pub kind: Option<Kind>,
    /// The canonical checkout folder; `None` when refused.
    pub path: Option<PathBuf>,
    /// The Project checkout does not exist yet ([`Resolved::commands`] make it).
    pub create: bool,
    /// Other Projects whose links use this same checkout, `(project, base)`.
    pub shared_with: Vec<(String, String)>,
    /// Every other Project linking the Source, at any base.
    pub also_in: Vec<(String, String)>,
    /// Why there is no checkout (a `USAGE` message); no commands then.
    pub refused: Option<String>,
    pub commands: Vec<Command>,
    /// What to keep in `link_checkouts` (`None` while refused or still to create).
    pub record: Option<LinkCheckout>,
}

impl Resolved {
    /// The refusal as a `USAGE` error.
    pub fn error(&self) -> Option<Error> {
        self.refused.clone().map(Error::usage)
    }

    /// The plan line (`rade project plan`, New Project dialog): `main
    /// checkout · shared with shop`, `new project checkout <path> · main
    /// checkout is on main`; `root` shortens paths under it.
    pub fn text(&self, main_branch: Option<&str>, root: &Path) -> String {
        if let Some(r) = &self.refused {
            return format!("refused: {r}");
        }
        let path = self.path.as_deref().unwrap_or(Path::new(""));
        let rel = path
            .strip_prefix(root)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        let mut t = match (self.kind, self.create) {
            (Some(Kind::Main), _) => "main checkout".to_string(),
            (Some(Kind::Project), true) => format!("new project checkout {rel}"),
            (Some(Kind::Project), false) => format!("project checkout {rel}"),
            _ => format!("checkout {rel}"),
        };
        if self.kind == Some(Kind::Project) {
            if let Some(b) = main_branch.filter(|b| *b != self.base) {
                t += &format!(" · main checkout is on {b}");
            }
        }
        if !self.shared_with.is_empty() {
            let names: Vec<&str> = self.shared_with.iter().map(|(p, _)| p.as_str()).collect();
            t += &format!(" · shared with {}", names.join(", "));
        }
        t
    }
}

impl Resolved {
    /// The dialogs' plan text (R106.1, spec C.18–C.19): the kind (`main
    /// checkout`, `new project checkout`, …) and the reason after it —
    /// `on develop · shared with Shop`, `Shop uses develop, Billing
    /// release/2.4` (other Projects holding the Source at other bases), or
    /// `main checkout is on feat/checkout-v2`. Project names, shown order.
    pub fn plan_line(
        &self,
        main_head: Option<&str>,
        names: &crate::linkview::Names,
    ) -> (String, String) {
        if let Some(r) = &self.refused {
            return ("refused".into(), r.clone());
        }
        let shared = names.named(&self.shared_with);
        let shared_txt = (!shared.is_empty()).then(|| {
            let n: Vec<&str> = shared.iter().map(|(p, _)| p.as_str()).collect();
            format!("shared with {}", n.join(", "))
        });
        let mut parts: Vec<String> = Vec::new();
        let kind = match (self.kind, self.create) {
            (Some(Kind::Main), _) => {
                parts.push(format!("on {}", self.base));
                parts.extend(shared_txt);
                "main checkout"
            }
            (Some(Kind::Path), _) => {
                parts.push(format!("on {}", self.base));
                parts.extend(shared_txt);
                "checkout"
            }
            (_, create) => {
                let elsewhere: Vec<(String, String)> = names
                    .named(&self.also_in)
                    .into_iter()
                    .filter(|(p, b)| *b != self.base && !shared.iter().any(|(s, _)| s == p))
                    .collect();
                if let Some(t) = shared_txt {
                    parts.push(t);
                } else if !elsewhere.is_empty() {
                    let v: Vec<String> = elsewhere
                        .iter()
                        .enumerate()
                        .map(|(i, (p, b))| {
                            if i == 0 {
                                format!("{p} uses {b}")
                            } else {
                                format!("{p} {b}")
                            }
                        })
                        .collect();
                    parts.push(v.join(", "));
                } else if let Some(h) = main_head.filter(|h| *h != self.base) {
                    parts.push(format!("main checkout is on {h}"));
                }
                if create {
                    "new project checkout"
                } else {
                    "project checkout"
                }
            }
        };
        (kind.into(), parts.join(" · "))
    }
}

/// `<folder>/.projects/<project>/<source>` (R98.1).
pub fn project_checkout_dir(folder: &Path, project: &str, source: &str) -> PathBuf {
    folder.join(PROJECTS).join(project).join(source)
}

/// `~/x` → `<home>/x`.
fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_path_buf(),
        None => PathBuf::from(path),
    }
}

struct Taken {
    base: String,
    project: String,
}

/// Resolve every link (R98.1–R98.4); one [`Resolved`] per link, in order.
pub fn resolve_checkout(links: &[Link], world: &World, records: &[LinkCheckout]) -> Vec<Resolved> {
    let canon = world.canon;
    let projects_dir = canon(&world.folder).join(PROJECTS);
    let mut out: Vec<Resolved> = links
        .iter()
        .map(|l| Resolved {
            project: l.project.clone(),
            source: l.source.clone(),
            base: l.base.clone(),
            kind: None,
            path: None,
            create: false,
            shared_with: Vec::new(),
            also_in: Vec::new(),
            refused: None,
            commands: Vec::new(),
            record: None,
        })
        .collect();
    let mut order: Vec<usize> = (0..links.len()).collect();
    order.sort_by(|&a, &b| {
        (&links[a].project, &links[a].source).cmp(&(&links[b].project, &links[b].source))
    });
    let mut taken: BTreeMap<PathBuf, Taken> = BTreeMap::new();
    let mut done = vec![false; links.len()];
    let kind_of = |p: &Path, repo: &Repo| {
        if *p == canon(&repo.main) {
            Kind::Main
        } else if p.starts_with(&projects_dir) {
            Kind::Project
        } else {
            Kind::Path
        }
    };
    // The repository of a link, or why there is none.
    let repo_of = |l: &Link| -> std::result::Result<&Repo, String> {
        match world.repos.get(&l.source) {
            None => Err(format!(
                "source '{}' is not a Source of this Workspace",
                l.source
            )),
            Some(r) if !r.git => Err(format!(
                "{}: not a git repository (run: git init -b {})",
                r.main.display(),
                l.base
            )),
            Some(r) => Ok(r),
        }
    };
    for &i in &order {
        if let Err(e) = repo_of(&links[i]) {
            out[i].refused = Some(e);
            done[i] = true;
        }
    }

    // 1. Records still valid.
    for &i in &order {
        if done[i] {
            continue;
        }
        let l = &links[i];
        let Some(rec) = records.iter().find(|r| {
            r.project == l.project
                && r.source == l.source
                && r.base == l.base
                && r.checkout == l.checkout.as_str()
        }) else {
            continue;
        };
        let repo = repo_of(l).unwrap();
        let p = canon(&rec.path);
        if repo.checkout(&p, canon).is_none() {
            continue;
        }
        if taken.get(&p).is_some_and(|t| t.base != l.base) {
            continue;
        }
        taken.insert(
            p.clone(),
            Taken {
                base: l.base.clone(),
                project: l.project.clone(),
            },
        );
        let r = &mut out[i];
        r.kind = Some(kind_of(&p, repo));
        r.record = Some(LinkCheckout {
            path: p.clone(),
            ..rec.clone()
        });
        r.path = Some(p);
        done[i] = true;
    }

    let holder = |taken: &BTreeMap<PathBuf, Taken>, p: &Path, base: &str| -> Option<String> {
        taken
            .get(p)
            .filter(|t| t.base != base)
            .map(|t| format!("it is used by {} at {}", t.project, t.base))
    };
    let assign = |r: &mut Resolved, l: &Link, p: PathBuf, kind: Kind| {
        r.record = Some(LinkCheckout {
            project: l.project.clone(),
            source: l.source.clone(),
            base: l.base.clone(),
            checkout: l.checkout.as_str().to_string(),
            path: p.clone(),
        });
        r.kind = Some(kind);
        r.path = Some(p);
    };

    // 2. `main` and `<path>`.
    for &i in &order {
        if done[i] || links[i].checkout == Checkout::Auto {
            continue;
        }
        let l = &links[i];
        let repo = repo_of(l).unwrap();
        let (p, what) = match &l.checkout {
            Checkout::Main => (canon(&repo.main), "main checkout".to_string()),
            Checkout::Path(s) => (canon(&expand(s, &world.home)), s.clone()),
            Checkout::Auto => unreachable!(),
        };
        let Some(co) = repo.checkout(&p, canon) else {
            out[i].refused = Some(format!(
                "{what} ({}) is not a checkout of '{}'",
                p.display(),
                l.source
            ));
            done[i] = true;
            continue;
        };
        if let Some(h) = holder(&taken, &p, &l.base) {
            out[i].refused = Some(format!(
                "'{}' {what} {} can't be used at {}: {h}",
                l.source,
                p.display(),
                l.base
            ));
            done[i] = true;
            continue;
        }
        if co.branch.as_deref() != Some(l.base.as_str()) {
            let head = co.branch.clone().unwrap_or_else(|| co.commit.clone());
            let dirty = match co.dirty {
                0 => String::new(),
                1 => ", with 1 uncommitted change".into(),
                n => format!(", with {n} uncommitted changes"),
            };
            out[i].refused = Some(format!(
                "'{}' {what} is on {head}, not {}{dirty}: RADE never switches it; \
                 put it on {} yourself or use checkout = \"auto\"",
                l.source, l.base, l.base
            ));
            done[i] = true;
            continue;
        }
        taken.insert(
            p.clone(),
            Taken {
                base: l.base.clone(),
                project: l.project.clone(),
            },
        );
        let kind = kind_of(&p, repo);
        assign(&mut out[i], l, p, kind);
        done[i] = true;
    }

    // 3. `auto`.
    let mut planned: BTreeMap<(String, String), (PathBuf, Command)> = BTreeMap::new();
    for &i in &order {
        if done[i] {
            continue;
        }
        let l = &links[i];
        let repo = repo_of(l).unwrap();
        let main = canon(&repo.main);
        let main_on_base = repo
            .checkout(&main, canon)
            .is_some_and(|c| c.branch.as_deref() == Some(l.base.as_str()));
        if main_on_base && holder(&taken, &main, &l.base).is_none() {
            taken.insert(
                main.clone(),
                Taken {
                    base: l.base.clone(),
                    project: l.project.clone(),
                },
            );
            assign(&mut out[i], l, main, Kind::Main);
            done[i] = true;
            continue;
        }
        // An existing Project checkout of this Source on the base.
        let existing = repo.checkouts.iter().find(|c| {
            let p = canon(&c.path);
            p.starts_with(&projects_dir)
                && c.branch.as_deref() == Some(l.base.as_str())
                && holder(&taken, &p, &l.base).is_none()
        });
        if let Some(c) = existing {
            let p = canon(&c.path);
            taken.insert(
                p.clone(),
                Taken {
                    base: l.base.clone(),
                    project: l.project.clone(),
                },
            );
            assign(&mut out[i], l, p, Kind::Project);
            done[i] = true;
            continue;
        }
        let key = (l.source.clone(), l.base.clone());
        if let Some((dir, cmd)) = planned.get(&key) {
            let r = &mut out[i];
            r.kind = Some(Kind::Project);
            r.path = Some(dir.clone());
            r.create = true;
            r.commands = vec![cmd.clone()];
            done[i] = true;
            continue;
        }
        if let Some(c) = repo
            .checkouts
            .iter()
            .find(|c| c.branch.as_deref() == Some(l.base.as_str()))
        {
            out[i].refused = Some(format!(
                "{} is checked out in {}, which this link can't use; RADE can't make a second \
                 checkout of it (use checkout = \"<path>\" or switch that checkout)",
                l.base,
                c.path.display()
            ));
            done[i] = true;
            continue;
        }
        let dir = projects_dir.join(&l.project).join(&l.source);
        if (world.exists)(&dir) || repo.checkout(&dir, canon).is_some() {
            out[i].refused = Some(format!(
                "{} already exists and is not {}'s checkout of {}: move it away first",
                dir.display(),
                l.source,
                l.base
            ));
            done[i] = true;
            continue;
        }
        // `git worktree add` needs a commit to start from (R131.3).
        if repo.unborn.is_some() && repo.local.is_empty() && repo.remote.is_empty() {
            out[i].refused = Some(format!(
                "{} has no commits yet: commit once, then try again",
                repo.main.display()
            ));
            done[i] = true;
            continue;
        }
        let track = if repo.local.contains(&l.base) {
            None
        } else if let Some(r) = repo.remote.get(&l.base) {
            Some(r.clone())
        } else {
            out[i].refused = Some(format!(
                "'{}' has no branch {} (local or remote)",
                l.source, l.base
            ));
            done[i] = true;
            continue;
        };
        let cmd = Command::AddWorktree {
            repo: repo.main.clone(),
            dir: dir.clone(),
            branch: l.base.clone(),
            track,
        };
        taken.insert(
            dir.clone(),
            Taken {
                base: l.base.clone(),
                project: l.project.clone(),
            },
        );
        planned.insert(key, (dir.clone(), cmd.clone()));
        let r = &mut out[i];
        r.kind = Some(Kind::Project);
        r.path = Some(dir);
        r.create = true;
        r.commands = vec![cmd];
        done[i] = true;
    }

    // Labels.
    let snapshot: Vec<(String, String, String, Option<PathBuf>)> = out
        .iter()
        .map(|r| {
            (
                r.project.clone(),
                r.source.clone(),
                r.base.clone(),
                r.path.clone(),
            )
        })
        .collect();
    for r in out.iter_mut() {
        let mut shared: Vec<(String, String)> = snapshot
            .iter()
            .filter(|(p, _, _, path)| *p != r.project && path.is_some() && *path == r.path)
            .map(|(p, _, b, _)| (p.clone(), b.clone()))
            .collect();
        shared.sort();
        shared.dedup();
        let mut also: Vec<(String, String)> = snapshot
            .iter()
            .filter(|(p, s, _, _)| *p != r.project && *s == r.source)
            .map(|(p, _, b, _)| (p.clone(), b.clone()))
            .collect();
        also.sort();
        also.dedup();
        r.shared_with = shared;
        r.also_in = also;
    }
    out
}

/// Run a plan's command (R98-g). A folder that already is that checkout of
/// the repository is left as it is.
pub fn apply(cmd: &Command) -> Result<()> {
    let Command::AddWorktree {
        repo,
        dir,
        branch,
        track,
    } = cmd;
    if dir.exists() {
        let same = crate::git::list_worktrees(repo)?
            .iter()
            .any(|w| crate::git::same(&w.path, dir) && w.branch.as_deref() == Some(branch));
        if same {
            return Ok(());
        }
        return Err(Error::usage(format!(
            "{} already exists: move it away first",
            dir.display()
        )));
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::internal(format!("{}: {e}", parent.display())))?;
    }
    let d = dir.to_string_lossy();
    match track {
        Some(t) => crate::git::run(repo, &["worktree", "add", "--track", "-b", branch, &d, t]),
        None => crate::git::run(repo, &["worktree", "add", &d, branch]),
    }
    .map(drop)
}

/// The git facts of each `(source id, folder)` (off the UI thread).
pub fn observe(sources: &[(String, PathBuf)]) -> BTreeMap<String, Repo> {
    sources
        .iter()
        .map(|(id, path)| (id.clone(), observe_one(path)))
        .collect()
}

fn observe_one(path: &Path) -> Repo {
    let mut repo = Repo {
        main: path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        ..Repo::default()
    };
    if crate::git::require_toplevel(path).is_err() {
        return repo;
    }
    let Ok(list) = crate::git::list_worktrees(path) else {
        return repo;
    };
    repo.git = true;
    if crate::git::run(path, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        repo.unborn = crate::git::run(path, &["symbolic-ref", "--short", "HEAD"])
            .ok()
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty());
    }
    for w in list.into_iter().filter(|w| !w.bare && !w.prunable) {
        let dirty = crate::git::dirty(&w.path).map_or(0, |d| d.len());
        repo.checkouts.push(Co {
            commit: w.head.as_deref().unwrap_or("").chars().take(7).collect(),
            path: w.path,
            branch: w.branch,
            dirty,
        });
    }
    let refs = crate::git::run(
        path,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .unwrap_or_default();
    for r in refs.lines() {
        if let Some(b) = r.strip_prefix("refs/heads/") {
            repo.local.insert(b.to_string());
        } else if let Some(rb) = r.strip_prefix("refs/remotes/") {
            if let Some((remote, b)) = rb.split_once('/') {
                if b == "HEAD" {
                    continue;
                }
                let e = repo.remote.entry(b.to_string()).or_insert(rb.to_string());
                if remote == "origin" {
                    *e = rb.to_string();
                }
            }
        }
    }
    repo
}

/// HEAD, dirty files and `ahead` / `behind` of a link's checkout at `path`
/// against `base` (off the UI thread; a remote-only base counts against
/// its remote branch).
pub fn facts(repo: &Repo, path: &Path, base: &str, canon: &dyn Fn(&Path) -> PathBuf) -> Facts {
    let exists = path.exists();
    let Some(co) = repo.checkout(path, canon).filter(|_| exists) else {
        return Facts {
            exists,
            ..Facts::default()
        };
    };
    // A local base that tracks a remote branch counts against that one
    // (`↓2`: the checkout is behind the shared base, R105-b).
    let base_ref = if repo.local.contains(base) {
        crate::git::run(
            path,
            &[
                "rev-parse",
                "--abbrev-ref",
                "--verify",
                "--quiet",
                &format!("{base}@{{upstream}}"),
            ],
        )
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| base.to_string())
    } else {
        repo.remote
            .get(base)
            .cloned()
            .unwrap_or_else(|| base.into())
    };
    let counts = crate::git::run(
        path,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base_ref}...HEAD"),
        ],
    )
    .ok()
    .and_then(|o| {
        let mut it = o.split_whitespace().map(|n| n.parse::<usize>().ok());
        Some((it.next()??, it.next()??))
    });
    Facts {
        exists,
        head: Some(crate::drift::Head::of(co.branch.as_deref(), &co.commit)),
        dirty: co.dirty,
        behind: counts.map(|c| c.0),
        ahead: counts.map(|c| c.1),
    }
}

/// The ref a Worktree's Review, `+/−` counts and Create PR compare with
/// (R99.2): its recorded base, else its link's base; `None` = the
/// Source's main branch.
pub fn comparison_base(recorded: Option<&str>, link_base: Option<&str>) -> Option<String> {
    recorded.or(link_base).map(String::from)
}

/// The links of every Project of a Workspace (R97.1): a broken file and a
/// Source that is not registered give none; a link without `base` gets
/// `main_branch(source)`.
pub fn links_of(
    projects: &[crate::project::Project],
    registered: &dyn Fn(&str) -> bool,
    main_branch: &dyn Fn(&str) -> String,
) -> Vec<Link> {
    projects
        .iter()
        .filter(|p| p.error.is_none())
        .flat_map(|p| {
            p.sources
                .iter()
                .filter(|s| registered(&s.source))
                .map(|s| Link {
                    project: p.id.clone(),
                    source: s.source.clone(),
                    base: s.base.clone().unwrap_or_else(|| main_branch(&s.source)),
                    checkout: s.checkout.clone(),
                })
        })
        .collect()
}

/// `records` with what this resolve chose (R98.1): each link's new record
/// replaces its old one (one per project + source); a refused or still
/// planned link records nothing. `None` when nothing changed.
pub fn merge_records(records: &[LinkCheckout], resolved: &[Resolved]) -> Option<Vec<LinkCheckout>> {
    let mut out = records.to_vec();
    let mut changed = false;
    for rec in resolved.iter().filter_map(|r| r.record.as_ref()) {
        if out.contains(rec) {
            continue;
        }
        out.retain(|o| !(o.project == rec.project && o.source == rec.source));
        out.push(rec.clone());
        changed = true;
    }
    changed.then_some(out)
}

/// A git worktree RADE must never import as a Worktree (R98.1, R98-n): a
/// folder under `<folder>/.projects`, or a link's recorded checkout.
pub fn is_project_checkout(
    path: &Path,
    folder: &Path,
    records: &[LinkCheckout],
    canon: &dyn Fn(&Path) -> PathBuf,
) -> bool {
    let p = canon(path);
    p.starts_with(canon(&folder.join(PROJECTS))) || records.iter().any(|r| canon(&r.path) == p)
}

/// What git says about a link's checkout (`project.show`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub exists: bool,
    pub head: Option<crate::drift::Head>,
    pub dirty: usize,
    /// Commits on HEAD not on the base, and the other way.
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
}

/// A link's `project.show` fields (API.md): the resolved checkout, its
/// labels, HEAD, `ahead` / `behind`, `dirty` and `drift` (R100).
pub fn show_json(r: &Resolved, f: &Facts) -> serde_json::Value {
    use serde_json::json;
    let pairs = |v: &[(String, String)]| -> Vec<serde_json::Value> {
        v.iter()
            .map(|(p, b)| json!({"project": p, "base": b}))
            .collect()
    };
    let drift = f
        .head
        .as_ref()
        .filter(|_| f.exists && r.refused.is_none())
        .and_then(|h| crate::drift::drift(&r.base, h, f.dirty));
    let mut v = json!({
        "resolved_base": r.base,
        "checkout_kind": r.kind.map(Kind::as_str),
        "path": r.path,
        "exists": f.exists,
        "shared_with": pairs(&r.shared_with),
        "also_in": pairs(&r.also_in),
        "head": f.head.as_ref().map(|h| h.name().to_string()),
        "ahead": f.ahead,
        "behind": f.behind,
        "dirty": f.dirty,
        "drift": drift,
    });
    if let Some(why) = &r.refused {
        v["refused"] = why.clone().into();
    }
    v
}

/// Another checkout of `repo` (not `path`) that has `base` checked out:
/// Back to base can't switch to it then (R100.2).
pub fn elsewhere(
    repo: &Repo,
    base: &str,
    path: &Path,
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Option<PathBuf> {
    let me = canon(path);
    repo.checkouts
        .iter()
        .find(|c| c.branch.as_deref() == Some(base) && canon(&c.path) != me)
        .map(|c| c.path.clone())
}

#[cfg(test)]
#[path = "checkout_tests.rs"]
mod tests;
