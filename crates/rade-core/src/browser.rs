//! Browser Panes (PRD R18, ADR-0011): where one opens, which one a
//! `rade browser …` command drives, URL bar input, the text snapshot an
//! Agent reads (`[ref=e3] button "Save"`), and key specs for `press`.
//!
//! Placement mirrors the Viewer's: a Tab has at most one Browser that URLs
//! open into; without one, a new Browser splits right of the Pane it was
//! opened from, so the Agent stays visible beside it.

use crate::annotate::Annotation;
use crate::layout::{Pane, PaneKind, SplitDir};
use crate::model::State;
use crate::rpc::{Error, Result};
use serde::{Deserialize, Serialize};

/// What a new Browser shows before a URL is given.
pub const BLANK: &str = "about:blank";

/// The page-side library both engines run in their isolated `rade` world:
/// snapshot / refs / actions (`browser.js`, ADR-0011) and annotate mode
/// (`annotate.js`, R71).
pub const AUTOMATION_JS: &str = concat!(
    include_str!("../assets/browser.js"),
    "\n",
    include_str!("../assets/annotate.js")
);

/// The body that calls `__rade.<f>(<args>)` and returns `{ok}` or
/// `{error}` (never throws: a throw would reach the page's handlers).
pub fn automation_body(f: &str, args: &[serde_json::Value]) -> String {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    format!(
        "{AUTOMATION_JS}\ntry {{ return {{ ok: await window.__rade.{f}({}) }}; }}\n\
         catch (e) {{ return {{ error: String(e && e.message || e) }}; }}",
        args.join(", ")
    )
}

/// The async function bodies `eval` can run: `js` as an expression, or as
/// statements that `return` a value; and a probe that only parses the
/// expression form (`if (false)`), so the code runs once. A DOM node (or a
/// list of them) comes back as its HTML, a function as its source.
pub fn eval_bodies(js: &str) -> [String; 3] {
    let wrap = |f: String| {
        format!(
            "const __r = await (async () => {f})();\n\
             const __n = v => v && typeof v === 'object' && typeof v.nodeType === 'number' ? (v.outerHTML ?? v.textContent) : v;\n\
             if (typeof __r === 'function') return String(__r);\n\
             if (__r && typeof __r === 'object' && typeof __r.length === 'number' && typeof __r.item === 'function') return Array.from(__r, __n);\n\
             return __n(__r);"
        )
    };
    [
        format!("if (false) {{ (async () => (\n{js}\n)); }}\nreturn true;"),
        wrap(format!("(\n{js}\n)")),
        wrap(format!("{{\n{js}\n}}")),
    ]
}

impl Pane {
    pub fn browser(id: String, url: &str) -> Self {
        let mut p = Pane::new(id, Default::default(), None, None);
        p.kind = PaneKind::Browser {
            url: url.into(),
            annotations: Vec::new(),
        };
        p
    }

    pub fn is_browser(&self) -> bool {
        matches!(self.kind, PaneKind::Browser { .. })
    }

    pub fn url(&self) -> Option<&str> {
        match &self.kind {
            PaneKind::Browser { url, .. } => Some(url),
            _ => None,
        }
    }
}

impl State {
    /// Open `url` from Pane `from`: in the Browser of `from`'s Tab, else a
    /// new Browser split right of `from`. `focus` also focuses it. Returns
    /// (Browser id, whether it was created).
    pub fn open_browser(&mut self, from: &str, url: &str, focus: bool) -> Result<(String, bool)> {
        let loc = self.locate_or_err(from)?;
        let existing = self.tab(&loc).and_then(tab_browser);
        let r = match existing {
            Some(id) => {
                self.set_browser_url(&id, url)?;
                (id, false)
            }
            None => {
                let pane = Pane::browser(self.next("p"), url);
                (self.split_with(from, SplitDir::Right, pane, false)?, true)
            }
        };
        if focus {
            self.focus_pane(&r.0)?;
        }
        Ok(r)
    }

    /// Open `url` in a Worktree (no caller): beside its active Tab's focused
    /// Pane, else in a new Tab.
    pub fn open_browser_in(
        &mut self,
        source: &str,
        worktree: &str,
        url: &str,
        focus: bool,
    ) -> Result<(String, bool)> {
        let wt = self
            .worktree(source, worktree)
            .ok_or_else(|| Error::not_found(format!("no worktree {source}/{worktree}")))?;
        let from = wt
            .active_tab
            .as_deref()
            .and_then(|t| wt.tab(t))
            .map(|t| t.focused.clone());
        if let Some(from) = from {
            return self.open_browser(&from, url, focus);
        }
        let pane = Pane::browser(self.next("p"), url);
        let (tab, id) = self.new_tab_with(source, worktree, pane)?;
        if focus {
            self.select_tab(&tab)?;
            self.focus_pane(&id)?;
        }
        Ok((id, true))
    }

    /// Record the page a Browser shows (Session Restore reloads it).
    pub fn set_browser_url(&mut self, pane: &str, to: &str) -> Result<()> {
        let loc = self.locate_or_err(pane)?;
        match &mut self.tab_mut(&loc)?.pane_mut(pane).unwrap().kind {
            PaneKind::Browser { url, .. } => {
                *url = to.into();
                Ok(())
            }
            _ => Err(Error::usage(format!("pane {pane} is not a Browser"))),
        }
    }

    /// The Browser a `rade browser` command drives: `pane` (must be one),
    /// else the one in the caller's Tab, then in the caller's Worktree; with
    /// no caller, the Tab on screen's, then the Worktree on screen's.
    pub fn find_browser(&self, pane: Option<&str>, caller: Option<&str>) -> Result<String> {
        if let Some(q) = pane {
            let id = self.resolve_pane(q, caller)?;
            return match self.pane(&id) {
                Some(p) if p.is_browser() => Ok(id),
                _ => Err(Error::usage(format!("pane {id} is not a Browser"))),
            };
        }
        let (p, w, tab) = match caller.and_then(|c| self.locate(c)) {
            Some(l) => (l.source, l.worktree, Some(l.tab)),
            None => {
                let (p, w) = self.active();
                (p.id.clone(), w.id.clone(), w.active_tab.clone())
            }
        };
        let wt = self.worktree(&p, &w).expect("located");
        tab.and_then(|t| wt.tab(&t).and_then(tab_browser))
            .or_else(|| wt.tabs.iter().find_map(tab_browser))
            .ok_or_else(|| {
                Error::not_found("no Browser Pane here: open one with `rade browser open <url>`")
            })
    }
}

impl State {
    fn annotations_mut(&mut self, pane: &str) -> Result<&mut Vec<Annotation>> {
        let loc = self.locate_or_err(pane)?;
        match &mut self.tab_mut(&loc)?.pane_mut(pane).unwrap().kind {
            PaneKind::Browser { annotations, .. } => Ok(annotations),
            _ => Err(Error::usage(format!("pane {pane} is not a Browser"))),
        }
    }

    /// A Browser's pending Annotations (R71.3).
    pub fn annotations(&self, pane: &str) -> Result<&[Annotation]> {
        match self.pane(pane).map(|p| &p.kind) {
            Some(PaneKind::Browser { annotations, .. }) => Ok(annotations),
            Some(_) => Err(Error::usage(format!("pane {pane} is not a Browser"))),
            None => Err(Error::not_found(format!("no pane {pane}"))),
        }
    }

    /// Keep an Annotation on a Browser; its id = the next number there.
    pub fn add_annotation(&mut self, pane: &str, mut a: Annotation) -> Result<u64> {
        let list = self.annotations_mut(pane)?;
        a.id = list.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        let id = a.id;
        list.push(a);
        Ok(id)
    }

    /// Change an Annotation's comment; an empty one deletes it.
    pub fn edit_annotation(&mut self, pane: &str, id: u64, text: &str) -> Result<()> {
        let list = self.annotations_mut(pane)?;
        let i = list
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| Error::not_found(format!("no annotation {id} on {pane}")))?;
        if text.trim().is_empty() {
            list.remove(i);
        } else {
            list[i].comment = text.trim().to_string();
        }
        Ok(())
    }

    /// Take them all (sent or cleared).
    pub fn take_annotations(&mut self, pane: &str) -> Result<Vec<Annotation>> {
        Ok(std::mem::take(self.annotations_mut(pane)?))
    }
}

/// A Tab's Browser: its focused one, else its first.
fn tab_browser(t: &crate::Tab) -> Option<String> {
    let ps = t.panes();
    ps.iter()
        .find(|x| x.is_browser() && x.id == t.focused)
        .or_else(|| ps.iter().find(|x| x.is_browser()))
        .map(|x| x.id.clone())
}

/// URL bar / `rade browser open` input → URL. Localhost first: `3000`,
/// `:3000/x`, `localhost:5173`, `127.0.0.1:8000` → `http://…`; a URL with a
/// scheme is kept; a host with a dot gets `https://`. Words with spaces
/// are refused (no web search).
pub fn normalize_url(input: &str) -> Result<String> {
    let s = input.trim();
    if s.is_empty() {
        return Err(Error::usage("empty URL"));
    }
    let has_scheme = s.split_once(':').is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && (rest.starts_with("//") || scheme == "about" || scheme == "data")
    });
    if has_scheme {
        return Ok(s.into());
    }
    if s.contains(char::is_whitespace) {
        return Err(Error::usage(format!("not a URL: {s}")));
    }
    // `3000`, `:3000`, `3000/path`, `:3000/path`
    let port_part = s.strip_prefix(':').unwrap_or(s);
    let digits = port_part.split(['/', '?', '#']).next().unwrap_or("");
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        let port: u16 = digits
            .parse()
            .map_err(|_| Error::usage(format!("bad port: {digits}")))?;
        let rest = &port_part[digits.len()..];
        let rest = if rest.is_empty() { "/" } else { rest };
        return Ok(format!("http://localhost:{port}{rest}"));
    }
    let host = match s.strip_prefix('[') {
        Some(v6) => &s[..v6.find(']').map_or(s.len(), |i| i + 2)],
        None => s.split(['/', ':', '?', '#']).next().unwrap_or(""),
    };
    let host = host.to_ascii_lowercase();
    let local = host == "localhost"
        || host.ends_with(".localhost")
        || host == "0.0.0.0"
        || host == "[::1]"
        || host.starts_with("127.");
    if local {
        return Ok(format!("http://{s}"));
    }
    if host.starts_with('[') || host.contains('.') {
        return Ok(format!("https://{s}"));
    }
    Err(Error::usage(format!("not a URL: {s}")))
}

/// One node of a page snapshot, as `browser.js` returns it: depth, ARIA-ish
/// role, accessible name, and a ref for elements an Agent can act on.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct SnapNode {
    pub d: usize,
    pub role: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "ref")]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub checked: Option<bool>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub href: Option<String>,
}

/// Longest name / value shown in a snapshot line.
const MAX_TEXT: usize = 160;

fn quoted(s: &str) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = match flat.char_indices().nth(MAX_TEXT) {
        Some((i, _)) => format!("{}…", &flat[..i]),
        None => flat,
    };
    format!("\"{}\"", cut.replace('\\', "\\\\").replace('"', "\\\""))
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The snapshot text (`rade browser snapshot`): a header, then one line per
/// node, indented by depth: `[ref=e3] button "Save" [disabled]`,
/// `textbox "Email" value="a@b"`, `link "Docs" href=/docs`, `text "Saved"`.
pub fn format_snapshot(url: &str, title: &str, nodes: &[SnapNode], omitted: usize) -> String {
    let mut s = format!("url: {url}\ntitle: {}\n", quoted(title));
    for n in nodes {
        s += &"  ".repeat(n.d);
        // Page-controlled strings (`role`, `href`) must not start a line of
        // their own: a page could forge `[ref=…]` entries (R18.5).
        if let Some(r) = n.r#ref.as_deref().filter(|r| is_word(r)) {
            s += &format!("[ref={r}] ");
        }
        s += if is_word(&n.role) { &n.role } else { "generic" };
        if !n.name.trim().is_empty() {
            s += &format!(" {}", quoted(&n.name));
        }
        if let Some(l) = n.level {
            s += &format!(" [level={l}]");
        }
        if let Some(v) = &n.value {
            s += &format!(" value={}", quoted(v));
        }
        match n.checked {
            Some(true) => s += " [checked]",
            Some(false) => s += " [unchecked]",
            None => {}
        }
        if n.disabled {
            s += " [disabled]";
        }
        if let Some(h) = &n.href {
            let h: String = h.split_whitespace().collect::<Vec<_>>().join("%20");
            s += &format!(" href={}", h.replace('"', "%22"));
        }
        s.push('\n');
    }
    if omitted > 0 {
        s += &format!("… {omitted} more nodes not shown (snapshot is capped)\n");
    }
    s
}

/// `browser.snapshot`'s result from what `__rade.snapshot()` returned:
/// `{"url", "title", "refs", "text"}` (`interactive`: only nodes with a
/// ref, unindented). Both engines.
pub fn snapshot_result(page: &serde_json::Value, interactive: bool) -> Result<serde_json::Value> {
    let mut nodes: Vec<SnapNode> = serde_json::from_value(page["nodes"].clone())
        .map_err(|e| Error::internal(format!("snapshot: {e}")))?;
    if interactive {
        nodes.retain(|n| n.r#ref.is_some());
        nodes.iter_mut().for_each(|n| n.d = 0);
    }
    let text = format_snapshot(
        page["url"].as_str().unwrap_or(""),
        page["title"].as_str().unwrap_or(""),
        &nodes,
        page["omitted"].as_u64().unwrap_or(0) as usize,
    );
    Ok(serde_json::json!({
        "url": page["url"], "title": page["title"],
        "refs": nodes.iter().filter(|n| n.r#ref.is_some()).count(), "text": text,
    }))
}

/// A key for `rade browser press`: DOM `KeyboardEvent` fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KeySpec {
    pub key: String,
    pub code: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// `Enter`, `Shift+Tab`, `Control+a`, `ArrowDown`, `esc`, `F5` → [`KeySpec`].
pub fn parse_key(spec: &str) -> Result<KeySpec> {
    let bad = || {
        Error::usage(format!(
            "unknown key '{spec}' (e.g. Enter, Tab, Shift+Tab, Control+a, ArrowDown, Escape)"
        ))
    };
    let parts: Vec<&str> = if spec == "+" {
        vec!["+"]
    } else {
        spec.split('+').collect()
    };
    let (last, mods) = parts.split_last().ok_or_else(bad)?;
    let mut k = KeySpec {
        key: String::new(),
        code: String::new(),
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => k.ctrl = true,
            "shift" => k.shift = true,
            "alt" | "option" => k.alt = true,
            "meta" | "super" | "cmd" | "command" => k.meta = true,
            _ => return Err(bad()),
        }
    }
    let named = |n: &str| -> Option<&'static str> {
        Some(match n.to_ascii_lowercase().as_str() {
            "enter" | "return" => "Enter",
            "tab" => "Tab",
            "esc" | "escape" => "Escape",
            "backspace" => "Backspace",
            "delete" | "del" => "Delete",
            "space" => " ",
            "up" | "arrowup" => "ArrowUp",
            "down" | "arrowdown" => "ArrowDown",
            "left" | "arrowleft" => "ArrowLeft",
            "right" | "arrowright" => "ArrowRight",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            "insert" => "Insert",
            _ => return None,
        })
    };
    let mut chars = last.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => {
            k.key = if k.shift {
                c.to_uppercase().collect()
            } else {
                c.to_string()
            };
            k.code = match c {
                c if c.is_ascii_alphabetic() => format!("Key{}", c.to_ascii_uppercase()),
                c if c.is_ascii_digit() => format!("Digit{c}"),
                ' ' => "Space".into(),
                _ => String::new(),
            };
        }
        (Some(_), Some(_)) => {
            if let Some(n) = named(last) {
                k.key = n.into();
                k.code = if n == " " { "Space".into() } else { n.into() };
            } else if let Some(f) = last
                .strip_prefix(['F', 'f'])
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| (1..=12).contains(n))
            {
                k.key = format!("F{f}");
                k.code = k.key.clone();
            } else {
                return Err(bad());
            }
        }
        _ => return Err(bad()),
    }
    Ok(k)
}

/// What `rade browser wait` waits for.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaitFor {
    /// The page finished loading.
    Load,
    /// The page's text contains this.
    Text(String),
    /// This ref is on the page and visible.
    Ref(String),
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
