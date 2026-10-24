use super::*;
use crate::config::DesktopNotify;
use crate::SplitDir;
use std::path::Path;

/// Source x: Tab 1 = [a | b], Tab 2 = [c]; General has Tab 1 = [g] and is on
/// screen. Returns (workspace, a, b, c, g).
fn ws() -> (State, String, String, String, String) {
    let mut w = State::new(Path::new("/home/u"));
    let (gp, gw) = {
        let (p, wt) = w.active();
        (p.id.clone(), wt.id.clone())
    };
    let (_, g) = w.new_tab(&gp, &gw, None, None).unwrap();
    w.add_source(Path::new("/tmp/x"), None).unwrap();
    let xw = w.source("x").unwrap().worktrees[0].id.clone();
    let (_, a) = w.new_tab("x", &xw, None, Some("lead".into())).unwrap();
    let b = w.split(&a, SplitDir::Right, None, None).unwrap();
    let (_, c) = w.new_tab("x", &xw, None, None).unwrap();
    w.select_source("general").unwrap();
    (w, a, b, c, g)
}

#[test]
fn notify_stores_path_and_ids() {
    let (mut w, a, _, _, _) = ws();
    let id = w.notify(&a, Some("T".into()), "hello", 10).unwrap();
    let n = w.notification(id).unwrap();
    assert_eq!(
        (n.pane.as_str(), n.title.as_deref(), n.text.as_str(), n.read),
        (a.as_str(), Some("T"), "hello", false)
    );
    assert_eq!(n.path, "x / main / Tab 1 / lead");
    let id2 = w.notify(&a, None, "other", 10).unwrap();
    assert!(id2 > id, "ids grow");
}

#[test]
fn r3_1_bursts_of_the_same_text_on_the_same_pane_collapse() {
    let (mut w, a, b, _, _) = ws();
    assert!(w.notify(&a, None, "build done", 100).is_some());
    assert_eq!(w.notify(&a, None, "build done", 101), None, "1 s later");
    assert_eq!(
        w.notify(&a, None, "build done", 102),
        None,
        "2 s after the last"
    );
    assert_eq!(w.notifications.len(), 1);
    assert_eq!(w.notifications[0].time, 102, "time refreshed");
    assert!(
        w.notify(&b, None, "build done", 102).is_some(),
        "other Pane"
    );
    assert!(
        w.notify(&a, None, "other text", 102).is_some(),
        "other text"
    );
    assert!(
        w.notify(&a, Some("T".into()), "build done", 102).is_some(),
        "other title"
    );
    assert!(
        w.notify(&a, None, "build done", 105).is_some(),
        "3 s later is new"
    );
    assert_eq!(w.notifications.len(), 5);
}

#[test]
fn notifications_are_capped() {
    let (mut w, a, _, _, _) = ws();
    for i in 0..(MAX_NOTIFICATIONS + 5) {
        w.notify(&a, None, &format!("n{i}"), 0);
    }
    assert_eq!(w.notifications.len(), MAX_NOTIFICATIONS);
    assert_eq!(w.notifications[0].text, "n5");
}

#[test]
fn r3_4_unread_counts_roll_up_pane_tab_worktree_source() {
    let (mut w, a, b, c, g) = ws();
    w.notify(&a, None, "1", 0);
    w.notify(&a, None, "2", 0);
    w.notify(&b, None, "3", 0);
    w.notify(&c, None, "4", 0);
    w.notify(&g, None, "5", 0);
    let (ta, tc, wa) = {
        let (la, lc) = (w.locate(&a).unwrap(), w.locate(&c).unwrap());
        (la.tab, lc.tab, la.worktree)
    };
    let u = w.unread_counts();
    assert_eq!(u.pane(&a), 2);
    assert_eq!(u.pane(&b), 1);
    assert_eq!(u.tab(&ta), 3);
    assert_eq!(u.tab(&tc), 1);
    assert_eq!(u.worktree(&wa), 4);
    assert_eq!(u.source("x"), 4);
    assert_eq!(u.source("general"), 1);
    assert_eq!(u.total, 5);

    // Focusing a Pane reads its Notifications only.
    w.focus_pane(&a).unwrap();
    let u = w.unread_counts();
    assert_eq!((u.pane(&a), u.tab(&ta), u.source("x")), (0, 1, 2));
    assert_eq!(w.unread(&b), 1);

    // A closed Pane's Notifications stay unread in the total only.
    w.close_pane(&c).unwrap();
    let u = w.unread_counts();
    assert_eq!((u.source("x"), u.total), (1, 3));
}

#[test]
fn r3_3_activating_focuses_the_pane_and_reads_it() {
    let (mut w, a, b, c, _) = ws();
    let n1 = w.notify(&c, None, "hi", 0).unwrap();
    w.notify(&c, None, "again", 0);
    assert_eq!(w.activate_notification(n1).unwrap(), Some(c.clone()));
    let loc = w.locate(&c).unwrap();
    assert_eq!(w.active_source, "x");
    assert_eq!(w.active_tab().unwrap().id, loc.tab);
    assert_eq!(w.active_tab().unwrap().focused, c);
    assert_eq!(w.unread(&c), 0, "every Notification of the Pane is read");
    // Also un-hides it from another Pane's Zoom.
    w.focus_pane(&a).unwrap();
    let t = w.locate(&a).unwrap().tab;
    w.toggle_zoom(&t).unwrap();
    let nb = w.notify(&b, None, "b", 0).unwrap();
    w.activate_notification(nb).unwrap();
    let tab = w.active_tab().unwrap();
    assert_eq!(
        (tab.focused.as_str(), tab.zoomed.as_deref()),
        (b.as_str(), Some(b.as_str()))
    );
    assert!(w.activate_notification(999).is_err());
}

#[test]
fn r3_3_a_gone_panes_notification_is_only_marked_read() {
    let (mut w, _, _, c, _) = ws();
    let n = w.notify(&c, None, "bye", 0).unwrap();
    w.close_pane(&c).unwrap();
    let before = w.active_source.clone();
    assert_eq!(w.activate_notification(n).unwrap(), None);
    assert!(w.notification(n).unwrap().read);
    assert_eq!(w.active_source, before, "nothing else changes");
    assert!(!w.pane_exists(&c));
}

#[test]
fn newest_unread_skips_read_and_gone_panes() {
    let (mut w, a, b, c, _) = ws();
    assert_eq!(w.newest_unread(), None);
    let na = w.notify(&a, None, "a", 1).unwrap();
    let nb = w.notify(&b, None, "b", 2).unwrap();
    let nc = w.notify(&c, None, "c", 3).unwrap();
    assert_eq!(w.newest_unread(), Some(nc));
    w.close_pane(&c).unwrap();
    assert_eq!(w.newest_unread(), Some(nb));
    w.activate_notification(nb).unwrap();
    assert_eq!(w.newest_unread(), Some(na));
}

#[test]
fn mark_all_read_and_clear() {
    let (mut w, a, b, _, _) = ws();
    w.notify(&a, None, "a", 0);
    w.notify(&b, None, "b", 0);
    w.mark_all_read();
    assert_eq!(w.unread_counts().total, 0);
    assert_eq!(w.notifications.len(), 2);
    w.clear_notifications();
    assert!(w.notifications.is_empty());
}

#[test]
fn r3_5_on_screen_means_active_tab_and_not_hidden() {
    let (mut w, a, b, c, g) = ws();
    assert!(w.pane_on_screen(&g));
    assert!(!w.pane_on_screen(&a), "other Source");
    w.focus_pane(&a).unwrap();
    assert!(
        w.pane_on_screen(&a) && w.pane_on_screen(&b),
        "split: both shown"
    );
    assert!(!w.pane_on_screen(&c), "other Tab");
    let t = w.locate(&a).unwrap().tab;
    w.toggle_zoom(&t).unwrap();
    assert!(w.pane_on_screen(&a));
    assert!(!w.pane_on_screen(&b), "hidden by Zoom");
    assert!(!w.pane_on_screen("p_404"));
}

#[test]
fn r3_5_title_bar_is_not_on_screen() {
    let mut w = State::new(Path::new("/home/u"));
    let limits = crate::config::Limits {
        max_children: 20,
        ..Default::default()
    };
    let lead = w
        .spawn(
            crate::SpawnSpec {
                agent: Some("claude".into()),
                ..Default::default()
            },
            &limits,
        )
        .unwrap();
    let kids: Vec<String> = (0..9)
        .map(|_| {
            w.spawn(
                crate::SpawnSpec {
                    caller: Some(lead.clone()),
                    ..Default::default()
                },
                &limits,
            )
            .unwrap()
        })
        .collect();
    w.focus_pane(&lead).unwrap();
    assert!(w.pane_on_screen(&kids[0]));
    assert!(!w.pane_on_screen(&kids[8]), "collapsed to a Title Bar");
}

#[test]
fn r3_5_desktop_decision() {
    use DesktopNotify::*;
    assert_eq!(
        desktop_decision(Off, false, false),
        (false, "desktop_notify = off")
    );
    assert_eq!(
        desktop_decision(Always, true, true),
        (true, "desktop_notify = always")
    );
    assert_eq!(
        desktop_decision(Smart, false, true),
        (true, "window not active")
    );
    assert_eq!(
        desktop_decision(Smart, true, false),
        (true, "pane not visible")
    );
    assert_eq!(
        desktop_decision(Smart, false, false),
        (true, "pane not visible")
    );
    assert_eq!(
        desktop_decision(Smart, true, true),
        (false, "pane visible in the active window")
    );
}

#[test]
fn relative_time_is_short() {
    assert_eq!(relative_time(100, 100), "now");
    assert_eq!(relative_time(100, 159), "now");
    assert_eq!(relative_time(0, 60), "1 min");
    assert_eq!(relative_time(0, 3599), "59 min");
    assert_eq!(relative_time(0, 3600), "1 h");
    assert_eq!(relative_time(0, 86400 * 3), "3 d");
    assert_eq!(relative_time(200, 100), "now", "clock skew");
}

#[test]
fn old_state_without_ids_gets_unique_ids() {
    let (mut w, a, _, _, _) = ws();
    w.notify(&a, None, "1", 0);
    w.notify(&a, None, "2", 0);
    for n in &mut w.notifications {
        n.id = 0;
    }
    w.ensure_general(Path::new("/home/u"));
    let ids: Vec<u64> = w.notifications.iter().map(|n| n.id).collect();
    assert_eq!(ids, vec![1, 2]);
}

// ---- R3.1 coalescing: one Agent event = one Notification (M6 live bug) ----

fn texts(w: &State) -> Vec<(&str, &str)> {
    w.notifications
        .iter()
        .map(|n| (n.pane.as_str(), n.text.as_str()))
        .collect()
}

#[test]
fn r3_1_hook_status_then_osc_is_one_entry() {
    // Codex: Stop hook ("rev finished"), then its own OSC 9 "PONG".
    let (mut w, a, _, _, _) = ws();
    let id = w.notify_status(&a, "rev finished", 100).unwrap();
    assert_eq!(w.notify(&a, None, "PONG", 100), Some(id), "merged, same id");
    assert_eq!(texts(&w), [(a.as_str(), "rev finished — PONG")]);
    assert_eq!(w.notifications[0].source, Origin::Merged);
    assert_eq!(w.unread(&a), 1);
}

#[test]
fn r3_1_osc_then_hook_status_is_one_entry_in_either_order() {
    // Claude: OSC 777 may arrive before the Notification hook.
    let (mut w, a, _, _, _) = ws();
    let id = w
        .notify(
            &a,
            Some("Claude Code".into()),
            "Claude needs your permission",
            100,
        )
        .unwrap();
    assert_eq!(w.notify_status(&a, "perm needs input", 104), Some(id));
    assert_eq!(
        texts(&w),
        [(
            a.as_str(),
            "perm needs input — Claude Code: Claude needs your permission"
        )]
    );
    assert_eq!(w.notifications[0].title, None);
}

#[test]
fn r3_1_coalescing_window_is_5_seconds_and_per_pane() {
    let (mut w, a, b, _, _) = ws();
    w.notify_status(&a, "a finished", 100).unwrap();
    assert!(
        w.notify(&b, None, "PONG", 101).is_some(),
        "other Pane: own entry"
    );
    assert!(
        w.notify(&a, None, "late", 106).is_some(),
        "6 s later: own entry"
    );
    assert_eq!(w.notifications.len(), 3);
    assert_eq!(w.notifications[0].text, "a finished");
}

#[test]
fn r3_1_one_message_per_status_entry_extra_ones_are_absorbed() {
    // Codex sends two OSC 9 for one turn; two statuses are two events.
    let (mut w, a, _, _, _) = ws();
    let id = w.notify_status(&a, "rev finished", 100).unwrap();
    w.notify(&a, None, "PONG", 100);
    assert_eq!(w.notify(&a, None, "Codex · done", 101), Some(id));
    assert_eq!(texts(&w), [(a.as_str(), "rev finished — PONG")]);
    assert!(w.notify_status(&a, "rev needs input", 102).is_some());
    assert_eq!(w.notifications.len(), 2, "a new status is a new event");
    // Two plain messages never merge (only the 2 s same-text dedupe).
    let (mut w, a, _, _, _) = ws();
    w.notify(&a, None, "one", 100);
    w.notify(&a, None, "two", 100);
    assert_eq!(w.notifications.len(), 2);
}

#[test]
fn r3_1_bell_adds_no_text() {
    let (mut w, a, b, _, _) = ws();
    let id = w.notify_status(&a, "a needs input", 100).unwrap();
    assert_eq!(w.notify_bell(&a, 101), Some(id));
    assert_eq!(texts(&w), [(a.as_str(), "a needs input")]);
    let id = w.notify_bell(&b, 100).unwrap();
    assert_eq!(w.notify_status(&b, "b needs input", 100), Some(id));
    assert_eq!(w.notification(id).unwrap().text, "b needs input");
    // A bell alone (plain shell) is still a Notification.
    let (mut w, a, _, _, _) = ws();
    assert!(w.notify_bell(&a, 100).is_some());
    assert_eq!(w.notifications[0].text, "Bell");
}

#[test]
fn r3_1_merge_keeps_read_state() {
    let (mut w, a, _, _, _) = ws();
    w.notify_status(&a, "a finished", 100).unwrap();
    w.mark_read(&a);
    w.notify(&a, None, "PONG", 101);
    assert_eq!(w.notifications.len(), 1);
    assert!(w.notifications[0].read, "same event, already seen");
}

#[test]
fn r3_1_status_transition_goes_through_coalescing() {
    let (mut w, a, _, _, _) = ws();
    w.set_status(&a, Some(crate::AgentStatus::Done), None, None, 100)
        .unwrap();
    w.notify(&a, None, "PONG", 100);
    assert_eq!(texts(&w), [(a.as_str(), "lead finished — PONG")]);
}

#[test]
fn r3_1_window_does_not_slide_on_absorbed_messages() {
    // Review: every absorbed message used to refresh the time, so a chatty
    // Pane lost distinct messages forever.
    let (mut w, a, _, _, _) = ws();
    let id = w.notify_status(&a, "a finished", 100).unwrap();
    w.notify(&a, None, "PONG", 102);
    assert_eq!(
        w.notify(&a, None, "more", 104),
        Some(id),
        "absorbed within 5 s"
    );
    let later = w.notify(&a, None, "later", 106).unwrap();
    assert_ne!(later, id, "6 s after the event: its own entry");
}

#[test]
fn r3_1_a_status_never_merges_into_a_read_entry() {
    // The user read the OSC message; a status after it is news.
    let (mut w, a, _, _, _) = ws();
    let first = w.notify(&a, None, "hello", 100).unwrap();
    w.mark_read(&a);
    let s = w.notify_status(&a, "a needs input", 101).unwrap();
    assert_ne!(s, first);
    assert_eq!(w.unread(&a), 1);
}
