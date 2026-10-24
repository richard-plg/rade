//! `state.json` load and atomic save (Session Restore skeleton, R11.1).

use crate::model::{State, STATE_VERSION};
use std::io::Write;
use std::path::Path;

/// Load saved state. `Ok(None)` if there is no file yet.
pub fn load(path: &Path) -> Result<Option<State>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut st: State =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if st.version > STATE_VERSION {
        return Err(format!(
            "{}: version {} is newer than {STATE_VERSION}",
            path.display(),
            st.version
        ));
    }
    if st.version < 2 {
        migrate_v1(path, &text, &mut st)?;
    } else if st.version == 2 {
        migrate_v2(path, &text, &mut st)?;
    }
    Ok(Some(st))
}

/// v2 → v3 (1.0.0, R101.3). Serde already ignored the removed keys
/// (`plan_gates`, `tasks_page`, Batches, Initiatives, `Tab.task`, …) and
/// kept every Workspace, Source, Worktree, Tab and Pane. Here: keep the
/// original once as `state.json.v2.bak` (never overwritten), before the
/// first save drops those keys.
fn migrate_v2(path: &Path, text: &str, st: &mut State) -> Result<(), String> {
    write_backup_once(path, ".v2.bak", text)?;
    st.version = STATE_VERSION;
    Ok(())
}

/// `<path><suffix>` holding `text`, unless that file exists already.
fn write_backup_once(path: &Path, suffix: &str, text: &str) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut bak = path.as_os_str().to_owned();
    bak.push(suffix);
    let bak = std::path::PathBuf::from(bak);
    // Recorded Agent argv/env may hold private session information.
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&bak);
    match f {
        Ok(mut f) => f
            .write_all(text.as_bytes())
            .and_then(|_| f.sync_all())
            .map_err(|e| format!("{}: {e}", bak.display())),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("{}: {e}", bak.display())),
    }
}

/// Preserve an unreadable/newer session before allowing a fresh session to save.
/// Exclusive creation never replaces an earlier recovery copy. The source stays
/// in place until the caller successfully saves its new session.
pub fn recovery_backup(path: &Path) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut source = std::fs::File::open(path)?;
    for n in 1.. {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".recovery-{n}.bak"));
        let backup = std::path::PathBuf::from(name);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            // Recorded Agent argv/env may contain private session information.
            .mode(0o600)
            .open(&backup)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        if let Err(e) = std::io::copy(&mut source, &mut file).and_then(|_| file.sync_all()) {
            let _ = std::fs::remove_file(&backup);
            return Err(e);
        }
        std::fs::File::open(path.parent().expect("state path has a parent"))?.sync_all()?;
        return Ok(backup);
    }
    unreachable!()
}

/// v1 → v2 (M15, R24.4). Serde aliases already read the renamed keys
/// (`projects`, `active_project`, a Fan-out's `project`) and put every
/// Source in `default` ([`State::ensure_general`] adds it). Here: keep the
/// original once as `state.json.v1.bak` (the way back to an older RADE,
/// which refuses v2), and rename the palette's recently used keys.
fn migrate_v1(path: &Path, text: &str, st: &mut State) -> Result<(), String> {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".v1.bak");
    let bak = std::path::PathBuf::from(bak);
    if !bak.exists() {
        std::fs::write(&bak, text).map_err(|e| format!("{}: {e}", bak.display()))?;
    }
    for k in &mut st.palette_recent {
        *k = recent_key_v2(k);
    }
    st.version = STATE_VERSION;
    Ok(())
}

/// A v1 palette recent key in v2: the kind `project:` and the action names
/// (`project_2`, `rename_project`, …) are renamed; ids (`project:my-project`,
/// `worktree:my-project/w_1`) and file paths never are.
fn recent_key_v2(k: &str) -> String {
    if let Some(id) = k.strip_prefix("project:") {
        format!("source:{id}")
    } else if let Some(action) = k.strip_prefix("action:") {
        // `action:spawn:<preset>`: only the action name, not its argument.
        let (name, arg) = action
            .split_once(':')
            .map_or((action, None), |(n, a)| (n, Some(a)));
        let name = name.replace("project", "source");
        arg.map_or(format!("action:{name}"), |a| format!("action:{name}:{a}"))
    } else {
        k.to_string()
    }
}

/// Write via a temp file in the same directory, fsync, then rename, so a
/// crash leaves either the old or the new file, never a torn one.
pub fn save(path: &Path, ws: &State) -> std::io::Result<()> {
    let dir = path.parent().expect("state path has a parent");
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(
        serde_json::to_string_pretty(ws)
            .expect("serializable")
            .as_bytes(),
    )?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(dir)?.sync_all()
}

/// Hold `<state>.lock` for the app's lifetime: one app per `state.json`,
/// whatever socket it serves (two would overwrite each other's saves, and a
/// second one in test mode would act on the owner's state, ADR-0021).
/// `Err` = another app holds it.
pub fn lock(state: &Path) -> std::io::Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    if let Some(dir) = state.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.with_extension("json.lock"))?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_app_per_state_file() {
        let d = std::env::temp_dir().join(format!("rade-lock-{}", std::process::id()));
        let state = d.join("state.json");
        let held = super::lock(&state).unwrap();
        assert!(super::lock(&state).is_err());
        drop(held);
        // A child forked by a parallel test may hold a copy of the fd until
        // its exec closes it: wait a moment.
        let ok = (0..100).any(|_| {
            super::lock(&state).is_ok()
                || (
                    std::thread::sleep(std::time::Duration::from_millis(20)),
                    false,
                )
                    .1
        });
        assert!(ok);
        let _ = std::fs::remove_dir_all(d);
    }

    use super::*;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rade-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn save_then_load_roundtrips() {
        let d = tmpdir("rt");
        let path = d.join("sub/state.json");
        let mut ws = State::new(Path::new("/h"));
        ws.add_source(Path::new("/p"), None).unwrap();
        save(&path, &ws).unwrap();
        assert_eq!(load(&path).unwrap(), Some(ws));
        assert!(!d.join("sub/.state.json.tmp").exists());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": 3"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn missing_is_none_and_garbage_or_future_is_error() {
        let d = tmpdir("bad");
        assert_eq!(load(&d.join("nope.json")).unwrap(), None);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("g.json"), "{").unwrap();
        assert!(load(&d.join("g.json")).is_err());
        let mut v = serde_json::to_value(State::new(Path::new("/h"))).unwrap();
        v["version"] = 99.into();
        std::fs::write(d.join("f.json"), v.to_string()).unwrap();
        assert!(load(&d.join("f.json")).unwrap_err().contains("newer"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn recovery_preserves_invalid_and_future_state_and_existing_backups() {
        let d = tmpdir("recovery");
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("state.json");
        let mut future = serde_json::to_value(State::new(Path::new("/h"))).unwrap();
        future["version"] = 99.into();
        let future = future.to_string();
        for text in ["{", future.as_str()] {
            std::fs::write(&path, text).unwrap();
            assert!(load(&path).is_err());
            let backup = recovery_backup(&path).unwrap();
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backup).unwrap().permissions().mode() & 0o077,
                0
            );
            assert_eq!(std::fs::read_to_string(&backup).unwrap(), text);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        assert_eq!(
            std::fs::read_to_string(d.join("state.json.recovery-1.bak")).unwrap(),
            "{"
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn failed_backup_and_save_keep_original_state() {
        let d = tmpdir("failed-recovery");
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("state.json");
        std::fs::write(&path, "original").unwrap();
        // Opening a directory for reading fails during copying, including as root.
        assert!(recovery_backup(&d).is_err());
        assert!(!d
            .with_file_name(format!(
                "{}.recovery-1.bak",
                d.file_name().unwrap().to_string_lossy()
            ))
            .exists());
        // A colliding directory is also preserved rather than replaced.
        std::fs::create_dir(d.join("state.json.recovery-1.bak")).unwrap();
        let backup = recovery_backup(&path).unwrap();
        assert_eq!(backup, d.join("state.json.recovery-2.bak"));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "original");
        std::fs::create_dir(d.join(".state.json.tmp")).unwrap();
        assert!(save(&path, &State::new(Path::new("/h"))).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        std::fs::remove_dir_all(d).unwrap();
    }

    fn v1_fixture(dir: &Path) -> std::path::PathBuf {
        let text = include_str!("../../../tests/e2e/fixtures/state-v1.json")
            .replace("@HOME@", "/home/u")
            .replace("@DATA@", "/home/u/.local/share");
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn r24_4_v1_migrates_to_v2_with_a_one_time_backup() {
        let d = tmpdir("v1");
        let path = v1_fixture(&d);
        let original = std::fs::read(&path).unwrap();
        let mut st = load(&path).unwrap().unwrap();
        st.ensure_general(Path::new("/home/u"));
        assert_eq!(st.version, STATE_VERSION);
        let bak = d.join("state.json.v1.bak");
        assert_eq!(
            std::fs::read(&bak).unwrap(),
            original,
            "byte-identical backup"
        );
        // Keys renamed, every Source in `default`, Tabs and session ids kept.
        assert_eq!(st.active_source, "demo");
        assert!(st.sources.iter().all(|s| s.workspace == "default"));
        assert_eq!(st.workspaces.len(), 1);
        assert_eq!(st.workspaces[0].id, "default");
        assert_eq!(st.workspaces[0].root, Path::new("/home/u"));
        let demo = st.source("demo").unwrap();
        assert_eq!(demo.worktrees.len(), 2);
        assert_eq!(
            st.pane("p_4").unwrap().session_id.as_deref(),
            Some("sess-v1")
        );
        assert_eq!(st.locate("p_3").unwrap().tab, "t_2");
        assert_eq!(
            st.palette_recent,
            ["source:demo", "action:add_source", "worktree:demo/w_5"]
        );
        // Saved as v2; the next load leaves the backup alone.
        save(&path, &st).unwrap();
        std::fs::write(&bak, b"kept").unwrap();
        let again = load(&path).unwrap().unwrap();
        assert_eq!(again.version, STATE_VERSION);
        assert_eq!(std::fs::read(&bak).unwrap(), b"kept");
        // A second v1 file never overwrites an existing backup either.
        v1_fixture(&d);
        load(&path).unwrap().unwrap();
        assert_eq!(std::fs::read(&bak).unwrap(), b"kept");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn r24_4_recents_rename_the_kind_and_action_never_an_id() {
        let keys = [
            "project:my-project",
            "worktree:my-project/w_1",
            "action:project_2",
            "action:rename_project",
            "action:spawn:project-bot",
            "pane:p_8",
            "file:project/notes.md",
        ];
        let renamed: Vec<String> = keys.iter().map(|k| recent_key_v2(k)).collect();
        assert_eq!(
            renamed,
            [
                "source:my-project",
                "worktree:my-project/w_1",
                "action:source_2",
                "action:rename_source",
                "action:spawn:project-bot",
                "pane:p_8",
                "file:project/notes.md",
            ]
        );
    }

    /// The R101-c fixture: a 0.7.1 `state.json` written by the 0.7.1
    /// structs (2 Workspaces, 4 Sources besides the 2 Generals, 3
    /// Worktrees besides `main`, 5 Tabs — `t_11` a Task Run —, 9 Panes).
    const V071: &str = include_str!("../tests/fixtures/state-0.7.1.json");
    const DROPPED: [&str; 9] = [
        "tasks_page",
        "reports_page",
        "batches",
        "batch_page",
        "plan_gates",
        "verify_confirmed",
        "initiatives",
        "initiative_page",
        "start_here_dismissed",
    ];

    fn v071(dir: &Path) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, V071.replace("@ROOT@", "/r")).unwrap();
        path
    }

    /// (Workspaces, Sources without Generals, Worktrees without `main`,
    /// Tabs, Panes) ids of a `state.json` text.
    fn ids(text: &str) -> [Vec<String>; 5] {
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let strs = |it: Vec<&serde_json::Value>| -> Vec<String> {
            it.iter().map(|x| x.as_str().unwrap().to_string()).collect()
        };
        let sources: Vec<&serde_json::Value> = v["sources"].as_array().unwrap().iter().collect();
        let wts: Vec<&serde_json::Value> = sources
            .iter()
            .flat_map(|s| s["worktrees"].as_array().unwrap())
            .collect();
        let tabs: Vec<&serde_json::Value> = wts
            .iter()
            .flat_map(|w| w["tabs"].as_array().unwrap())
            .collect();
        fn leaves<'a>(l: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
            match l["type"].as_str() {
                Some("leaf") => out.push(&l["pane"]["id"]),
                _ => {
                    for k in ["first", "second", "a", "b", "children"] {
                        match &l[k] {
                            serde_json::Value::Array(xs) => xs.iter().for_each(|x| leaves(x, out)),
                            serde_json::Value::Object(_) => leaves(&l[k], out),
                            _ => {}
                        }
                    }
                }
            }
        }
        let mut panes = Vec::new();
        for t in &tabs {
            leaves(&t["layout"], &mut panes);
        }
        [
            strs(
                v["workspaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|w| &w["id"])
                    .collect(),
            ),
            strs(
                sources
                    .iter()
                    .filter(|s| !s["id"].as_str().unwrap().starts_with("general"))
                    .map(|s| &s["id"])
                    .collect(),
            ),
            strs(
                wts.iter()
                    .filter(|w| w["name"] != "main")
                    .map(|w| &w["id"])
                    .collect(),
            ),
            strs(tabs.iter().map(|t| &t["id"]).collect()),
            strs(panes),
        ]
    }

    #[test]
    fn r101_c_v2_loads_and_saves_as_v3_keeping_every_id_with_a_backup() {
        let d = tmpdir("r101c");
        let path = v071(&d);
        let original = std::fs::read(&path).unwrap();
        let before = ids(&String::from_utf8(original.clone()).unwrap());
        let counts: Vec<usize> = before.iter().map(Vec::len).collect();
        assert_eq!(counts, [2, 4, 3, 5, 9], "the fixture itself");
        let st = load(&path).unwrap().unwrap();
        assert_eq!(st.version, 3);
        save(&path, &st).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], 3);
        assert_eq!(ids(&text), before, "every id kept");
        for k in DROPPED {
            assert!(v.get(k).is_none(), "{k} still saved");
        }
        for t in ["\"task\"", "\"initiative\""] {
            assert!(!text.contains(t), "{t} still on a Tab");
        }
        assert_eq!(
            std::fs::read(d.join("state.json.v2.bak")).unwrap(),
            original,
            "byte-identical backup"
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn r101_d_v2_backup_is_written_once_and_never_for_v3() {
        let d = tmpdir("r101d");
        let path = v071(&d);
        let bak = d.join("state.json.v2.bak");
        std::fs::write(&bak, b"first").unwrap();
        // Another version 2 file: the backup keeps its first bytes.
        let st = load(&path).unwrap().unwrap();
        assert_eq!(std::fs::read(&bak).unwrap(), b"first");
        // A version 3 load writes no backup.
        save(&path, &st).unwrap();
        std::fs::remove_file(&bak).unwrap();
        let again = load(&path).unwrap().unwrap();
        assert_eq!(again.version, 3);
        assert!(!bak.exists(), "no backup for a v3 file");
        std::fs::remove_dir_all(d).unwrap();
    }
}
