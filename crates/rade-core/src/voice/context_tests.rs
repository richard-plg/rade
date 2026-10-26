use super::*;
use std::process::Command;

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/voice")
        .join(name);
    std::fs::read_to_string(p).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rade-voice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn git_in(d: &Path, args: &[&str]) {
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

/// A Claude transcript with `n` exchanges, each with a tool call and result
/// and some noise lines.
fn transcript(n: usize, len: usize) -> String {
    let mut out = vec![r#"{"type":"queue-operation"}"#.to_string()];
    for i in 0..n {
        let u = format!("question {i} {}", "u".repeat(len));
        let a = format!("answer {i} {}", "a".repeat(len));
        out.push(format!(
            r#"{{"type":"user","message":{{"role":"user","content":{}}}}}"#,
            serde_json::json!(u)
        ));
        out.push(r#"{"type":"api-request-blob","message":{"role":"user","content":[{"type":"text","text":"blob"}]}}"#.into());
        out.push(format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t{i}","name":"Read","input":{{"file_path":"/secret/INPUT{i}"}}}}]}}}}"#));
        out.push(format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t{i}","content":"RESULT{i}"}}]}}}}"#));
        out.push(format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":""}},{{"type":"text","text":{}}}]}}}}"#, serde_json::json!(a)));
    }
    out.join("\n")
}

#[test]
fn r124_a_claude_tail_keeps_six_exchanges_of_texts_only() {
    let tail = claude_tail(&transcript(9, 10));
    assert_eq!(tail.len(), 12, "6 exchanges, user + assistant each");
    assert_eq!(tail[0].0, Role::User);
    assert!(tail[0].1.starts_with("question 3 "), "{}", tail[0].1);
    assert!(tail[11].1.starts_with("answer 8 "));
    let all: String = tail.iter().map(|(_, t)| t.as_str()).collect();
    assert!(!all.contains("INPUT") && !all.contains("RESULT") && !all.contains("blob"));

    // Each message <= 2,000 chars; the whole tail <= 16,000, newest kept.
    let tail = claude_tail(&transcript(9, 3_000));
    assert!(tail.iter().all(|(_, t)| t.chars().count() <= 2_000));
    let total: usize = tail.iter().map(|(_, t)| t.chars().count()).sum();
    assert!(total <= 16_000, "{total}");
    assert!(tail.last().unwrap().1.starts_with("answer 8 "));
    assert!(tail.len() < 12, "the oldest were dropped");

    // The real recorded transcript: 2 exchanges, texts only.
    let t = claude_tail(&fixture("claude-transcript.jsonl"));
    assert_eq!(t.len(), 4);
    assert_eq!(
        t[0].1,
        "Read src/checkout/promo.ts and tell me in one line what applyPromo does."
    );
    assert!(t[3].1.contains("handleSubmit") && !t[3].1.contains("thinking"));

    // Files: the slug folder first, else any projects/*/ folder.
    let cfg = tmp("claude-cfg");
    let sid = "a617d674-98f3-4cfd-ba63-5eab4d4c8ccb";
    let cwd = Path::new("/home/u/ws/shop");
    assert_eq!(claude_transcript(&cfg, cwd, sid), None);
    let other = cfg.join("projects/-somewhere-else");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join(format!("{sid}.jsonl")), "").unwrap();
    assert_eq!(
        claude_transcript(&cfg, cwd, sid),
        Some(other.join(format!("{sid}.jsonl")))
    );
    let direct = cfg.join("projects/-home-u-ws-shop");
    std::fs::create_dir_all(&direct).unwrap();
    std::fs::write(direct.join(format!("{sid}.jsonl")), "").unwrap();
    assert_eq!(
        claude_transcript(&cfg, cwd, sid),
        Some(direct.join(format!("{sid}.jsonl")))
    );
    assert_eq!(claude_transcript(&cfg, cwd, ""), None);
}

#[test]
fn r124_b_codex_tail_and_screen_fallback() {
    let home = tmp("codex-home");
    let day = home.join("sessions/2026/10/25");
    std::fs::create_dir_all(&day).unwrap();
    let sid = "01a1731f-8435-7112-aab1-a58e349d5fe5";
    let mine = day.join(format!("rollout-2026-10-25T12-53-28-{sid}.jsonl"));
    std::fs::write(&mine, fixture("codex-rollout.jsonl")).unwrap();
    std::fs::write(
        day.join("rollout-2026-10-25T13-00-00-99999999-0000-0000-0000-000000000000.jsonl"),
        "",
    )
    .unwrap();
    assert_eq!(codex_rollout(&home, sid), Some(mine.clone()));
    assert_eq!(codex_rollout(&home, "nope"), None);

    let tail = codex_tail(&std::fs::read_to_string(&mine).unwrap());
    let roles: Vec<Role> = tail.iter().map(|(r, _)| *r).collect();
    assert_eq!(roles, [Role::User, Role::Assistant]);
    assert!(tail[0].1.starts_with("Read src/checkout/useTotals.ts"));
    assert!(tail[1].1.contains("useTotals") && tail[1].1.contains("applied twice"));
    let all: String = tail.iter().map(|(_, t)| t.as_str()).collect();
    assert!(!all.contains("AGENTS.md") && !all.contains("environment_context"));

    // No session id: the screen's last 120 lines, trailing blanks trimmed.
    let screen: String = (1..=200).map(|i| format!("line {i}\n")).collect::<String>() + "\n  \n\n";
    let t = screen_tail(&screen);
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines.len(), 120);
    assert_eq!((lines[0], lines[119]), ("line 81", "line 200"));
    let g = gather(&GatherInput {
        dir: tmp("nosession"),
        agent: Some("codex".into()),
        codex_home: home,
        screen: Some(screen),
        ..GatherInput::default()
    });
    let names: Vec<&str> = g.sections.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Pane", "Screen"]);
    assert!(g.sections[1].1.ends_with("line 200"));
}

#[test]
fn r124_c_gather_has_git_rules_and_layout() {
    let d = tmp("repo");
    git_in(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("CLAUDE.md"), "R".repeat(7_000)).unwrap();
    for i in 0..500 {
        let dir = d.join(format!("pkg{}/sub{}/deep", i % 25, i % 4));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("f{i}.txt")), "x\n").unwrap();
    }
    git_in(&d, &["add", "-A"]);
    git_in(&d, &["commit", "-qm", "first one"]);
    std::fs::write(d.join("pkg0/sub0/deep/f0.txt"), "changed\n").unwrap();
    std::fs::write(d.join("pkg1/sub1/deep/f1.txt"), "changed\n").unwrap();
    std::fs::write(d.join("applyPromo.ts"), "new\n").unwrap();
    git_in(&d, &["commit", "-qam", "second"]);
    std::fs::write(d.join("pkg0/sub0/deep/f0.txt"), "changed again\n").unwrap();
    std::fs::write(d.join("pkg1/sub1/deep/f1.txt"), "changed again\n").unwrap();
    std::fs::write(d.join("applyPromo.ts"), "newer\n").unwrap();
    git_in(&d, &["add", "applyPromo.ts"]);
    git_in(&d, &["commit", "-qm", "third"]);
    for f in [
        "pkg0/sub0/deep/f0.txt",
        "pkg1/sub1/deep/f1.txt",
        "applyPromo.ts",
    ] {
        std::fs::write(d.join(f), "dirty\n").unwrap();
    }

    let b = gather(&GatherInput {
        dir: d.clone(),
        agent: Some("claude".into()),
        where_label: "main".into(),
        ..GatherInput::default()
    });
    let sec = |n: &str| {
        b.sections
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, t)| t.clone())
            .unwrap_or_default()
    };
    let names: Vec<&str> = b.sections.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Pane", "Git", "Rules", "Layout"]);
    let git = sec("Git");
    assert!(git.contains("branch: main"), "{git}");
    assert!(git.contains(" M pkg0/sub0/deep/f0.txt") && git.contains(" M applyPromo.ts"));
    assert!(git.contains("git diff --stat HEAD") && git.contains("3 files changed"));
    let subjects: Vec<&str> = git
        .rsplit_once("recent commits:")
        .unwrap()
        .1
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(subjects, ["third", "second", "first one"]);
    let rules = sec("Rules");
    assert!(rules.starts_with("### CLAUDE.md\nRRR"));
    assert!(
        rules.chars().count() <= 6_000 + 20,
        "{}",
        rules.chars().count()
    );
    let layout = sec("Layout");
    assert!(layout.chars().count() <= 3_000);
    assert!(layout.contains("pkg0/sub0/ (") && layout.contains("./ ("));
    assert!(!layout.contains("deep"), "depth 2 at most");
    let n: usize = layout
        .lines()
        .map(|l| {
            l.rsplit_once('(')
                .unwrap()
                .1
                .trim_end_matches(')')
                .parse::<usize>()
                .unwrap()
        })
        .sum();
    assert_eq!(n, 502, "every tracked file is counted once");
    assert!(b.vocabulary.contains("applyPromo"));
    // A long layout is cut, not unbounded.
    let many: String = (0..3_000).map(|i| format!("d{i}/e{i}/f.txt\n")).collect();
    assert!(super::layout(&many).chars().count() <= 3_000);
    let r = render(&b);
    assert!(r.starts_with("## Pane\n") && r.contains("\n\n## Git\n"));
}

#[test]
fn r124_d_vocabulary_is_names_newest_first() {
    let tail = vec![
        (Role::User, "Fix handleSubmit please".to_string()),
        (
            Role::Assistant,
            "It calls use_totals then reads promo.ts. the end".to_string(),
        ),
    ];
    let changed = vec!["src/checkout/applyPromo.ts".to_string()];
    assert_eq!(
        vocabulary(&tail, &changed),
        "applyPromo promo.ts use_totals handleSubmit"
    );
    // At most 800 chars.
    let big = vec![(
        Role::User,
        (0..400).map(|i| format!("name_{i} ")).collect::<String>(),
    )];
    let v = vocabulary(&big, &[]);
    assert!(v.chars().count() <= 800 && v.starts_with("name_399 "));
}
