use super::*;
use crate::config::{VoiceAgent, VoiceConfig};

const SEC: u64 = 1000;

fn composer() -> Composer {
    let mut c = Composer::new(&VoiceConfig::default(), "F8");
    c.set_target("checkout-v2");
    c
}

fn key_down(at_ms: u64) -> Event {
    Event::KeyDown {
        repeat: false,
        at_ms,
    }
}

fn tool(label: &str) -> Event {
    Event::AgentEvent(AgentEvent::Tool {
        label: label.into(),
    })
}

fn delta(t: &str) -> Event {
    Event::AgentEvent(AgentEvent::Delta(t.into()))
}

fn done(t: &str) -> Event {
    Event::AgentEvent(AgentEvent::Final { text: t.into() })
}

fn turns(a: &[Action]) -> Vec<&str> {
    a.iter()
        .filter_map(|x| match x {
            Action::SendTurn(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// Hold for 400 ms at `t`, say `words`, context ready, a draft comes back.
fn ready(c: &mut Composer, t: u64, words: &str, draft: &str) {
    c.on(key_down(t));
    c.on(Event::KeyUp { at_ms: t + 400 });
    c.on(Event::Final(words.into()));
    c.set_context("## Pane\nagent: claude".into());
    c.on(tool("Read src/a.ts"));
    c.on(done(draft));
    assert_eq!(c.state(), Some(State::Ready));
}

#[test]
fn r124_g_composer_states() {
    // D2: a 400 ms hold is push-to-talk; auto-repeat is ignored; the first
    // press warms the voice agent up.
    let mut c = composer();
    assert!(!c.view().open && c.on(Event::KeyUp { at_ms: 1 }).is_empty());
    let a = c.on(key_down(1_000));
    assert_eq!(
        a,
        [Action::StartRecorder, Action::SpawnAgent, Action::Render]
    );
    for t in [1_100, 1_200, 1_300] {
        assert!(c
            .on(Event::KeyDown {
                repeat: true,
                at_ms: t
            })
            .is_empty());
    }
    let v = c.view();
    assert_eq!(
        (v.label.as_str(), v.dot, v.listening, v.mode),
        ("Listening", "rec", true, Some(Mode::Hold))
    );
    assert_eq!(v.hints, ["Release F8 to draft", "Esc cancel"]);
    assert_eq!(
        v.activity,
        "Reading the session and checkout-v2 while you speak"
    );
    assert_eq!(
        v.placeholder.as_deref(),
        Some("The prompt appears here when you let go of F8.")
    );
    assert!(!v.send_enabled && !v.paste_enabled);
    c.on(Event::Tick {
        at_ms: 1_000 + 6 * SEC,
    });
    assert_eq!(c.view().time, "0:06");
    // Live words: the last 3 are tentative.
    c.on(Event::Partial("ừ thì cái phần promo code á".into()));
    let v = c.view();
    assert_eq!(
        (v.heard.as_str(), v.tentative.as_str()),
        ("ừ thì cái phần", "promo code á")
    );
    let a = c.on(Event::KeyUp { at_ms: 1_400 });
    assert_eq!(
        a[..2],
        [Action::StopRecorder { keep: true }, Action::SttFinal]
    );
    assert_eq!(c.state(), Some(State::Transcribing));
    assert_eq!(c.view().hints, ["Esc stop"]);
    // The words arrive before the context: the turn waits for it.
    let a = c.on(Event::Final(" sửa promo code ".into()));
    assert!(turns(&a).is_empty() && c.state() == Some(State::Drafting));
    let a = c.set_context("## Pane\nagent: claude".into());
    assert_eq!(
        turns(&a),
        ["CONTEXT\n## Pane\nagent: claude\n\nSPEECH\nsửa promo code"]
    );
    assert!(
        !a.contains(&Action::SpawnAgent),
        "started at the first press"
    );
    // D9/D14: activity with n / max, streamed text, final replaces it.
    c.on(tool("Read src/checkout/useTotals.ts"));
    let v = c.view();
    assert_eq!(
        (v.activity.as_str(), v.activity_icon, v.step.as_deref()),
        (
            "Read src/checkout/useTotals.ts",
            ActivityIcon::Read,
            Some("1 / 6")
        )
    );
    assert_eq!((v.label.as_str(), v.dot), ("Drafting", "run"));
    c.on(delta("Fix the "));
    c.on(delta("promo"));
    let v = c.view();
    assert_eq!(
        (v.draft.as_str(), v.caret, v.draft_editable),
        ("Fix the promo", true, false)
    );
    c.on(Event::Tick {
        at_ms: 1_400 + 3_200,
    });
    let a = c.on(done("Fix the promo discount.\n- keep the API"));
    assert!(a.contains(&Action::FocusDraft));
    let v = c.view();
    assert_eq!((v.label.as_str(), v.dot), ("Ready", "ready"));
    assert_eq!(v.time, "0:00 · 3.2 s");
    assert_eq!(v.draft, "Fix the promo discount.\n- keep the API");
    assert!(v.send_enabled && v.paste_enabled && v.draft_editable && v.heard_folded && !v.caret);
    assert_eq!(v.hints, ["Hold F8 to revise", "Esc discard"]);

    // D11: revising carries the draft as it is in the box (edits included);
    // the same process answers (no SpawnAgent); changed words are marked.
    c.on(Event::DraftEdited(
        "Fix the promo discount.\n- add a test".into(),
    ));
    c.on(key_down(10_000));
    let v = c.view();
    assert_eq!((v.label.as_str(), v.stale), ("Revising", true));
    assert_eq!(v.hints, ["Release F8 to apply", "Esc keep the draft"]);
    let a = c.on(Event::KeyUp { at_ms: 10_500 });
    assert_eq!(
        a[..2],
        [Action::StopRecorder { keep: true }, Action::SttFinal]
    );
    let a = c.on(Event::Final("keep the API instead".into()));
    assert_eq!(
        turns(&a),
        ["REVISE\nCURRENT DRAFT\nFix the promo discount.\n- add a test\n\nSPEECH\nkeep the API instead\n\nRewrite the whole prompt with this change."]
    );
    assert!(!a.contains(&Action::SpawnAgent));
    c.on(Event::Tick {
        at_ms: 10_500 + 1_900,
    });
    c.on(done("Fix the promo discount.\n- keep the API"));
    let v = c.view();
    let marked: Vec<&str> = v.changed.iter().map(|r| &v.draft[r.clone()]).collect();
    assert_eq!(marked, ["keep the API"]);
    assert_eq!((v.revision, v.time.as_str()), (1, "revision 1 · 1.9 s"));
    // Typing clears the marks.
    c.on(Event::DraftEdited("Fix it.".into()));
    assert!(c.view().changed.is_empty());

    // D2 tap: two 100 ms taps toggle; the release of the ending press is ignored.
    let mut c = composer();
    c.on(key_down(0));
    c.on(Event::KeyUp { at_ms: 100 });
    let v = c.view();
    assert_eq!(
        (v.state, v.mode),
        (Some(State::Listening), Some(Mode::Toggle))
    );
    assert_eq!(v.hints, ["Press F8 to draft", "Esc cancel"]);
    c.on(Event::Tick { at_ms: 5_000 });
    assert_eq!(
        c.state(),
        Some(State::Listening),
        "a toggle take keeps running"
    );
    let a = c.on(key_down(5_000));
    assert_eq!(
        a[..2],
        [Action::StopRecorder { keep: true }, Action::SttFinal]
    );
    assert!(c.on(Event::KeyUp { at_ms: 5_100 }).is_empty());
    assert_eq!(c.state(), Some(State::Transcribing));
    // The button and palette are taps; FocusLost and the cap end a take.
    let mut c = composer();
    c.on(Event::Tap);
    assert_eq!(c.view().mode, Some(Mode::Toggle));
    assert!(c.on(Event::Tap).contains(&Action::SttFinal));
    let mut c = composer();
    c.on(key_down(0));
    assert!(c.on(Event::FocusLost).contains(&Action::SttFinal));
    let mut c = composer();
    c.on(Event::Tap);
    assert!(c
        .on(Event::Tick { at_ms: 119_900 })
        .iter()
        .all(|a| *a != Action::SttFinal));
    assert!(c
        .on(Event::Tick { at_ms: 120_000 })
        .contains(&Action::SttFinal));

    // D13: Esc in each state.
    let mut c = composer();
    c.on(key_down(0));
    let a = c.on(Event::Escape { at_ms: 50 });
    assert_eq!(
        a,
        [
            Action::StopRecorder { keep: false },
            Action::KillAgent,
            Action::Close
        ]
    );
    assert!(!c.is_open(), "Listening, no draft: cancel and close");
    let mut c = composer();
    ready(&mut c, 0, "words", "Draft.");
    c.on(key_down(5_000));
    let a = c.on(Event::Escape { at_ms: 5_100 });
    assert_eq!(a[0], Action::StopRecorder { keep: false });
    assert!(!a.contains(&Action::Close));
    let v = c.view();
    assert_eq!(
        (v.state, v.draft.as_str(), v.stale),
        (Some(State::Ready), "Draft.", false)
    );
    // Drafting: stop the turn, keep the streamed text, a new take starts a new process.
    c.on(key_down(6_000));
    c.on(Event::KeyUp { at_ms: 6_400 });
    c.on(Event::Final("more words".into()));
    c.on(delta("Half a dra"));
    let a = c.on(Event::Escape { at_ms: 6_800 });
    assert!(a.contains(&Action::KillAgent) && !a.contains(&Action::Close));
    let v = c.view();
    assert_eq!(
        (v.state, v.draft.as_str()),
        (Some(State::Ready), "Half a dra")
    );
    let a = c.on(key_down(7_000));
    assert!(a.contains(&Action::SpawnAgent));
    c.on(Event::KeyUp { at_ms: 7_400 });
    let a = c.on(Event::Final("again".into()));
    assert_eq!(
        turns(&a),
        ["CONTEXT\n## Pane\nagent: claude\n\nCURRENT DRAFT\nHalf a dra\n\nSPEECH\nagain"]
    );
    // A late agent line after Esc is ignored.
    let mut c = composer();
    c.on(key_down(0));
    c.on(Event::KeyUp { at_ms: 400 });
    c.on(Event::Final("x".into()));
    c.on(Event::Escape { at_ms: 500 });
    assert!(c.on(done("late")).is_empty());
    // Ready: unedited closes at once; edited asks once, for 2 s.
    let mut c = composer();
    ready(&mut c, 0, "w", "Draft.");
    assert!(c
        .on(Event::Escape { at_ms: 9_000 })
        .contains(&Action::Close));
    let mut c = composer();
    ready(&mut c, 0, "w", "Draft.");
    c.on(Event::DraftEdited("Draft edited.".into()));
    let a = c.on(Event::Escape { at_ms: 9_000 });
    assert!(!a.contains(&Action::Close));
    let v = c.view();
    assert_eq!(
        (v.hints[1].as_str(), v.hint_warn),
        ("Esc again to discard", true)
    );
    c.on(Event::Tick { at_ms: 11_001 });
    assert_eq!(c.view().hints[1], "Esc discard");
    c.on(Event::Escape { at_ms: 12_000 });
    assert!(c
        .on(Event::Escape { at_ms: 13_000 })
        .contains(&Action::Close));
    // Empty and error close at once.
    let mut c = composer();
    c.on(key_down(0));
    c.on(Event::KeyUp { at_ms: 400 });
    c.on(Event::Final("  ".into()));
    let v = c.view();
    assert_eq!(
        (v.label.as_str(), v.dot, v.state),
        ("Didn't catch that", "idle", Some(State::Empty))
    );
    assert_eq!(v.hints, ["Hold F8 and speak", "Esc close"]);
    assert_eq!(v.placeholder.as_deref(), Some("Nothing heard yet."));
    assert!(c.on(Event::Escape { at_ms: 600 }).contains(&Action::Close));

    // Enter sends with Enter, Alt+Enter pastes only; both close and kill.
    let mut c = composer();
    ready(&mut c, 0, "w", "Draft.\n- b");
    let a = c.on(Event::Send);
    assert_eq!(
        a,
        [
            Action::SendToPane {
                enter: true,
                text: "Draft.\n- b".into()
            },
            Action::KillAgent,
            Action::Close
        ]
    );
    let mut c = composer();
    ready(&mut c, 0, "w", "Draft.");
    assert_eq!(
        c.on(Event::Paste)[0],
        Action::SendToPane {
            enter: false,
            text: "Draft.".into()
        }
    );
    let mut c = composer();
    c.on(key_down(0));
    assert!(
        c.on(Event::Send).is_empty() && c.is_open(),
        "Send needs a Ready draft"
    );

    // D17: errors, labels and retry.
    let mut c = composer();
    c.on(key_down(0));
    let a = c.on(Event::RecordError("pw-record: no source\nmore".into()));
    assert_eq!(a[0], Action::StopRecorder { keep: false });
    let v = c.view();
    assert_eq!(
        (v.label.as_str(), v.dot, v.error.as_deref()),
        (
            "Can't record",
            "err",
            Some("Can't record: pw-record: no source")
        )
    );
    assert_eq!(
        (v.activity_icon, v.activity_error, v.setup_link),
        (ActivityIcon::Warning, true, false)
    );
    assert_eq!(v.hints, ["Hold F8 to try again", "Esc close"]);
    assert!(
        c.on(key_down(1_000)).contains(&Action::StartRecorder),
        "hold again retries"
    );
    c.on(Event::KeyUp { at_ms: 1_400 });
    c.on(Event::SttError(
        "No speech-to-text server at 127.0.0.1:8178".into(),
    ));
    let v = c.view();
    assert_eq!((v.label.as_str(), v.setup_link), ("Can't transcribe", true));
    c.on(key_down(2_000));
    c.on(Event::KeyUp { at_ms: 2_400 });
    c.on(Event::SttError("Speech-to-text failed: 401 bad key".into()));
    assert!(!c.view().setup_link);
    let mut c = composer();
    c.on(key_down(0));
    c.on(Event::KeyUp { at_ms: 400 });
    c.on(Event::Final("x".into()));
    c.set_context("c".into());
    c.on(Event::AgentExited {
        ok: false,
        stderr: "\nboom: no login\nx".into(),
    });
    let v = c.view();
    assert_eq!(
        (v.label.as_str(), v.error.as_deref()),
        (
            "Voice agent failed",
            Some("Voice agent failed: boom: no login")
        )
    );
    let mut c = composer();
    c.on(key_down(0));
    c.on(Event::AgentStartFailed("No such file or directory".into()));
    assert_eq!(
        c.view().error.as_deref(),
        Some("Can't start claude: No such file or directory")
    );

    // D9: the step limit gets one more turn, then fails.
    let mut c = composer();
    c.on(key_down(0));
    c.on(Event::KeyUp { at_ms: 400 });
    c.on(Event::Final("x".into()));
    c.set_context("c".into());
    let a = c.on(Event::AgentEvent(AgentEvent::MaxTurns));
    assert_eq!(a[0], Action::SendTurn(agent::WRITE_NOW.into()));
    assert_eq!(c.state(), Some(State::Drafting));
    let a = c.on(Event::AgentEvent(AgentEvent::MaxTurns));
    assert!(a.contains(&Action::KillAgent));
    assert_eq!(
        c.view().error.as_deref(),
        Some("Voice agent failed: used all 6 steps")
    );

    // D17: a turn past timeout_secs is killed; the streamed text stays.
    let mut c = composer();
    c.on(key_down(0));
    c.on(Event::KeyUp { at_ms: 400 });
    c.on(Event::Final("x".into()));
    c.set_context("c".into());
    c.on(delta("Fix the pro"));
    c.on(Event::Tick {
        at_ms: 400 + 59_900,
    });
    assert_eq!(c.state(), Some(State::Drafting));
    let a = c.on(Event::Tick {
        at_ms: 400 + 60_000,
    });
    assert!(a.contains(&Action::KillAgent));
    let v = c.view();
    assert_eq!(
        (v.label.as_str(), v.error.as_deref(), v.draft.as_str()),
        ("Timed out", Some("Took longer than 60 s"), "Fix the pro")
    );
    assert!(
        c.on(key_down(70_000)).contains(&Action::SpawnAgent),
        "a new process after a timeout"
    );

    // The pane closing ends everything.
    let mut c = composer();
    c.on(key_down(0));
    let a = c.on(Event::PaneClosed);
    assert_eq!(
        a,
        [
            Action::StopRecorder { keep: false },
            Action::KillAgent,
            Action::Close
        ]
    );
    assert!(c.on(Event::PaneClosed).is_empty());
    let mut c = composer();
    ready(&mut c, 0, "w", "Draft.");
    assert_eq!(c.on(Event::Discard), [Action::KillAgent, Action::Close]);

    // Codex: no warm start; a process and the whole prompt per turn.
    let cfg = VoiceConfig {
        agent: VoiceAgent::Codex,
        ..VoiceConfig::default()
    };
    let mut c = Composer::new(&cfg, "F8");
    assert!(!c.on(key_down(0)).contains(&Action::SpawnAgent));
    c.on(Event::KeyUp { at_ms: 400 });
    c.set_context("CTX".into());
    let a = c.on(Event::Final("words".into()));
    assert_eq!(a[0], Action::SpawnAgent);
    assert!(turns(&a)[0].contains("CONTEXT\nCTX\n\nSPEECH\nwords"));
    c.on(tool("Ran cat a.ts"));
    assert_eq!(c.view().step, None, "max_turns does not apply to Codex");
    c.on(done("Draft."));
    c.on(key_down(5_000));
    c.on(Event::KeyUp { at_ms: 5_400 });
    let a = c.on(Event::Final("change".into()));
    assert_eq!(a[0], Action::SpawnAgent);
    assert!(
        turns(&a)[0].contains("CURRENT DRAFT\nDraft.")
            && turns(&a)[0].ends_with("Rewrite the whole prompt with this change.")
    );
}
