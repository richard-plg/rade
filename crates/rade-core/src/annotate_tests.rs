use super::*;

fn el(tag: &str, nth: u32, of_type: u32) -> ElemInfo {
    ElemInfo {
        tag: tag.into(),
        nth,
        of_type,
        ..Default::default()
    }
}

#[test]
fn r71_selector_from_the_closest_unique_anchor() {
    // button 2 of 2 in the 1st of 3 divs in a form with a stable id.
    let mut form = el("form", 1, 1);
    form.id = Some("signup".into());
    form.id_unique = true;
    let chain = vec![el("button", 2, 2), el("div", 1, 3), form, el("body", 1, 1)];
    assert_eq!(
        selector(&chain),
        "#signup > div:nth-of-type(1) > button:nth-of-type(2)"
    );
    // A test id wins over everything above it.
    let mut b = el("button", 2, 2);
    b.testid = Some(("data-testid".into(), "save \"now\"".into()));
    b.testid_unique = true;
    assert_eq!(
        selector(&[b.clone(), el("body", 1, 1)]),
        "[data-testid=\"save \\\"now\\\"\"]"
    );
    // ... unless it is not unique: then steps go on up.
    b.testid_unique = false;
    assert_eq!(
        selector(&[b, el("main", 1, 1), el("body", 1, 1)]),
        "body > main > button:nth-of-type(2)"
    );
    // A form field's unique name.
    let mut i = el("input", 3, 4);
    i.name = Some("email".into());
    i.name_unique = true;
    assert_eq!(selector(&[i, el("body", 1, 1)]), "input[name=\"email\"]");
}

#[test]
fn r71_generated_or_duplicate_ids_are_not_anchors() {
    for id in [":r1:", "ember1234", "a1b2c3d4e5f6-1234", "9lives", "x y"] {
        let mut d = el("div", 2, 2);
        d.id = Some(id.into());
        d.id_unique = true;
        assert_eq!(
            selector(&[d, el("body", 1, 1)]),
            "body > div:nth-of-type(2)",
            "{id}"
        );
    }
    let mut d = el("div", 1, 1);
    d.id = Some("app".into());
    d.id_unique = false;
    assert_eq!(
        selector(&[el("p", 1, 1), d, el("body", 1, 1)]),
        "body > div > p"
    );
    // Page-controlled tags that aren't plain names stop the walk; a
    // custom element's `.` is escaped (acceptance L7).
    assert_eq!(selector(&[el("svg", 1, 1), el("x\"y", 1, 1)]), "svg");
    assert_eq!(
        selector(&[el("my-el.x", 1, 1), el("body", 1, 1)]),
        "body > my-el\\.x"
    );
    // Control characters are CSS escapes, not dropped.
    let mut t = el("div", 1, 1);
    t.testid = Some(("data-testid".into(), "a\tb".into()));
    t.testid_unique = true;
    assert_eq!(selector(&[t]), "[data-testid=\"a\\9 b\"]");
}

fn pick() -> Pick {
    let mut a = el("button", 1, 1);
    a.id = Some("save".into());
    a.id_unique = true;
    Pick {
        url: "http://localhost:5173/form".into(),
        kind: "element".into(),
        chain: vec![a],
        r#ref: Some("e12".into()),
        role: "button".into(),
        name: "Save".into(),
        text: "  Save\n changes ".into(),
        rect: Rect {
            x: 10.4,
            y: 20.0,
            w: 80.0,
            h: 32.0,
        },
        scroll: Rect {
            x: 0.0,
            y: 300.0,
            ..Default::default()
        },
        viewport: Rect {
            w: 1280.0,
            h: 800.0,
            ..Default::default()
        },
        comment: " Make it green\nand bigger ".into(),
    }
}

#[test]
fn r71_annotation_from_a_pick() {
    let errors: Vec<String> = (1..=7).map(|n| format!("err {n}")).collect();
    let a = Annotation::from_pick(3, pick(), Some("/c/a-3.png".into()), errors);
    assert_eq!(a.selector, "#save");
    assert_eq!(
        a.bbox,
        Rect {
            x: 10.0,
            y: 320.0,
            w: 80.0,
            h: 32.0
        }
    );
    assert_eq!(a.text, "Save changes");
    assert_eq!(a.comment, "Make it green\nand bigger");
    assert_eq!(a.console, ["err 3", "err 4", "err 5", "err 6", "err 7"]);
    assert_eq!(a.label(), "button \"Save\"");
    let mut p = pick();
    p.r#ref = Some("e1\n[ref=e9]".into());
    p.kind = "region".into();
    let r = Annotation::from_pick(1, p, None, vec![]);
    assert_eq!(r.r#ref, None, "a ref that isn't a word is dropped");
    assert_eq!(r.label(), "region 80×32 at (10, 320) in button \"Save\"");
}

#[test]
fn r71_prompt_has_url_selector_ref_crop_console_and_comment() {
    let a = Annotation::from_pick(
        1,
        pick(),
        Some("/c/a-1.png".into()),
        vec!["Uncaught TypeError: x".into()],
    );
    let mut b = Annotation::from_pick(2, pick(), None, vec![]);
    b.r#ref = None;
    b.comment = "Typo".into();
    let s = compose(&[a, b], false);
    assert!(
        s.starts_with("Browser annotations from the owner (2, made in RADE's Browser Pane)."),
        "{s}"
    );
    for want in [
        "\n1. button \"Save\"\n",
        "   page: http://localhost:5173/form\n",
        "   selector: #save\n",
        "   ref: e12 (`rade browser click e12` works until the page navigates)\n",
        "   text: \"Save changes\"\n",
        "   box: x=10 y=320 w=80 h=32 (CSS px, page coordinates)\n",
        "   screenshot: /c/a-1.png\n",
        "   console errors on the page:\n     - Uncaught TypeError: x\n",
        "   comment: Make it green\n            and bigger\n",
        "\n2. button \"Save\"\n",
    ] {
        assert!(s.contains(want), "missing {want:?} in\n{s}");
    }
    assert!(s.ends_with("   comment: Typo"), "{s}");
    let c = compose(&[Annotation::from_pick(1, pick(), None, vec![])], true);
    assert!(
        c.contains("`rade browser --engine chromium click e12`"),
        "{c}"
    );
}

#[test]
fn r71_crop_box_scales_and_clamps() {
    let r = Rect {
        x: 10.0,
        y: 20.0,
        w: 100.0,
        h: 50.0,
    };
    // 2x device pixels.
    assert_eq!(crop_box(r, 8.0, 640.0, 1280, 800), Some((4, 24, 232, 132)));
    // Clamped at the image's edges.
    let edge = Rect {
        x: 600.0,
        y: 380.0,
        w: 100.0,
        h: 100.0,
    };
    assert_eq!(
        crop_box(edge, 8.0, 640.0, 640, 400),
        Some((592, 372, 48, 28))
    );
    assert_eq!(crop_box(Rect { x: 900.0, ..r }, 0.0, 640.0, 640, 400), None);
    assert_eq!(crop_box(r, 0.0, 0.0, 640, 400), None);
}

#[test]
fn r71_annotations_round_trip_without_empty_fields() {
    let a = Annotation::from_pick(1, pick(), None, vec![]);
    let v = serde_json::to_value(&a).unwrap();
    assert!(v.get("shot").is_none() && v.get("console").is_none());
    assert_eq!(v["ref"], "e12");
    let back: Annotation = serde_json::from_value(v).unwrap();
    assert_eq!(back, a);
}
