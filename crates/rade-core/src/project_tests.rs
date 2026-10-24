use super::*;

fn tmp(tag: &str) -> PathBuf {
    let d =
        crate::paths::outside_temp_dir().join(format!("rade-project-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const SHOP_V2: &str = r#"name = "Shop"

[[source]]
id = "shop-web"
base = "develop"

[[source]]
id = "shop-api"
base = "develop"

[[source]]
id = "shared-ui"
base = "develop"
"#;

/// The 0.7.1 template's shape (`rade project add` of 0.7.1).
const SHOP_V1: &str =
    "# RADE Project (`rade project show shop`; PRD R28). Named Bases: the first is the default.\n\
name = \"Shop\"\n\
bases = [\"dev\", \"prod\"]\n\
\n\
[[source]]\n\
id = \"shop-web\"\n\
base = \"develop\"\n\
\n\
[[source]]\n\
id = \"shared-ui\" # tokens + components\n\
base = { dev = \"develop\", prod = \"release/2.4\" }\n\
\n\
# Config for this Project's Sources (R29; `rade config explain`).\n\
# [agents.claude]\n";

#[test]
fn r97_a_v2_parses_three_links_with_auto_checkout() {
    let p = Project::parse(
        "shop",
        Path::new("/w/.rade/projects/shop/project.toml"),
        SHOP_V2,
    );
    assert_eq!(p.error, None);
    assert_eq!(p.name, "Shop");
    assert_eq!(p.sources.len(), 3);
    for s in &p.sources {
        assert_eq!(s.base.as_deref(), Some("develop"), "{}", s.source);
        assert_eq!(s.checkout, Checkout::Auto);
    }
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    // `checkout` main / a path, and no base = the main branch.
    let p = Project::parse(
        "x",
        Path::new("/f"),
        "[[source]]\nid = \"a\"\ncheckout = \"main\"\n[[source]]\nid = \"b\"\ncheckout = \"~/src/b\"\n",
    );
    assert_eq!(p.error, None);
    assert_eq!(p.sources[0].checkout, Checkout::Main);
    assert_eq!(p.sources[0].base, None);
    assert_eq!(p.sources[1].checkout, Checkout::Path("~/src/b".into()));
}

#[test]
fn r97_b_migration_takes_first_named_base_keeps_comment_writes_backup() {
    let d = tmp("r97b");
    let f = d.join(FILE);
    std::fs::write(&f, SHOP_V1).unwrap();
    let p = Project::load("shop", &f);
    let new = std::fs::read_to_string(&f).unwrap();
    assert_eq!(p.error, None, "{new}");
    assert_eq!(
        p.source("shared-ui").unwrap().base.as_deref(),
        Some("develop")
    );
    assert_eq!(
        p.source("shop-web").unwrap().base.as_deref(),
        Some("develop")
    );
    assert!(!new.contains("bases"), "{new}");
    assert!(new.contains("base = \"develop\"\n"), "{new}");
    assert!(new.starts_with("# RADE Project"), "comment kept: {new}");
    assert!(
        new.contains("id = \"shared-ui\" # tokens + components"),
        "{new}"
    );
    assert!(new.contains("# [agents.claude]"), "{new}");
    assert_eq!(
        std::fs::read(d.join(V1_BACKUP)).unwrap(),
        SHOP_V1.as_bytes()
    );
    // Only the two changed lines differ.
    let old: Vec<&str> = SHOP_V1.lines().collect();
    let gone: Vec<&&str> = old
        .iter()
        .filter(|l| !new.lines().any(|n| n == **l))
        .collect();
    assert_eq!(
        gone,
        [
            &"bases = [\"dev\", \"prod\"]",
            &"base = { dev = \"develop\", prod = \"release/2.4\" }"
        ]
    );
    // A comment right above `bases` moves to the next key.
    let t = "name = \"S\"\n# dev first\nbases = [\"dev\"]\nfoo = 1\n";
    let (m, _) = migrate_text(t).unwrap().unwrap();
    assert_eq!(m, "name = \"S\"\n# dev first\nfoo = 1\n");
    // A `[source.base]` sub-table form is migrated too.
    let t = "bases = [\"prod\", \"dev\"]\n[[source]]\nid = \"a\"\n[source.base]\ndev = \"d\"\nprod = \"p\"\n";
    let (m, miss) = migrate_text(t).unwrap().unwrap();
    assert!(miss.is_empty());
    let p = Project::parse("x", Path::new("/f"), &m);
    assert_eq!(p.error, None, "{m}");
    assert_eq!(p.sources[0].base.as_deref(), Some("p"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r97_c_second_migration_is_byte_identical_backup_untouched() {
    let d = tmp("r97c");
    let f = d.join(FILE);
    std::fs::write(&f, SHOP_V1).unwrap();
    Project::load("shop", &f);
    let once = std::fs::read(&f).unwrap();
    let bak = d.join(V1_BACKUP);
    let bak_m = std::fs::metadata(&bak).unwrap().modified().unwrap();
    let file_m = std::fs::metadata(&f).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let p = Project::load("shop", &f);
    assert_eq!(p.error, None);
    assert_eq!(std::fs::read(&f).unwrap(), once);
    assert_eq!(std::fs::metadata(&f).unwrap().modified().unwrap(), file_m);
    assert_eq!(std::fs::read(&bak).unwrap(), SHOP_V1.as_bytes());
    assert_eq!(std::fs::metadata(&bak).unwrap().modified().unwrap(), bak_m);
    assert_eq!(
        migrate_text(std::str::from_utf8(&once).unwrap()).unwrap(),
        None
    );
    // A later v1 file (restored by hand) never overwrites the first backup.
    std::fs::write(&f, SHOP_V1.replace("Shop", "Shop2")).unwrap();
    Project::load("shop", &f);
    assert_eq!(std::fs::read(&bak).unwrap(), SHOP_V1.as_bytes());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r97_d_source_without_first_named_base_gets_main_branch_and_one_warning() {
    let d = tmp("r97d");
    let f = d.join(FILE);
    let v1 = "name = \"Billing\"\nbases = [\"dev\", \"prod\"]\n\n[[source]]\nid = \"billing\"\nbase = { prod = \"release/2.4\" }\n\n[[source]]\nid = \"shared-ui\"\nbase = { dev = \"develop\", prod = \"release/2.4\" }\n";
    std::fs::write(&f, v1).unwrap();
    for round in 0..2 {
        let p = Project::load("billing", &f);
        assert_eq!(p.error, None);
        assert_eq!(p.source("billing").unwrap().base, None, "round {round}");
        assert_eq!(p.warnings.len(), 1, "round {round}: {:?}", p.warnings);
        assert!(
            p.warnings[0].contains("'billing'") && p.warnings[0].contains("'dev'"),
            "{:?}",
            p.warnings
        );
    }
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(!text.contains("prod"), "{text}");
    // Once the owner sets a base, the warning goes.
    std::fs::write(
        &f,
        link_set_base("billing", &text, "billing", "main").unwrap(),
    )
    .unwrap();
    assert!(Project::load("billing", &f).warnings.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r97_e_read_only_folder_leaves_file_and_no_backup_or_temp() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("r97e");
    let f = d.join(FILE);
    std::fs::write(&f, SHOP_V1).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root can write anyway: then there is nothing to prove here.
    let writable = std::fs::write(d.join("probe"), "").is_ok();
    if !writable {
        let e = migrate_file(&f, SHOP_V1).unwrap_err();
        assert!(e.message.contains(&f.display().to_string()), "{e}");
        let p = Project::load("shop", &f);
        assert_eq!(p.error, None);
        assert_eq!(
            p.source("shared-ui").unwrap().base.as_deref(),
            Some("develop"),
            "read in memory"
        );
        assert!(p
            .warnings
            .iter()
            .any(|w| w.contains(&f.display().to_string())));
    }
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), SHOP_V1.as_bytes());
    let names: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "probe")
        .collect();
    assert_eq!(names, [FILE], "no backup or temp file left");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r97_g_source_spec_with_equals_is_usage() {
    assert_eq!(
        parse_source_spec("shop-web").unwrap(),
        ("shop-web".into(), None)
    );
    assert_eq!(
        parse_source_spec("shop-web:develop").unwrap(),
        ("shop-web".into(), Some("develop".into()))
    );
    assert_eq!(
        parse_source_spec("shared-ui:release/2.4")
            .unwrap()
            .1
            .as_deref(),
        Some("release/2.4")
    );
    let e = parse_source_spec("shop-web:dev=develop").unwrap_err();
    assert_eq!(e.kind, crate::rpc::ErrorKind::Usage);
    assert!(e.message.contains("--source <src>[:<branch>]"), "{e}");
    for bad in ["", ":develop", "a:"] {
        let e = parse_source_spec(bad).unwrap_err();
        assert!(
            e.message.contains("--source <src>[:<branch>]"),
            "{bad}: {e}"
        );
    }
}

#[test]
fn r97_h_link_edits_change_only_their_lines() {
    let first = template(
        "Shop",
        &[
            ("shop-web".into(), Some("develop".into())),
            ("shop-api".into(), Some("develop".into())),
            ("shared-ui".into(), Some("develop".into())),
        ],
    );
    assert_eq!(Project::parse("shop", Path::new("/f"), &first).error, None);
    let lines = |s: &str| s.lines().map(String::from).collect::<Vec<_>>();
    let added = |a: &str, b: &str| -> Vec<String> {
        let mut a = lines(a);
        lines(b)
            .into_iter()
            .filter(|l| match a.iter().position(|x| x == l) {
                Some(i) => {
                    a.remove(i);
                    false
                }
                None => true,
            })
            .collect()
    };
    let a = link_add("shop", &first, "billing", Some("main"), None).unwrap();
    let mut new = added(&first, &a);
    new.sort();
    assert_eq!(
        new,
        ["", "[[source]]", "base = \"main\"", "id = \"billing\""],
        "{a}"
    );
    assert!(
        a.contains("base = \"develop\"\n\n[[source]]\nid = \"billing\"\nbase = \"main\"\n"),
        "{a}"
    );
    assert!(added(&a, &first).is_empty(), "nothing else changed");
    let b = link_set_base("shop", &a, "shared-ui", "release/2.5").unwrap();
    assert_eq!(added(&a, &b), ["base = \"release/2.5\""]);
    assert_eq!(added(&b, &a), ["base = \"develop\""]);
    let c = link_remove("shop", &b, "billing").unwrap();
    assert_eq!(
        c,
        first.replacen(
            "id = \"shared-ui\"\nbase = \"develop\"",
            "id = \"shared-ui\"\nbase = \"release/2.5\"",
            1
        )
    );
    // Refusals leave the text to the caller (an Err).
    assert!(
        link_add("shop", &first, "shop-web", None, None).is_err(),
        "linked"
    );
    assert!(link_remove("shop", &first, "nope").is_err());
    assert!(link_set_base("shop", &first, "nope", "x").is_err());
    let with_checkout = link_add("shop", &first, "notes", None, Some(&Checkout::Main)).unwrap();
    let p = Project::parse("shop", Path::new("/f"), &with_checkout);
    assert_eq!(p.source("notes").unwrap().checkout, Checkout::Main);
    assert_eq!(p.source("notes").unwrap().base, None);
}

#[test]
fn config_files_that_are_not_regular_or_too_big_are_errors_not_hangs() {
    let d = tmp("read");
    // A repository can ship `.rade.toml -> /dev/zero` (endless) …
    let zero = d.join("zero.toml");
    std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
    assert!(read_file(&zero)
        .unwrap_err()
        .to_string()
        .contains("not a regular file"));
    // … or a FIFO nobody writes to (would block).
    let fifo = d.join("fifo.toml");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    assert!(read_file(&fifo).is_err());
    let big = d.join("big.toml");
    std::fs::write(&big, vec![b'#'; MAX_FILE as usize + 1]).unwrap();
    assert!(read_file(&big).unwrap_err().to_string().contains("too big"));
    std::fs::write(&big, "a = 1").unwrap();
    assert_eq!(read_file(&big).unwrap(), "a = 1");
    assert_eq!(
        read_file(&d.join("none")).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn r28_d_broken_files_report_with_the_path() {
    let p = Project::parse("x", Path::new("/w/x/project.toml"), "name = ");
    assert!(p
        .error
        .as_deref()
        .unwrap()
        .starts_with("/w/x/project.toml: "));
    assert_eq!(p.name, "x");
    assert!(resolve_base(&p, "a", None).is_err());
    for bad in [
        "[[source]]\nbase = \"x\"",
        "[[source]]\nid = \"a\"\n[[source]]\nid = \"a\"",
        "[[source]]\nid = \"a\"\nbase = 3",
        "[[source]]\nid = \"a\"\nbogus = 1",
        "[[source]]\nid = \"a\"\ncheckout = \"\"",
        "[[source]]\nid = \"a\"\ncheckout = 1",
    ] {
        assert!(
            Project::parse("x", Path::new("/f"), bad).error.is_some(),
            "{bad}"
        );
    }
    let p = Project::parse(
        "x",
        Path::new("/f"),
        "bogus = 1\nlimits = {}\n[[source]]\nid=\"a\"",
    );
    assert_eq!(p.error, None);
    assert_eq!(
        p.warnings,
        ["unknown key 'bogus'", "limits: ignored (app-wide)"]
    );
}

#[test]
fn r99_resolve_base_is_the_given_ref_else_the_links() {
    let p = Project::parse("shop", Path::new("/f"), SHOP_V2);
    assert_eq!(
        resolve_base(&p, "shop-web", None).unwrap().as_deref(),
        Some("develop")
    );
    assert_eq!(
        resolve_base(&p, "shop-web", Some("main"))
            .unwrap()
            .as_deref(),
        Some("main")
    );
    let p = Project::parse("x", Path::new("/f"), "[[source]]\nid = \"a\"\n");
    assert_eq!(resolve_base(&p, "a", None).unwrap(), None, "main branch");
    assert!(resolve_base(&p, "b", None)
        .unwrap_err()
        .message
        .contains("not in project 'x'"));
}

#[test]
fn list_reads_folders_with_a_project_toml_sorted() {
    let root = tmp("list");
    for (id, text) in [("b", "name = \"B\""), ("a", "name = ")] {
        let f = Project::path(&root, id);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, text).unwrap();
    }
    std::fs::create_dir_all(root.join(DIR).join("empty")).unwrap();
    let l = list(&root);
    assert_eq!(
        l.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(l[0].error.is_some() && l[1].name == "B");
    let before = signature(&root);
    std::fs::write(Project::path(&root, "b"), "name = \"Bee\"").unwrap();
    assert_ne!(signature(&root), before);
    assert!(list(&root.join("none")).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn r97_set_key_changes_one_line_and_keeps_the_rest() {
    let text = "# Shop\nname = \"Shop\" # shown\nfoo = 1 # unknown kept\n\n[[source]]\nid = \"app\"\nbase = \"develop\" # dev\n\n[[source]]\nid = \"api\" # backend\n";
    let diff = |a: &str, b: &str| -> Vec<String> {
        b.lines()
            .filter(|l| !a.lines().any(|x| x == *l))
            .map(String::from)
            .collect()
    };
    // The 0.7.1 keys and the removed engines' keys answer USAGE (R97.3).
    for k in [
        "bases",
        "source.app.base.dev",
        "task.verify",
        "batch.parallel",
    ] {
        let e = set_key("shop", text, k, "x").unwrap_err();
        assert_eq!(e.kind, crate::rpc::ErrorKind::Usage, "{k}");
        assert!(e.message.contains(&format!("unknown key '{k}'")), "{e}");
    }
    let b = set_key("shop", text, "source.app.base", "release/2.5").unwrap();
    assert_eq!(diff(text, &b), ["base = \"release/2.5\" # dev"]);
    let b = set_key("shop", text, "source.api.base", "main").unwrap();
    assert_eq!(diff(text, &b), ["base = \"main\""]);
    assert!(b.contains("id = \"api\" # backend"));
    let c = set_key("shop", text, "source.app.checkout", "main").unwrap();
    assert_eq!(diff(text, &c), ["checkout = \"main\""]);
    let n = set_key("shop", text, "name", "Shop 2").unwrap();
    assert_eq!(diff(text, &n), ["name = \"Shop 2\" # shown"]);
    let r = set_key("shop", text, "source.app.base", "").unwrap();
    assert!(!r.contains("develop"));
    for (k, v) in [
        ("nope", "x"),
        ("source.web.base", "x"),
        ("name", "a\nb"),
        ("source.app.checkout", "   "),
    ] {
        if k == "source.app.checkout" {
            // An empty value removes the key: allowed.
            assert!(set_key("shop", text, k, v).is_ok());
            continue;
        }
        assert!(set_key("shop", text, k, v).is_err(), "{k}={v}");
    }
}

#[test]
fn r99_1_project_of_a_new_worktree_is_the_only_link() {
    let p = |id: &str, text: &str| Project::parse(id, Path::new("/x"), text);
    let shop = p(
        "shop",
        "[[source]]\nid = \"shop-web\"\n[[source]]\nid = \"shared-ui\"\n",
    );
    let billing = p(
        "billing",
        "[[source]]\nid = \"shared-ui\"\nbase = \"release/2.4\"\n",
    );
    let broken = p("x", "[[source]]\nnope = 1\n");
    let all = [shop, billing, broken];
    assert_eq!(infer(&all, "shop-web"), Ok(Some("shop".into())));
    assert_eq!(infer(&all, "billing"), Ok(None), "no Project: Scratch");
    let e = infer(&all, "shared-ui").unwrap_err();
    assert_eq!(e.kind, crate::rpc::ErrorKind::Usage);
    assert_eq!(
        e.message,
        "'shared-ui' is in several Projects: pass --project (billing, shop)"
    );
}

#[test]
fn r106_b_template_links_write_checkout_auto() {
    let t = template_links(
        "Checkout revamp",
        &[
            (
                "shop-web".into(),
                Some("develop".into()),
                Some("auto".into()),
            ),
            (
                "shared-ui".into(),
                Some("release/2.5".into()),
                Some("auto".into()),
            ),
        ],
    );
    let p = Project::parse("checkout-revamp", Path::new("/f"), &t);
    assert_eq!(p.error, None, "{t}");
    assert_eq!(p.sources.len(), 2);
    assert_eq!(t.matches("checkout = \"auto\"").count(), 2, "{t}");
    assert_eq!(p.sources[1].base.as_deref(), Some("release/2.5"));
    assert!(!template("Shop", &[("a".into(), None)]).contains("checkout ="));
}
