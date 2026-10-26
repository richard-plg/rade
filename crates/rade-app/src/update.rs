//! Updates, the controller (PRD R127, ADR-0048, PLAN-updates.md T3): the
//! daily Update check of GitHub Releases, download + SHA-256 + install of
//! the `.deb` on a click, and the restart the owner asks for. The decisions
//! (which release, which phase, every text) are `rade_core::update`; this
//! module carries them out and keeps `Ctl` current. Blocking work (curl,
//! the installed-version query, hashing, the installer) runs off the UI
//! thread through `worktrees::in_background`; every child process drops the
//! app's fds and has a null stdin. The widgets are `update_view`.

use crate::app::{app, try_app};
use crate::update_view;
use gtk::glib;
use gtk4 as gtk;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use rade_core::paths::{self, Flavor};
use rade_core::rpc::{Error, Result};
use rade_core::update::{self as core, Cache, Channel, InstallKind, Offer, Phase, Status, Version};
use rade_core::{AgentStatus, State};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

/// D27: removed from the app's own environment after `read_env`.
pub const ENV_VARS: [&str; 5] = [
    "RADE_UPDATE_CHECK",
    "RADE_UPDATE_URL",
    "RADE_UPDATE_DELAY",
    "RADE_UPDATE_INSTALL",
    "RADE_UPDATE_DPKG_QUERY",
];

/// The hourly tick (D3).
const TICK_S: u64 = 3_600;
/// The download's progress poll (D11).
const POLL: Duration = Duration::from_millis(250);

// -------------------------------------------------------------------- env

/// D4, D27: the five variables, read once before they are scrubbed. All but
/// `RADE_UPDATE_CHECK` count only in the Devel flavor.
struct Env {
    /// `RADE_UPDATE_CHECK=0`.
    check_off: bool,
    url: Option<String>,
    delay: u64,
    install: Option<String>,
    dpkg_query: Option<String>,
}

impl Env {
    fn read() -> Env {
        let devel = Flavor::current() == Flavor::Devel;
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let only_devel = |k: &str| get(k).filter(|_| devel);
        Env {
            check_off: std::env::var("RADE_UPDATE_CHECK").as_deref() == Ok("0"),
            url: only_devel("RADE_UPDATE_URL"),
            delay: only_devel("RADE_UPDATE_DELAY")
                .map(|d| {
                    d.trim()
                        .parse::<i64>()
                        .map_or(60, |n| n.clamp(0, 3_600) as u64)
                })
                .unwrap_or(60),
            install: only_devel("RADE_UPDATE_INSTALL"),
            dpkg_query: only_devel("RADE_UPDATE_DPKG_QUERY"),
        }
    }
}

static ENV: OnceLock<Env> = OnceLock::new();

/// Called by `main` before `scrub_inherited_agent_session_env`.
pub fn read_env() {
    let _ = ENV.set(Env::read());
}

fn env() -> &'static Env {
    ENV.get_or_init(Env::read)
}

// ------------------------------------------------------------- controller

/// A download in flight.
struct Download {
    id: u64,
    child: Child,
    part: PathBuf,
    size: u64,
    bytes: u64,
}

struct Ctl {
    current: Version,
    exe: PathBuf,
    kind: InstallKind,
    installed: Option<Version>,
    phase: Phase,
    offer: Option<Offer>,
    /// Only a copy for the views: writes go through `write_cache`.
    cache: Cache,
    /// A request is in flight (D3: at most one).
    checking: bool,
    download: Option<Download>,
    /// A verified `.deb` kept after the owner cancelled the system dialog.
    deb_ready: Option<(Version, PathBuf)>,
    next_id: u64,
    requests: u32,
    toasts: Vec<String>,
    launched: Vec<String>,
    installer: Option<(Vec<String>, Option<i32>)>,
    /// Activity texts logged this run (D23).
    logged: HashSet<String>,
    pending_log: Vec<String>,
    phases: Vec<&'static str>,
    dialog: Option<adw::AlertDialog>,
}

impl Ctl {
    fn new() -> Ctl {
        Ctl {
            current: Version::parse(env!("CARGO_PKG_VERSION")).expect("a SemVer package version"),
            exe: PathBuf::new(),
            kind: InstallKind::Other,
            installed: None,
            phase: Phase::Idle,
            offer: None,
            cache: Cache::default(),
            checking: false,
            download: None,
            deb_ready: None,
            next_id: 1,
            requests: 0,
            toasts: Vec::new(),
            launched: Vec::new(),
            installer: None,
            logged: HashSet::new(),
            pending_log: Vec::new(),
            phases: Vec::new(),
            dialog: None,
        }
    }

    /// D10, D14: the install kind from what is known now.
    fn work_out_kind(&mut self) {
        let e = env();
        self.kind = core::install_kind(
            Flavor::current() == Flavor::Devel,
            &self.exe,
            self.installed.as_ref(),
            e.install.is_some(),
            e.url.is_some(),
            std::env::consts::ARCH,
        );
    }

    fn set_phase(&mut self, p: Phase) {
        let name = p.as_str();
        if self.phases.last() != Some(&name) {
            self.phases.push(name);
        }
        if let Some(t) = core::activity_text(&p, self.offer.as_ref()) {
            if self.logged.insert(t.clone()) {
                self.pending_log.push(t);
            }
        }
        self.phase = p;
    }

    /// D15: the phase and offer the cache implies (`devel` first, then
    /// `off`, then the rest).
    fn settle(&mut self, channel: Channel) {
        let (phase, offer) = if self.kind == InstallKind::Devel {
            (Phase::Devel, None)
        } else if env().check_off {
            (Phase::Off, None)
        } else {
            core::phase_after(
                &self.cache,
                &self.current,
                self.installed.as_ref(),
                self.kind,
                channel,
            )
        };
        self.offer = offer;
        self.set_phase(phase);
    }

    /// `settle`, unless a download, an install, a failure or a request owns
    /// the phase.
    fn refresh(&mut self, channel: Channel) {
        if matches!(
            self.phase,
            Phase::Idle | Phase::Available | Phase::RestartPending(_) | Phase::Off | Phase::Devel
        ) {
            self.settle(channel);
        }
    }

    /// D3: no check while a download, an install or a restart is pending.
    fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading { .. }
                | Phase::Verifying
                | Phase::Installing
                | Phase::RestartPending(_)
        )
    }

    /// A download, its check or the installer is running.
    fn busy_installing(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading { .. } | Phase::Verifying | Phase::Installing
        )
    }

    /// The version a restart would start (D14, D15).
    fn pending_version(&self) -> Option<Version> {
        match &self.phase {
            Phase::RestartPending(v) => Some(v.clone()),
            _ => self
                .installed
                .clone()
                .filter(|i| self.kind == InstallKind::Deb && *i > self.current),
        }
    }

    fn status(&self, channel: Channel, auto: bool) -> Status {
        Status {
            phase: self.phase.clone(),
            current: self.current.clone(),
            installed: self.installed.clone(),
            install: self.kind,
            checked_at: self.cache.checked_at,
            error: self.cache.error.clone(),
            offer: self.offer.clone(),
            channel,
            auto,
            now: crate::panes::now(),
        }
    }
}

thread_local! {
    static CTL: RefCell<Ctl> = RefCell::new(Ctl::new());
}

fn with<R>(f: impl FnOnce(&mut Ctl) -> R) -> R {
    CTL.with(|c| f(&mut c.borrow_mut()))
}

/// `([update] channel, [update] check)`.
fn cfg() -> (Channel, bool) {
    try_app().map_or((Channel::Stable, true), |a| {
        let c = a.config.borrow();
        (c.update.channel.into(), c.update.check)
    })
}

fn dir() -> PathBuf {
    paths::update_dir(&paths::process_env)
}

/// D9: load → change the writer's own fields → save, off the UI thread
/// (writes are serialized by the cache's flock). The views' copy gets the
/// same change at once, so the result of the write is not applied again.
fn write_cache(f: impl Fn(&mut Cache) + Send + 'static) {
    with(|x| f(&mut x.cache));
    let base = env().url.clone();
    let dir = dir();
    crate::worktrees::in_background(
        move || Cache::update(&dir, base.as_deref(), f),
        |r| {
            if let Err(e) = r {
                eprintln!("rade: update cache: {e}");
            }
        },
    );
}

/// The cache file, read through the validation of `Cache::load`.
fn load_cache() -> Cache {
    Cache::load(&dir(), env().url.as_deref())
}

/// Activity rows queued by phase changes, then the widgets.
fn finish() {
    let logs = with(|c| std::mem::take(&mut c.pending_log));
    for t in logs {
        crate::activity_view::push(rade_core::activity::Event::update(&t, crate::panes::now()));
    }
    render();
}

/// The cell and the About block from the controller's state (in place).
pub fn render() {
    let Some(app) = try_app() else { return };
    let (channel, auto) = cfg();
    let (cell, about) = CTL.with(|c| {
        let s = c.borrow().status(channel, auto);
        (core::cell(&s), core::about(&s))
    });
    app.chrome.update_cell().set(cell);
    update_view::render_about(&about);
}

// ------------------------------------------------------------- child processes

/// A child of the update code: no inherited fds, stdin null.
fn command(prog: &str, args: &[String]) -> Command {
    let mut cmd = Command::new(prog);
    cmd.args(args).stdin(Stdio::null());
    // SAFETY: `no_inherit` is a plain syscall (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            rade_core::process::no_inherit();
            Ok(())
        });
    }
    cmd
}

/// D14: the installed version (blocking; run it off the UI thread).
fn query_installed() -> Option<Version> {
    let mut cmd = match &env().dpkg_query {
        Some(q) => command("sh", &["-c".into(), q.clone()]),
        None => command(
            "dpkg-query",
            &[
                "-W".into(),
                "-f=${db:Status-Abbrev}${Version}".into(),
                "rade".into(),
            ],
        ),
    };
    let out = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    core::installed_from_query(&String::from_utf8_lossy(&out.stdout))
}

// -------------------------------------------------------------------- start

/// F7: leftovers of an earlier run (`rade_*.deb`, `*.part`) go; nothing is
/// ready at start, so none is kept.
fn clean_dir() {
    let Ok(rd) = std::fs::read_dir(dir()) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if (name.starts_with("rade_") && name.ends_with(".deb")) || name.ends_with(".part") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// On shutdown: a running download is killed and its `.part` removed.
pub fn shutdown() {
    if let Some(mut d) = with(|c| c.download.take()) {
        let _ = d.child.kill();
        let _ = d.child.wait();
        let _ = std::fs::remove_file(&d.part);
    }
}

/// Called at the end of `build`: read the cache, settle the phase, tell
/// about a finished update, schedule the first check and the hourly tick.
pub fn start() {
    let (channel, _) = cfg();
    clean_dir();
    let exe = core::restart_exe(&std::env::current_exe().unwrap_or_default());
    let cache = load_cache();
    let just_installed = cache.just_installed.clone();
    with(|c| {
        c.cache = cache;
        c.exe = exe;
        c.work_out_kind();
        c.settle(channel);
    });
    if let Some(v) = just_installed {
        // Cleared at every start, whatever its value (D17).
        write_cache(move |c| c.just_installed = None);
        let current = with(|c| c.current.clone());
        if Version::parse(&v).as_ref() == Some(&current) {
            toast(
                &format!("Updated to RADE {current}"),
                Some((
                    "What's new",
                    Box::new(move || {
                        launch(&format!(
                            "https://github.com/richard-plg/rade/releases/tag/v{current}"
                        ))
                    }),
                )),
            );
        }
    }
    finish();
    let e = env();
    if with(|c| c.kind) == InstallKind::Devel || e.check_off {
        return;
    }
    // The installed-version query decides `deb` and a hand install (D14).
    crate::worktrees::in_background(query_installed, |installed| {
        let (channel, _) = cfg();
        with(|c| {
            c.installed = installed;
            c.work_out_kind();
            c.refresh(channel);
        });
        finish();
        maybe_toast();
    });
    glib::timeout_add_local_once(Duration::from_secs(e.delay), tick);
    glib::timeout_add_seconds_local(TICK_S as u32, || {
        tick();
        glib::ControlFlow::Continue
    });
}

/// D3: re-read the cache, run the installed-version query, and check when
/// one is due.
pub fn tick() {
    if with(|c| c.kind) == InstallKind::Devel || env().check_off {
        return;
    }
    let cache = load_cache();
    with(|c| c.cache = cache);
    crate::worktrees::in_background(query_installed, |installed| {
        let (channel, auto) = cfg();
        let due = with(|c| {
            c.installed = installed;
            c.work_out_kind();
            c.refresh(channel);
            auto && !c.busy() && !c.checking && core::due(&c.cache, crate::panes::now())
        });
        finish();
        maybe_toast();
        if due {
            check(false);
        }
    });
}

/// A config change (`[update]`): recompute from the cache, no request.
pub fn config_changed() {
    let (channel, _) = cfg();
    with(|c| c.refresh(channel));
    finish();
}

// -------------------------------------------------------------------- check

/// Check now (Settings, palette, `debug.update`): always one request.
pub fn check_now() {
    check(true);
}

fn check(manual: bool) {
    let refused = with(|c| {
        if c.kind == InstallKind::Devel {
            return Some(Some("Update checks are off for this build"));
        }
        if env().check_off {
            return Some(Some("Update checks are off (RADE_UPDATE_CHECK=0)"));
        }
        match c.phase {
            Phase::RestartPending(_) => {
                return Some(Some("Restart RADE to finish the update first"))
            }
            Phase::Downloading { .. } | Phase::Verifying | Phase::Installing => {
                return Some(Some("An update is already in progress"))
            }
            _ => {}
        }
        c.checking.then_some(None)
    });
    if let Some(msg) = refused {
        if let (true, Some(m)) = (manual, msg) {
            toast(m, None);
        }
        return;
    }
    let ua = with(|c| {
        c.checking = true;
        c.requests += 1;
        c.set_phase(Phase::Checking);
        format!("rade/{}", c.current)
    });
    finish();
    let url = env().url.clone();
    crate::worktrees::in_background(
        move || {
            let target = url.as_deref().unwrap_or(core::REPO_API);
            let res = match core::curl_get(target, &ua, false, url.is_some()) {
                Ok((200, body)) => core::parse_releases(&body, url.as_deref()),
                Ok((status, body)) => Err(core::check_error(status, &body)),
                Err(e) => Err(e),
            };
            (res, query_installed())
        },
        |(res, installed)| {
            let now = crate::panes::now();
            write_cache(move |c| {
                c.checked_at = Some(now);
                match &res {
                    Ok(r) => {
                        c.releases = r.clone();
                        c.error = None;
                    }
                    Err(e) => c.error = Some(e.clone()),
                }
            });
            let (channel, _) = cfg();
            with(|c| {
                c.checking = false;
                c.installed = installed;
                c.work_out_kind();
                c.settle(channel);
            });
            finish();
            maybe_toast();
        },
    );
}

// ------------------------------------------------------------------ install

fn fail(e: String) {
    with(|c| {
        c.download = None;
        c.set_phase(Phase::Failed(e));
    });
    finish();
}

/// Install <v> / Try again (D11): only for a `deb` install and an offer.
pub fn install() {
    let go = with(|c| {
        c.kind == InstallKind::Deb
            && matches!(c.phase, Phase::Available | Phase::Failed(_))
            && c.offer.is_some()
    });
    if !go {
        return;
    }
    let (offer, ready) = with(|c| (c.offer.clone().expect("checked"), c.deb_ready.clone()));
    let dir = dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        fail(format!("Couldn't create {}: {e}", dir.display()));
        return;
    }
    // The .deb a cancelled system dialog left: verified again, not downloaded.
    if let Some((v, path)) = ready {
        if v == offer.release.version && path.is_file() {
            verify(offer, path.clone(), path);
            return;
        }
    }
    let deb = dir.join(&offer.release.deb.name);
    let part = dir.join(format!("{}.part", offer.release.deb.name));
    let _ = std::fs::remove_file(&part);
    let e = env();
    let mut args: Vec<String> = [
        "-q",
        "-sS",
        "-L",
        "--fail",
        "--noproxy",
        "127.0.0.1,localhost,::1",
    ]
    .map(String::from)
    .into();
    args.extend(["--connect-timeout", "15", "--max-time", "900"].map(String::from));
    if e.url.is_none() {
        args.extend(["--proto", "=https", "--proto-redir", "=https"].map(String::from));
    }
    args.extend([
        "-H".into(),
        format!("User-Agent: rade/{}", with(|c| c.current.clone())),
        "-o".into(),
        part.to_string_lossy().into_owned(),
        "--".into(),
        offer.release.deb.url.clone(),
    ]);
    let child = command("curl", &args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => {
            fail(format!("Download failed: couldn't run curl: {e}"));
            return;
        }
    };
    let id = with(|c| {
        let id = c.next_id;
        c.next_id += 1;
        c.download = Some(Download {
            id,
            child,
            part,
            size: offer.release.deb.size,
            bytes: 0,
        });
        c.set_phase(Phase::Downloading { pct: 0, bytes: 0 });
        id
    });
    finish();
    // `deb` is where the part goes once it is verified.
    glib::timeout_add_local(POLL, move || poll(id, &offer, &deb));
}

/// One 250 ms tick of the download.
fn poll(id: u64, offer: &Offer, deb: &Path) -> glib::ControlFlow {
    enum Step {
        Gone,
        Running,
        Done(Option<i32>, String, PathBuf),
    }
    let step = with(|c| {
        let Some(d) = c.download.as_mut().filter(|d| d.id == id) else {
            return Step::Gone;
        };
        match d.child.try_wait() {
            Ok(None) => {
                d.bytes = std::fs::metadata(&d.part).map_or(0, |m| m.len());
                let pct = d
                    .bytes
                    .saturating_mul(100)
                    .checked_div(d.size)
                    .map_or(0, |p| p.min(100) as u8);
                let phase = Phase::Downloading {
                    pct,
                    bytes: d.bytes,
                };
                if c.phase != phase {
                    c.set_phase(phase);
                }
                Step::Running
            }
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut e) = d.child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                let part = d.part.clone();
                c.download = None;
                Step::Done(status.code(), err, part)
            }
            Err(e) => {
                let part = d.part.clone();
                c.download = None;
                Step::Done(None, e.to_string(), part)
            }
        }
    });
    match step {
        Step::Gone => glib::ControlFlow::Break,
        Step::Running => {
            render();
            glib::ControlFlow::Continue
        }
        Step::Done(Some(0), _, part) => {
            verify(offer.clone(), part, deb.to_path_buf());
            glib::ControlFlow::Break
        }
        Step::Done(_, err, part) => {
            let _ = std::fs::remove_file(part);
            let line = err
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map_or_else(|| "curl failed".to_string(), |l| l.trim().to_string());
            fail(format!("Download failed: {line}"));
            glib::ControlFlow::Break
        }
    }
}

/// D11 (2)–(3): fetch `SHA256SUMS`, check the file, rename `from` to `to`
/// (the same path when the `.deb` was kept), then install.
fn verify(offer: Offer, from: PathBuf, to: PathBuf) {
    with(|c| c.set_phase(Phase::Verifying));
    finish();
    let ua = format!("rade/{}", with(|c| c.current.clone()));
    let over = env().url.is_some();
    crate::worktrees::in_background(
        move || -> std::result::Result<(PathBuf, String), String> {
            let sums = match core::curl_get(&offer.release.sums.url, &ua, true, over) {
                Ok((200, body)) => body,
                Ok((status, body)) => {
                    return Err(format!(
                        "Couldn't get SHA256SUMS: {}",
                        core::check_error(status, &body)
                    ))
                }
                Err(e) => return Err(format!("Couldn't get SHA256SUMS: {e}")),
            };
            if let Err(e) = core::verify(&from, &sums, &offer.release.deb) {
                let _ = std::fs::remove_file(&from);
                return Err(e);
            }
            if from != to {
                std::fs::rename(&from, &to)
                    .map_err(|e| format!("Couldn't keep the download: {e}"))?;
            }
            // `verify` found this entry equal to the file's hash.
            let sha = core::sums_entry(&sums, &offer.release.deb.name).unwrap_or_default();
            Ok((to, sha))
        },
        |r| match r {
            Ok((deb, sha)) => run_installer(deb, &sha),
            Err(e) => {
                with(|c| c.deb_ready = None);
                fail(e)
            }
        },
    );
}

/// D11 (4)–(5), D12: the installer, off the UI thread.
fn run_installer(deb: PathBuf, sha256: &str) {
    let argv = core::install_argv(&deb, sha256, env().install.as_deref());
    with(|c| {
        c.installer = Some((argv.clone(), None));
        c.set_phase(Phase::Installing);
    });
    finish();
    crate::worktrees::in_background(
        move || -> std::result::Result<(i32, String), String> {
            let out = command(&argv[0], &argv[1..])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("Couldn't start the installer: {e}"))?;
            Ok((
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ))
        },
        move |r| match r {
            Err(e) => fail(e),
            Ok((code, stderr)) => {
                with(|c| {
                    if let Some(i) = c.installer.as_mut() {
                        i.1 = Some(code);
                    }
                });
                installed_with(core::install_outcome(code, &stderr), deb);
            }
        },
    );
}

fn installed_with(outcome: core::InstallOutcome, deb: PathBuf) {
    let (channel, _) = cfg();
    match outcome {
        core::InstallOutcome::Done => {
            let _ = std::fs::remove_file(&deb);
            let version = with(|c| {
                c.deb_ready = None;
                c.offer.as_ref().map(|o| o.release.version.to_string())
            });
            write_cache(move |c| c.just_installed = version.clone());
            // The installed version decides the next phase (D14, D15).
            crate::worktrees::in_background(query_installed, |installed| {
                let (channel, _) = cfg();
                with(|c| {
                    c.installed = installed;
                    c.work_out_kind();
                    c.settle(channel);
                });
                finish();
            });
        }
        core::InstallOutcome::Cancelled => {
            // The owner closed the system dialog: back to Install, no error.
            with(|c| {
                c.deb_ready = c.offer.as_ref().map(|o| (o.release.version.clone(), deb));
                c.settle(channel);
            });
            finish();
        }
        core::InstallOutcome::Failed(e) => {
            let _ = std::fs::remove_file(&deb);
            with(|c| c.deb_ready = None);
            fail(e);
        }
    }
}

/// Cancel (downloading only): kill curl, drop the part, back to available.
pub fn cancel() {
    let Some(mut d) = with(|c| c.download.take()) else {
        return;
    };
    let _ = d.child.kill();
    let _ = d.child.wait();
    let _ = std::fs::remove_file(&d.part);
    let (channel, _) = cfg();
    with(|c| c.settle(channel));
    finish();
}

/// Skip This Version (D22): quiet until a higher release comes out.
pub fn skip() {
    let version = with(|c| {
        (matches!(c.phase, Phase::Available | Phase::Failed(_)))
            .then(|| c.offer.as_ref().map(|o| o.release.version.to_string()))
            .flatten()
    });
    let Some(v) = version else { return };
    write_cache(move |c| c.skipped = Some(v.clone()));
    let (channel, _) = cfg();
    with(|c| c.settle(channel));
    finish();
}

/// Download (a `.deb`-less install): the release page.
pub fn download() {
    if let Some(url) = with(|c| c.offer.as_ref().map(|o| o.release.html_url.clone())) {
        launch(&url);
    }
}

// ------------------------------------------------------------- Toast, URIs

/// A Toast raised by the update code (recorded for `debug.update`).
fn toast(text: &str, button: Option<crate::chrome::ToastButton>) {
    with(|c| c.toasts.push(text.to_string()));
    if let Some(app) = try_app() {
        app.chrome.toast(&glib::markup_escape_text(text), button);
    }
}

/// D17: every URI the update code opens; in test mode it only records it.
pub fn launch(uri: &str) {
    if crate::handlers::DEBUG_SOCKET
        .get()
        .copied()
        .unwrap_or(false)
    {
        with(|c| c.launched.push(uri.to_string()));
    } else {
        crate::viewer::launch_uri(uri);
    }
}

/// D17: the security Toast, once per version, held while an Agent waits.
pub fn maybe_toast_in(ws: &State) {
    let Some((text, version)) = with(|c| {
        if c.phase != Phase::Available {
            return None;
        }
        let o = c.offer.as_ref().filter(|o| !o.skipped)?;
        let v = o.release.version.to_string();
        if c.cache.toasted.as_deref() == Some(v.as_str()) {
            return None;
        }
        Some((core::toast_text(o)?, v))
    }) else {
        return;
    };
    if !ws.waiting_panes().is_empty() {
        return;
    }
    write_cache(move |c| c.toasted = Some(version.clone()));
    toast(
        &text,
        Some((
            "Details",
            Box::new(|| crate::settings_view::open_at("Updates")),
        )),
    );
}

/// [`maybe_toast_in`] on the app's model (skipped while it is borrowed:
/// the model update that holds it calls `maybe_toast_in` itself).
pub fn maybe_toast() {
    let Some(app) = try_app() else { return };
    let model = app.model.try_borrow();
    if let Ok(m) = model {
        maybe_toast_in(&m);
    }
}

// ------------------------------------------------------------------ restart

/// The Restart dialog (D20); a Toast when nothing waits for a restart.
pub fn restart_dialog() {
    if with(|c| c.busy_installing()) {
        toast(
            "An update is being installed: restart when it is done",
            None,
        );
        return;
    }
    let Some(version) = with(|c| c.pending_version()) else {
        toast("No update is waiting for a restart", None);
        return;
    };
    if with(|c| c.dialog.is_some()) {
        return;
    }
    let app = app();
    let facts = rade_core::overview::facts(&app.model.borrow(), &|_| String::new());
    let body = core::restart_body(&facts, &version);
    let running = facts.iter().any(|f| f.status == Some(AgentStatus::Running));
    let d = update_view::restart_dialog(&body, running, |id| answer(id, false));
    d.present(Some(&app.window));
    with(|c| c.dialog = Some(d));
}

/// The dialog's answer (`not_now` | `restart`); `close` also closes it
/// (`debug.update respond`).
fn answer(id: &str, close: bool) {
    if let Some(d) = with(|c| c.dialog.take()) {
        if close {
            d.force_close();
        }
    }
    if id == "restart" {
        glib::idle_add_local_once(restart);
    }
}

/// D21: the helper that starts the new binary once this process is gone,
/// then the normal quit.
fn restart() {
    let exe = core::restart_exe(&std::env::current_exe().unwrap_or_default());
    if !exe.is_file() {
        toast(
            &format!("Can't find the new RADE at {}", exe.display()),
            None,
        );
        return;
    }
    let lock = app().paths.state.with_extension("json.lock");
    if let Err(e) = core::spawn_restart(std::process::id(), &exe, &lock) {
        toast(&format!("Couldn't restart: {e}"), None);
        return;
    }
    app().quit();
}

// -------------------------------------------------------------- views' clicks

/// A click on the Statusline cell (D16).
pub fn cell_clicked() {
    if matches!(with(|c| c.phase.clone()), Phase::RestartPending(_)) {
        restart_dialog();
    } else {
        crate::settings_view::open_at("Updates");
    }
}

/// A button of the About block (§5.1), by its label.
pub fn about_click(label: &str) {
    match label {
        "Check now" => check_now(),
        "Skip This Version" => skip(),
        "Cancel" => cancel(),
        "Restart…" => restart_dialog(),
        "Release notes ›" => notes(),
        "Download" => download(),
        "Try again" => install(),
        l if l.starts_with("Install ") => install(),
        _ => {}
    }
}

/// Release notes › (D19): the notes in the Markdown Viewer, Settings closed.
fn notes() {
    let Some((version, text)) = with(|c| {
        c.offer
            .as_ref()
            .map(|o| (o.release.version.to_string(), o.notes.clone()))
    }) else {
        return;
    };
    let dir = dir();
    let path = dir.join(format!("notes-{version}.md"));
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, text)) {
        toast(&format!("Couldn't write the release notes: {e}"), None);
        return;
    }
    let app = app();
    let opened = app.update(|m| {
        let (s, w) = m.active();
        let (s, w) = (s.id.clone(), w.id.clone());
        m.open_viewer_at(&s, &w, &path, None, true, false)
    });
    if let Err(e) = opened {
        eprintln!("rade: release notes: {e}");
    }
    crate::settings_view::close();
}

// -------------------------------------------------------------------- socket

/// `app.info` (D25).
pub fn info() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "flavor": Flavor::current().name(),
        "update": with(|c| c.phase.as_str()),
    })
}

fn debug_info() -> Value {
    let (channel, auto) = cfg();
    let cell = try_app().and_then(|a| {
        let uc = a.chrome.update_cell();
        let last = uc.last.borrow();
        let c = last.as_ref()?;
        Some(json!({
            "text": c.text,
            "icon": c.icon,
            "classes": uc.button.css_classes().iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "tooltip": c.tooltip,
        }))
    });
    CTL.with(|c| {
        let c = c.borrow();
        let s = c.status(channel, auto);
        let about = core::about(&s);
        json!({
            "phase": c.phase.as_str(),
            "current": c.current.to_string(),
            "install": c.kind.as_str(),
            "enabled": auto && !env().check_off && c.kind != InstallKind::Devel,
            "checked_at": c.cache.checked_at,
            "error": c.cache.error,
            "installed": c.installed.as_ref().map(|v| v.to_string()),
            "offer": c.offer.as_ref().map(|o| json!({
                "version": o.release.version.to_string(),
                "kind": o.kind.as_str(),
                "security": o.security,
                "skipped": o.skipped,
                "html_url": o.release.html_url,
            })),
            "phases": c.phases,
            "cell": cell,
            "about": {
                "status": about.status,
                "status_class": about.status_class,
                "sub": about.sub,
                "buttons": about.buttons,
                "notes": about.notes,
            },
            "toasts": c.toasts,
            "launched": c.launched,
            "requests": c.requests,
            "download": c.download.as_ref().map(|d| json!({
                "path": d.part.to_string_lossy(),
                "bytes": d.bytes,
                "size": d.size,
            })),
            "installer": c.installer.as_ref().map(|(argv, exit)| json!({"argv": argv, "exit": exit})),
        })
    })
}

/// `debug.update {"op", "arg"?}` (test mode, §6): every op answers `info`
/// afterwards, except `dialog`.
pub fn debug(p: &Value) -> Result<Value> {
    let op = p.get("op").and_then(Value::as_str).unwrap_or("info");
    let arg = p.get("arg");
    match op {
        "info" => {}
        "age" => {
            let secs = arg
                .and_then(|a| a.as_u64().or_else(|| a.as_str()?.parse().ok()))
                .ok_or_else(|| Error::usage("want arg = seconds"))?;
            write_cache(move |c| c.checked_at = c.checked_at.map(|at| at.saturating_sub(secs)));
            let (channel, _) = cfg();
            with(|c| c.refresh(channel));
            finish();
        }
        "tick" => tick(),
        "check_now" => check_now(),
        "install" => install(),
        "cancel" => cancel(),
        "skip" => skip(),
        "cell_click" => cell_clicked(),
        "restart_dialog" => restart_dialog(),
        "dialog" => {
            return Ok(match with(|c| c.dialog.clone()) {
                Some(d) => json!({
                    "open": true,
                    "heading": d.heading().map(|h| h.to_string()),
                    "body": d.body().to_string(),
                    "responses": ["not_now", "restart"],
                }),
                None => json!({"open": false, "heading": null, "body": null, "responses": []}),
            })
        }
        "respond" => {
            let id = arg.and_then(Value::as_str).unwrap_or("");
            if !matches!(id, "not_now" | "restart") {
                return Err(Error::usage("arg must be not_now or restart"));
            }
            if with(|c| c.dialog.is_none()) {
                return Err(Error::usage("the Restart dialog is not open"));
            }
            answer(id, true);
        }
        "notes" => notes(),
        "download" => download(),
        "about_click" => {
            let label = arg
                .and_then(Value::as_str)
                .ok_or_else(|| Error::usage("want arg = a button label"))?;
            let (channel, auto) = cfg();
            let known = with(|c| core::about(&c.status(channel, auto)).buttons)
                .iter()
                .any(|b| b == label)
                || label == "Release notes ›";
            if !known {
                return Err(Error::usage(format!("no button '{label}'")));
            }
            about_click(label);
        }
        op => return Err(Error::usage(format!("unknown op '{op}'"))),
    }
    Ok(debug_info())
}
