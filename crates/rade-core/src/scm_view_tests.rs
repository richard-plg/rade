use super::*;
use crate::scm::parse_status;

fn item(path: &str, letter: char) -> Item {
    Item {
        path: path.into(),
        orig: None,
        letter,
        section: Some(Section::Unstaged),
    }
}

fn names(rows: &[Row], items: &[Item]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            Row::Folder { name, depth, .. } => format!("{}{name}", " ".repeat(*depth)),
            Row::File { index, depth, .. } => {
                format!("{}{}", " ".repeat(*depth), items[*index].path)
            }
        })
        .collect()
}

#[test]
fn r120_a_groups_merge_staged_changes() {
    let st = parse_status(
        b"# branch.head main\0\
u UU N... 100644 100644 100644 100644 a b c conflict.rs\0\
1 A. N... 000000 100644 100644 0 b staged.rs\0\
1 MM N... 100644 100644 100644 a b both.rs\0\
1 .M N... 100644 100644 100644 a b mod.rs\0\
? new.rs\0",
    );
    let g = groups(&st);
    let kinds: Vec<Group> = g.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, vec![Group::Merge, Group::Staged, Group::Changes]);
    let letters = |i: usize| -> String { g[i].1.iter().map(|x| x.letter).collect() };
    assert_eq!((g[0].1.len(), letters(0)), (1, "!".to_string()));
    assert_eq!(g[0].1[0].section, None);
    assert_eq!((g[1].1.len(), letters(1)), (2, "AM".to_string()));
    assert!(g[1].1.iter().all(|x| x.section == Some(Section::Staged)));
    assert_eq!((g[2].1.len(), letters(2)), (3, "MMU".to_string()));
    assert_eq!(g[2].1[2].section, Some(Section::Untracked));
    assert_eq!(Group::Staged.title(), "STAGED CHANGES");
    // No conflict → no Merge Changes group.
    let st = parse_status(b"# branch.head main\0? new.rs\0");
    assert_eq!(groups(&st).len(), 1);
    assert_eq!(groups(&st)[0].0, Group::Changes);
}

#[test]
fn r120_b_tree_and_sorts() {
    let items = vec![
        item("src/ui/a.rs", 'M'),
        item("src/ui/b.rs", 'M'),
        item("src/main.rs", 'U'),
        item("README.md", 'M'),
        item("a/b/c/one.rs", 'U'),
    ];
    let none = HashSet::new();
    let t = rows(&items, ScmView::Tree, ScmSort::Path, &none);
    assert_eq!(
        names(&t, &items),
        vec![
            "a/b/c/",
            " a/b/c/one.rs",
            "src/",
            " ui/",
            "  src/ui/a.rs",
            "  src/ui/b.rs",
            " src/main.rs",
            "README.md"
        ]
    );
    // Folding a folder leaves its children out.
    let f: HashSet<String> = ["src/ui".to_string()].into();
    let t = rows(&items, ScmView::Tree, ScmSort::Path, &f);
    assert_eq!(
        names(&t, &items),
        vec![
            "a/b/c/",
            " a/b/c/one.rs",
            "src/",
            " ui/",
            " src/main.rs",
            "README.md"
        ]
    );
    assert!(matches!(&t[3], Row::Folder { path, folded: true, .. } if path == "src/ui"));

    let l = rows(&items, ScmView::List, ScmSort::Name, &none);
    let order: Vec<&str> = l
        .iter()
        .map(|r| match r {
            Row::File { name, .. } => name.as_str(),
            _ => "?",
        })
        .collect();
    assert_eq!(
        order,
        vec!["a.rs", "b.rs", "main.rs", "one.rs", "README.md"]
    );
    let l = rows(&items, ScmView::List, ScmSort::Status, &none);
    let order: Vec<&str> = l
        .iter()
        .map(|r| match r {
            Row::File { index, .. } => items[*index].path.as_str(),
            _ => "?",
        })
        .collect();
    assert_eq!(
        order,
        vec![
            "README.md",
            "src/ui/a.rs",
            "src/ui/b.rs",
            "a/b/c/one.rs",
            "src/main.rs"
        ]
    );
    let l = rows(&items, ScmView::List, ScmSort::Path, &none);
    if let Row::File {
        name, dir, depth, ..
    } = &l[1]
    {
        assert_eq!(
            (name.as_str(), dir.as_str(), *depth),
            ("one.rs", "a/b/c/", 0)
        );
    } else {
        panic!("a file");
    }
}

fn labels(s: &CommitState) -> Vec<(&'static str, bool)> {
    commit_menu(s)
        .iter()
        .map(|m| (m.label, m.enabled))
        .collect()
}

#[test]
fn r121_b_commit_menu_enablement_and_smart_prompt() {
    let s = CommitState {
        staged: 0,
        changes: 2,
        message: true,
        head: true,
        head_pushed: false,
    };
    assert_eq!(
        labels(&s),
        vec![
            ("Commit", true),
            ("Commit (Amend)", true),
            ("Commit & Push", true),
            ("Commit All", true),
            ("Undo Last Commit", true)
        ]
    );
    assert!(commit_enabled(&s) && smart_prompt(&s));
    // Staged: no smart prompt.
    let staged = CommitState { staged: 1, ..s };
    assert!(commit_enabled(&staged) && !smart_prompt(&staged));
    // No message: Commit and Commit All off, Amend and Undo stay.
    let nomsg = CommitState {
        message: false,
        ..s
    };
    assert_eq!(
        labels(&nomsg),
        vec![
            ("Commit", false),
            ("Commit (Amend)", true),
            ("Commit & Push", false),
            ("Commit All", false),
            ("Undo Last Commit", true)
        ]
    );
    assert!(!smart_prompt(&nomsg));
    // Pushed HEAD: Amend and Undo off.
    let pushed = CommitState {
        head_pushed: true,
        ..s
    };
    assert!(!labels(&pushed)[1].1 && !labels(&pushed)[4].1 && labels(&pushed)[0].1);
    // Nothing changed, no HEAD.
    let empty = CommitState {
        staged: 0,
        changes: 0,
        message: true,
        head: false,
        head_pushed: false,
    };
    assert!(labels(&empty).iter().all(|(_, e)| !e));
    assert!(!smart_prompt(&empty));
}

#[test]
fn r121_c_subject_warning_and_history() {
    let ok = "x".repeat(72);
    assert_eq!(subject_warning(&ok), None);
    assert_eq!(
        subject_warning(&format!("{ok}y\nbody")).as_deref(),
        Some("Subject: 73 characters (keep it ≤ 72)")
    );
    // Characters, not bytes.
    assert_eq!(subject_warning(&"é".repeat(72)), None);
    assert_eq!(
        subject_warning("short\nsecond line is long ".repeat(10).as_str()),
        None
    );

    let mut h = History::new(
        vec!["new".into(), "mid".into(), "old".into()],
        "draft".into(),
    );
    assert_eq!(h.down(), None, "already on the draft");
    assert_eq!(h.up(), Some("new"));
    assert_eq!(h.up(), Some("mid"));
    assert_eq!(h.up(), Some("old"));
    assert_eq!(h.up(), None, "the oldest stays");
    assert_eq!(h.down(), Some("mid"));
    assert_eq!(h.down(), Some("new"));
    assert_eq!(h.down(), Some("draft"));
    assert_eq!(h.down(), None);
    assert_eq!(History::new(vec![], "d".into()).up(), None);
}

#[test]
fn action_button_states() {
    let mut st = parse_status(b"# branch.head feat\0");
    assert_eq!(
        action_button(&st, false, true).as_deref(),
        Some("Publish Branch")
    );
    assert_eq!(action_button(&st, false, false), None);
    assert_eq!(action_button(&st, true, true), None);
    assert_eq!(sync_label(&st), Some(("publish", String::new())));
    st.upstream = Some("origin/feat".into());
    st.ahead = 1;
    st.behind = 2;
    assert_eq!(
        action_button(&st, false, true).as_deref(),
        Some("Sync Changes ↑1 ↓2")
    );
    assert_eq!(sync_label(&st), Some(("sync", "↑1 ↓2".to_string())));
    st.ahead = 0;
    st.behind = 0;
    assert_eq!(action_button(&st, false, true), None);
    // Upstream is another branch: not its own.
    st.upstream = Some("origin/main".into());
    assert_eq!(
        action_button(&st, false, true).as_deref(),
        Some("Publish Branch")
    );
    let det = parse_status(b"# branch.head (detached)\0");
    assert_eq!(
        (action_button(&det, false, true), sync_label(&det)),
        (None, None)
    );
    assert_eq!(header_title(Some("Shop"), "shop-web"), "PROJECT · SHOP");
    assert_eq!(header_title(None, "shop-api"), "SOURCE · SHOP-API");
    assert_eq!(
        placeholder(Some("feat")),
        "Message (Ctrl+Enter to commit on ‘feat’)"
    );
    assert_eq!(
        placeholder(None),
        "Message (Ctrl+Enter to commit on ‘(detached)’)"
    );
}

#[test]
fn discard_dialog_texts() {
    assert_eq!(
        discard_dialog(&["a.rs"], 1, 0),
        (
            "Discard changes in “a.rs”?".into(),
            "Its unstaged changes are lost; staged changes stay.".into(),
            "Discard".into()
        )
    );
    assert_eq!(
        discard_dialog(&["u.log"], 0, 1),
        (
            "Delete “u.log”?".into(),
            "The file is not tracked by git. It goes to the trash.".into(),
            "Move to Trash".into()
        )
    );
    assert_eq!(
        discard_dialog(&[], 2, 1),
        (
            "Discard changes in 3 files?".into(),
            "Unstaged changes in 2 tracked files are lost; 1 untracked file goes to the trash. Staged changes stay.".into(),
            "Discard 3 Files".into()
        )
    );
    assert_eq!(
        discard_dialog(&[], 1, 1).1,
        "Unstaged changes in 1 tracked file are lost; 1 untracked file goes to the trash. Staged changes stay."
    );
    assert_eq!(
        discard_dialog(&[], 3, 0).1,
        "Unstaged changes in 3 tracked files are lost. Staged changes stay."
    );
    assert_eq!(
        discard_dialog(&[], 0, 2).1,
        "2 untracked files go to the trash. Staged changes stay."
    );
}

#[test]
fn r120_missing_checkout_is_calm_not_an_error() {
    assert_eq!(card_read(false, false), CardRead::NoCheckout);
    // Nothing is run for it, so a failure cannot be reported.
    assert_eq!(card_read(false, true), CardRead::NoCheckout);
    assert_eq!(card_read(true, true), CardRead::Failed);
    assert_eq!(card_read(true, false), CardRead::Ready);
    assert_eq!(NO_CHECKOUT, "No checkout yet");
}
