use super::*;

/// A temp repo with one commit on `main`; `name` keeps parallel tests apart.
pub(crate) fn repo(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rade-git-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    sh(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    sh(&d, &["add", "-A"]);
    sh(&d, &["commit", "-qm", "init"]);
    d
}

pub(crate) fn sh(d: &Path, args: &[&str]) {
    let s = Command::new("git")
        .arg("-C")
        .arg(d)
        .args(["-c", "user.email=t@t", "-c", "user.name=t"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .unwrap();
    assert!(s.success(), "git {args:?}");
}

#[test]
fn reads_branch_detached_worktree_and_none() {
    let d = std::env::temp_dir().join(format!("rade-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("repo/.git")).unwrap();
    std::fs::write(d.join("repo/.git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
    assert_eq!(branch(&d.join("repo")).as_deref(), Some("feat/x"));

    std::fs::create_dir_all(d.join("gd")).unwrap();
    std::fs::write(d.join("gd/HEAD"), "0123456789abcdef\n").unwrap();
    std::fs::create_dir_all(d.join("wt")).unwrap();
    std::fs::write(
        d.join("wt/.git"),
        format!("gitdir: {}\n", d.join("gd").display()),
    )
    .unwrap();
    assert_eq!(branch(&d.join("wt")).as_deref(), Some("0123456"));

    assert_eq!(branch(&d), None);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn r12_1_names_follow_branch_rules() {
    for ok in ["feat-x", "feat/x", "a.b", "wt-codex-1", "UPPER_1", "tiếng"] {
        assert!(validate_name(ok).is_ok(), "{ok}");
    }
    for bad in [
        "", "new", "@", "-x", "a b", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b", "a..b",
        "a@{b", "a//b", "/a", "a/", "a.", ".a", "a/.b", "a.lock", "a/b.lock", "a\tb",
    ] {
        assert!(validate_name(bad).is_err(), "{bad:?}");
    }
    assert!(validate_name("a b").unwrap_err().message.contains("' '"));
}

#[test]
fn r12_1_dir_and_auto_names() {
    assert_eq!(
        worktree_dir(Path::new("/d/rade"), "proj", "feat-x"),
        PathBuf::from("/d/rade/worktrees/proj/feat-x")
    );
    let taken = |n: &str| n == "wt-codex-1" || n == "wt-codex-2";
    assert_eq!(auto_name(Some("codex"), &taken), "wt-codex-3");
    assert_eq!(auto_name(Some("Claude Code"), &taken), "wt-claude-code-1");
    assert_eq!(auto_name(None, &|_| false), "wt-shell-1");
}

#[test]
fn r12_parses_worktree_list_porcelain() {
    let text = "worktree /r/main\nHEAD 1111\nbranch refs/heads/main\n\n\
                worktree /r/wt/feat x\nHEAD 2222\nbranch refs/heads/feat/x\nlocked busy agent\n\n\
                worktree /r/wt/det\nHEAD 3333\ndetached\nprunable gitdir file points to non-existent location\n\n\
                worktree /r/bare\nbare\n";
    let l = parse_worktree_list(text);
    assert_eq!(l.len(), 4);
    assert_eq!(l[0].branch.as_deref(), Some("main"));
    assert_eq!(l[1].path, PathBuf::from("/r/wt/feat x"));
    assert_eq!(l[1].branch.as_deref(), Some("feat/x"));
    assert_eq!(l[1].locked.as_deref(), Some("busy agent"));
    assert_eq!(l[2].branch, None);
    assert!(l[2].prunable && l[2].locked.is_none());
    assert!(l[3].bare);
    assert!(parse_worktree_list("").is_empty());
}

#[test]
fn r12_3_status_paths_skip_rename_sources_and_survive_unicode() {
    let out = "R  ééé\0old name\0?? new.txt\0 M a\0C  c2\0c1\0";
    assert_eq!(parse_status_z(out), ["ééé", "new.txt", "a", "c2"]);
    assert!(parse_status_z("").is_empty());
}

#[test]
fn r12_parses_worktree_list_porcelain_z() {
    let z = "worktree /r/a\nb\0HEAD 1\0branch refs/heads/x\0\0worktree /r/c\0HEAD 2\0detached\0\0";
    let l = parse_worktree_list(z);
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].path, PathBuf::from("/r/a\nb"));
    assert_eq!(l[1].branch, None);
}

#[test]
fn r13_1_default_branch_order() {
    let none = |_: &str| false;
    let both = |_: &str| true;
    let master = |b: &str| b == "master";
    assert_eq!(
        choose_default_branch(Some("refs/remotes/origin/trunk\n"), &both).as_deref(),
        Some("origin/trunk")
    );
    assert_eq!(choose_default_branch(None, &both).as_deref(), Some("main"));
    assert_eq!(
        choose_default_branch(Some(""), &master).as_deref(),
        Some("master")
    );
    assert_eq!(choose_default_branch(None, &none), None);
}

#[test]
fn r12_add_list_dirty_remove_and_branch_delete_on_a_real_repo() {
    let r = repo("wt");
    let dir = std::env::temp_dir().join(format!("rade-git-wtdirs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(default_branch(&r).as_deref(), Some("main"));

    // New branch from the default branch; a path with a space.
    let a = dir.join("feat x");
    let added = add_worktree(&r, &a, "feat-x", None).unwrap();
    assert_eq!(
        added,
        Added {
            created_branch: true,
            base: Some("main".into())
        }
    );
    assert_eq!(branch(&a).as_deref(), Some("feat-x"));
    let l = list_worktrees(&r).unwrap();
    assert!(l
        .iter()
        .any(|w| same(&w.path, &a) && w.branch.as_deref() == Some("feat-x")));
    assert!(branches(&r).contains(&"feat-x".to_string()));

    // Existing branch: checked out, never -b; --base with it is refused.
    sh(&r, &["branch", "old"]);
    let b = dir.join("old");
    assert!(add_worktree(&r, &b, "old", Some("main"))
        .unwrap_err()
        .message
        .contains("already exists"));
    assert!(!add_worktree(&r, &b, "old", None).unwrap().created_branch);
    // A branch checked out elsewhere: git's own message comes through.
    let e = add_worktree(&r, &dir.join("again"), "feat-x", None).unwrap_err();
    assert!(e.message.starts_with("git worktree:"), "{}", e.message);

    // Dirty (untracked) → listed; remove without force fails, with force works.
    std::fs::write(a.join("new.txt"), "x").unwrap();
    assert_eq!(dirty(&a).unwrap(), vec!["new.txt".to_string()]);
    assert!(remove_worktree(&r, &a, false).is_err());
    // Locked: refused even with force.
    sh(
        &r,
        &["worktree", "lock", "--reason", "busy", &a.to_string_lossy()],
    );
    assert!(remove_worktree(&r, &a, true)
        .unwrap_err()
        .message
        .contains("locked (busy)"));
    sh(&r, &["worktree", "unlock", &a.to_string_lossy()]);
    remove_worktree(&r, &a, true).unwrap();
    assert!(!a.exists());

    // Branch delete: -d refuses an unmerged branch, -D deletes it.
    std::fs::write(b.join("c.txt"), "c").unwrap();
    sh(&b, &["add", "-A"]);
    sh(&b, &["commit", "-qm", "c"]);
    remove_worktree(&r, &b, false).unwrap();
    assert!(delete_branch(&r, "old", false)
        .unwrap_err()
        .message
        .contains("not fully merged"));
    delete_branch(&r, "old", true).unwrap();
    delete_branch(&r, "feat-x", false).unwrap();
    assert!(!has_branch(&r, "feat-x"));

    // Detached HEAD in the main checkout still reports a short hash.
    sh(&r, &["checkout", "-q", "--detach"]);
    assert_eq!(branch(&r).map(|b| b.len()), Some(7));

    // Options can't be smuggled in as a base; detached HEAD has no branch.
    assert!(add_worktree(&r, &dir.join("o"), "o", Some("--lock"))
        .unwrap_err()
        .message
        .contains("invalid base"));
    assert_eq!(worktree_branch(&r, &r), None);
    // A folder inside the repository is not a Source repository.
    std::fs::create_dir_all(r.join("sub")).unwrap();
    assert!(add_worktree(&r.join("sub"), &dir.join("s"), "s", None)
        .unwrap_err()
        .message
        .contains("not its top folder"));

    // Not a repository (outside any work tree, whatever TMPDIR is).
    let plain =
        crate::paths::outside_temp_dir().join(format!("rade-git-plain-{}", std::process::id()));
    std::fs::create_dir_all(&plain).unwrap();
    assert!(add_worktree(&plain, &dir.join("p2"), "x", None)
        .unwrap_err()
        .message
        .contains("not a git repository"));
    let _ = std::fs::remove_dir_all(&plain);
    std::fs::remove_dir_all(&r).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// R99.1: a base that exists only on the remote (`origin/release/2.4`) starts
/// the new branch there; git's DWIM must not check out a local
/// `release/2.4` instead, and the new branch tracks nothing. The Review's
/// base resolves the same way (`diff::changes`).
#[test]
fn r99_b_remote_only_base_starts_the_new_branch_not_a_local_copy_of_it() {
    let r = repo("remote-base");
    let bare = r.with_extension("git");
    let _ = std::fs::remove_dir_all(&bare);
    sh(&r, &["init", "-q", "--bare", &bare.to_string_lossy()]);
    sh(&r, &["branch", "release/2.4"]);
    sh(&r, &["remote", "add", "origin", &bare.to_string_lossy()]);
    sh(&r, &["push", "-q", "origin", "main", "release/2.4"]);
    sh(&r, &["branch", "-qD", "release/2.4"]);
    sh(&r, &["fetch", "-q", "origin"]);
    assert_eq!(start_point(&r, "release/2.4"), "origin/release/2.4");
    assert_eq!(start_point(&r, "main"), "main");
    assert_eq!(start_point(&r, "nope"), "nope");

    let w = r.with_extension("wt");
    let _ = std::fs::remove_dir_all(&w);
    let added = add_worktree(&r, &w, "tokens", Some("release/2.4")).unwrap();
    assert_eq!(added.base.as_deref(), Some("release/2.4"));
    assert_eq!(branch(&w).as_deref(), Some("tokens"));
    assert!(!has_branch(&r, "release/2.4"), "no local copy of the base");
    assert!(run(&r, &["config", "--get", "branch.tokens.merge"]).is_err());
    let _ = remove_worktree(&r, &w, true);
    std::fs::remove_dir_all(&bare).unwrap();
    std::fs::remove_dir_all(&r).unwrap();
}

#[test]
fn r131_d_worktree_of_a_repository_without_commits_is_refused() {
    let d = std::env::temp_dir().join(format!("rade-git-r131d-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    sh(&d, &["init", "-q", "-b", "main"]);
    let dir = d.with_file_name(format!("rade-git-r131d-wt-{}", std::process::id()));
    let e = add_worktree(&d, &dir, "feature", None).unwrap_err();
    assert_eq!(
        e.message,
        format!(
            "{} has no commits yet: commit once, then try again",
            d.display()
        )
    );
    assert!(!dir.exists());
    let _ = std::fs::remove_dir_all(&d);
}
