//! Updates (PRD R127, ADR-0048): a newer RADE release on GitHub Releases
//! that RADE found and can install. Pure logic — versions, which release to
//! offer, the cache shared by the app and `rade version`, SHA-256
//! verification, the install and restart commands, and the texts of the
//! Statusline cell and the About block. The app (`rade-app/src/update.rs`)
//! only moves bytes and widgets.

use crate::overview::Fact;
use crate::AgentStatus;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const REPO_API: &str = "https://api.github.com/repos/richard-plg/rade/releases?per_page=20";
pub const DOWNLOAD_PREFIX: &str = "https://github.com/richard-plg/rade/releases/download/";
/// A good check is repeated after this many seconds.
pub const CHECK_EVERY_S: u64 = 86_400;
/// A failed check is repeated after this many seconds.
pub const RETRY_AFTER_S: u64 = 3_600;
/// Where the .deb puts the app.
pub const INSTALLED_EXE: &str = "/usr/lib/rade/rade-app";
pub const PRIVACY: &str = "RADE asks api.github.com for the release list once a day. It sends its version in the User-Agent and nothing else.";

const ICON_UP: &str = "ph-arrow-circle-up-symbolic";
const ICON_RESTART: &str = "ph-arrow-clockwise-symbolic";
const ICON_WARN: &str = "ph-warning-diamond-symbolic";

// ---------------------------------------------------------------- versions

/// One dot-separated identifier of a pre-release part (SemVer 2.0.0 §11:
/// numeric < alphanumeric, numeric by value).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ident {
    Num(u64),
    Alpha(String),
}

/// `MAJOR.MINOR.PATCH[-PRE]`; build metadata is dropped (it never orders).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<Ident>,
}

impl Version {
    /// `1.1.0`, `1.1.0-rc.1`, `1.1.0+build`; anything else is `None`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.split_once('+').map_or(s, |(a, _)| a);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let mut nums = core.split('.');
        let mut num = || -> Option<u64> {
            let p = nums.next()?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let (major, minor, patch) = (num()?, num()?, num()?);
        if nums.next().is_some() {
            return None;
        }
        let mut idents = Vec::new();
        if let Some(pre) = pre {
            for id in pre.split('.') {
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return None;
                }
                idents.push(if id.bytes().all(|b| b.is_ascii_digit()) {
                    Ident::Num(id.parse().ok()?)
                } else {
                    Ident::Alpha(id.to_string())
                });
            }
        }
        Some(Version {
            major,
            minor,
            patch,
            pre: idents,
        })
    }

    /// A tag: `v1.1.0` (the `v` is required).
    pub fn from_tag(t: &str) -> Option<Version> {
        Version::parse(t.strip_prefix('v')?)
    }

    /// A dpkg version: the first `~` is the `-` of SemVer.
    pub fn from_deb(s: &str) -> Option<Version> {
        Version::parse(&s.trim().replacen('~', "-", 1))
    }

    /// `rade_1.1.0~rc.1_amd64.deb`: the first `-` becomes `~`, as
    /// `scripts/version.sh` and `ci/publish-release.sh` do.
    pub fn deb_name(&self) -> String {
        format!("rade_{}_amd64.deb", self.to_string().replacen('-', "~", 1))
    }

    pub fn is_pre(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        for (i, id) in self.pre.iter().enumerate() {
            f.write_str(if i == 0 { "-" } else { "." })?;
            match id {
                Ident::Num(n) => write!(f, "{n}")?,
                Ident::Alpha(a) => f.write_str(a)?,
            }
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Version) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&o.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Version) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Version, D::Error> {
        let s = String::deserialize(d)?;
        Version::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a version: {s}")))
    }
}

/// How far an offer is from the running version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Major,
    Minor,
    Patch,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Major => "major",
            Kind::Minor => "minor",
            Kind::Patch => "patch",
        }
    }
}

/// Which releases an Update may offer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Channel {
    #[default]
    Stable,
    Rc,
}

impl From<crate::config::UpdateChannel> for Channel {
    fn from(c: crate::config::UpdateChannel) -> Channel {
        match c {
            crate::config::UpdateChannel::Stable => Channel::Stable,
            crate::config::UpdateChannel::Rc => Channel::Rc,
        }
    }
}

// ---------------------------------------------------------------- releases

/// A release asset. `digest` is the hex SHA-256 without `sha256:`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub digest: Option<String>,
}

/// A release RADE could install: not a draft, with its `.deb` and
/// `SHA256SUMS` on the allowed URLs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    /// `YYYY-MM-DD`.
    pub published: String,
    pub html_url: String,
    pub body: String,
    pub prerelease: bool,
    pub deb: Asset,
    pub sums: Asset,
}

/// `scheme://host[:port]/` of the override URL.
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}/"))
}

/// D13: without the override, exactly the GitHub download URL of the tag;
/// with it, anything on the override URL's origin.
pub fn asset_allowed(url: &str, tag: &str, name: &str, base: Option<&str>) -> bool {
    match base {
        None => url == format!("{DOWNLOAD_PREFIX}{tag}/{name}"),
        Some(b) => origin_of(b).is_some_and(|o| url.starts_with(&o)),
    }
}

fn asset_of(v: &serde_json::Value, name: &str, tag: &str, base: Option<&str>) -> Option<Asset> {
    let a = v
        .get("assets")?
        .as_array()?
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))?;
    let url = a.get("browser_download_url")?.as_str()?;
    if !asset_allowed(url, tag, name, base) {
        return None;
    }
    Some(Asset {
        name: name.to_string(),
        url: url.to_string(),
        size: a.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
        digest: a
            .get("digest")
            .and_then(|d| d.as_str())
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|d| d.to_lowercase()),
    })
}

/// The candidates among GitHub's `releases` answer (D5 except the channel,
/// D13): not a draft, a `v<semver>` tag, and both assets on allowed URLs.
pub fn parse_releases(json: &str, base: Option<&str>) -> Result<Vec<Release>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("GitHub sent something else: {e}"))?;
    let list = v
        .as_array()
        .ok_or_else(|| "GitHub sent something else than a release list".to_string())?;
    let mut out = Vec::new();
    for r in list {
        if r.get("draft").and_then(|d| d.as_bool()).unwrap_or(true) {
            continue;
        }
        let Some(tag) = r.get("tag_name").and_then(|t| t.as_str()) else {
            continue;
        };
        let Some(version) = Version::from_tag(tag) else {
            continue;
        };
        let Some(deb) = asset_of(r, &version.deb_name(), tag, base) else {
            continue;
        };
        let Some(sums) = asset_of(r, "SHA256SUMS", tag, base) else {
            continue;
        };
        let text = |k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        out.push(Release {
            version,
            tag: tag.to_string(),
            published: text("published_at").chars().take(10).collect(),
            html_url: text("html_url"),
            body: text("body"),
            prerelease: r
                .get("prerelease")
                .and_then(|p| p.as_bool())
                .unwrap_or(false),
            deb,
            sums,
        });
    }
    Ok(out)
}

/// The release RADE offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub release: Release,
    pub kind: Kind,
    pub security: bool,
    pub skipped: bool,
    pub notes: String,
}

fn channel_allows(r: &Release, channel: Channel) -> bool {
    channel == Channel::Rc || (!r.prerelease && !r.version.is_pre())
}

/// D5, D7: the highest candidate of the channel above `floor`; its kind,
/// security flag and notes are worked out against `current`.
pub fn pick(
    releases: &[Release],
    floor: &Version,
    current: &Version,
    channel: Channel,
    skipped: Option<&str>,
) -> Option<Offer> {
    let release = releases
        .iter()
        .filter(|r| channel_allows(r, channel) && r.version > *floor)
        .max_by(|a, b| a.version.cmp(&b.version))?;
    let kind = if release.version.major > current.major {
        Kind::Major
    } else if release.version.minor > current.minor {
        Kind::Minor
    } else {
        Kind::Patch
    };
    let mut between: Vec<&Release> = releases
        .iter()
        .filter(|r| {
            channel_allows(r, channel) && r.version > *current && r.version <= release.version
        })
        .collect();
    between.sort_by(|a, b| b.version.cmp(&a.version));
    let security = between
        .iter()
        .any(|r| r.body.lines().any(|l| l.trim_end() == "### Security"));
    let mut notes = format!("# What's new since RADE {current}\n");
    for r in &between {
        let body: Vec<&str> = r
            .body
            .lines()
            .filter(|l| !l.starts_with("Install: "))
            .collect();
        notes.push_str(&format!(
            "\n## RADE {} · {}\n\n{}\n",
            r.version,
            r.published,
            body.join("\n").trim()
        ));
    }
    Some(Offer {
        kind,
        security,
        skipped: skipped.and_then(Version::parse).as_ref() == Some(&release.version),
        notes,
        release: release.clone(),
    })
}

// ------------------------------------------------------------------- cache

/// `update.json` (D9): shared by the app and `rade version`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cache {
    /// Unix seconds; `None`: never checked.
    pub checked_at: Option<u64>,
    pub error: Option<String>,
    pub releases: Vec<Release>,
    pub skipped: Option<String>,
    pub toasted: Option<String>,
    pub just_installed: Option<String>,
}

impl Cache {
    /// A missing or unparsable file is an empty cache.
    pub fn load(dir: &Path) -> Cache {
        std::fs::read_to_string(dir.join("update.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Load, let `f` change its own fields, save atomically
    /// (`update.json.<pid>.tmp` + rename), so two writers never undo each
    /// other. The only write path.
    pub fn update(dir: &Path, f: impl FnOnce(&mut Cache)) -> std::io::Result<Cache> {
        std::fs::create_dir_all(dir)?;
        let mut c = Cache::load(dir);
        f(&mut c);
        let tmp = dir.join(format!("update.json.{}.tmp", std::process::id()));
        let text = serde_json::to_string_pretty(&c).map_err(std::io::Error::other)?;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, dir.join("update.json"))?;
        Ok(c)
    }
}

/// D3: no cache, a day after a good check, an hour after a failed one.
pub fn due(cache: &Cache, now: u64) -> bool {
    let Some(at) = cache.checked_at else {
        return true;
    };
    let wait = if cache.error.is_some() {
        RETRY_AFTER_S
    } else {
        CHECK_EVERY_S
    };
    now.saturating_sub(at) >= wait
}

/// D2: one GET with `curl`, `(status, body)`. The release list uses
/// `follow = false`; `SHA256SUMS` and the `.deb` follow GitHub's redirect.
/// `override_url`: `RADE_UPDATE_URL` is in effect (plain http allowed).
pub fn curl_get(
    url: &str,
    user_agent: &str,
    follow: bool,
    override_url: bool,
) -> Result<(u16, String), String> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("curl");
    cmd.args([
        "-q",
        "-sS",
        "--noproxy",
        "127.0.0.1,localhost,::1",
        "--connect-timeout",
        "15",
        "--max-time",
        "30",
    ]);
    if follow {
        cmd.args(["-L", "--max-redirs", "5"]);
    }
    if !override_url {
        cmd.args(["--proto", "=https", "--proto-redir", "=https"]);
    }
    if !follow {
        cmd.args([
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "X-GitHub-Api-Version: 2022-11-28",
        ]);
    }
    cmd.args(["-H", &format!("User-Agent: {user_agent}")]);
    cmd.args(["-w", "\n%{http_code}", url]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: `no_inherit` is a plain syscall (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            crate::process::no_inherit();
            Ok(())
        });
    }
    let out = cmd
        .output()
        .map_err(|e| format!("Couldn't run curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty());
        return Err(match line {
            Some(l) => l.trim().to_string(),
            None => format!("curl exited with {}", out.status.code().unwrap_or(-1)),
        });
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text
        .rsplit_once('\n')
        .ok_or_else(|| "curl gave no status".to_string())?;
    let code = code
        .trim()
        .parse::<u16>()
        .map_err(|_| "curl gave no status".to_string())?;
    Ok((code, body.to_string()))
}

/// The error text of a check that got `status` (curl errors pass through).
pub fn check_error(status: u16, _body: &str) -> String {
    match status {
        403 | 429 => "GitHub rate limit reached".to_string(),
        s => format!("GitHub answered {s}"),
    }
}

/// `just now` under a minute, else `5m ago`, `2h ago`, `3d ago`.
pub fn ago(secs: u64) -> String {
    if secs < 60 {
        "just now".to_string()
    } else {
        format!("{} ago", crate::design::age(secs))
    }
}

// ------------------------------------------------------------ verification

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The hash of `name` in a `SHA256SUMS` text (`<hex>  <name>` or
/// `<hex> *<name>`).
pub fn sums_entry(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && !hash.is_empty()).then(|| hash.to_lowercase())
    })
}

/// D11 (3): the file's SHA-256 equals its `SHA256SUMS` line and, when the
/// API gave one, the asset's digest.
pub fn verify(path: &Path, sums: &str, asset: &Asset) -> Result<(), String> {
    const BAD: &str = "Download didn't match SHA256SUMS. Nothing was installed.";
    let actual = sha256_file(path).map_err(|e| format!("Couldn't read the download: {e}"))?;
    if sums_entry(sums, &asset.name).as_deref() != Some(actual.as_str()) {
        return Err(BAD.to_string());
    }
    if let Some(d) = &asset.digest {
        if d.to_lowercase() != actual {
            return Err(BAD.to_string());
        }
    }
    Ok(())
}

// ----------------------------------------------------------------- install

/// D10: how an Update can be applied here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    Deb,
    Other,
    Devel,
}

impl InstallKind {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallKind::Deb => "deb",
            InstallKind::Other => "other",
            InstallKind::Devel => "devel",
        }
    }
}

/// D10. The overrides count only in the Devel flavor (D4).
pub fn install_kind(
    devel: bool,
    exe: &Path,
    installed: Option<&Version>,
    override_install: bool,
    override_url: bool,
    arch: &str,
) -> InstallKind {
    if devel && !override_url {
        return InstallKind::Devel;
    }
    if arch != "x86_64" {
        return InstallKind::Other;
    }
    let deb = if devel {
        override_install
    } else {
        exe == Path::new(INSTALLED_EXE) && installed.is_some()
    };
    if deb {
        InstallKind::Deb
    } else {
        InstallKind::Other
    }
}

/// `current_exe()` of a process whose binary was replaced reads
/// `… (deleted)`: the path of the new file.
pub fn restart_exe(current_exe: &Path) -> PathBuf {
    let s = current_exe.to_string_lossy();
    PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s).to_string())
}

/// D12: the install command.
pub fn install_argv(deb: &Path, override_cmd: Option<&str>) -> Vec<String> {
    let deb = deb.to_string_lossy().into_owned();
    match override_cmd {
        Some(cmd) => vec![
            "sh".into(),
            "-c".into(),
            format!("{cmd} \"$1\""),
            "rade-install".into(),
            deb,
        ],
        None => vec![
            "pkexec".into(),
            "/usr/bin/apt-get".into(),
            "install".into(),
            "-y".into(),
            "--no-remove".into(),
            deb,
        ],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    Done,
    /// The owner closed the polkit dialog.
    Cancelled,
    Failed(String),
}

/// D12: what the installer's exit code and stderr mean.
pub fn install_outcome(code: i32, stderr: &str) -> InstallOutcome {
    match code {
        0 => return InstallOutcome::Done,
        126 => return InstallOutcome::Cancelled,
        127 => return InstallOutcome::Failed("Not authorized to install software.".into()),
        _ => {}
    }
    if stderr.contains("Could not get lock") {
        return InstallOutcome::Failed(
            "Another package manager is running. Try again in a minute.".into(),
        );
    }
    match stderr.lines().rev().find(|l| !l.trim().is_empty()) {
        Some(l) => InstallOutcome::Failed(l.trim().chars().take(200).collect()),
        None => InstallOutcome::Failed(format!("apt-get exited with {code}")),
    }
}

const RESTART_SCRIPT: &str = r#"i=0; while [ "$i" -lt 300 ] && [ -e "/proc/$1" ] && [ "$(cut -d" " -f3 "/proc/$1/stat" 2>/dev/null)" != Z ]; do sleep 0.2; i=$((i+1)); done; exec "$2""#;

fn restart_child(pid: u32, exe: &Path) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("sh");
    cmd.args(["-c", RESTART_SCRIPT, "rade-restart", &pid.to_string()])
        .arg(exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: both calls are plain syscalls (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            crate::process::no_inherit();
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn()
}

/// D21: a detached helper that waits for `pid` to go (up to 60 s), then
/// execs `exe`.
pub fn spawn_restart(pid: u32, exe: &Path) -> std::io::Result<()> {
    restart_child(pid, exe).map(|_| ())
}

// ------------------------------------------------------------------ phases

/// D15.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Devel,
    Off,
    Idle,
    Checking,
    Available,
    Downloading { pct: u8, bytes: u64 },
    Verifying,
    Installing,
    RestartPending(Version),
    Failed(String),
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Phase::Devel => "devel",
            Phase::Off => "off",
            Phase::Idle => "idle",
            Phase::Checking => "checking",
            Phase::Available => "available",
            Phase::Downloading { .. } => "downloading",
            Phase::Verifying => "verifying",
            Phase::Installing => "installing",
            Phase::RestartPending(_) => "restart_pending",
            Phase::Failed(_) => "failed",
        }
    }
}

/// D5, D14, D15: the phase (and offer) the cache implies. A `devel` install
/// never offers anything. The offer is computed against the floor: the
/// running version, or for `deb` the higher of running and installed.
pub fn phase_after(
    cache: &Cache,
    current: &Version,
    installed: Option<&Version>,
    kind: InstallKind,
    channel: Channel,
) -> (Phase, Option<Offer>) {
    if kind == InstallKind::Devel {
        return (Phase::Devel, None);
    }
    let installed = installed.filter(|_| kind == InstallKind::Deb);
    let floor = installed.filter(|i| *i > current).unwrap_or(current);
    let offer = pick(
        &cache.releases,
        floor,
        current,
        channel,
        cache.skipped.as_deref(),
    );
    if offer.is_some() {
        return (Phase::Available, offer);
    }
    match installed {
        Some(i) if i > current => (Phase::RestartPending(i.clone()), None),
        _ => (Phase::Idle, None),
    }
}

/// D14: `dpkg-query -W -f='${db:Status-Abbrev}${Version}'` prints `ii `
/// then the version for an installed package (`rc ` = config files only).
pub fn installed_from_query(stdout: &str) -> Option<Version> {
    Version::from_deb(stdout.trim().strip_prefix("ii ")?)
}

// ------------------------------------------------------------------- texts

/// What the views are built from.
#[derive(Clone, Debug)]
pub struct Status {
    pub phase: Phase,
    pub current: Version,
    pub installed: Option<Version>,
    pub install: InstallKind,
    pub checked_at: Option<u64>,
    pub error: Option<String>,
    pub offer: Option<Offer>,
    pub channel: Channel,
    /// `[update] check`.
    pub auto: bool,
    pub now: u64,
}

/// The Statusline cell (D16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellView {
    pub icon: &'static str,
    pub text: String,
    pub class: Option<&'static str>,
    pub tooltip: String,
}

/// D16: `None` when the cell is absent.
pub fn cell(s: &Status) -> Option<CellView> {
    let offer = s.offer.as_ref();
    let available = |o: &Offer| {
        format!(
            "RADE {} is available · click for details",
            o.release.version
        )
    };
    match &s.phase {
        Phase::Available | Phase::Checking => {
            let o = offer.filter(|o| !o.skipped)?;
            let v = &o.release.version;
            let (text, class) = if o.security {
                (format!("{v} security"), Some("wait"))
            } else if o.kind == Kind::Major {
                (format!("{v} major"), Some("wait"))
            } else {
                (v.to_string(), None)
            };
            Some(CellView {
                icon: ICON_UP,
                text,
                class,
                tooltip: available(o),
            })
        }
        Phase::Downloading { pct, .. } => {
            let o = offer?;
            Some(CellView {
                icon: ICON_UP,
                text: format!("{} {pct}%", o.release.version),
                class: None,
                tooltip: available(o),
            })
        }
        Phase::Verifying | Phase::Installing => {
            let o = offer?;
            Some(CellView {
                icon: ICON_UP,
                text: format!("{} installing", o.release.version),
                class: None,
                tooltip: available(o),
            })
        }
        Phase::RestartPending(v) => Some(CellView {
            icon: ICON_RESTART,
            text: format!("restart for {v}"),
            class: Some("acc"),
            tooltip: format!("RADE {v} is installed · click to restart"),
        }),
        Phase::Failed(e) => Some(CellView {
            icon: ICON_WARN,
            text: "update failed".to_string(),
            class: Some("err"),
            tooltip: e.clone(),
        }),
        Phase::Devel | Phase::Off | Phase::Idle => None,
    }
}

/// The About block (§5.1).
#[derive(Clone, Debug, PartialEq)]
pub struct AboutView {
    pub title: String,
    pub status: String,
    pub status_class: Option<&'static str>,
    pub sub: String,
    /// Button labels, primary first.
    pub buttons: Vec<String>,
    pub primary_enabled: bool,
    /// `Release notes ›` is shown.
    pub notes: bool,
    pub progress: Option<f64>,
}

fn mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

fn offer_sub(s: &Status, o: &Offer) -> String {
    let mut sub = if o.security {
        "Fixes a security issue.".to_string()
    } else {
        match o.kind {
            Kind::Major => format!(
                "{} changes the rade CLI, socket or config files: read the notes first, scripts and Agent skills may need changes.",
                o.release.version
            ),
            Kind::Minor => "New features and fixes.".to_string(),
            Kind::Patch => "Fixes.".to_string(),
        }
    };
    if s.install != InstallKind::Deb {
        sub.push_str(" Installed outside the .deb: download it from the release page.");
    }
    sub
}

fn install_button(s: &Status, o: &Offer) -> String {
    if s.install == InstallKind::Deb {
        format!("Install {}", o.release.version)
    } else {
        "Download".to_string()
    }
}

/// The sub line of an `idle` phase (also "as before the check").
fn idle_row(s: &Status) -> (String, Option<&'static str>, String) {
    let since = |at: u64| ago(s.now.saturating_sub(at));
    if !s.auto {
        let sub = match s.checked_at {
            Some(at) => format!("Last checked {}.", since(at)),
            None => "Never checked.".to_string(),
        };
        return ("Automatic checks are off".into(), None, sub);
    }
    match (s.checked_at, &s.error) {
        (None, _) => (
            "Not checked yet".into(),
            None,
            "RADE checks a minute after it starts.".into(),
        ),
        (Some(at), Some(e)) => (
            "Couldn't check".into(),
            Some("wait"),
            format!("{e} · tried {}.", since(at)),
        ),
        (Some(at), None) => (
            "Up to date".into(),
            Some("ok"),
            format!(
                "Checked {} · {}.",
                since(at),
                match s.channel {
                    Channel::Stable => "Stable releases",
                    Channel::Rc => "Release candidates too",
                }
            ),
        ),
    }
}

/// §5.1: the About block's texts.
pub fn about(s: &Status) -> AboutView {
    let mut v = AboutView {
        title: format!("RADE {}", s.current),
        status: String::new(),
        status_class: None,
        sub: String::new(),
        buttons: Vec::new(),
        primary_enabled: true,
        notes: s.offer.is_some(),
        progress: None,
    };
    let offer = s.offer.as_ref();
    match &s.phase {
        Phase::Devel => {
            v.status = "Development build".into();
            v.sub = "Built from source: updates are off.".into();
        }
        Phase::Off => {
            v.status = "Update checks are off".into();
            v.sub = "RADE_UPDATE_CHECK=0 is set.".into();
        }
        Phase::Idle => {
            let (status, class, sub) = idle_row(s);
            (v.status, v.status_class, v.sub) = (status, class, sub);
            v.buttons = vec!["Check now".into()];
        }
        Phase::Checking => {
            v.status = "Checking…".into();
            v.sub = match offer {
                Some(o) => offer_sub(s, o),
                None => idle_row(s).2,
            };
            v.buttons = vec!["Check now".into()];
            v.primary_enabled = false;
        }
        Phase::Available => {
            if let Some(o) = offer {
                let ver = &o.release.version;
                if o.skipped {
                    v.status = format!("RADE {ver} skipped");
                    v.sub = "You skipped this version. RADE tells you again when a newer one comes out."
                        .into();
                    v.buttons = vec![install_button(s, o)];
                } else {
                    v.status = format!("RADE {ver} is available");
                    v.status_class = Some(if o.security || o.kind == Kind::Major {
                        "wait"
                    } else {
                        "acc"
                    });
                    v.sub = offer_sub(s, o);
                    v.buttons = vec![install_button(s, o), "Skip This Version".into()];
                    if let Some(i) = s
                        .installed
                        .as_ref()
                        .filter(|i| s.install == InstallKind::Deb && **i > s.current)
                    {
                        v.sub.push_str(&format!(
                            " RADE {i} is installed already: restart to use it."
                        ));
                        v.buttons.push("Restart…".into());
                    }
                }
            }
        }
        Phase::Downloading { pct, bytes } => {
            let ver = offer
                .map(|o| o.release.version.to_string())
                .unwrap_or_default();
            let size = offer.map_or(0, |o| o.release.deb.size);
            v.status = format!("Downloading {ver}… {pct}%");
            v.sub = format!("{} of {} MB", mb(*bytes), mb(size));
            v.buttons = vec!["Cancel".into()];
            v.progress = Some(f64::from(*pct) / 100.0);
        }
        Phase::Verifying => {
            v.status = "Checking the download…".into();
            v.sub = "SHA-256 against SHA256SUMS.".into();
        }
        Phase::Installing => {
            let ver = offer
                .map(|o| o.release.version.to_string())
                .unwrap_or_default();
            v.status = format!("Installing {ver}…");
            v.sub = "Approve the system dialog to continue.".into();
        }
        Phase::RestartPending(ver) => {
            v.status = format!("RADE {ver} is installed");
            v.status_class = Some("acc");
            v.sub = format!(
                "Restart RADE to finish. This window keeps running {} until then.",
                s.current
            );
            v.buttons = vec!["Restart…".into()];
        }
        Phase::Failed(e) => {
            v.status = "Update failed".into();
            v.status_class = Some("err");
            v.sub = e.clone();
            v.buttons = vec!["Try again".into(), "Skip This Version".into()];
        }
    }
    v
}

/// D20: the Restart dialog's body, from every Agent Pane's facts.
pub fn restart_body(facts: &[Fact], version: &Version) -> String {
    let running = facts
        .iter()
        .filter(|f| f.status == Some(AgentStatus::Running))
        .count();
    let waiting: Vec<&str> = facts
        .iter()
        .filter(|f| f.status == Some(AgentStatus::Waiting))
        .map(|f| f.name.as_str())
        .collect();
    let mut lines = Vec::new();
    match running {
        0 => {}
        1 => lines.push(
            "1 Agent is running. It stops now; Session Restore reopens it as interrupted — send “continue” to go on."
                .to_string(),
        ),
        n => lines.push(format!(
            "{n} Agents are running. They stop now; Session Restore reopens them as interrupted — send “continue” to go on."
        )),
    }
    match waiting.len() {
        0 => {}
        1 => lines.push(format!(
            "1 Agent is waiting for you: {}. Session Restore reopens it as interrupted.",
            waiting[0]
        )),
        n => {
            let mut names = waiting[..n.min(3)].join(", ");
            if n > 3 {
                names.push_str(&format!(" +{}", n - 3));
            }
            lines.push(format!(
                "{n} Agents are waiting for you: {names}. Session Restore reopens them as interrupted."
            ));
        }
    }
    if lines.is_empty() {
        return format!("RADE {version} opens with your Tabs and Panes as they are now.");
    }
    lines.join("\n\n")
}

/// D17: the Toast text, only for a security release.
pub fn toast_text(offer: &Offer) -> Option<String> {
    offer
        .security
        .then(|| format!("RADE {} fixes a security issue", offer.release.version))
}

/// D23: the Activity row's text for a phase (a skipped offer logs nothing).
pub fn activity_text(phase: &Phase, offer: Option<&Offer>) -> Option<String> {
    match phase {
        Phase::Available => {
            let o = offer.filter(|o| !o.skipped)?;
            let v = &o.release.version;
            Some(if o.security {
                format!("{v} is available · security fix")
            } else {
                format!("{v} is available")
            })
        }
        Phase::RestartPending(v) => Some(format!("{v} installed · restart to finish")),
        Phase::Failed(e) => Some(format!("update failed: {e}")),
        _ => None,
    }
}

/// PRD R127 item 8: the lines `rade version` prints. `app`: `None` = not
/// running; `Some((None, _))` = an app without `app.info`.
pub fn version_lines(
    cli: &str,
    app: Option<(Option<&str>, Option<u32>)>,
    cache: &Cache,
    current: &Version,
    channel: Channel,
    now: u64,
) -> Vec<String> {
    const RESTART: &str = " · restart RADE to finish updating";
    let mut out = vec![format!("rade {cli}")];
    out.push(match app {
        None => "app not running".to_string(),
        Some((None, _)) => format!("app older than 1.2.0 (no app.info){RESTART}"),
        Some((Some(v), pid)) => {
            let mut l = format!("app {v}");
            if let Some(p) = pid {
                l.push_str(&format!(" (pid {p})"));
            }
            if v != cli {
                l.push_str(RESTART);
            }
            l
        }
    });
    let since = cache
        .checked_at
        .map(|at| ago(now.saturating_sub(at)))
        .unwrap_or_default();
    let offer = pick(
        &cache.releases,
        current,
        current,
        channel,
        cache.skipped.as_deref(),
    );
    out.push(match (offer, &cache.error, cache.checked_at) {
        (Some(o), _, _) if o.skipped => {
            format!("update {} skipped · checked {since}", o.release.version)
        }
        (Some(o), _, _) => format!(
            "update {} available ({}{}) · checked {since}",
            o.release.version,
            o.kind.as_str(),
            if o.security { ", security" } else { "" }
        ),
        (None, Some(e), _) => format!("last check failed: {e} · {since}"),
        (None, None, Some(_)) => format!("up to date · checked {since}"),
        (None, None, None) => "not checked yet".to_string(),
    });
    out
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
