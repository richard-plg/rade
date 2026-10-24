//! Browser Annotations (PRD R71, ADR-0026): the owner points at an element
//! or drags a region in a Browser Pane (or a headed Chromium) and types a
//! comment; the annotations go to an Agent as one prompt. The page side
//! (`assets/annotate.js`) reports what was picked as a [`Pick`]; this module
//! turns it into a stable CSS selector, an [`Annotation`] and the prompt.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One element on the way from the picked one up to `<body>`, as the page
/// reports it (`annotate.js` `chain`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ElemInfo {
    /// Lower-case tag name.
    pub tag: String,
    pub id: Option<String>,
    /// `data-testid`, `data-test`, `data-cy` or `data-qa` (attr, value).
    pub testid: Option<(String, String)>,
    /// The `name` attribute of a form field.
    pub name: Option<String>,
    /// 1-based position among siblings with the same tag, and their count.
    pub nth: u32,
    pub of_type: u32,
    /// Whether that id / test id / name is unique in the document.
    pub id_unique: bool,
    pub testid_unique: bool,
    pub name_unique: bool,
}

/// An id a framework generated (`:r1:`, `ember123`, a hash) changes from
/// build to build or render to render: not worth anchoring on.
fn stable_id(id: &str) -> bool {
    let mut run = 0;
    let digits_ok = id.chars().all(|c| {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        run < 3
    });
    id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_alphabetic())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && digits_ok
}

/// A CSS string literal: `"…"` with `\` and `"` escaped, line breaks as
/// escapes.
pub fn css_string(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                o.push('\\');
                o.push(c);
            }
            '\n' => o.push_str("\\a "),
            c if c.is_control() => o.push_str(&format!("\\{:x} ", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn tag_ok(t: &str) -> bool {
    !t.is_empty()
        && t.starts_with(|c: char| c.is_ascii_alphabetic())
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
}

/// A tag as a CSS identifier: `.` in a custom element's name escaped.
fn css_tag(t: &str) -> String {
    t.replace('.', "\\.")
}

/// A selector that finds the picked element again (`chain[0]`), built
/// from the closest unique anchor — a test id, a stable id, a form field's
/// name — then `tag:nth-of-type(n)` steps with `>`, so it matches exactly
/// one element by construction. No anchor: from `body`.
pub fn selector(chain: &[ElemInfo]) -> String {
    let mut steps: Vec<String> = Vec::new();
    for e in chain {
        if !tag_ok(&e.tag) {
            break;
        }
        if let Some((attr, v)) = e.testid.as_ref().filter(|_| e.testid_unique) {
            if attr.starts_with("data-") && tag_ok(attr) {
                steps.push(format!("[{attr}={}]", css_string(v)));
                break;
            }
        }
        if let Some(id) = e.id.as_deref().filter(|i| e.id_unique && stable_id(i)) {
            steps.push(format!("#{id}"));
            break;
        }
        if let Some(n) = e.name.as_deref().filter(|_| e.name_unique) {
            steps.push(format!("{}[name={}]", css_tag(&e.tag), css_string(n)));
            break;
        }
        if e.tag == "body" || e.tag == "html" {
            steps.push(e.tag.clone());
            break;
        }
        steps.push(if e.of_type > 1 {
            format!("{}:nth-of-type({})", css_tag(&e.tag), e.nth)
        } else {
            css_tag(&e.tag)
        });
    }
    steps.reverse();
    steps.join(" > ")
}

/// A rectangle in CSS pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// What `annotate.js` posts when the owner adds an annotation.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Pick {
    pub url: String,
    /// `element` (a click) or `region` (a drag; its anchor is the smallest
    /// element holding the whole region).
    pub kind: String,
    pub chain: Vec<ElemInfo>,
    /// The anchor's `rade browser` ref (this engine's ref table).
    #[serde(rename = "ref")]
    pub r#ref: Option<String>,
    pub role: String,
    pub name: String,
    pub text: String,
    /// In the viewport, when it was added (the crop is cut from it).
    pub rect: Rect,
    /// The page's scroll offset then (`rect` + it = page coordinates).
    pub scroll: Rect,
    pub viewport: Rect,
    pub comment: String,
}

/// One Browser Annotation, kept on its Browser Pane in `state.json` until
/// sent or deleted (R71.3).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: u64,
    pub url: String,
    pub kind: String,
    pub selector: String,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub text: String,
    /// Page coordinates (CSS px).
    pub bbox: Rect,
    /// The cropped screenshot (PNG).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shot: Option<PathBuf>,
    /// Recent console errors of the page when it was added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub console: Vec<String>,
    pub comment: String,
}

/// Longest element text kept.
const MAX_TEXT: usize = 300;

fn cut(s: &str, n: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &flat[..i]),
        None => flat,
    }
}

impl Annotation {
    pub fn from_pick(id: u64, p: Pick, shot: Option<PathBuf>, console: Vec<String>) -> Self {
        let r = p.rect;
        Annotation {
            id,
            selector: selector(&p.chain),
            url: p.url,
            kind: if p.kind == "region" {
                "region"
            } else {
                "element"
            }
            .into(),
            r#ref: p
                .r#ref
                .filter(|r| r.len() <= 12 && r.chars().all(|c| c.is_ascii_alphanumeric())),
            role: cut(&p.role, 40),
            name: cut(&p.name, 160),
            text: cut(&p.text, MAX_TEXT),
            bbox: Rect {
                x: (r.x + p.scroll.x).round(),
                y: (r.y + p.scroll.y).round(),
                w: r.w.round(),
                h: r.h.round(),
            },
            shot,
            console: console.into_iter().rev().take(MAX_ERRORS).rev().collect(),
            comment: p.comment.trim().to_string(),
        }
    }

    /// `button "Save"`, `region 300×120 at (40, 220)`.
    pub fn label(&self) -> String {
        let el = match (self.role.as_str(), self.name.as_str()) {
            ("", _) => "element".to_string(),
            (r, "") => r.to_string(),
            (r, n) => format!("{r} \"{}\"", cut(n, 60)),
        };
        if self.kind == "region" {
            format!(
                "region {}×{} at ({}, {}) in {el}",
                self.bbox.w, self.bbox.h, self.bbox.x, self.bbox.y
            )
        } else {
            el
        }
    }
}

/// Console errors kept per annotation.
pub const MAX_ERRORS: usize = 5;

/// The prompt "Send to Agent" pastes (R71.4): per annotation its page,
/// what it points at (selector, ref, text, box), the crop to look at, the
/// page's recent console errors and the comment.
pub fn compose(annotations: &[Annotation], chromium: bool) -> String {
    let n = annotations.len();
    let (place, flag) = if chromium {
        (
            "Chromium (`rade browser --engine chromium`)",
            " --engine chromium",
        )
    } else {
        ("RADE's Browser Pane", "")
    };
    let mut s = format!(
        "Browser annotations from the owner ({n}, made in {place}). For each: open the \
         screenshot to see what is meant, find the element by its selector, and change the \
         code. Check the result in the browser (`rade browser{flag} …`) before you stop.\n"
    );
    for (i, a) in annotations.iter().enumerate() {
        s += &format!("\n{}. {}\n", i + 1, a.label());
        s += &format!("   page: {}\n", a.url);
        s += &format!("   selector: {}\n", a.selector);
        if let Some(r) = &a.r#ref {
            s += &format!(
                "   ref: {r} (`rade browser{flag} click {r}` works until the page navigates)\n"
            );
        }
        if !a.text.is_empty() {
            s += &format!("   text: \"{}\"\n", a.text.replace('"', "\\\""));
        }
        let b = a.bbox;
        s += &format!(
            "   box: x={} y={} w={} h={} (CSS px, page coordinates)\n",
            b.x, b.y, b.w, b.h
        );
        if let Some(p) = &a.shot {
            s += &format!("   screenshot: {}\n", p.display());
        }
        if !a.console.is_empty() {
            s += "   console errors on the page:\n";
            for e in &a.console {
                s += &format!("     - {}\n", cut(e, 300));
            }
        }
        let mut lines = a.comment.lines();
        s += &format!("   comment: {}\n", lines.next().unwrap_or(""));
        for l in lines {
            s += &format!("            {l}\n");
        }
    }
    s.trim_end().to_string()
}

/// Where a crop of `rect` (viewport CSS px, padded by `pad`) lies in a
/// screenshot of the viewport that is `img_w` device pixels wide for a
/// `view_w` CSS px viewport: (x, y, w, h) in device pixels, clamped to the
/// image; None when nothing is left.
pub fn crop_box(
    rect: Rect,
    pad: f64,
    view_w: f64,
    img_w: u32,
    img_h: u32,
) -> Option<(u32, u32, u32, u32)> {
    if view_w <= 0.0 {
        return None;
    }
    let k = img_w as f64 / view_w;
    let x0 = ((rect.x - pad) * k).floor().max(0.0);
    let y0 = ((rect.y - pad) * k).floor().max(0.0);
    let x1 = ((rect.x + rect.w + pad) * k).ceil().min(img_w as f64);
    let y1 = ((rect.y + rect.h + pad) * k).ceil().min(img_h as f64);
    (x1 > x0 && y1 > y0).then_some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

#[cfg(test)]
#[path = "annotate_tests.rs"]
mod tests;
