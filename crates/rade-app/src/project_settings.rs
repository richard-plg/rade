//! Project settings' writes (PRD R106.3, ADR-0028): every edit is one key
//! written by `project::set_key` (comments and order kept), through the
//! `project.set` socket method an Agent can call too; the form itself is
//! the link-table dialog of `project_dialogs`.

use crate::app::app;
use crate::handlers::params;
use rade_core::project::{self, Project};
use rade_core::rpc::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Set {
    project: String,
    key: String,
    #[serde(default)]
    value: String,
    caller: Option<String>,
}

/// `project.set` (R82.2), on the GTK thread: one key, nothing confirmed.
pub fn dispatch(p: Value) -> Result<Value> {
    let s: Set = params(p)?;
    crate::handlers::on_main(move || {
        let (ws, p) = {
            let app = app();
            let m = app.model.borrow();
            crate::projects::find(&m, &s.project, s.caller.as_deref())?
        };
        write(&p, &s.key, &s.value)?;
        // A link's base / checkout changed: its checkout is chosen again (R98.1).
        if let Some(src) = s
            .key
            .strip_prefix("source.")
            .and_then(|k| k.rsplit_once('.'))
            .map(|(src, _)| src.to_string())
        {
            let app = app();
            app.update(|m| {
                if let Some(w) = m.workspaces.iter_mut().find(|w| w.id == ws) {
                    w.link_checkouts
                        .retain(|r| !(r.project == p.id && r.source == src));
                }
            });
            app.save_now();
        }
        Ok(
            json!({"project": p.id, "workspace": ws, "file": p.file, "key": s.key, "value": s.value}),
        )
    })
}

/// Write one key of Project `p`'s file (temp file + rename).
fn write(p: &Project, key: &str, value: &str) -> Result<()> {
    let old = project::read_file(&p.file)
        .map_err(|e| Error::usage(format!("{}: {e}", p.file.display())))?;
    let new = project::set_key(&p.id, &old, key, value)
        .map_err(|e| Error::usage(format!("{}: {}", p.file.display(), e.message)))?;
    write_text(p, &old, &new)
}

/// Replace Project `p`'s file `old` with `new` (temp file + rename; nothing
/// written when equal).
pub(crate) fn write_text(p: &Project, old: &str, new: &str) -> Result<()> {
    if new != old {
        // Through a symlinked project.toml to its target.
        let file = std::fs::canonicalize(&p.file).unwrap_or_else(|_| p.file.clone());
        let tmp = file.with_extension("toml.tmp");
        std::fs::write(&tmp, new)
            .and_then(|_| std::fs::rename(&tmp, &file))
            .map_err(|e| Error::internal(format!("{}: {e}", p.file.display())))?;
    }
    crate::sidebar::projects_changed();
    Ok(())
}

/// An edit made in the form (`<ws>/<project>`): the same write.
pub fn set_from_ui(page: &str, key: &str, value: &str) -> Result<()> {
    let (_, p) = crate::projects::lookup(page)
        .ok_or_else(|| Error::not_found(format!("no Project {page}")))?;
    write(&p, key, value)
}

/// Project settings… for `<ws>/<project>` (R106.3): the link-table dialog.
pub fn open(page: &str) {
    crate::project_dialogs::settings(page);
}

/// `debug.ui settings…`: `settings` (the open dialog's fields),
/// `settings_open` (`arg` = Project id), `settings_set` (`key`, `value`:
/// what saving that field does), `settings_close`.
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    let ws = app().model.borrow().active_workspace().to_string();
    let page = format!("{ws}/{}", s("arg"));
    match s("op") {
        "settings" => {}
        "settings_open" => open(&page),
        "settings_set" => set_from_ui(&page, s("key"), s("value"))?,
        "settings_close" => crate::project_dialogs::close(),
        op => return Err(Error::usage(format!("unknown debug.ui op '{op}'"))),
    }
    Ok(json!({"settings": crate::project_dialogs::settings_info()}))
}
