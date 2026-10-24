//! Projects (PRD R97, ADR-0034): a set of Source links of one Workspace,
//! defined by `<root>/.rade/projects/<id>/project.toml` (v2). A link =
//! Project × Source × exactly one base, plus which checkout it uses
//! ([`Checkout`], resolved by [`crate::checkout`]). A v1 file (Named Bases)
//! is migrated once on its first read ([`migrate_file`]). Read from disk
//! when needed; never stored in `state.json`.

use crate::config::{APP_WIDE, LAYERED};
use crate::rpc::{Error, Result};
use std::path::{Path, PathBuf};

/// Under a Workspace root.
pub const DIR: &str = ".rade/projects";
pub const FILE: &str = "project.toml";
/// The original bytes of a migrated v1 file, next to it (R97.2).
pub const V1_BACKUP: &str = "project.toml.v1.bak";

/// Which checkout a link uses (R98.1): `checkout = "auto" | "main" | "<path>"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checkout {
    /// The main checkout when it is on the base and free, else a Project checkout.
    Auto,
    /// Always the Source's main checkout.
    Main,
    /// That existing checkout of the Source (`~` allowed).
    Path(String),
}

impl Checkout {
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        match s.trim() {
            "" => Err("`checkout` must be auto, main or a path".into()),
            "auto" => Ok(Self::Auto),
            "main" => Ok(Self::Main),
            p => Ok(Self::Path(p.into())),
        }
    }

    /// As written in `project.toml`.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Main => "main",
            Self::Path(p) => p,
        }
    }
}

/// One `[[source]]`: a Source link (R97.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSource {
    pub source: String,
    /// `None` = the Source's main branch.
    pub base: Option<String>,
    pub checkout: Checkout,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// Its folder's name.
    pub id: String,
    pub file: PathBuf,
    /// `name`, else the id.
    pub name: String,
    pub sources: Vec<ProjectSource>,
    /// The whole file, for the config layers (R29).
    pub table: toml::Table,
    pub warnings: Vec<String>,
    /// The file doesn't parse (`<file>: <message>`); everything else empty.
    pub error: Option<String>,
}

impl Project {
    /// Parse a `project.toml`; never fails: a broken file sets `error`. A
    /// v1 text is read as its migration would write it (with its warnings).
    pub fn parse(id: &str, file: &Path, text: &str) -> Self {
        let mut p = Project {
            id: id.into(),
            file: file.into(),
            name: id.into(),
            sources: Vec::new(),
            table: toml::Table::new(),
            warnings: Vec::new(),
            error: None,
        };
        let migrated = match migrate_text(text) {
            Ok(m) => m,
            Err(e) => {
                p.error = Some(format!("{}: {e}", file.display()));
                return p;
            }
        };
        let text = match &migrated {
            Some((t, missing)) => {
                p.warnings
                    .extend(missing.iter().map(|(s, b)| no_branch_warning(s, b)));
                t.as_str()
            }
            None => text,
        };
        if let Err(e) = p.fill(text) {
            p.error = Some(format!("{}: {e}", file.display()));
            p.sources.clear();
            p.table.clear();
            p.warnings.clear();
        }
        p
    }

    fn fill(&mut self, text: &str) -> std::result::Result<(), String> {
        let table: toml::Table = toml::from_str(text).map_err(|e| toml_error(text, &e))?;
        for (k, v) in &table {
            match k.as_str() {
                "name" => {
                    self.name = v
                        .as_str()
                        .ok_or("`name` must be a string")?
                        .trim()
                        .to_string();
                    if self.name.is_empty() {
                        self.name = self.id.clone();
                    }
                }
                "source" => {
                    let a = v.as_array().ok_or("`source` must be [[source]] tables")?;
                    for s in a {
                        self.sources.push(parse_source(s)?);
                    }
                }
                k if LAYERED.contains(&k) => {}
                k if APP_WIDE.contains(&k) => {
                    self.warnings.push(format!("{k}: ignored (app-wide)"))
                }
                k => self.warnings.push(format!("unknown key '{k}'")),
            }
        }
        let mut seen = Vec::new();
        for s in &self.sources {
            if seen.contains(&&s.source) {
                return Err(format!("source '{}' is listed twice", s.source));
            }
            seen.push(&s.source);
        }
        self.table = table;
        Ok(())
    }

    /// Read `file`, migrating a v1 file first (R97.2). A migration that
    /// can't be written leaves the file as it was and reads the v1 text in
    /// memory, with the failure as a warning.
    pub fn load(id: &str, file: &Path) -> Self {
        let text = match read_file(file) {
            Ok(t) => t,
            Err(e) => {
                let mut p = Self::parse(id, file, "");
                p.error = Some(format!("{}: {e}", file.display()));
                return p;
            }
        };
        let mut failed = None;
        let text = match migrate_file(file, &text) {
            Ok(Some(new)) => new,
            Ok(None) => text,
            Err(e) => {
                failed = Some(e.message);
                text
            }
        };
        let mut p = Self::parse(id, file, &text);
        if let Some(e) = failed {
            p.warnings.push(e);
        } else if p.error.is_none() {
            p.warnings.extend(backup_warnings(file, &p));
        }
        p
    }

    pub fn source(&self, id: &str) -> Option<&ProjectSource> {
        self.sources.iter().find(|s| s.source == id)
    }

    pub fn lists(&self, source: &str) -> bool {
        self.source(source).is_some()
    }

    /// `project.list` entry (API.md); `registered` tells whether a source
    /// id is a Source of the Workspace.
    pub fn to_json(&self, workspace: &str, registered: &dyn Fn(&str) -> bool) -> serde_json::Value {
        let mut v = serde_json::json!({
            "project": self.id,
            "name": self.name,
            "workspace": workspace,
            "file": self.file,
            "links": self.sources.iter().map(|s| serde_json::json!({
                "source": s.source,
                "registered": registered(&s.source),
                "base": s.base,
                "checkout": s.checkout.as_str(),
            })).collect::<Vec<_>>(),
            "warnings": self.warnings,
        });
        if let Some(e) = &self.error {
            v["error"] = e.clone().into();
        }
        v
    }

    /// Where its file lives under a Workspace `root`.
    pub fn path(root: &Path, id: &str) -> PathBuf {
        root.join(DIR).join(id).join(FILE)
    }
}

fn parse_source(v: &toml::Value) -> std::result::Result<ProjectSource, String> {
    let t = v.as_table().ok_or("`source` must be [[source]] tables")?;
    let source = t
        .get("id")
        .and_then(|i| i.as_str())
        .ok_or("every [[source]] needs `id = \"<source id>\"`")?
        .to_string();
    let base = match t.get("base") {
        None => None,
        Some(toml::Value::String(b)) if !b.trim().is_empty() => Some(b.trim().to_string()),
        Some(_) => return Err(format!("source '{source}': `base` must be a branch name")),
    };
    let checkout = match t.get("checkout") {
        None => Checkout::Auto,
        Some(toml::Value::String(c)) => {
            Checkout::parse(c).map_err(|e| format!("source '{source}': {e}"))?
        }
        Some(_) => {
            return Err(format!(
                "source '{source}': `checkout` must be auto, main or a path"
            ))
        }
    };
    if let Some(k) = t
        .keys()
        .find(|k| !matches!(k.as_str(), "id" | "base" | "checkout"))
    {
        return Err(format!("source '{source}': unknown key '{k}'"));
    }
    Ok(ProjectSource {
        source,
        base,
        checkout,
    })
}

fn no_branch_warning(source: &str, named: &str) -> String {
    format!(
        "source '{source}' has no branch for Named Base '{named}': it uses its main branch \
         (set one with `rade project link set-base`)"
    )
}

/// (source, first Named Base) of every Source of a v1 text that had no
/// branch for the first Named Base.
type Missing = Vec<(String, String)>;

/// A v1 text (top-level `bases`, or a table-valued `base`) as v2 (R97.2):
/// each Source's `base` = its branch for the first Named Base (with no
/// `bases`, the first key of its table), `bases` dropped, every other
/// byte kept (`toml_edit`). `None` for a v2 text.
pub fn migrate_text(text: &str) -> std::result::Result<Option<(String, Missing)>, String> {
    use toml_edit::{value, DocumentMut, Item};
    let mut doc: DocumentMut = match text.parse() {
        Ok(d) => d,
        // Not TOML: the parser reports it with its line.
        Err(_) => return Ok(None),
    };
    let named: Vec<String> = doc
        .get("bases")
        .and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut v1 = doc.contains_key("bases");
    let mut missing = Vec::new();
    if let Some(tables) = doc.get_mut("source").and_then(Item::as_array_of_tables_mut) {
        for t in tables.iter_mut() {
            let id = t.get("id").and_then(Item::as_str).unwrap_or("").to_string();
            let branches: Option<Vec<(String, String)>> = match t.get("base") {
                Some(i) if i.is_inline_table() || i.is_table() => {
                    let pairs = match (i.as_inline_table(), i.as_table()) {
                        (Some(m), _) => m
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.as_str().unwrap_or("").to_string()))
                            .collect(),
                        (_, Some(m)) => m
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.as_str().unwrap_or("").to_string()))
                            .collect(),
                        _ => Vec::new(),
                    };
                    Some(pairs)
                }
                _ => None,
            };
            let Some(branches) = branches else { continue };
            v1 = true;
            let first = named
                .first()
                .cloned()
                .or_else(|| branches.first().map(|(k, _)| k.clone()));
            let branch = first.as_ref().and_then(|f| {
                branches
                    .iter()
                    .find(|(k, b)| k == f && !b.trim().is_empty())
                    .map(|(_, b)| b.clone())
            });
            match branch {
                Some(b) => {
                    let decor = t
                        .get("base")
                        .and_then(Item::as_value)
                        .map(|v| v.decor().clone());
                    let mut new = value(b);
                    if let (Some(d), Some(v)) = (decor, new.as_value_mut()) {
                        *v.decor_mut() = d;
                    }
                    t["base"] = new;
                }
                None => {
                    t.remove("base");
                    missing.push((id, first.unwrap_or_default()));
                }
            }
        }
    }
    if !v1 {
        return Ok(None);
    }
    if doc.contains_key("bases") {
        // Comment lines above `bases` move to the next key, so no comment goes.
        let prefix = doc
            .as_table()
            .key("bases")
            .and_then(|k| k.leaf_decor().prefix())
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();
        let next = {
            let keys: Vec<String> = doc.as_table().iter().map(|(k, _)| k.to_string()).collect();
            let at = keys.iter().position(|k| k == "bases").unwrap_or(0);
            keys.get(at + 1)
                .filter(|k| doc.get(k).is_some_and(Item::is_value))
                .cloned()
        };
        doc.remove("bases");
        if prefix.contains('#') {
            match next.and_then(|k| doc.as_table_mut().key_mut(&k)) {
                Some(mut k) => {
                    let old = k
                        .leaf_decor()
                        .prefix()
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    k.leaf_decor_mut().set_prefix(format!("{prefix}{old}"));
                }
                None => {
                    let old = doc.trailing().as_str().unwrap_or("").to_string();
                    doc.set_trailing(format!("{old}{prefix}"));
                }
            }
        }
    }
    Ok(Some((doc.to_string(), missing)))
}

/// Migrate the v1 file `file` whose content is `text` (R97.2): writes
/// [`V1_BACKUP`] first (never overwritten), then the v2 text atomically
/// (temp + rename). `None` = already v2, nothing written. On error nothing
/// is left behind (no backup made by this run, no temp file) and the error
/// names the file.
pub fn migrate_file(file: &Path, text: &str) -> Result<Option<String>> {
    let Some((new, _)) = migrate_text(text).map_err(Error::usage)? else {
        return Ok(None);
    };
    let fail = |e: std::io::Error| {
        Error::usage(format!(
            "{}: cannot migrate to project.toml v2: {e}",
            file.display()
        ))
    };
    let dir = file.parent().unwrap_or(Path::new("."));
    let bak = dir.join(V1_BACKUP);
    let made_bak = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&bak)
    {
        Ok(mut f) => {
            use std::io::Write;
            if let Err(e) = f.write_all(text.as_bytes()).and_then(|_| f.sync_all()) {
                let _ = std::fs::remove_file(&bak);
                return Err(fail(e));
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(fail(e)),
    };
    let tmp = dir.join(format!(".{FILE}.{}.tmp", std::process::id()));
    let res = std::fs::write(&tmp, &new).and_then(|_| std::fs::rename(&tmp, file));
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        if made_bak {
            let _ = std::fs::remove_file(&bak);
        }
        return Err(fail(e));
    }
    Ok(Some(new))
}

/// The migration warnings that outlive the migration: a Source the
/// [`V1_BACKUP`] had no branch for under the first Named Base, still with
/// no `base` (R97-d).
fn backup_warnings(file: &Path, p: &Project) -> Vec<String> {
    let bak = file.with_file_name(V1_BACKUP);
    let Ok(old) = read_file(&bak) else {
        return Vec::new();
    };
    let Ok(Some((_, missing))) = migrate_text(&old) else {
        return Vec::new();
    };
    missing
        .into_iter()
        .filter(|(s, _)| p.source(s).is_some_and(|x| x.base.is_none()))
        .map(|(s, b)| no_branch_warning(&s, &b))
        .collect()
}

/// The most a config file may hold (`.rade.toml`, `project.toml`).
pub const MAX_FILE: u64 = 1 << 20;

/// Read a config file. A `.rade.toml` comes from a repository, so it may be
/// a symlink to `/dev/zero` or a FIFO: only regular files up to
/// [`MAX_FILE`] are read, so reading it can't hang or exhaust the app.
pub fn read_file(path: &Path) -> std::io::Result<String> {
    use std::io::{Error, ErrorKind, Read};
    let m = std::fs::metadata(path)?;
    if !m.is_file() {
        return Err(Error::new(ErrorKind::InvalidInput, "not a regular file"));
    }
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MAX_FILE + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE {
        return Err(Error::new(ErrorKind::InvalidData, "too big (over 1 MiB)"));
    }
    Ok(text)
}

/// A TOML parse error on one line: `line N: <message>`.
pub fn toml_error(text: &str, e: &toml::de::Error) -> String {
    match e.span() {
        Some(r) => format!(
            "line {}: {}",
            text[..r.start.min(text.len())].matches('\n').count() + 1,
            e.message()
        ),
        None => e.message().to_string(),
    }
}

/// Whether `text` is TOML (`line N: <message>` if not).
pub fn check_toml(text: &str) -> std::result::Result<(), String> {
    toml::from_str::<toml::Table>(text)
        .map(drop)
        .map_err(|e| toml_error(text, &e))
}

/// Every Project of the Workspace at `root`, by id.
pub fn list(root: &Path) -> Vec<Project> {
    files(root)
        .into_iter()
        .map(|(id, f)| Project::load(&id, &f))
        .collect()
}

fn files(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(root.join(DIR)) else {
        return Vec::new();
    };
    let mut v: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let f = e.path().join(FILE);
            f.is_file()
                .then(|| (e.file_name().to_string_lossy().into_owned(), f))
        })
        .collect();
    v.sort();
    v
}

/// `(file, mtime ns, size)` per `project.toml`.
pub type Signature = Vec<(PathBuf, i128, u64)>;

/// `(file, mtime ns, size)` of every `project.toml` under `root`: the
/// sidebar re-reads them when this changes (R30-d).
/// ponytail: polled every 2 s; a gio monitor if Workspaces grow large.
pub fn signature(root: &Path) -> Signature {
    files(root)
        .into_iter()
        .map(|(_, f)| {
            let m = std::fs::metadata(&f).ok();
            let ns = m
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i128);
            (f, ns, m.map_or(0, |m| m.len()))
        })
        .collect()
}

/// The ref a new Worktree of `source` in Project `p` starts from (R99.1):
/// `base` as given, else the link's base (`None` = the Source's main branch).
pub fn resolve_base(p: &Project, source: &str, base: Option<&str>) -> Result<Option<String>> {
    if let Some(e) = &p.error {
        return Err(Error::usage(e.clone()));
    }
    let s = p.source(source).ok_or_else(|| {
        Error::usage(format!(
            "source '{source}' is not in project '{}' (rade project link add {} {source})",
            p.id, p.id
        ))
    })?;
    Ok(base.map(String::from).or_else(|| s.base.clone()))
}

/// The Project a new Worktree of `source` belongs to when none is named
/// (R99.1): the only Project linking it; several → `USAGE` naming them
/// (by id); none → `None`.
pub fn infer(projects: &[Project], source: &str) -> Result<Option<String>> {
    let mut ids: Vec<&str> = projects
        .iter()
        .filter(|p| p.error.is_none() && p.lists(source))
        .map(|p| p.id.as_str())
        .collect();
    ids.sort();
    match ids.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one.to_string())),
        many => Err(Error::usage(format!(
            "'{source}' is in several Projects: pass --project ({})",
            many.join(", ")
        ))),
    }
}

/// `<src>[:<branch>]` (CLI `--source`, R97.3). The 0.7.1 form
/// `<src>:<base>=<branch>` is refused naming the new one.
pub fn parse_source_spec(spec: &str) -> Result<(String, Option<String>)> {
    if spec.contains('=') {
        return Err(Error::usage(format!(
            "'--source <src>:<base>=<branch>' is gone in 1.0.0 (got '{spec}'): one base per link, \
             use --source <src>[:<branch>]"
        )));
    }
    let (src, branch) = match spec.split_once(':') {
        Some((s, b)) => (s.trim(), Some(b.trim())),
        None => (spec.trim(), None),
    };
    if src.is_empty() || branch.is_some_and(str::is_empty) {
        return Err(Error::usage(format!(
            "--source '{spec}': expected --source <src>[:<branch>]"
        )));
    }
    Ok((src.into(), branch.map(String::from)))
}

fn quoted(s: &str) -> String {
    toml::Value::String(s.into()).to_string()
}

/// A new `project.toml` v2 (`rade project add`, R97.3): one `[[source]]`
/// per link, `base` when given.
pub fn template(name: &str, sources: &[(String, Option<String>)]) -> String {
    let links: Vec<(String, Option<String>, Option<String>)> = sources
        .iter()
        .map(|(s, b)| (s.clone(), b.clone(), None))
        .collect();
    template_links(name, &links)
}

/// [`template`] with a `checkout` line per link when given (the New Project
/// dialog writes `checkout = "auto"`, R106-b).
pub fn template_links(name: &str, sources: &[(String, Option<String>, Option<String>)]) -> String {
    let mut out = format!(
        "# RADE Project (`rade project show {id}`; PRD R97). One base per Source link.\n\
         name = {name}\n",
        id = crate::model::slug(name),
        name = quoted(name),
    );
    for (src, b, c) in sources {
        out += &format!("\n[[source]]\nid = {}\n", quoted(src));
        if let Some(b) = b {
            out += &format!("base = {}\n", quoted(b));
        }
        if let Some(c) = c {
            out += &format!("checkout = {}\n", quoted(c));
        }
    }
    out += "\n# Config for this Project's Sources (R29; `rade config explain`): agents, scm, fanout, viewer, sources.\n\
            # [agents.claude]\n\
            # command = \"claude --model opus\"\n\
            # [scm]\n\
            # draft_command = \"codex exec -\"\n";
    out
}

/// Keys `project.set` / Project settings write (R97.3).
pub const SET_KEYS: [&str; 3] = ["name", "source.<id>.base", "source.<id>.checkout"];

fn document(text: &str) -> Result<toml_edit::DocumentMut> {
    text.parse()
        .map_err(|e: toml_edit::TomlError| Error::usage(format!("project.toml: {e}")))
}

/// The edited text, refused when it no longer parses as a Project.
fn checked(id: &str, out: String) -> Result<String> {
    let p = Project::parse(id, Path::new(FILE), &out);
    match p.error {
        Some(e) => Err(Error::usage(e.replace(&format!("{FILE}: "), ""))),
        None => Ok(out),
    }
}

fn link_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    source: &str,
) -> Result<&'a mut toml_edit::Table> {
    doc.get_mut("source")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
        .and_then(|t| {
            t.iter_mut()
                .find(|t| t.get("id").and_then(toml_edit::Item::as_str) == Some(source))
        })
        .ok_or_else(|| Error::usage(format!("source '{source}' is not linked in the project")))
}

fn one_line(key: &str, value: &str) -> Result<()> {
    if value.contains('\n') {
        return Err(Error::usage(format!("{key}: one line only")));
    }
    Ok(())
}

/// A value replaced keeps its line's comment (`decor`).
fn put(slot: &mut toml_edit::Item, new: toml_edit::Item) {
    let mut new = new;
    if let (Some(old), Some(n)) = (slot.as_value(), new.as_value_mut()) {
        *n.decor_mut() = old.decor().clone();
    }
    *slot = new;
}

/// `project.toml` with a new link appended (`rade project link add`,
/// R97.3): only the new `[[source]]` lines are added.
pub fn link_add(
    id: &str,
    text: &str,
    source: &str,
    base: Option<&str>,
    checkout: Option<&Checkout>,
) -> Result<String> {
    use toml_edit::{value, ArrayOfTables, Item, Table};
    let mut doc = document(text)?;
    if migrate_text(text).ok().flatten().is_some() {
        return Err(Error::usage(
            "project.toml is v1: read it once to migrate it",
        ));
    }
    if link_table(&mut doc, source).is_ok() {
        return Err(Error::usage(format!("source '{source}' is already linked")));
    }
    let mut t = Table::new();
    t.decor_mut().set_prefix("\n");
    t["id"] = value(source);
    if let Some(b) = base.map(str::trim).filter(|b| !b.is_empty()) {
        one_line("base", b)?;
        t["base"] = value(b);
    }
    if let Some(c) = checkout {
        one_line("checkout", c.as_str())?;
        t["checkout"] = value(c.as_str());
    }
    match doc.get_mut("source") {
        Some(Item::ArrayOfTables(a)) => a.push(t),
        Some(_) => return Err(Error::usage("`source` must be [[source]] tables")),
        None => {
            let mut a = ArrayOfTables::new();
            a.push(t);
            doc.insert("source", Item::ArrayOfTables(a));
        }
    }
    checked(id, doc.to_string())
}

/// `project.toml` without the link of `source` (`rade project link remove`).
pub fn link_remove(id: &str, text: &str, source: &str) -> Result<String> {
    let mut doc = document(text)?;
    link_table(&mut doc, source)?;
    let a = doc
        .get_mut("source")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
        .unwrap();
    a.retain(|t| t.get("id").and_then(toml_edit::Item::as_str) != Some(source));
    if a.is_empty() {
        doc.remove("source");
    }
    checked(id, doc.to_string())
}

/// `project.toml` with one key set (`value` empty = removed), every other
/// byte kept (R97.3): `name`, `source.<id>.base`, `source.<id>.checkout`.
/// The result must still parse as a Project.
pub fn set_key(id: &str, text: &str, key: &str, value: &str) -> Result<String> {
    use toml_edit::value as v;
    let mut doc = document(text)?;
    let value = value.trim();
    one_line(key, value)?;
    let unknown = || {
        Error::usage(format!(
            "unknown key '{key}': one of {}",
            SET_KEYS.join(", ")
        ))
    };
    match key {
        "name" => match value {
            "" => {
                doc.remove("name");
            }
            n => put(&mut doc["name"], v(n)),
        },
        k => {
            let (src, field) = k
                .strip_prefix("source.")
                .and_then(|r| r.rsplit_once('.'))
                .filter(|(s, f)| {
                    !s.is_empty() && !s.contains('.') && matches!(*f, "base" | "checkout")
                })
                .ok_or_else(unknown)?;
            if field == "checkout" && !value.is_empty() {
                Checkout::parse(value).map_err(Error::usage)?;
            }
            let t = link_table(&mut doc, src)?;
            if value.is_empty() {
                t.remove(field);
            } else {
                match t.get_mut(field) {
                    Some(slot) => put(slot, v(value)),
                    None => t[field] = v(value),
                }
            }
        }
    }
    checked(id, doc.to_string())
}

/// `project.toml` with the link's base set (`rade project link set-base`).
pub fn link_set_base(id: &str, text: &str, source: &str, base: &str) -> Result<String> {
    if base.trim().is_empty() {
        return Err(Error::usage("set-base needs a branch"));
    }
    set_key(id, text, &format!("source.{source}.base"), base)
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
