use super::*;

const HOME: &str = "/home/u";

fn home() -> &'static Path {
    Path::new(HOME)
}

#[test]
fn r25_includeif_conditions_and_root_matching_normalize_every_form() {
    let text = "[core]\n\tx = 1\n[includeIf \"gitdir:~/ws/acme/\"]\n\tpath = a\n  [IncludeIf \"gitdir/i:/HOME/U/WS/Bravo/**\"]\n[includeIf \"onbranch:main\"]\n";
    assert_eq!(
        includeif_conditions(text),
        [
            "gitdir:~/ws/acme/",
            "gitdir/i:/HOME/U/WS/Bravo/**",
            "onbranch:main"
        ]
    );
    let root = Path::new("/home/u/ws/acme");
    assert!(
        condition_matches_root("gitdir:~/ws/acme/", root, home()),
        "~/"
    );
    assert!(
        condition_matches_root("gitdir:/home/u/ws/acme/", root, home()),
        "absolute"
    );
    // git adds `**` only after a trailing `/`: without it the pattern
    // matches the one gitdir `/home/u/ws/acme`, not the repos under it.
    assert!(
        !condition_matches_root("gitdir:/home/u/ws/acme", root, home()),
        "no slash: not the root's repos"
    );
    assert!(!condition_matches_root("gitdir:~/ws/acme/*", root, home()));
    assert!(
        condition_matches_root("gitdir/i:~/WS/Acme/", root, home()),
        "/i folds case"
    );
    assert!(
        !condition_matches_root("gitdir:~/WS/Acme/", root, home()),
        "case-sensitive"
    );
    assert!(
        !condition_matches_root("gitdir:~/ws/", root, home()),
        "a parent is not the root"
    );
    assert!(!condition_matches_root("gitdir:~/ws/acme-2/", root, home()));
    assert!(!condition_matches_root("onbranch:acme", root, home()));
    assert!(condition_matches_root(
        "gitdir/i:/HOME/U/WS/Bravo/**",
        Path::new("/home/u/ws/bravo"),
        home()
    ));
}

#[test]
fn r25_gitconfig_block_added_once_replaced_on_move_skipped_when_hand_written() {
    let root = Path::new("/home/u/ws/acme");
    let user = "[user]\n\tuseConfigOnly = true\n";
    let once = gitconfig_block(user, "acme", root, home()).unwrap();
    assert_eq!(
        once,
        "[user]\n\tuseConfigOnly = true\n\n# >>> rade workspace acme >>>\n[includeIf \"gitdir:~/ws/acme/\"]\n\tpath = ~/ws/acme/.gitconfig\n# <<< rade workspace acme <<<\n"
    );
    assert_eq!(
        gitconfig_block(&once, "acme", root, home()),
        None,
        "idempotent"
    );
    let moved = gitconfig_block(&once, "acme", Path::new("/srv/acme"), home()).unwrap();
    assert!(moved.contains("[includeIf \"gitdir:/srv/acme/\"]\n\tpath = /srv/acme/.gitconfig\n"));
    assert_eq!(
        moved.matches(">>> rade workspace acme").count(),
        1,
        "in place"
    );
    assert!(!moved.contains("~/ws/acme"));
    let hand = format!("{user}[includeIf \"gitdir:/home/u/ws/acme/\"]\n\tpath = x\n");
    assert_eq!(gitconfig_block(&hand, "acme", root, home()), None);
    // Our own block alone never counts as hand-written.
    let other = gitconfig_block(&once, "bravo", Path::new("/home/u/ws/bravo"), home()).unwrap();
    assert_eq!(other.matches(">>> rade workspace").count(), 2);
}

#[test]
fn r25_gitconfig_key_plan_follows_the_identity() {
    let mut id = Identity {
        git_name: Some("Ann".into()),
        git_email: Some("ann@acme.test".into()),
        github: Some("ann".into()),
        ssh_key: Some("id_acme".into()),
        sign: Some(Sign::Ssh),
    };
    let keys = gitconfig_keys(&id, home());
    assert_eq!(
        keys,
        [
            ("user.name", "Ann".to_string()),
            ("user.email", "ann@acme.test".into()),
            (
                "core.sshCommand",
                "ssh -i /home/u/.ssh/id_acme -o IdentitiesOnly=yes".into()
            ),
            ("gpg.format", "ssh".into()),
            ("user.signingkey", "/home/u/.ssh/id_acme.pub".into()),
            ("commit.gpgsign", "true".into()),
            ("tag.gpgsign", "true".into()),
        ]
    );
    id.sign = Some(Sign::None);
    assert_eq!(gitconfig_keys(&id, home()).len(), 3, "no signing keys");
}

#[test]
fn r25_merge_toml_fills_gaps_and_reports_conflicts() {
    let old = "# mine\nname = \"Acme\"\n[env]\nGH_CONFIG_DIR = \"~/.config/gh-x\"\n";
    let (new, conflicts) = merge_toml(
        Some(old),
        &[
            ("", "name", "Acme".into()),
            ("env", "GH_CONFIG_DIR", "~/.config/gh-acme".into()),
            ("identity", "git_email", "a@acme.test".into()),
        ],
    )
    .unwrap();
    assert_eq!(conflicts, ["env.GH_CONFIG_DIR = \"~/.config/gh-x\""]);
    assert!(
        new.starts_with("# mine\nname = \"Acme\"\n"),
        "comments kept"
    );
    assert!(new.contains("GH_CONFIG_DIR = \"~/.config/gh-x\""), "left");
    assert!(new.contains("[identity]\ngit_email = \"a@acme.test\"\n"));
    let f = crate::workspace::WorkspaceFile::parse(&new).unwrap();
    assert_eq!(f.identity.git_email.as_deref(), Some("a@acme.test"));
    assert_eq!(merge_toml(Some(&new), &[]).unwrap().0, new);
    assert!(merge_toml(Some("x = ["), &[]).is_err());
}

#[test]
fn r25_login_argv_key_matching_and_versions() {
    let a = login_argv(false, true).join(" ");
    assert!(a.starts_with("gh auth login --hostname github.com --git-protocol ssh"));
    assert!(a.contains("--insecure-storage"));
    assert!(a.contains("--scopes admin:public_key,admin:ssh_signing_key"));
    assert!(!a.contains("--skip-ssh-key"));
    assert!(login_argv(true, false)
        .join(" ")
        .ends_with("--scopes admin:public_key --skip-ssh-key"));
    let listed = "ssh-ed25519 AAAAother\nssh-ed25519 AAAAmine\n";
    assert!(key_on_account("ssh-ed25519 AAAAmine ann@acme\n", listed));
    assert!(!key_on_account("ssh-ed25519 AAAAnew ann@acme", listed));
    assert!(!key_on_account("", listed));
    assert!(version_at_least("git version 2.53.0", (2, 36)));
    assert!(!version_at_least("git version 2.34.1", (2, 36)));
    assert!(version_at_least(
        "gh version 2.46.0 (2025-12-13 Ubuntu 2.46.0-4)",
        (2, 40)
    ));
    assert!(!version_at_least("gh version 2.39.9", (2, 40)));
    assert!(version_at_least("git version 3.0", (2, 36)));
    assert!(!version_at_least("nonsense", (2, 36)));
}

#[test]
fn r25_3_import_infers_identity_owner_and_key() {
    let raw = b"user.name\nAnn\0user.email\nann@acme.test\0user.signingkey\n/home/u/.ssh/id_acme.pub\0gpg.format\nssh\0commit.gpgsign\ntrue\0url.github-acme:.insteadof\ngit@github.com:\0core.bare\0";
    let cfg = parse_config_z(raw);
    assert_eq!(cfg.get("core.bare").map(String::as_str), Some("true"));
    let id = infer_identity(&cfg, None, home());
    assert_eq!(id.git_name.as_deref(), Some("Ann"));
    assert_eq!(id.ssh_key.as_deref(), Some("id_acme"), "from signingkey");
    assert!(id.signs());
    assert_eq!(insteadof_host(&cfg).as_deref(), Some("github-acme"));
    // No signingkey: sshCommand's -i, else the alias's IdentityFile.
    let mut cfg2 = BTreeMap::new();
    cfg2.insert(
        "core.sshcommand".to_string(),
        "ssh -i ~/.ssh/id_b -o IdentitiesOnly=yes".to_string(),
    );
    assert_eq!(
        infer_identity(&cfg2, None, home()).ssh_key.as_deref(),
        Some("id_b")
    );
    assert!(!infer_identity(&cfg2, None, home()).signs());
    let alias =
        ssh_g_identityfile("user git\nidentityfile ~/.ssh/id_c\nidentityfile ~/.ssh/id_rsa\n");
    assert_eq!(alias.as_deref(), Some("~/.ssh/id_c"));
    assert_eq!(
        infer_identity(&BTreeMap::new(), alias.as_deref(), home())
            .ssh_key
            .as_deref(),
        Some("id_c")
    );
    assert_eq!(ssh_key_name("/etc/keys/k", home()), None, "outside ~/.ssh");
    let mut c3 = BTreeMap::new();
    c3.insert(
        "url.git@github.com:.insteadof".to_string(),
        "https://github.com/".to_string(),
    );
    assert_eq!(insteadof_host(&c3).as_deref(), Some("github.com"));
    assert_eq!(ssh_key_from_command("ssh -i/k/x"), Some("/k/x".into()));
    for (url, owner) in [
        ("git@github.com:acme-ai/api.git", "acme-ai"),
        ("github-acme:acme-ai/web", "acme-ai"),
        ("https://github.com/ann/dots", "ann"),
        ("ssh://git@github.com/ann/dots.git", "ann"),
    ] {
        assert_eq!(owner_of(url).as_deref(), Some(owner), "{url}");
    }
    assert_eq!(owner_of("/srv/git/x.git"), None);
    assert_eq!(
        most_common_owner([
            "github-acme:acme-ai/a",
            "git@github.com:ann/b",
            "https://github.com/acme-ai/c"
        ])
        .as_deref(),
        Some("acme-ai")
    );
    assert_eq!(most_common_owner([]), None);
    assert_eq!(
        tilde(Path::new("/home/u/.config/gh-a"), home()),
        "~/.config/gh-a"
    );
    assert_eq!(tilde(Path::new("/srv/x"), home()), "/srv/x");
}

#[test]
fn r26_hosts_yml_token_only_counts_a_real_key() {
    assert!(hosts_has_token(
        "github.com:\n    oauth_token: gho_x\n    user: ann\n"
    ));
    assert!(!hosts_has_token(
        "github.com:\n    user: ann\n    git_protocol: ssh\n"
    ));
    assert!(!hosts_has_token("# oauth_token: none\n"));
}

#[test]
fn r26_rc_lines_name_the_file_and_line_that_set_a_variable() {
    let files = vec![
        (
            PathBuf::from("/h/.zshrc"),
            "alias x=y\nexport GH_CONFIG_DIR=~/.config/gh-globex\n# GH_CONFIG_DIR=no\n".to_string(),
        ),
        (
            PathBuf::from("/h/.bashrc"),
            "export GH_CONFIG_DIR_X=1\n".to_string(),
        ),
        (
            PathBuf::from("/h/.config/zsh/hooks.zsh"),
            "  [[ $PWD == ~/w/* ]] && GH_CONFIG_DIR=$HOME/x\n".to_string(),
        ),
    ];
    assert_eq!(
        rc_lines_setting("GH_CONFIG_DIR", &files),
        vec!["/h/.zshrc:2", "/h/.config/zsh/hooks.zsh:1"]
    );
}

#[test]
fn r26_claude_json_lives_beside_home_for_the_default_dir() {
    let home = Path::new("/h");
    assert_eq!(
        claude_json(Path::new("/h/.claude"), home),
        PathBuf::from("/h/.claude.json")
    );
    assert_eq!(
        claude_json(Path::new("/h/.claude-work"), home),
        PathBuf::from("/h/.claude-work/.claude.json")
    );
    assert!(claude_logged_in(
        r#"{"oauthAccount": {"emailAddress": "a@b"}}"#
    ));
    assert!(!claude_logged_in(r#"{"oauthAccount": {}}"#));
    assert!(!claude_logged_in("not json"));
}

#[test]
fn r26_includeif_keys_from_git_config_get_regexp() {
    let out = "includeif.gitdir:~/ws/acme/.path ~/ws/acme/.gitconfig\nincludeif.gitdir/i:/abs/x/.path /abs/x/.gitconfig\n";
    assert_eq!(
        includeif_conditions_from_regexp(out),
        vec!["gitdir:~/ws/acme/", "gitdir/i:/abs/x/"]
    );
}
