use super::*;
use crate::account::{Account, AgentKind};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/voice")
        .join(name);
    std::fs::read_to_string(p).unwrap()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn r124_e_claude_command_and_env() {
    let cfg = VoiceConfig::default();
    let sys = system_prompt(&cfg);
    assert!(sys.contains(
        "Language: the language the developer spoke (keep English technical terms as they are)."
    ));
    assert!(!sys.contains("{LANGUAGE}"));
    let mut want = strs(&[
        "claude",
        "-p",
        "--model",
        "haiku",
        "--tools",
        "Read,Grep,Glob",
        "--max-turns",
        "6",
        "--no-session-persistence",
        "--setting-sources",
        "",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--append-system-prompt",
    ]);
    want.push(sys.clone());
    assert_eq!(claude_argv(&cfg, &sys), want);
    assert_eq!(command(&cfg), want);

    let cfg3 = VoiceConfig {
        max_turns: 3,
        tools: VoiceTools::None,
        model: "sonnet".into(),
        draft_language: DraftLanguage::En,
        ..VoiceConfig::default()
    };
    let a = command(&cfg3);
    assert_eq!(
        &a[2..8],
        ["--model", "sonnet", "--tools", "", "--max-turns", "3"]
    );
    assert!(a.last().unwrap().contains("Language: English."));

    // Codex, and agent_command replacing both.
    let cx = VoiceConfig {
        agent: VoiceAgent::Codex,
        ..VoiceConfig::default()
    };
    assert_eq!(
        command(&cx),
        strs(&[
            "codex",
            "exec",
            "--ephemeral",
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--json",
            "-"
        ])
    );
    let cx = VoiceConfig {
        model: "gpt-x".into(),
        ..cx
    };
    assert_eq!(&codex_argv(&cx)[7..], ["-m", "gpt-x", "-"]);
    let custom = VoiceConfig {
        agent_command: "python3 fake.py".into(),
        ..VoiceConfig::default()
    };
    assert_eq!(command(&custom), strs(&["sh", "-c", "python3 fake.py"]));

    // The Pane's env with the account, minus every RADE_* key.
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), "/usr/bin".into()),
        ("RADE_PANE_ID".into(), "p1".into()),
        ("RADE_SOCKET".into(), "/run/rade.sock".into()),
        ("RADE_DEPTH".into(), "1".into()),
        ("CLAUDE_CONFIG_DIR".into(), "/home/u/.claude".into()),
    ];
    let acme = Account {
        workspace: "acme".into(),
        agent: AgentKind::Claude,
        name: "acme-work".into(),
        config_dir: PathBuf::from("/home/u/.claude-acme"),
        declared_dir: "~/.claude-acme".into(),
        default: true,
        detailed_limits: false,
    };
    crate::account::apply_env(&mut env, &acme);
    assert!(env.iter().any(|(k, _)| k == "RADE_ACCOUNT"));
    let out = env_for(&env);
    assert_eq!(
        out,
        vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            (
                "CLAUDE_CONFIG_DIR".to_string(),
                "/home/u/.claude-acme".to_string()
            ),
        ]
    );

    // Turn texts (§3.5).
    assert_eq!(
        turn_message(
            Turn::First {
                context: "## Pane\nx",
                draft: None
            },
            "fix it"
        ),
        "CONTEXT\n## Pane\nx\n\nSPEECH\nfix it"
    );
    assert_eq!(
        turn_message(
            Turn::First {
                context: "c",
                draft: Some("old")
            },
            "w"
        ),
        "CONTEXT\nc\n\nCURRENT DRAFT\nold\n\nSPEECH\nw"
    );
    assert_eq!(
        turn_message(Turn::Revise { draft: "d" }, "drop 2"),
        "REVISE\nCURRENT DRAFT\nd\n\nSPEECH\ndrop 2\n\nRewrite the whole prompt with this change."
    );
    let line: Value = serde_json::from_str(&claude_stdin_line("a \"b\"\nc")).unwrap();
    assert_eq!(line["type"], "user");
    assert_eq!(
        line["message"],
        serde_json::json!({"role": "user", "content": "a \"b\"\nc"})
    );
    assert!(codex_prompt("SYS", "ctx", Some("d"), "w")
        .ends_with("Rewrite the whole prompt with this change."));
    assert!(codex_prompt("SYS", "ctx", None, "w").starts_with("SYS\n\nCONTEXT\nctx"));
}

/// Events per message: a message ends at a `Final`, `MaxTurns` or `Error`.
fn messages(events: Vec<AgentEvent>) -> Vec<Vec<AgentEvent>> {
    let mut out = vec![Vec::new()];
    for e in events {
        let end = matches!(
            e,
            AgentEvent::Final { .. } | AgentEvent::MaxTurns | AgentEvent::Error { .. }
        );
        out.last_mut().unwrap().push(e);
        if end {
            out.push(Vec::new());
        }
    }
    out.pop();
    out
}

#[test]
fn r124_f_stream_fixtures_parse_to_events() {
    let cwd = Path::new("/home/u/ws/shop");
    let all: Vec<AgentEvent> = fixture("claude-stream.jsonl")
        .lines()
        .flat_map(|l| parse_claude_line(l, cwd))
        .collect();
    let m = messages(all);
    assert_eq!(m.len(), 2);
    let tools = |m: &[AgentEvent]| -> Vec<String> {
        m.iter()
            .filter_map(|e| match e {
                AgentEvent::Tool { label } => Some(label.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        tools(&m[0]),
        [
            "Read src/checkout/useTotals.ts",
            "Read src/checkout/promo.ts",
            "Read src/checkout/CheckoutForm.tsx"
        ]
    );
    assert_eq!(m[0].last(), Some(&AgentEvent::MaxTurns));
    assert!(m[0].iter().all(|e| !matches!(e, AgentEvent::Delta(_))));
    assert_eq!(
        m[1],
        vec![
            AgentEvent::Delta("appl".into()),
            AgentEvent::Delta("yPromo".into()),
            AgentEvent::Final {
                text: "applyPromo".into()
            },
        ]
    );

    let mut last = None;
    let cx: Vec<AgentEvent> = fixture("codex-exec.jsonl")
        .lines()
        .flat_map(|l| parse_codex_line(l, &mut last))
        .collect();
    assert_eq!(
        cx,
        vec![
            AgentEvent::Tool { label: "Ran cat src/checkout/useTotals.ts".into() },
            AgentEvent::Final {
                text: "In `src/checkout/useTotals.ts:5`, the percent promo is applied again after `applyPromo(sum, promo)` on line 4.".into()
            },
        ]
    );

    // Fences, labels, errors.
    assert_eq!(strip_fence("```\nFix A.\n- b\n```"), "Fix A.\n- b");
    assert_eq!(strip_fence("```text\nFix A.\n```\n"), "Fix A.");
    assert_eq!(strip_fence("Fix `A`."), "Fix `A`.");
    let fenced =
        r#"{"type":"result","subtype":"success","is_error":false,"result":"```\nFix A.\n```"}"#;
    assert_eq!(
        parse_claude_line(fenced, cwd),
        vec![AgentEvent::Final {
            text: "Fix A.".into()
        }]
    );
    let j = |t: &str, i: Value| activity(t, &i, cwd);
    assert_eq!(
        j("Grep", serde_json::json!({"pattern": "applyPromo"})),
        "Searched “applyPromo”"
    );
    assert_eq!(
        j("Glob", serde_json::json!({"pattern": "src/**/*.ts"})),
        "Listed src/**/*.ts"
    );
    assert_eq!(j("WebFetch", serde_json::json!({})), "WebFetch");
    assert_eq!(
        j("Read", serde_json::json!({"file_path": "/etc/hosts"})),
        "Read /etc/hosts"
    );
    let err = r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#;
    assert_eq!(
        parse_claude_line(err, cwd),
        vec![AgentEvent::Error {
            text: "error_during_execution".into()
        }]
    );
    assert_eq!(parse_claude_line("not json", cwd), vec![]);
}
