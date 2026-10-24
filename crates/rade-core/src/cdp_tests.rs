use super::*;
use crate::browser::parse_key;

#[test]
fn r72_pipe_frames_split_on_nul_and_keep_the_rest() {
    let mut buf = b"{\"id\":1}\0{\"id\":2}\0{\"par".to_vec();
    let f = take_frames(&mut buf);
    assert_eq!(f, vec![b"{\"id\":1}".to_vec(), b"{\"id\":2}".to_vec()]);
    assert_eq!(buf, b"{\"par");
    buf.extend_from_slice(b"t\":1}\0");
    assert_eq!(take_frames(&mut buf), vec![b"{\"part\":1}".to_vec()]);
    assert!(buf.is_empty());
    let out = frame(7, "Page.navigate", json!({"url": "x"}), Some("S1"));
    assert_eq!(out.last(), Some(&0));
    let v: Value = serde_json::from_slice(&out[..out.len() - 1]).unwrap();
    assert_eq!(
        v,
        json!({"id": 7, "method": "Page.navigate", "params": {"url": "x"}, "sessionId": "S1"})
    );
    let v: Value = serde_json::from_slice(
        frame(1, "Target.getTargets", json!({}), None)[..]
            .strip_suffix(&[0])
            .unwrap(),
    )
    .unwrap();
    assert!(v.get("sessionId").is_none());
}

#[test]
fn r72_press_maps_to_trusted_key_events() {
    let ev = key_events(&parse_key("Enter").unwrap());
    assert_eq!(ev[0]["type"], "keyDown");
    assert_eq!(
        ev[0]["text"], "\r",
        "Enter types a carriage return (submits a form)"
    );
    assert_eq!(ev[0]["windowsVirtualKeyCode"], 13);
    assert_eq!(ev[1]["type"], "keyUp");
    let a = key_events(&parse_key("Shift+a").unwrap());
    assert_eq!(
        (
            a[0]["key"].as_str(),
            a[0]["text"].as_str(),
            a[0]["code"].as_str()
        ),
        (Some("A"), Some("A"), Some("KeyA"))
    );
    assert_eq!(a[0]["modifiers"], 8);
    assert_eq!(a[0]["windowsVirtualKeyCode"], 65);
    let c = key_events(&parse_key("Control+a").unwrap());
    assert_eq!(c[0]["type"], "rawKeyDown", "a shortcut types nothing");
    assert!(c[0].get("text").is_none());
    assert_eq!(c[0]["modifiers"], 2);
    let t = key_events(&parse_key("Shift+Tab").unwrap());
    assert_eq!(
        (
            t[0]["type"].as_str(),
            t[0]["windowsVirtualKeyCode"].as_u64()
        ),
        (Some("rawKeyDown"), Some(9))
    );
    assert_eq!(
        key_events(&parse_key("Backspace").unwrap())[0]["windowsVirtualKeyCode"],
        8
    );
    assert_eq!(
        key_events(&parse_key("ArrowDown").unwrap())[0]["windowsVirtualKeyCode"],
        40
    );
    assert_eq!(
        key_events(&parse_key("F5").unwrap())[0]["windowsVirtualKeyCode"],
        116
    );
    let sp = key_events(&parse_key("Space").unwrap());
    assert_eq!(
        (
            sp[0]["text"].as_str(),
            sp[0]["windowsVirtualKeyCode"].as_u64()
        ),
        (Some(" "), Some(32))
    );
    assert_eq!(
        modifiers(&parse_key("Control+Alt+Meta+Shift+x").unwrap()),
        15
    );
}

#[test]
fn r72_console_lines_match_the_webkit_engine() {
    let m = console_message(&json!({
        "type": "log",
        "args": [{"type": "string", "value": "page loaded"}, {"type": "object", "value": {"n": 1}}],
        "stackTrace": {"callFrames": [{"url": "http://localhost:8000/", "lineNumber": 41}]}
    }));
    assert_eq!(
        m,
        json!({"level": "log", "text": "page loaded {\"n\":1}", "source": "http://localhost:8000/", "line": 42})
    );
    // Objects as Chromium sends them to the console: a preview.
    let o = console_message(&json!({"type": "log", "args": [
        {"type": "string", "value": "page loaded"},
        {"type": "object", "className": "Object", "description": "Object",
         "preview": {"type": "object", "overflow": false, "properties": [
            {"name": "n", "type": "number", "value": "1"}, {"name": "s", "type": "string", "value": "x"},
            {"name": "z", "type": "object", "subtype": "null", "value": "null"}]}},
        {"type": "object", "subtype": "array", "className": "Array", "description": "Array(2)",
         "preview": {"type": "object", "subtype": "array", "overflow": true, "properties": [
            {"name": "0", "type": "number", "value": "1"}]}},
        {"type": "object", "subtype": "node", "className": "HTMLDivElement", "description": "div#x",
         "preview": {"properties": []}}
    ]}));
    assert_eq!(
        o["text"],
        "page loaded {\"n\":1,\"s\":\"x\",\"z\":null} [1,…] div#x"
    );
    let w = console_message(
        &json!({"type": "warning", "args": [{"type": "string", "value": "careful"}]}),
    );
    assert_eq!(
        (w["level"].as_str(), w["text"].as_str()),
        (Some("warn"), Some("careful"))
    );
    let o = console_message(
        &json!({"type": "error", "args": [{"type": "object", "description": "HTMLDivElement"}, {"type": "undefined"}, {"type": "number", "unserializableValue": "NaN"}]}),
    );
    assert_eq!(o["text"], "HTMLDivElement undefined NaN");
    let e = exception_message(&json!({"exceptionDetails": {
        "text": "Uncaught", "url": "http://localhost:8000/", "lineNumber": 9,
        "exception": {"description": "Error: async boom\n    at http://localhost:8000/:10:40"}
    }}));
    assert_eq!(
        e["text"],
        "Uncaught Error: async boom\n    at http://localhost:8000/:10:40"
    );
    assert_eq!(
        (e["source"].as_str(), e["line"].as_u64()),
        (Some("http://localhost:8000/"), Some(10))
    );
    let r = exception_message(
        &json!({"exceptionDetails": {"text": "Uncaught (in promise)", "exception": {"type": "string", "value": "nope"}}}),
    );
    assert_eq!(r["text"], "Unhandled rejection: nope");
}

#[test]
fn r72_find_chromium_in_order() {
    let listing = |p: &Path| -> Vec<String> {
        if p == Path::new("/h/.cache/ms-playwright") {
            vec![
                "chromium-1100".into(),
                "chromium_headless_shell-1234".into(),
                "chromium-1234".into(),
                "ffmpeg-1011".into(),
            ]
        } else {
            vec![]
        }
    };
    let c = candidates(
        Some(Path::new("/opt/c/chrome")),
        Path::new("/h"),
        None,
        "/usr/local/bin:/usr/bin",
        &listing,
    );
    let s: Vec<String> = c.iter().map(|p| p.display().to_string()).collect();
    assert_eq!(
        &s[..5],
        [
            "/opt/c/chrome",
            "/h/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome",
            "/h/.cache/ms-playwright/chromium-1234/chrome-linux/chrome",
            "/h/.cache/ms-playwright/chromium-1100/chrome-linux64/chrome",
            "/h/.cache/ms-playwright/chromium-1100/chrome-linux/chrome",
        ]
    );
    assert_eq!(
        &s[5..9],
        [
            "/usr/local/bin/chromium",
            "/usr/bin/chromium",
            "/usr/local/bin/chromium-browser",
            "/usr/bin/chromium-browser"
        ]
    );
    assert_eq!(s.last().unwrap(), "/usr/bin/chrome");
    let pw = candidates(
        None,
        Path::new("/h"),
        Some(Path::new("/pw")),
        "",
        &|p: &Path| {
            if p == Path::new("/pw") {
                vec!["chromium-7".into()]
            } else {
                vec![]
            }
        },
    );
    assert_eq!(pw[0], PathBuf::from("/pw/chromium-7/chrome-linux64/chrome"));
}

#[test]
fn r72_launch_args_pipe_profile_headless() {
    let a = launch_args(Path::new("/tmp/p"), false, true);
    for want in [
        "--remote-debugging-pipe",
        "--user-data-dir=/tmp/p",
        "--headless=new",
        "--no-first-run",
        "about:blank",
    ] {
        assert!(a.iter().any(|x| x == want), "{want}");
    }
    assert!(
        !a.iter().any(|x| x.starts_with("--remote-debugging-port")),
        "no TCP port"
    );
    let h = launch_args(Path::new("/tmp/p"), true, true);
    assert!(!h.iter().any(|x| x.starts_with("--headless")));
    assert!(h.iter().any(|x| x == "--ozone-platform=wayland"));
    assert!(!launch_args(Path::new("/tmp/p"), true, false)
        .iter()
        .any(|x| x == "--ozone-platform=wayland"));
    assert_eq!(h.last().unwrap(), "about:blank");
}

#[test]
fn r72_history_steps() {
    let h = json!({"currentIndex": 1, "entries": [{"id": 10}, {"id": 11}, {"id": 12}]});
    assert_eq!(history_step(&h, -1), Some(10));
    assert_eq!(history_step(&h, 1), Some(12));
    let start = json!({"currentIndex": 0, "entries": [{"id": 10}]});
    assert_eq!(history_step(&start, -1), None);
    assert_eq!(history_step(&start, 1), None);
}
