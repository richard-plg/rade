//! Layered config (PRD R29, ADR-0017): built-in → user `config.toml` →
//! `workspace.toml` → `project.toml` → the Source's `.rade.toml` (trusted
//! only). Tables merge key by key; scalars and arrays replace. Only
//! [`LAYERED`] keys count below the user file; [`APP_WIDE`] ones there are
//! reported as ignored.

use crate::config::{Config, APP_WIDE, LAYERED, REMOVED};
use crate::model::State;
use crate::project::{self, Project};
use crate::rpc::{Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A Source's own config file (R29.4).
pub const SOURCE_FILE: &str = ".rade.toml";

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// `built-in`, `user`, `workspace`, `project`, `source`.
    pub layer: &'static str,
    pub file: Option<PathBuf>,
    /// Config keys the file sets, app-wide ones included (for `explain`).
    pub table: toml::Table,
    pub error: Option<String>,
    /// A `.rade.toml` whose content was not trusted: shown, not applied.
    pub untrusted: bool,
}

impl Layer {
    fn below_user(&self) -> bool {
        !matches!(self.layer, "built-in" | "user")
    }

    /// Why `key` (dotted) of this layer is ignored: `app-wide` below the
    /// user file (`user config only` for `[voice]`, R124), `removed in 1.0.0`
    /// in any file (R101.4).
    pub fn ignores(&self, key: &str) -> Option<&'static str> {
        let root = key.split('.').next().unwrap_or("");
        if REMOVED.contains(&root) {
            Some("removed in 1.0.0")
        } else if self.below_user() && root == "voice" {
            Some("user config only")
        } else if self.below_user() && APP_WIDE.contains(&root) {
            Some("app-wide")
        } else {
            None
        }
    }

    /// What it contributes to the effective config.
    fn applied(&self) -> toml::Table {
        if self.error.is_some() || self.untrusted {
            return toml::Table::new();
        }
        let mut t = self.table.clone();
        if self.below_user() {
            t.retain(|k, _| LAYERED.contains(&k));
        }
        t
    }

    /// Dotted keys set here but ignored (app-wide or removed).
    pub fn ignored(&self) -> Vec<String> {
        leaves(&self.table, "")
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| self.ignores(k).is_some())
            .collect()
    }

    /// A file layer from its text: keeps config keys (`workspace.toml`'s own
    /// keys and unknown ones are not config), maps `.rade.toml`'s `setup`,
    /// and validates what applies alone so the error names this file.
    fn from_text(layer: &'static str, file: &Path, text: &str, source: Option<&str>) -> Self {
        let mut l = Layer {
            layer,
            file: Some(file.into()),
            table: toml::Table::new(),
            error: None,
            untrusted: false,
        };
        let parsed: std::result::Result<toml::Table, String> =
            toml::from_str(text).map_err(|e| project::toml_error(text, &e));
        match parsed {
            Err(e) => l.error = Some(format!("{}: {e}", file.display())),
            Ok(mut t) => {
                if layer == "source" {
                    if let (Some(s), Some(id)) = (t.remove("setup"), source) {
                        let mut src = toml::Table::new();
                        src.insert("setup".into(), s);
                        let mut sources = toml::Table::new();
                        sources.insert(id.into(), toml::Value::Table(src));
                        t.insert("sources".into(), toml::Value::Table(sources));
                    }
                }
                if layer != "user" {
                    t.retain(|k, _| {
                        LAYERED.contains(&k) || APP_WIDE.contains(&k) || REMOVED.contains(&k)
                    });
                }
                l.table = t;
                if let Err(e) = Config::from_table(l.applied()) {
                    l.error = Some(format!("{}: {e}", file.display()));
                }
            }
        }
        l
    }
}

/// Built-in values: `Config::default()` plus the built-in Agent Presets.
pub fn builtin() -> Layer {
    let mut table = match toml::Value::try_from(Config::default()) {
        Ok(toml::Value::Table(t)) => t,
        _ => toml::Table::new(),
    };
    if let Ok(v) = toml::Value::try_from(crate::preset::builtins()) {
        table.insert("agents".into(), v);
    }
    table.remove("keybindings");
    Layer {
        layer: "built-in",
        file: None,
        table,
        error: None,
        untrusted: false,
    }
}

/// Deep merge: tables key by key, anything else replaces.
pub fn merge(into: &mut toml::Table, over: &toml::Table) {
    for (k, v) in over {
        match (into.get_mut(k), v) {
            (Some(toml::Value::Table(a)), toml::Value::Table(b)) => merge(a, b),
            _ => {
                into.insert(k.clone(), v.clone());
            }
        }
    }
}

/// The effective config of `layers` (built-in excluded: `Config`'s own
/// defaults are the built-in values). A layer that fails validation was
/// already emptied, so this only fails on a broken user file, which then
/// counts as empty, like `config.toml` reload does.
pub fn effective(layers: &[Layer]) -> Config {
    let mut t = toml::Table::new();
    for l in layers.iter().filter(|l| l.layer != "built-in") {
        merge(&mut t, &l.applied());
    }
    Config::from_table(t).unwrap_or_default()
}

/// Where the layers come from (R29.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub workspace: String,
    pub root: PathBuf,
    pub project: Option<Project>,
    /// (id, folder)
    pub source: Option<(String, PathBuf)>,
    pub worktree: Option<String>,
    /// The Source's `.rade.toml` exists and its content is trusted.
    pub trusted: Option<bool>,
}

impl Context {
    pub fn to_json(&self) -> Value {
        json!({
            "workspace": self.workspace,
            "project": self.project.as_ref().map(|p| p.id.clone()),
            "source": self.source.as_ref().map(|s| s.0.clone()),
            "worktree": self.worktree,
            "trusted": self.trusted,
        })
    }

    /// `context: workspace globex · project beacon (worktree otp) · source app (…)`.
    pub fn line(&self) -> String {
        let mut parts = vec![format!("workspace {}", self.workspace)];
        if let Some(p) = &self.project {
            parts.push(match &self.worktree {
                Some(w) => format!("project {} (worktree {w})", p.id),
                None => format!("project {}", p.id),
            });
        }
        if let Some((s, _)) = &self.source {
            parts.push(match self.trusted {
                Some(false) => {
                    format!("source {s} ({SOURCE_FILE} untrusted: rade config trust {s})")
                }
                _ => format!("source {s}"),
            });
        }
        format!("context: {}", parts.join(" · "))
    }
}

impl State {
    /// A Pane's Project (R29.2): its Worktree's `project`, else the one
    /// Project of the Workspace that lists its Source (none if 0 or 2+).
    pub fn project_of(&self, source: &str, worktree: Option<&str>) -> Option<Project> {
        let ws = self.source_workspace(source)?;
        let all = project::list(&ws.root);
        let recorded = worktree
            .and_then(|w| self.worktree(source, w))
            .and_then(|w| w.project.clone());
        if let Some(id) = recorded {
            return all.into_iter().find(|p| p.id == id);
        }
        let mut listing = all.into_iter().filter(|p| p.lists(source));
        match (listing.next(), listing.next()) {
            (Some(p), None) => Some(p),
            _ => None,
        }
    }

    /// Every Worktree's Agent Presets from its layers (R29.2), keyed
    /// `(source id, worktree id)`: Session Restore and pausing a Source
    /// restart Panes with them. `source`: only that Source's. A Worktree
    /// whose layers don't make valid Presets is left out.
    pub fn presets_by_worktree(
        &self,
        user: &Path,
        source: Option<&str>,
    ) -> std::collections::BTreeMap<
        (String, String),
        std::collections::BTreeMap<String, crate::preset::Preset>,
    > {
        let mut out = std::collections::BTreeMap::new();
        for s in self
            .sources
            .iter()
            .filter(|s| source.is_none_or(|q| q == s.id))
        {
            for w in &s.worktrees {
                let Ok(ctx) = self.config_context(Some(&s.id), Some(&w.id), None, None) else {
                    continue;
                };
                if let Ok(p) = effective(&load(user, &ctx)).presets() {
                    out.insert((s.id.clone(), w.id.clone()), p);
                }
            }
        }
        out
    }

    /// A Project of Workspace `ws` (`None`: any Workspace, the shown one
    /// first) by id or name.
    pub fn find_project(&self, ws: Option<&str>, query: &str) -> Result<Project> {
        let shown = self.active_workspace().to_string();
        let mut order: Vec<&crate::model::Workspace> = self.workspaces.iter().collect();
        order.sort_by_key(|w| w.id != shown);
        let mut found = Vec::new();
        for w in order.iter().filter(|w| ws.is_none_or(|q| q == w.id)) {
            if let Some(p) = project::list(&w.root)
                .into_iter()
                .find(|p| p.id == query || p.name == query)
            {
                if ws.is_some() || w.id == shown {
                    return Ok(p);
                }
                found.push((w.id.clone(), p));
            }
        }
        match found.len() {
            0 => Err(Error::not_found(format!("no project '{query}'"))),
            1 => Ok(found.remove(0).1),
            _ => Err(Error::new(
                crate::rpc::ErrorKind::Ambiguous,
                format!(
                    "project '{query}' is in several workspaces: {}",
                    found
                        .iter()
                        .map(|f| f.0.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
        }
    }

    /// The layer context of a Source (and Worktree), a Project or a
    /// Workspace (R29.3).
    pub fn config_context(
        &self,
        source: Option<&str>,
        worktree: Option<&str>,
        project: Option<&str>,
        workspace: Option<&str>,
    ) -> Result<Context> {
        if let Some(q) = source {
            let s = self.find_source(q)?;
            let ws = self
                .workspace(&s.workspace)
                .ok_or_else(|| Error::internal("source without workspace"))?;
            let wt = match worktree {
                Some(w) => Some(self.find_worktree(&s.id, Some(w))?.1),
                None => None,
            };
            let file = s.path.join(SOURCE_FILE);
            let trusted = (!s.is_general())
                .then(|| project::read_file(&file).ok())
                .flatten()
                .map(|text| self.trusted_configs.get(&s.id) == Some(&text));
            return Ok(Context {
                workspace: ws.id.clone(),
                root: ws.root.clone(),
                project: if s.is_general() {
                    None
                } else {
                    self.project_of(&s.id, wt.as_deref())
                },
                source: (!s.is_general()).then(|| (s.id.clone(), s.path.clone())),
                worktree: wt
                    .and_then(|w| self.worktree(&s.id, &w))
                    .map(|w| w.name.clone()),
                trusted,
            });
        }
        let ws = match (project, workspace) {
            (_, Some(w)) => self.find_workspace(w)?,
            _ => self
                .workspace(self.active_workspace())
                .ok_or_else(|| Error::internal("no workspace"))?,
        };
        let project = match project {
            Some(p) => Some(self.find_project(workspace.map(|_| ws.id.as_str()), p)?),
            None => None,
        };
        let ws = match &project {
            Some(p) => self
                .workspaces
                .iter()
                .find(|w| p.file.starts_with(&w.root) && w.id != crate::DEFAULT_WORKSPACE)
                .or_else(|| self.workspaces.iter().find(|w| p.file.starts_with(&w.root)))
                .unwrap_or(ws),
            None => ws,
        };
        Ok(Context {
            workspace: ws.id.clone(),
            root: ws.root.clone(),
            project,
            source: None,
            worktree: None,
            trusted: None,
        })
    }
}

/// Read every layer of `ctx`; `user` = the user's `config.toml`.
pub fn load(user: &Path, ctx: &Context) -> Vec<Layer> {
    let read = |f: &Path| match project::read_file(f) {
        Ok(t) => Some(Ok(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(format!("{}: {e}", f.display()))),
    };
    let file_layer = |name: &'static str, f: &Path, source: Option<&str>| {
        read(f).map(|r| match r {
            Ok(text) => Layer::from_text(name, f, &text, source),
            Err(e) => Layer {
                layer: name,
                file: Some(f.into()),
                table: toml::Table::new(),
                error: Some(e),
                untrusted: false,
            },
        })
    };
    let mut out = vec![builtin()];
    out.extend(file_layer("user", user, None));
    out.extend(file_layer(
        "workspace",
        &ctx.root.join(".rade/workspace.toml"),
        None,
    ));
    if let Some(p) = &ctx.project {
        let mut l = Layer {
            layer: "project",
            file: Some(p.file.clone()),
            table: p.table.clone(),
            error: p.error.clone(),
            untrusted: false,
        };
        l.table
            .retain(|k, _| LAYERED.contains(&k) || APP_WIDE.contains(&k) || REMOVED.contains(&k));
        if l.error.is_none() {
            if let Err(e) = Config::from_table(l.applied()) {
                l.error = Some(format!("{}: {e}", p.file.display()));
            }
        }
        out.push(l);
    }
    if let Some((id, path)) = &ctx.source {
        if let Some(mut l) = file_layer("source", &path.join(SOURCE_FILE), Some(id)) {
            l.untrusted = ctx.trusted != Some(true);
            out.push(l);
        }
    }
    out
}

/// Every leaf (dotted key, value) of a table.
pub fn leaves(t: &toml::Table, prefix: &str) -> Vec<(String, toml::Value)> {
    let mut out = Vec::new();
    for (k, v) in t {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(sub) if !sub.is_empty() => out.extend(leaves(sub, &key)),
            _ => out.push((key, v.clone())),
        }
    }
    out
}

/// The value at a dotted key.
pub fn get<'a>(t: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let mut parts = key.split('.');
    let mut v = t.get(parts.next()?)?;
    for p in parts {
        v = v.as_table()?.get(p)?;
    }
    Some(v)
}

/// One key across the layers (R29.3): the effective value, the layer it
/// came from, and every layer's own value.
pub fn explain(layers: &[Layer], key: &str) -> Value {
    let mut value = None;
    let mut from = None;
    let rows: Vec<Value> = layers
        .iter()
        .map(|l| {
            let v = get(&l.table, key);
            let ignored = v.and(l.ignores(key));
            if v.is_some() && ignored.is_none() && !l.untrusted && l.error.is_none() {
                value = v.cloned();
                from = Some(l.layer);
            }
            let mut row = json!({
                "layer": l.layer,
                "file": l.file,
                "value": v.map(json_of),
            });
            if let Some(why) = ignored {
                row["ignored"] = json!(why);
            }
            if l.untrusted {
                row["untrusted"] = json!(true);
            }
            if let Some(e) = &l.error {
                row["error"] = json!(e);
            }
            row
        })
        .collect();
    json!({
        "key": key,
        "value": value.as_ref().map(json_of),
        "from": from,
        "layers": rows,
    })
}

/// No key or a prefix (R29.3): every key set in a file under `prefix`,
/// explained; app-wide keys set below the user file; layer errors.
pub fn explain_all(layers: &[Layer], prefix: &str) -> Value {
    let mut keys: Vec<String> = Vec::new();
    let mut ignored = Vec::new();
    let mut errors = Vec::new();
    let under = |k: &str| prefix.is_empty() || k == prefix || k.starts_with(&format!("{prefix}."));
    for l in layers.iter().filter(|l| l.layer != "built-in") {
        for (k, _) in leaves(&l.table, "") {
            if !under(&k) {
                continue;
            }
            if let Some(why) = l.ignores(&k) {
                ignored.push(json!({"layer": l.layer, "file": l.file, "key": k, "why": why}));
            } else if !keys.contains(&k) {
                keys.push(k);
            }
        }
        if let Some(e) = &l.error {
            errors.push(json!(e));
        }
    }
    keys.sort();
    json!({
        "values": keys.iter().map(|k| explain(layers, k)).collect::<Vec<_>>(),
        "ignored": ignored,
        "errors": errors,
    })
}

fn json_of(v: &toml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// A value as `explain` prints it (`"x"`, `3`, `["a"]`).
pub fn show(v: &Value) -> String {
    match v {
        Value::Null => "(not set)".into(),
        Value::Object(_) => "{…}".into(),
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(name: &'static str, text: &str) -> Layer {
        Layer::from_text(name, Path::new(&format!("/{name}.toml")), text, Some("app"))
    }

    #[test]
    fn r29_1_merge_tables_replace_scalars_and_arrays() {
        let mut a: toml::Table = toml::from_str("x = [1]\n[t]\na = 1\nb = 2").unwrap();
        let b: toml::Table = toml::from_str("x = [2, 3]\n[t]\nb = 9\nc = 3").unwrap();
        merge(&mut a, &b);
        assert_eq!(
            a,
            toml::from_str::<toml::Table>("x = [2, 3]\n[t]\na = 1\nb = 9\nc = 3").unwrap()
        );
    }

    #[test]
    fn r124_j_voice_config_defaults_settings_and_layers() {
        use crate::config::{DraftLanguage, VoiceAgent, VoiceTools};
        use crate::settings;
        let c = Config::default();
        let v = &c.voice;
        assert_eq!(
            (v.agent, v.model.as_str(), v.agent_command.as_str(), v.tools),
            (VoiceAgent::Claude, "", "", VoiceTools::Read)
        );
        assert_eq!(
            (v.max_turns, v.timeout_secs, v.max_recording_secs),
            (6, 60, 120)
        );
        assert!(v.live_preview);
        assert_eq!(v.draft_language, DraftLanguage::Same);
        assert_eq!(v.stt_url, "http://127.0.0.1:8178/v1/audio/transcriptions");
        assert_eq!(
            (v.stt_model.as_str(), v.stt_language.as_str()),
            ("whisper-1", "auto")
        );
        assert_eq!(v.stt_key_command, "");
        assert_eq!(
            v.record_command,
            "pw-record --rate 16000 --channels 1 --format s16 -"
        );
        assert!(c
            .keybindings()
            .iter()
            .any(|(a, k)| a == "voice" && k == "F8"));
        // Read through the clamps.
        let c = Config::parse("[voice]\nmax_turns = 99\ntimeout_secs = 1\nmax_recording_secs = 9999\nagent = \"codex\"\ntools = \"none\"\ndraft_language = \"en\"\n").unwrap();
        assert_eq!(c.voice.max_turns(), 20);
        assert_eq!(c.voice.timeout().as_secs(), 10);
        assert_eq!(c.voice.max_recording().as_secs(), 600);
        assert_eq!(
            (c.voice.agent, c.voice.tools, c.voice.draft_language),
            (VoiceAgent::Codex, VoiceTools::None, DraftLanguage::En)
        );
        assert!(Config::parse("[voice]\nbogus = 1\n").is_err());
        let lang = |l: &str| {
            Config::parse(&format!("[voice]\nstt_language = \"{l}\"\n"))
                .unwrap()
                .voice
                .stt_language()
                .to_string()
        };
        assert_eq!(c.voice.stt_language(), "auto");
        for (set, read) in [
            ("vi", "vi"),
            ("ja", "ja"),
            ("yue", "yue"),
            ("VI", "auto"),
            ("english", "auto"),
            ("v", "auto"),
            ("v1", "auto"),
            ("", "auto"),
        ] {
            assert_eq!(lang(set), read, "{set}");
        }
        // Settings rows: the Voice category sits between Source Control
        // (M45) and Statusline.
        let cats = settings::CATEGORIES;
        let at = |n: &str| cats.iter().position(|c| *c == n).unwrap();
        assert_eq!(at("Voice"), at("Source Control") + 1);
        assert_eq!(at("Statusline"), at("Voice") + 1);
        let rows: Vec<_> = settings::SETTINGS
            .iter()
            .filter(|s| s.category == "Voice")
            .collect();
        assert_eq!(rows.len(), 8);
        let kind = |k: &str| settings::find(k).unwrap().kind;
        assert_eq!(
            kind("voice.max_turns"),
            settings::Kind::Integer { min: 1, max: 20 }
        );
        assert_eq!(
            kind("voice.timeout_secs"),
            settings::Kind::Integer { min: 10, max: 300 }
        );
        assert_eq!(
            kind("voice.max_recording_secs"),
            settings::Kind::Integer { min: 10, max: 600 }
        );
        assert_eq!(
            kind("voice.stt_language"),
            settings::Kind::Choice(&["auto", "vi", "en"])
        );
        // User config only: a `.rade.toml` `[voice]` is explained as ignored.
        assert!(!LAYERED.contains(&"voice") && APP_WIDE.contains(&"voice"));
        let user = builtin();
        assert_eq!(user.ignores("voice.agent"), None);
        let text = "[voice]\nstt_url = \"http://x\"\n";
        let src = layer("source", text);
        assert_eq!(src.ignores("voice.stt_url"), Some("user config only"));
        assert_eq!(src.ignored(), vec!["voice.stt_url".to_string()]);
        let eff = effective(&[user, src]);
        assert_eq!(eff.voice.stt_url, Config::default().voice.stt_url);
    }

    #[test]
    fn r29_a_project_beats_workspace_beats_user() {
        let layers = vec![
            builtin(),
            layer(
                "user",
                "[scm]\ndraft_command = \"u\"\n[viewer]\neditor = \"vim\"",
            ),
            layer("workspace", "name = \"V\"\n[scm]\ndraft_command = \"w\""),
            layer("project", "[scm]\ndraft_command = \"p\""),
        ];
        let c = effective(&layers);
        assert_eq!(c.scm.draft_command, "p");
        assert_eq!(c.viewer.editor, "vim");
        let e = explain(&layers, "scm.draft_command");
        assert_eq!(
            (e["value"].as_str(), e["from"].as_str()),
            (Some("p"), Some("project"))
        );
        let vals: Vec<&str> = e["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["value"].as_str().unwrap_or("-"))
            .collect();
        assert!(vals[0].starts_with("claude -p"));
        assert_eq!(&vals[1..], ["u", "w", "p"]);
        // Built-in only.
        let e = explain(&layers, "fanout.judge");
        assert_eq!(
            (e["value"].as_str(), e["from"].as_str()),
            (Some("claude"), Some("built-in"))
        );
        assert_eq!(
            explain(&layers, "agents.claude.command")["from"],
            "built-in"
        );
        // No key: values set in files, `name` (not config) left out.
        let all = explain_all(&layers, "");
        let keys: Vec<&str> = all["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, ["scm.draft_command", "viewer.editor"]);
        assert_eq!(
            explain_all(&layers, "viewer")["values"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn r29_b_agents_override_per_layer() {
        let layers = vec![
            builtin(),
            layer(
                "user",
                "[agents.fake]\ncommand = \"fake\"\nprompt = \"type\"",
            ),
            layer("workspace", "[agents.fake]\ncommand = \"fake-a\""),
        ];
        let p = effective(&layers).presets().unwrap();
        assert_eq!(p["fake"].command, "fake-a");
        assert_eq!(
            p["fake"].prompt,
            crate::preset::PromptMode::Type,
            "merged, not replaced"
        );
    }

    #[test]
    fn r29_c_untrusted_source_file_is_shown_not_applied() {
        let mut s = layer(
            "source",
            "setup = \"touch marker\"\n[scm]\ndraft_command = \"s\"",
        );
        assert_eq!(s.error, None);
        s.untrusted = true;
        let layers = vec![builtin(), s.clone()];
        let c = effective(&layers);
        assert_eq!(c.setup_for("app", "app"), None);
        let e = explain(&layers, "sources.app.setup");
        assert_eq!(e["value"], Value::Null);
        assert_eq!(e["layers"][1]["untrusted"], true);
        assert_eq!(e["layers"][1]["value"], "touch marker");
        s.untrusted = false;
        let c = effective(&[builtin(), s]);
        assert_eq!(c.setup_for("app", "app").as_deref(), Some("touch marker"));
        assert_eq!(c.scm.draft_command, "s");
    }

    #[test]
    fn r29_d_app_wide_keys_below_user_are_ignored() {
        let w = layer(
            "workspace",
            "[keybindings]\nnew_tab = \"<Alt>t\"\n[limits]\nmax_depth = 9\n[env]\nX = \"1\"",
        );
        assert_eq!(w.error, None);
        assert_eq!(w.ignored(), ["keybindings.new_tab", "limits.max_depth"]);
        let layers = vec![builtin(), layer("user", ""), w];
        assert_eq!(effective(&layers).limits.max_depth, 3);
        let e = explain(&layers, "limits.max_depth");
        assert_eq!(e["layers"][2]["ignored"], "app-wide");
        assert_eq!(e["from"], "built-in");
        let all = explain_all(&layers, "");
        assert_eq!(all["ignored"].as_array().unwrap().len(), 2);
        assert!(all["values"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_broken_layer_names_its_file_and_is_skipped() {
        let w = layer("workspace", "[scm]\nbogus = 1");
        assert!(
            w.error.as_deref().unwrap().starts_with("/workspace.toml: "),
            "{:?}",
            w.error
        );
        let p = layer("project", "[scm\n");
        assert!(
            p.error.as_deref().unwrap().contains("line 1"),
            "{:?}",
            p.error
        );
        let layers = vec![builtin(), layer("user", "[viewer]\neditor = \"vim\""), w, p];
        assert_eq!(effective(&layers).viewer.editor, "vim");
        assert_eq!(
            explain_all(&layers, "")["errors"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn r29_2_context_project_of_a_source_and_trust() {
        let root = std::env::temp_dir().join(format!("rade-layers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("api")).unwrap();
        let mut st = State::new(Path::new("/home/u"));
        st.add_workspace(&root, "V").unwrap();
        st.add_source(&root.join("api"), None).unwrap();
        st.add_source(&root.join("web"), None).unwrap();
        let write = |id: &str, text: &str| {
            let f = Project::path(&root, id);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        };
        write("one", "[[source]]\nid = \"api\"\n[[source]]\nid = \"web\"");
        assert_eq!(st.project_of("api", None).unwrap().id, "one");
        write("two", "[[source]]\nid = \"api\"");
        assert_eq!(st.project_of("api", None), None, "in two Projects");
        assert_eq!(st.project_of("web", None).unwrap().id, "one");
        // A Worktree's recorded Project wins.
        let (_, w) = st.find_worktree("api", None).unwrap();
        st.worktree_mut("api", &w).unwrap().project = Some("two".into());
        assert_eq!(st.project_of("api", Some(&w)).unwrap().id, "two");
        let ctx = st
            .config_context(Some("api"), Some("main"), None, None)
            .unwrap();
        assert_eq!(ctx.project.as_ref().unwrap().id, "two");
        assert_eq!(ctx.trusted, None, "no .rade.toml");
        std::fs::write(root.join("api/.rade.toml"), "setup = \"x\"").unwrap();
        let ctx = st.config_context(Some("api"), None, None, None).unwrap();
        assert_eq!(ctx.trusted, Some(false));
        assert!(ctx.line().contains("untrusted: rade config trust api"));
        st.trusted_configs
            .insert("api".into(), "setup = \"x\"".into());
        let ctx = st.config_context(Some("api"), None, None, None).unwrap();
        assert_eq!(ctx.trusted, Some(true));
        let user = root.join("none.toml");
        let c = effective(&load(&user, &ctx));
        assert_eq!(c.setup_for("api", "api").as_deref(), Some("x"));
        let ctx = st.config_context(None, None, Some("one"), None).unwrap();
        assert_eq!((ctx.workspace.as_str(), ctx.source.is_none()), ("v", true));
        assert!(st.find_project(None, "nope").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn r29_2_restore_and_pause_use_each_worktrees_project() {
        let root = std::env::temp_dir().join(format!("rade-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("api")).unwrap();
        let mut st = State::new(Path::new("/home/u"));
        st.add_workspace(&root, "V").unwrap();
        st.add_source(&root.join("api"), None).unwrap();
        for (id, text) in [
            ("one", "[[source]]\nid = \"api\""),
            (
                "two",
                "[[source]]\nid = \"api\"\n[agents.x]\ncommand = \"two-x\"",
            ),
        ] {
            let f = Project::path(&root, id);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
        let user = root.join("config.toml");
        std::fs::write(&user, "[agents.x]\ncommand = \"user-x\"").unwrap();
        // api is in two Projects; Worktree d1 was made in `two`.
        let (_, d1) = st
            .add_worktree("api", "d1", &root.join("d1"), None)
            .unwrap();
        st.set_worktree_origin("api", &d1, Some("two".into()), None)
            .unwrap();
        let spawn = |st: &mut State, wt: Option<&str>| {
            st.spawn(
                crate::agents::SpawnSpec {
                    source: Some("api".into()),
                    worktree: wt.map(Into::into),
                    agent: Some("x".into()),
                    command: Some("x".into()),
                    ..Default::default()
                },
                &crate::config::Limits::default(),
            )
            .unwrap()
        };
        let in_d1 = spawn(&mut st, Some("d1"));
        let in_main = spawn(&mut st, Some("main"));
        let by = st.presets_by_worktree(&user, None);
        let cmd = |st: &State, p: &str| st.pane(p).unwrap().command().map(String::from);
        let mut back = st.clone();
        back.prepare_restore_with(&|s, w| by[&(s.to_string(), w.to_string())].clone(), 1);
        assert_eq!(cmd(&back, &in_d1).as_deref(), Some("two-x"));
        assert_eq!(cmd(&back, &in_main).as_deref(), Some("user-x"));
        st.pause_source(
            "api",
            &|s, w| by[&(s.to_string(), w.to_string())].clone(),
            true,
            1,
        )
        .unwrap();
        assert_eq!(cmd(&st, &in_d1).as_deref(), Some("two-x"));
        assert_eq!(cmd(&st, &in_main).as_deref(), Some("user-x"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn show_values_as_toml() {
        assert_eq!(show(&json!("a \"b\"")), "\"a \\\"b\\\"\"");
        assert_eq!(show(&Value::Null), "(not set)");
        assert_eq!(show(&json!(3)), "3");
    }

    #[test]
    fn r101_h_removed_engine_keys_are_ignored_not_errors() {
        let root = std::env::temp_dir().join(format!("rade-r101h-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("shop-api")).unwrap();
        let mut st = State::new(Path::new("/home/u"));
        st.add_workspace(&root, "Acme").unwrap();
        st.add_source(&root.join("shop-api"), None).unwrap();
        let f = Project::path(&root, "shop");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(
            &f,
            "name = \"Shop\"\n[[source]]\nid = \"shop-api\"\n[task]\nverify = \"make test\"\n[batch]\nparallel = 2\n[scm]\ndraft_command = \"p\"\n",
        )
        .unwrap();
        // The user file and the Workspace file may carry them too.
        let user = root.join("config.toml");
        std::fs::write(&user, "[profiles.deep]\nagent = \"claude\"\n[report]\ndraft_command = \"r\"\n[initiative]\nagents = 2\n").unwrap();
        let ctx = st.config_context(None, None, Some("shop"), None).unwrap();
        let layers = load(&user, &ctx);
        assert!(layers.iter().all(|l| l.error.is_none()), "{layers:?}");
        assert_eq!(effective(&layers).scm.draft_command, "p");
        let all = explain_all(&layers, "");
        assert_eq!(all["errors"].as_array().unwrap().len(), 0, "{all}");
        let ignored: Vec<(String, String, String)> = all["ignored"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["layer"].as_str().unwrap().to_string(),
                    i["key"].as_str().unwrap().to_string(),
                    i["why"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let r = |l: &str, k: &str| (l.to_string(), k.to_string(), "removed in 1.0.0".to_string());
        for want in [
            r("project", "task.verify"),
            r("project", "batch.parallel"),
            r("user", "profiles.deep.agent"),
            r("user", "report.draft_command"),
            r("user", "initiative.agents"),
        ] {
            assert!(ignored.contains(&want), "{want:?} in {ignored:?}");
        }
        let vals: Vec<&str> = all["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["key"].as_str().unwrap())
            .collect();
        assert_eq!(vals, ["scm.draft_command"]);
        let e = explain(&layers, "task.verify");
        assert_eq!(e["value"], Value::Null);
        let proj = e["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["layer"] == "project")
            .unwrap();
        assert_eq!(
            (proj["value"].as_str(), proj["ignored"].as_str()),
            (Some("make test"), Some("removed in 1.0.0"))
        );
        // A user config.toml with them still parses.
        assert!(Config::parse("[task]\nverify = \"x\"\n[batch]\nparallel = 2\n").is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
