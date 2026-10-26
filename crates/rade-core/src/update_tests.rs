use super::*;

const RELEASES: &str = include_str!("../../../tests/fixtures/update/releases.json");
const SECURITY: &str = include_str!("../../../tests/fixtures/update/releases-security.json");

fn v(s: &str) -> Version {
    Version::parse(s).unwrap_or_else(|| panic!("{s}"))
}

fn releases() -> Vec<Release> {
    parse_releases(RELEASES, None).unwrap()
}

fn security() -> Vec<Release> {
    parse_releases(SECURITY, None).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d =
        crate::paths::outside_temp_dir().join(format!("rade-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn r127_a_versions_parse_and_compare() {
    assert!(v("1.1.0-rc.1") < v("1.1.0-rc.2"));
    assert!(v("1.1.0-rc.2") < v("1.1.0-rc.10"));
    assert!(v("1.1.0-rc.10") < v("1.1.0"));
    assert!(v("1.1.0") < v("1.2.0"));
    assert!(v("1.1.0-1") < v("1.1.0-alpha"), "numeric < alphanumeric");
    assert_eq!(Version::from_tag("v1.1.0"), Some(v("1.1.0")));
    assert_eq!(Version::from_tag("1.1.0"), None, "a tag has its v");
    assert_eq!(Version::from_deb("1.1.0~rc.1"), Some(v("1.1.0-rc.1")));
    assert_eq!(v("1.1.0+git202610261200.abc1234"), v("1.1.0"));
    assert_eq!(v("1.1.0-rc.1").to_string(), "1.1.0-rc.1");
    assert_eq!(v("1.1.0-rc.1").deb_name(), "rade_1.1.0~rc.1_amd64.deb");
    assert_eq!(v("1.1.0").deb_name(), "rade_1.1.0_amd64.deb");
    assert!(v("1.1.0-rc.1").is_pre() && !v("1.1.0").is_pre());
    assert_eq!(Version::parse("1.1"), None);
    assert_eq!(Version::parse("x"), None);
    assert_eq!(Version::parse("1.1.0.0"), None);
    // The cache keeps a version as its string.
    assert_eq!(
        serde_json::to_string(&v("1.1.0-rc.1")).unwrap(),
        "\"1.1.0-rc.1\""
    );
    assert_eq!(
        serde_json::from_str::<Version>("\"1.2.0\"").unwrap(),
        v("1.2.0")
    );
}

#[test]
fn r127_b_the_offer_comes_from_the_release_list() {
    let all = releases();
    let tags: Vec<&str> = all.iter().map(|r| r.tag.as_str()).collect();
    assert_eq!(
        tags,
        ["v1.2.0-rc.1", "v1.1.0", "v1.0.1", "v1.0.0"],
        "no draft, no release without a .deb, none with a .deb on another host"
    );
    let o = pick(&all, &v("1.0.0"), &v("1.0.0"), Channel::Stable, None).unwrap();
    assert_eq!(
        (o.release.version.to_string(), o.kind),
        ("1.1.0".into(), Kind::Minor)
    );
    assert!(!o.security && !o.skipped);
    assert!(
        o.notes.starts_with("# What's new since RADE 1.0.0\n"),
        "{}",
        o.notes
    );
    let (a, b) = (
        o.notes.find("## RADE 1.1.0 · 2026-10-26").unwrap(),
        o.notes.find("## RADE 1.0.1 · 2026-10-25").unwrap(),
    );
    assert!(a < b);
    assert!(!o.notes.contains("Install: "));
    assert!(o.notes.contains("Voice prompts"));
    assert!(
        !o.notes.contains("First stable release"),
        "1.0.0 is the running one"
    );
    let rc = pick(&all, &v("1.0.0"), &v("1.0.0"), Channel::Rc, None).unwrap();
    assert_eq!(rc.release.version, v("1.2.0-rc.1"));
    assert!(pick(&all, &v("1.1.0"), &v("1.1.0"), Channel::Stable, None).is_none());
    // The release's assets and the API's digest.
    let r = &all[1];
    assert_eq!(r.deb.name, "rade_1.1.0_amd64.deb");
    assert_eq!(r.sums.name, "SHA256SUMS");
    assert_eq!(r.published, "2026-10-26");
    assert_eq!(r.deb.digest.as_ref().map(|d| d.len()), Some(64));
    assert!(!r.deb.digest.as_ref().unwrap().starts_with("sha256:"));
    // A skipped version is marked, a higher one is not.
    let s = pick(
        &all,
        &v("1.0.0"),
        &v("1.0.0"),
        Channel::Stable,
        Some("1.1.0"),
    )
    .unwrap();
    assert!(s.skipped);
    let s = pick(
        &all,
        &v("1.0.0"),
        &v("1.0.0"),
        Channel::Stable,
        Some("1.0.1"),
    )
    .unwrap();
    assert!(!s.skipped);
    assert!(parse_releases("{}", None).is_err());
    assert!(parse_releases("nope", None).is_err());
}

#[test]
fn r127_c_security_and_major() {
    let all = security();
    let from_100 = pick(&all, &v("1.0.0"), &v("1.0.0"), Channel::Stable, None).unwrap();
    assert_eq!(from_100.release.version, v("2.0.0"));
    assert!(
        from_100.security,
        "1.0.1 lists ### Security and lies between"
    );
    assert_eq!(from_100.kind, Kind::Major);
    let from_101 = pick(&all, &v("1.0.1"), &v("1.0.1"), Channel::Stable, None).unwrap();
    assert!(
        !from_101.security,
        "1.0.1 itself is not above the running version"
    );
    let from_110 = pick(&all, &v("1.1.0"), &v("1.1.0"), Channel::Stable, None).unwrap();
    assert_eq!((from_110.kind, from_110.security), (Kind::Major, false));
    // The Toast text exists only for a security offer.
    assert_eq!(
        toast_text(&from_100).as_deref(),
        Some("RADE 2.0.0 fixes a security issue")
    );
    assert_eq!(toast_text(&from_101), None);
}

#[test]
fn r127_d_when_a_check_is_due() {
    let now = 1_000_000;
    let good = |ago: u64| Cache {
        checked_at: Some(now - ago),
        ..Cache::default()
    };
    let failed = |ago: u64| Cache {
        checked_at: Some(now - ago),
        error: Some("GitHub answered 500".into()),
        ..Cache::default()
    };
    assert!(due(&Cache::default(), now));
    assert!(!due(&good(23 * 3600 + 59 * 60), now));
    assert!(due(&good(24 * 3600), now));
    assert!(!due(&failed(59 * 60), now));
    assert!(due(&failed(60 * 60), now));
    // A broken file reads as empty; a written cache reads back equal.
    let d = tmp("d");
    std::fs::write(d.join("update.json"), "{ not json").unwrap();
    assert_eq!(Cache::load(&d, None), Cache::default());
    assert!(due(&Cache::load(&d, None), now));
    let written = Cache::update(&d, None, |c| {
        c.checked_at = Some(now);
        c.releases = releases();
        c.skipped = Some("1.1.0".into());
    })
    .unwrap();
    assert_eq!(Cache::load(&d, None), written);
    let mut names: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["update.json", "update.json.lock"],
        "the temp file was renamed away"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cache_writers_keep_each_others_fields() {
    let d = tmp("writers");
    // The CLI's check and the app's Skip, in either order.
    Cache::update(&d, None, |c| {
        c.checked_at = Some(5);
        c.releases = releases();
    })
    .unwrap();
    Cache::update(&d, None, |c| c.skipped = Some("1.1.0".into())).unwrap();
    let c = Cache::update(&d, None, |c| c.error = Some("boom".into())).unwrap();
    assert_eq!(c.checked_at, Some(5));
    assert_eq!(c.releases.len(), 4);
    assert_eq!(c.skipped.as_deref(), Some("1.1.0"));
    assert_eq!(c.error.as_deref(), Some("boom"));
    assert_eq!(Cache::load(&d, None), c);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r127_e_verify_and_allowed_urls() {
    let d = tmp("e");
    let f = d.join("rade_1.1.0_amd64.deb");
    std::fs::write(&f, b"hello deb").unwrap();
    let sha = sha256_file(&f).unwrap();
    assert_eq!(sha.len(), 64);
    let sums = format!(
        "{sha}  rade_1.1.0_amd64.deb\n{}  other.deb\n",
        "0".repeat(64)
    );
    let asset = |digest: Option<String>| Asset {
        name: "rade_1.1.0_amd64.deb".into(),
        url: String::new(),
        size: 9,
        digest,
    };
    assert_eq!(verify(&f, &sums, &asset(Some(sha.clone()))), Ok(()));
    assert_eq!(
        verify(&f, &sums, &asset(None)),
        Ok(()),
        "SHA256SUMS alone decides"
    );
    let bad = "Download didn't match SHA256SUMS. Nothing was installed.";
    let wrong_sums = format!("{}  rade_1.1.0_amd64.deb\n", "1".repeat(64));
    assert_eq!(
        verify(&f, &wrong_sums, &asset(Some(sha.clone()))),
        Err(bad.into())
    );
    assert_eq!(
        verify(&f, &sums, &asset(Some("2".repeat(64)))),
        Err(bad.into())
    );
    assert_eq!(
        verify(&f, &format!("{sha}  other.deb\n"), &asset(None)),
        Err(bad.into()),
        "not listed in SHA256SUMS"
    );
    assert_eq!(
        sums_entry(
            &format!("{sha} *rade_1.1.0_amd64.deb"),
            "rade_1.1.0_amd64.deb"
        ),
        Some(sha.clone())
    );
    assert_eq!(sums_entry(&sums, "missing.deb"), None);
    let _ = std::fs::remove_dir_all(&d);

    let n = "rade_1.1.0_amd64.deb";
    let gh = format!("{DOWNLOAD_PREFIX}v1.1.0/{n}");
    assert!(asset_allowed(&gh, "v1.1.0", n, None));
    assert!(!asset_allowed(
        &gh.replace("richard-plg/rade", "evil/rade"),
        "v1.1.0",
        n,
        None
    ));
    assert!(!asset_allowed(&gh, "v1.2.0", n, None), "another tag");
    assert!(!asset_allowed(&gh, "v1.1.0", "SHA256SUMS", None));
    assert!(!asset_allowed(
        &format!("{DOWNLOAD_PREFIX}v1.1.0/{n}?x=1"),
        "v1.1.0",
        n,
        None
    ));
    let local = "http://127.0.0.1:8123/download/rade_1.1.0_amd64.deb";
    let url = "http://127.0.0.1:8123/repos/richard-plg/rade/releases?per_page=20";
    assert!(
        !asset_allowed(local, "v1.1.0", n, None),
        "not without the override"
    );
    assert!(asset_allowed(local, "v1.1.0", n, Some(url)));
    assert!(!asset_allowed(
        local,
        "v1.1.0",
        n,
        Some("http://127.0.0.1:9/x")
    ));
    assert!(!asset_allowed(&gh, "v1.1.0", n, Some(url)));
    // The fixture list read with the override keeps only the local URLs.
    assert!(parse_releases(RELEASES, Some(url)).unwrap().is_empty());
}

#[test]
fn r127_f_install_kind_outcome_and_restart_path() {
    let deb_exe = Path::new(INSTALLED_EXE);
    let cargo = Path::new("/home/u/.cargo/bin/rade-app");
    let dpkg = Some(v("1.1.0"));
    let k = |devel, exe, inst: Option<&Version>, oi, ou| {
        install_kind(devel, exe, inst, oi, ou, "x86_64")
    };
    assert_eq!(
        k(false, deb_exe, dpkg.as_ref(), false, false),
        InstallKind::Deb
    );
    assert_eq!(
        k(false, deb_exe, None, false, false),
        InstallKind::Other,
        "not a dpkg install"
    );
    assert_eq!(
        k(false, cargo, dpkg.as_ref(), false, false),
        InstallKind::Other
    );
    assert_eq!(k(true, cargo, None, false, false), InstallKind::Devel);
    assert_eq!(k(true, cargo, None, false, true), InstallKind::Other);
    assert_eq!(k(true, cargo, None, true, true), InstallKind::Deb);
    // The overrides count only in the Devel flavor (D4).
    assert_eq!(k(false, cargo, None, true, true), InstallKind::Other);
    assert_eq!(
        install_kind(false, deb_exe, dpkg.as_ref(), false, false, "aarch64"),
        InstallKind::Other
    );
    use InstallOutcome::*;
    assert_eq!(install_outcome(0, ""), Done);
    assert_eq!(
        install_outcome(126, "Error executing command as another user"),
        Cancelled
    );
    assert_eq!(
        install_outcome(127, ""),
        Failed("Not authorized to install software.".into())
    );
    assert_eq!(
        install_outcome(100, "E: Could not get lock /var/lib/dpkg/lock-frontend\n"),
        Failed("Another package manager is running. Try again in a minute.".into())
    );
    assert_eq!(
        install_outcome(100, "Reading...\nE: Broken package\n\n"),
        Failed("E: Broken package".into())
    );
    assert_eq!(
        install_outcome(1, ""),
        Failed("apt-get exited with 1".into())
    );
    match install_outcome(1, &format!("{}\n", "x".repeat(300))) {
        Failed(m) => assert_eq!(m.chars().count(), 200),
        o => panic!("{o:?}"),
    }
    assert_eq!(
        restart_exe(Path::new("/usr/lib/rade/rade-app (deleted)")),
        PathBuf::from(INSTALLED_EXE)
    );
    assert_eq!(restart_exe(cargo), cargo);
    let deb = Path::new("/c/rade/update/rade_1.2.0_amd64.deb");
    let sha = "ab".repeat(32);
    assert_eq!(
        install_argv(deb, &sha, None),
        [
            "/usr/bin/pkexec",
            "/usr/lib/rade/rade-install",
            "/c/rade/update/rade_1.2.0_amd64.deb",
            sha.as_str()
        ]
    );
    assert_eq!(
        install_argv(deb, &sha, Some("/t/fake-install.sh")),
        [
            "sh",
            "-c",
            "/t/fake-install.sh \"$1\" \"$2\"",
            "rade-install",
            "/c/rade/update/rade_1.2.0_amd64.deb",
            sha.as_str()
        ]
    );
    assert_eq!(
        install_outcome(3, "whatever"),
        Failed("Download didn't match SHA256SUMS. Nothing was installed.".into())
    );
}

#[test]
fn r127_f_rade_install_script_checks_the_copy_as_root_would() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    let d = tmp("script");
    let deb = d.join("in.deb");
    std::fs::write(&deb, b"the deb").unwrap();
    let sha = sha256_file(&deb).unwrap();
    let log = d.join("apt.log");
    let fake = d.join("fake-apt");
    // Records its args and the content of the file it was given.
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{0}'\ncat \"$4\" >> '{0}'\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/rade-install");
    let run = |sha: &str| {
        Command::new("sh")
            .arg(&script)
            .arg(&deb)
            .arg(sha)
            .env("RADE_INSTALL_APT", &fake)
            .output()
            .unwrap()
    };
    let out = run(&sha);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let seen = std::fs::read_to_string(&log).unwrap();
    let mut lines = seen.lines();
    let args = lines.next().unwrap();
    assert!(args.starts_with("install -y --no-remove /"), "{args}");
    let copy = args.rsplit(' ').next().unwrap();
    assert!(copy.ends_with(".deb"));
    assert!(
        !Path::new(copy).starts_with(&d),
        "{copy} is the original dir"
    );
    assert_eq!(lines.next(), Some("the deb"));
    assert!(!Path::new(copy).exists(), "the copy is removed");
    // A wrong digest: exit 3, apt never called.
    std::fs::remove_file(&log).unwrap();
    let out = run(&"0".repeat(64));
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(!log.exists());
    // Not a digest: exit 2.
    let out = run("xyz");
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(!log.exists());
    let _ = std::fs::remove_dir_all(&d);
}

fn status(phase: Phase, offer: Option<Offer>, install: InstallKind) -> Status {
    Status {
        phase,
        current: v("1.1.0"),
        installed: None,
        install,
        checked_at: Some(1_000),
        error: None,
        offer,
        channel: Channel::Stable,
        auto: true,
        now: 1_000 + 300,
    }
}

fn offer_of(list: &[Release], from: &str) -> Offer {
    pick(list, &v(from), &v(from), Channel::Stable, None).unwrap()
}

#[test]
fn r127_g_cell_and_about_texts() {
    let minor = offer_of(&releases(), "1.0.0"); // 1.1.0, minor
    let sec = offer_of(&security(), "1.0.0"); // 2.0.0, major + security
    let major = offer_of(&security(), "1.1.0"); // 2.0.0, major
    let mut skipped = minor.clone();
    skipped.skipped = true;
    let up = "ph-arrow-circle-up-symbolic";
    let cell_of = |p: Phase, o: Option<&Offer>| cell(&status(p, o.cloned(), InstallKind::Deb));
    let c = |icon, text: &str, class, tip: &str| {
        Some(CellView {
            icon,
            text: text.into(),
            class,
            tooltip: tip.into(),
        })
    };
    assert_eq!(
        cell_of(Phase::Available, Some(&minor)),
        c(
            up,
            "1.1.0",
            None,
            "RADE 1.1.0 is available · click for details"
        )
    );
    assert_eq!(
        cell_of(Phase::Available, Some(&sec)),
        c(
            up,
            "2.0.0 security",
            Some("wait"),
            "RADE 2.0.0 is available · click for details"
        )
    );
    assert_eq!(
        cell_of(Phase::Available, Some(&major)),
        c(
            up,
            "2.0.0 major",
            Some("wait"),
            "RADE 2.0.0 is available · click for details"
        )
    );
    assert_eq!(
        cell_of(Phase::Checking, Some(&minor)).unwrap().text,
        "1.1.0"
    );
    assert_eq!(cell_of(Phase::Checking, None), None);
    assert_eq!(cell_of(Phase::Available, Some(&skipped)), None);
    assert_eq!(
        cell_of(Phase::Downloading { pct: 42, bytes: 1 }, Some(&minor))
            .unwrap()
            .text,
        "1.1.0 42%"
    );
    assert_eq!(
        cell_of(Phase::Verifying, Some(&minor)).unwrap().text,
        "1.1.0 installing"
    );
    assert_eq!(
        cell_of(Phase::Installing, Some(&minor)).unwrap().text,
        "1.1.0 installing"
    );
    assert_eq!(
        cell_of(Phase::RestartPending(v("1.2.0")), None),
        c(
            "ph-arrow-clockwise-symbolic",
            "restart for 1.2.0",
            Some("acc"),
            "RADE 1.2.0 is installed · click to restart"
        )
    );
    assert_eq!(
        cell_of(Phase::Failed("boom".into()), None),
        c(
            "ph-warning-diamond-symbolic",
            "update failed",
            Some("err"),
            "boom"
        )
    );
    for p in [Phase::Devel, Phase::Off, Phase::Idle] {
        assert_eq!(cell_of(p, None), None);
    }

    // The About block, row by row (§5.1), for kinds deb and other.
    let ab = |p: Phase, o: Option<&Offer>, k| about(&status(p, o.cloned(), k));
    let deb = InstallKind::Deb;
    let other = InstallKind::Other;
    let a = ab(Phase::Devel, None, InstallKind::Devel);
    assert_eq!(a.title, "RADE 1.1.0");
    assert_eq!(
        (a.status.as_str(), a.sub.as_str(), a.buttons.len()),
        (
            "Development build",
            "Built from source: updates are off.",
            0
        )
    );
    let a = ab(Phase::Off, None, deb);
    assert_eq!(
        (a.status.as_str(), a.sub.as_str()),
        ("Update checks are off", "RADE_UPDATE_CHECK=0 is set.")
    );
    let a = ab(Phase::Idle, None, deb);
    assert_eq!(
        (
            a.status.as_str(),
            a.status_class,
            a.sub.as_str(),
            &a.buttons[..]
        ),
        (
            "Up to date",
            Some("ok"),
            "Checked 5m ago · Stable releases.",
            &["Check now".to_string()][..]
        )
    );
    let mut s = status(Phase::Idle, None, deb);
    s.channel = Channel::Rc;
    assert_eq!(about(&s).sub, "Checked 5m ago · Release candidates too.");
    s.checked_at = Some(s.now - 5);
    assert_eq!(about(&s).sub, "Checked just now · Release candidates too.");
    s.checked_at = None;
    let a = about(&s);
    assert_eq!(
        (a.status.as_str(), a.sub.as_str()),
        ("Not checked yet", "RADE checks a minute after it starts.")
    );
    s.checked_at = Some(1_000);
    s.error = Some("GitHub rate limit reached".into());
    let a = about(&s);
    assert_eq!(
        (a.status.as_str(), a.status_class, a.sub.as_str()),
        (
            "Couldn't check",
            Some("wait"),
            "GitHub rate limit reached · tried 5m ago."
        )
    );
    s.auto = false;
    let a = about(&s);
    assert_eq!(
        (a.status.as_str(), a.status_class, a.sub.as_str()),
        ("Automatic checks are off", None, "Last checked 5m ago.")
    );
    s.checked_at = None;
    assert_eq!(about(&s).sub, "Never checked.");
    let a = ab(Phase::Checking, Some(&minor), deb);
    assert_eq!((a.status.as_str(), a.primary_enabled), ("Checking…", false));
    assert_eq!(a.sub, "New features and fixes.");

    let a = ab(Phase::Available, Some(&minor), deb);
    assert_eq!(
        (a.status.as_str(), a.status_class, a.sub.as_str()),
        (
            "RADE 1.1.0 is available",
            Some("acc"),
            "New features and fixes."
        )
    );
    assert_eq!(a.buttons, ["Install 1.1.0", "Skip This Version"]);
    assert!(a.notes && a.primary_enabled);
    let a = ab(Phase::Available, Some(&minor), other);
    assert_eq!(
        a.sub,
        "New features and fixes. Installed outside the .deb: download it from the release page."
    );
    assert_eq!(a.buttons, ["Download", "Skip This Version"]);
    let a = ab(Phase::Available, Some(&sec), deb);
    assert_eq!(
        (a.status_class, a.sub.as_str()),
        (Some("wait"), "Fixes a security issue.")
    );
    let a = ab(Phase::Available, Some(&major), deb);
    assert_eq!(a.status_class, Some("wait"));
    assert_eq!(
        a.sub,
        "2.0.0 changes the rade CLI, socket or config files: read the notes first, scripts and Agent skills may need changes."
    );
    let patch = {
        let mut p = minor.clone();
        p.kind = Kind::Patch;
        p
    };
    assert_eq!(ab(Phase::Available, Some(&patch), deb).sub, "Fixes.");
    let a = ab(Phase::Available, Some(&skipped), deb);
    assert_eq!(
        (
            a.status.as_str(),
            a.status_class,
            a.sub.as_str(),
            &a.buttons[..]
        ),
        (
            "RADE 1.1.0 skipped",
            None,
            "You skipped this version. RADE tells you again when a newer one comes out.",
            &["Install 1.1.0".to_string()][..]
        )
    );
    assert_eq!(
        ab(Phase::Available, Some(&skipped), other).buttons,
        ["Download"]
    );
    // Installed above the running version while an offer is higher.
    let mut s = status(Phase::Available, Some(minor.clone()), deb);
    s.installed = Some(v("1.1.5"));
    let a = about(&s);
    assert_eq!(
        a.sub,
        "New features and fixes. RADE 1.1.5 is installed already: restart to use it."
    );
    assert_eq!(
        a.buttons,
        ["Install 1.1.0", "Skip This Version", "Restart…"]
    );

    let mut dl = status(
        Phase::Downloading {
            pct: 50,
            bytes: 1_000_000,
        },
        Some(minor.clone()),
        deb,
    );
    dl.offer.as_mut().unwrap().release.deb.size = 2_000_000;
    let a = about(&dl);
    assert_eq!(
        (a.status.as_str(), a.sub.as_str(), a.progress),
        ("Downloading 1.1.0… 50%", "1.0 of 2.0 MB", Some(0.5))
    );
    assert_eq!(a.buttons, ["Cancel"]);
    let a = ab(Phase::Verifying, Some(&minor), deb);
    assert_eq!(
        (a.status.as_str(), a.sub.as_str(), a.buttons.len()),
        ("Checking the download…", "SHA-256 against SHA256SUMS.", 0)
    );
    let a = ab(Phase::Installing, Some(&minor), deb);
    assert_eq!(
        (a.status.as_str(), a.sub.as_str()),
        (
            "Installing 1.1.0…",
            "Approve the system dialog to continue."
        )
    );
    let a = ab(Phase::RestartPending(v("1.2.0")), None, deb);
    assert_eq!(
        (
            a.status.as_str(),
            a.status_class,
            a.sub.as_str(),
            &a.buttons[..]
        ),
        (
            "RADE 1.2.0 is installed",
            Some("acc"),
            "Restart RADE to finish. This window keeps running 1.1.0 until then.",
            &["Restart…".to_string()][..]
        )
    );
    let a = ab(
        Phase::Failed("Download failed: x".into()),
        Some(&minor),
        deb,
    );
    assert_eq!(
        (a.status.as_str(), a.status_class, a.sub.as_str()),
        ("Update failed", Some("err"), "Download failed: x")
    );
    assert_eq!(a.buttons, ["Try again", "Skip This Version"]);
    assert_eq!(
        PRIVACY,
        "RADE asks api.github.com for the release list once a day. It sends its version in the User-Agent and nothing else."
    );
}

fn agent(name: &str, status: AgentStatus) -> Fact {
    Fact {
        pane: name.into(),
        name: name.into(),
        status: Some(status),
        ..Fact::default()
    }
}

#[test]
fn r127_h_restart_body() {
    use AgentStatus::*;
    let ver = v("1.2.0");
    let body = |f: &[Fact]| restart_body(f, &ver);
    assert_eq!(
        body(&[]),
        "RADE 1.2.0 opens with your Tabs and Panes as they are now."
    );
    assert_eq!(
        body(&[agent("a", Done), agent("b", Interrupted)]),
        "RADE 1.2.0 opens with your Tabs and Panes as they are now."
    );
    let run1 = "1 Agent is running. It stops now; Session Restore reopens it as interrupted — send “continue” to go on.";
    assert_eq!(body(&[agent("a", Running)]), run1);
    let run3: Vec<Fact> = ["a", "b", "c"].iter().map(|n| agent(n, Running)).collect();
    assert_eq!(
        body(&run3),
        "3 Agents are running. They stop now; Session Restore reopens them as interrupted — send “continue” to go on."
    );
    assert_eq!(
        body(&[agent("a", Waiting)]),
        "1 Agent is waiting for you: a. Session Restore reopens it as interrupted."
    );
    assert_eq!(
        body(&[agent("a", Running), agent("b", Waiting)]),
        format!(
            "{run1}\n\n1 Agent is waiting for you: b. Session Restore reopens it as interrupted."
        )
    );
    let wait5: Vec<Fact> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| agent(n, Waiting))
        .collect();
    assert_eq!(
        body(&wait5),
        "5 Agents are waiting for you: a, b, c +2. Session Restore reopens them as interrupted."
    );
    assert_eq!(
        body(&wait5[..2]),
        "2 Agents are waiting for you: a, b. Session Restore reopens them as interrupted."
    );
}

#[test]
fn installed_from_query_needs_ii() {
    assert_eq!(installed_from_query("ii 1.1.0"), Some(v("1.1.0")));
    assert_eq!(installed_from_query("ii 1.1.0\n"), Some(v("1.1.0")));
    assert_eq!(installed_from_query("ii 1.2.0~rc.1"), Some(v("1.2.0-rc.1")));
    assert_eq!(
        installed_from_query("rc 1.0.0"),
        None,
        "removed, config files left"
    );
    assert_eq!(installed_from_query(""), None);
    assert_eq!(installed_from_query("ii x"), None);
}

#[test]
fn phase_after_follows_the_cache_and_the_installed_version() {
    let cache = Cache {
        checked_at: Some(1),
        releases: releases(),
        ..Cache::default()
    };
    let run = |cur: &str, inst: Option<&str>, kind, cache: &Cache| {
        let inst = inst.map(v);
        phase_after(cache, &v(cur), inst.as_ref(), kind, Channel::Stable)
    };
    let (p, o) = run("1.0.0", None, InstallKind::Deb, &cache);
    assert_eq!(
        (p, o.unwrap().release.version),
        (Phase::Available, v("1.1.0"))
    );
    // An error of the last check never hides the offer.
    let failed = Cache {
        error: Some("GitHub answered 500".into()),
        ..cache.clone()
    };
    assert_eq!(
        run("1.0.0", None, InstallKind::Deb, &failed).0,
        Phase::Available
    );
    assert_eq!(run("1.1.0", None, InstallKind::Deb, &failed).0, Phase::Idle);
    // Installed above the running one and nothing higher: restart pending.
    let (p, o) = run("1.0.0", Some("1.1.0"), InstallKind::Deb, &cache);
    assert_eq!((p, o), (Phase::RestartPending(v("1.1.0")), None));
    // Other installs never enter restart pending.
    assert_eq!(
        run("1.0.0", Some("1.1.0"), InstallKind::Other, &cache).0,
        Phase::Available
    );
    assert_eq!(
        run("1.0.0", None, InstallKind::Devel, &cache).0,
        Phase::Devel
    );
    // Installed 1.2.0 and a 1.3.0 offer: available, the installed one is the floor.
    let more = Cache {
        releases: [
            releases(),
            parse_releases(&SECURITY.replace("2.0.0", "1.3.0"), None).unwrap(),
        ]
        .concat(),
        ..cache.clone()
    };
    let inst = v("1.2.0");
    let (p, o) = phase_after(
        &more,
        &v("1.1.0"),
        Some(&inst),
        InstallKind::Deb,
        Channel::Stable,
    );
    assert_eq!(p, Phase::Available);
    let o = o.unwrap();
    assert_eq!(o.release.version, v("1.3.0"));
    assert!(o.notes.starts_with("# What's new since RADE 1.1.0"));
    // The channel is read when the phase is worked out.
    let (p, o) = phase_after(&cache, &v("1.1.0"), None, InstallKind::Deb, Channel::Rc);
    assert_eq!(
        (p, o.unwrap().release.version),
        (Phase::Available, v("1.2.0-rc.1"))
    );
    // A skipped offer is marked, the phase stays available.
    let skipped = Cache {
        skipped: Some("1.1.0".into()),
        ..cache
    };
    let (p, o) = run("1.0.0", None, InstallKind::Deb, &skipped);
    assert!(p == Phase::Available && o.unwrap().skipped);
}

#[test]
fn check_errors_and_ago() {
    assert_eq!(check_error(403, "{}"), "GitHub rate limit reached");
    assert_eq!(check_error(429, ""), "GitHub rate limit reached");
    assert_eq!(check_error(500, ""), "GitHub answered 500");
    assert_eq!(ago(0), "just now");
    assert_eq!(ago(59), "just now");
    assert_eq!(ago(300), "5m ago");
    assert_eq!(ago(2 * 3600), "2h ago");
    assert_eq!(ago(3 * 86400), "3d ago");
}

#[test]
fn activity_texts_once_per_phase() {
    let minor = offer_of(&releases(), "1.0.0");
    let sec = offer_of(&security(), "1.0.0");
    let mut skipped = minor.clone();
    skipped.skipped = true;
    assert_eq!(
        activity_text(&Phase::Available, Some(&minor)).as_deref(),
        Some("1.1.0 is available")
    );
    assert_eq!(
        activity_text(&Phase::Available, Some(&sec)).as_deref(),
        Some("2.0.0 is available · security fix")
    );
    assert_eq!(activity_text(&Phase::Available, Some(&skipped)), None);
    assert_eq!(
        activity_text(&Phase::RestartPending(v("1.2.0")), None).as_deref(),
        Some("1.2.0 installed · restart to finish")
    );
    assert_eq!(
        activity_text(&Phase::Failed("x".into()), None).as_deref(),
        Some("update failed: x")
    );
    assert_eq!(activity_text(&Phase::Idle, None), None);
    let e = crate::activity::Event::update("1.1.0 is available", 7);
    assert_eq!(e.line(), "RADE 1.1.0 is available");
}

#[test]
fn version_lines_from_a_cache() {
    let now = 10_000;
    let cache = Cache {
        checked_at: Some(now - 5),
        releases: releases(),
        ..Cache::default()
    };
    let cur = v("1.0.0");
    let lines = |app, c: &Cache| version_lines("1.0.0", app, c, &cur, Channel::Stable, now);
    assert_eq!(
        lines(None, &cache),
        [
            "rade 1.0.0",
            "app not running",
            "update 1.1.0 available (minor) · checked just now"
        ]
    );
    assert_eq!(
        lines(Some((Some("1.0.0"), Some(42))), &cache)[1],
        "app 1.0.0 (pid 42)"
    );
    assert_eq!(
        lines(Some((Some("0.9.0"), Some(42))), &cache)[1],
        "app 0.9.0 (pid 42) · restart RADE to finish updating"
    );
    assert_eq!(
        lines(Some((None, None)), &cache)[1],
        "app older than 1.2.0 (no app.info) · restart RADE to finish updating"
    );
    let failed = Cache {
        error: Some("GitHub answered 500".into()),
        checked_at: Some(now - 300),
        releases: Vec::new(),
        ..Cache::default()
    };
    assert_eq!(
        lines(None, &failed)[2],
        "last check failed: GitHub answered 500 · 5m ago"
    );
    let good = Cache {
        error: None,
        ..failed.clone()
    };
    assert_eq!(lines(None, &good)[2], "up to date · checked 5m ago");
    assert_eq!(lines(None, &Cache::default())[2], "not checked yet");
    // An offer known from the cache wins over a failed check's line.
    let both = Cache {
        error: Some("GitHub answered 500".into()),
        ..cache.clone()
    };
    assert_eq!(
        lines(None, &both)[2],
        "update 1.1.0 available (minor) · checked just now"
    );
    let sec = Cache {
        releases: security(),
        ..cache.clone()
    };
    assert_eq!(
        lines(None, &sec)[2],
        "update 2.0.0 available (major, security) · checked just now"
    );
    let skipped = Cache {
        skipped: Some("1.1.0".into()),
        ..cache
    };
    assert_eq!(
        lines(None, &skipped)[2],
        "update 1.1.0 skipped · checked just now"
    );
}

#[test]
fn spawn_restart_runs_the_helper_which_execs_exe() {
    // A pid that is already gone: the helper execs `true` at once.
    let mut gone = std::process::Command::new("/bin/true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    let dir = std::env::temp_dir().join(format!("rade-restart-{}", std::process::id()));
    let state = dir.join("state.json");
    let lock = state.with_extension("json.lock");
    let mut child = restart_child(pid, Path::new("/bin/true"), &lock).unwrap();
    let started = std::time::Instant::now();
    assert!(child.wait().unwrap().success());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(spawn_restart(pid, Path::new("/bin/true"), &lock).is_ok());

    // The old app's lock still held: the helper waits until it is free.
    let held = crate::store::lock(&state).unwrap();
    let mut child = restart_child(pid, Path::new("/bin/true"), &lock).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(
        child.try_wait().unwrap().is_none(),
        "exec'd while the lock was held"
    );
    drop(held);
    let started = std::time::Instant::now();
    assert!(child.wait().unwrap().success());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn r127_i_a_cache_is_never_trusted() {
    let d = tmp("untrusted");
    let good = releases();
    let mut other_host = good.clone();
    for r in &mut other_host {
        r.deb.url = r.deb.url.replace("github.com", "evil.example");
    }
    let mut traversal = good.clone();
    for r in &mut traversal {
        r.deb.name = "../x.deb".into();
    }
    let mut sums_name = good.clone();
    for r in &mut sums_name {
        r.sums.name = "other".into();
    }
    for (what, bad) in [
        ("host", other_host),
        ("name", traversal),
        ("sums name", sums_name),
    ] {
        std::fs::write(
            d.join("update.json"),
            serde_json::to_string(&Cache {
                checked_at: Some(1),
                releases: bad,
                ..Cache::default()
            })
            .unwrap(),
        )
        .unwrap();
        let c = Cache::load(&d, None);
        assert!(c.releases.is_empty(), "{what}");
        let (p, o) = phase_after(&c, &v("1.0.0"), None, InstallKind::Deb, Channel::Stable);
        assert_eq!((p, o), (Phase::Idle, None), "{what}");
    }
    // Valid ones stay; the override base counts for its own origin only.
    let ok = Cache {
        releases: good.clone(),
        ..Cache::default()
    };
    std::fs::write(d.join("update.json"), serde_json::to_string(&ok).unwrap()).unwrap();
    assert_eq!(Cache::load(&d, None).releases, good);
    assert!(Cache::load(&d, Some("http://127.0.0.1:9/"))
        .releases
        .is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r127_j_concurrent_cache_writers_lose_nothing() {
    let d = tmp("flock");
    let spawn = |field: u8| {
        let d = d.clone();
        std::thread::spawn(move || {
            for i in 0..50u64 {
                Cache::update(&d, None, |c| match field {
                    0 => c.checked_at = Some(i + 1),
                    1 => c.skipped = Some(format!("1.{i}.0")),
                    2 => c.toasted = Some(format!("2.{i}.0")),
                    _ => c.error = Some(format!("e{i}")),
                })
                .unwrap();
            }
        })
    };
    let hs: Vec<_> = (0..4).map(spawn).collect();
    for h in hs {
        h.join().unwrap();
    }
    let c = Cache::load(&d, None);
    assert_eq!(c.checked_at, Some(50));
    assert_eq!(c.skipped.as_deref(), Some("1.49.0"));
    assert_eq!(c.toasted.as_deref(), Some("2.49.0"));
    assert_eq!(c.error.as_deref(), Some("e49"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r127_k_a_skipped_offer_does_not_hide_an_installed_update() {
    let cache = Cache {
        checked_at: Some(1),
        releases: releases(),
        skipped: Some("1.1.0".into()),
        ..Cache::default()
    };
    let inst = v("1.1.0");
    let (p, o) = phase_after(
        &cache,
        &v("1.0.0"),
        Some(&inst),
        InstallKind::Deb,
        Channel::Rc,
    );
    // Channel rc offers 1.2.0-rc.1 (not skipped): available. Stable below.
    assert_eq!(p, Phase::Available);
    assert!(o.is_some());
    let cache = Cache {
        skipped: Some("1.2.0-rc.1".into()),
        ..cache
    };
    let (p, o) = phase_after(
        &cache,
        &v("1.0.0"),
        Some(&inst),
        InstallKind::Deb,
        Channel::Rc,
    );
    assert_eq!(p, Phase::RestartPending(v("1.1.0")));
    assert!(o.unwrap().skipped, "the offer is still there for About");
    // Not installed: the skipped offer stays available.
    let (p, _) = phase_after(&cache, &v("1.0.0"), None, InstallKind::Deb, Channel::Rc);
    assert_eq!(p, Phase::Available);
}
