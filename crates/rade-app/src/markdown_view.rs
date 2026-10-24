//! Rendered Markdown in a Viewer (R7.3, ADR-0001: WebKitGTK only inside
//! Viewer Panes). Documents and their images load through the `rade-file:`
//! scheme served here, which renders `.md` with rade-core's `markdown` and
//! serves other files only inside the Viewer's access root (its Worktree,
//! or the document's folder outside one). Script in markup is disabled; the
//! only JavaScript is the bundled mermaid.js, injected as a user script.
//! Link clicks never navigate the WebView to another document: other files
//! open in the same Viewer through the model (with Back), web links in the
//! system browser, `#anchors` scroll.

use crate::app::try_app;
use crate::viewer::{is_dark, launch_uri};
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use rade_core::viewer::{allowed, file_uri, uri_path};
use serde_json::Value;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use webkit6::prelude::*;

const MERMAID_JS: &str = include_str!("../assets/mermaid.min.js");
const RENDER_JS: &str = r#"
(function () {
  var root = document.documentElement;
  var blocks = document.querySelectorAll('pre.mermaid');
  if (!blocks.length) { root.dataset.mermaid = 'none'; return; }
  try {
    // Natural size (readable labels); wide diagrams scroll sideways.
    var cfg = { startOnLoad: false, securityLevel: 'strict',
      theme: root.classList.contains('dark') ? 'dark' : 'default',
      fontFamily: getComputedStyle(document.body).fontFamily };
    ['flowchart', 'sequence', 'class', 'state', 'er', 'gantt', 'journey', 'gitGraph',
     'mindmap', 'timeline', 'requirement', 'c4', 'block', 'architecture'].forEach(
      function (k) { cfg[k] = { useMaxWidth: false }; });
    mermaid.initialize(cfg);
    mermaid.run({ nodes: blocks }).then(
      function () {
        // Fit the column unless that shrinks labels below 60%; else scroll.
        blocks.forEach(function (pre) {
          var svg = pre.querySelector('svg');
          var w = svg && parseFloat(svg.getAttribute('width'));
          if (w && w > pre.clientWidth && pre.clientWidth / w >= 0.6) {
            svg.setAttribute('width', '100%'); svg.removeAttribute('height');
          }
        });
        root.dataset.mermaid = 'done'; },
      function (e) { root.dataset.mermaid = 'error: ' + e; });
  } catch (e) { root.dataset.mermaid = 'error: ' + e; }
})();
"#;
const INFO_JS: &str = r#"JSON.stringify({
  url: location.href, title: document.title, scheme: document.documentElement.className,
  mermaid: document.documentElement.dataset.mermaid || '',
  mermaid_svgs: document.querySelectorAll('pre.mermaid svg').length,
  images: Array.from(document.images).map(function (i) {
    return { src: i.getAttribute('src'), loaded: i.complete && i.naturalWidth > 0 }; }),
  code_spans: document.querySelectorAll('pre code span').length,
  headings: document.querySelectorAll('h1,h2,h3').length,
  tables: document.querySelectorAll('table').length,
  tasks: document.querySelectorAll('input[type=checkbox]').length,
  scroll_y: window.scrollY })"#;

thread_local! {
    static SHARED: OnceCell<(webkit6::WebContext, webkit6::UserContentManager)> = const { OnceCell::new() };
    /// Access root per WebView (keyed by its pointer), for the scheme handler.
    static ROOTS: RefCell<HashMap<usize, PathBuf>> = RefCell::default();
}

fn key(w: &webkit6::WebView) -> usize {
    use glib::translate::ToGlibPtr;
    let p: *mut webkit6::ffi::WebKitWebView = w.to_glib_none().0;
    p as usize
}

fn denied(req: &webkit6::URISchemeRequest, why: &str) {
    req.finish_error(&mut glib::Error::new(
        gio::IOErrorEnum::PermissionDenied,
        why,
    ));
}

/// `rade-file:` requests: the document (rendered) and what it references.
fn serve(req: &webkit6::URISchemeRequest) {
    let uri = req.uri().map(|u| u.to_string()).unwrap_or_default();
    let root = req
        .web_view()
        .and_then(|w| ROOTS.with(|r| r.borrow().get(&key(&w)).cloned()));
    let (Some(path), Some(root)) = (uri_path(&uri), root) else {
        return denied(req, "bad request");
    };
    if !allowed(&path, &root) {
        eprintln!(
            "rade: viewer blocked {} (outside {})",
            path.display(),
            root.display()
        );
        return denied(req, "outside the Viewer's folder");
    }
    let (bytes, mime) = if rade_core::viewer::is_markdown(&path) {
        let Ok(md) = std::fs::read(&path) else {
            return denied(req, "unreadable");
        };
        let title = path.file_name().unwrap_or_default().to_string_lossy();
        let html = rade_core::markdown::to_html_page_styled(
            &String::from_utf8_lossy(&md),
            &title,
            is_dark(),
            &page_css(is_dark()),
        );
        (html.into_bytes(), "text/html; charset=utf-8".to_string())
    } else {
        let Ok(b) = std::fs::read(&path) else {
            return denied(req, "unreadable");
        };
        let (ctype, _) = gio::content_type_guess(Some(&path), &b[..b.len().min(4096)]);
        let mime = gio::content_type_get_mime_type(&ctype)
            .map_or("application/octet-stream".into(), |m| m.to_string());
        (b, mime)
    };
    let len = bytes.len() as i64;
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
    req.finish(&stream, len, Some(&mime));
}

fn shared() -> (webkit6::WebContext, webkit6::UserContentManager) {
    SHARED.with(|s| {
        s.get_or_init(|| {
            let ctx = webkit6::WebContext::new();
            ctx.register_uri_scheme("rade-file", serve);
            // The bundled fonts (ADR-0037): the web process finds them
            // through the inherited FONTCONFIG_FILE, inside its sandbox.
            ctx.add_path_to_sandbox(crate::fonts::dir(), true);
            let ucm = webkit6::UserContentManager::new();
            ucm.add_script(&webkit6::UserScript::new(
                &format!("{MERMAID_JS}\n{RENDER_JS}"),
                webkit6::UserContentInjectedFrames::TopFrame,
                webkit6::UserScriptInjectionTime::End,
                &[],
                &[],
            ));
            (ctx, ucm)
        })
        .clone()
    })
}

struct Inner {
    web: webkit6::WebView,
    pane: String,
    current: RefCell<PathBuf>,
    scroll_after_load: Cell<Option<f64>>,
}

#[derive(Clone)]
pub struct MarkdownView(Rc<Inner>);

impl MarkdownView {
    pub fn new(pane: &str) -> Self {
        let (ctx, ucm) = shared();
        let settings = webkit6::Settings::new();
        settings.set_enable_javascript(true); // API/user scripts only:
        settings.set_enable_javascript_markup(false); // none from the page
        settings.set_javascript_can_open_windows_automatically(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
        settings.set_enable_webgl(false);
        settings.set_enable_developer_extras(false);
        let web = webkit6::WebView::builder()
            .web_context(&ctx)
            .user_content_manager(&ucm)
            .settings(&settings)
            .build();
        web.set_vexpand(true);
        web.set_hexpand(true);
        let md = Self(Rc::new(Inner {
            web,
            pane: pane.into(),
            current: RefCell::default(),
            scroll_after_load: Cell::new(None),
        }));
        md.connect();
        md
    }

    fn connect(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.web.connect_decide_policy(move |_, decision, kind| {
            let Some(inner) = weak.upgrade() else {
                return false;
            };
            let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
                return false; // responses: default handling
            };
            let Some(action) = nav.navigation_action() else {
                return false;
            };
            let uri = action
                .request()
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                .unwrap_or_default();
            let clicked = kind == webkit6::PolicyDecisionType::NewWindowAction
                || action.navigation_type() == webkit6::NavigationType::LinkClicked;
            let target = uri_path(&uri);
            if target.as_deref() == Some(inner.current.borrow().as_path()) {
                decision.use_(); // this document: #anchors scroll
                return true;
            }
            if !clicked {
                // Our own loads; nothing else may navigate the page.
                if target.is_some() {
                    decision.use_();
                } else {
                    decision.ignore();
                }
                return true;
            }
            decision.ignore();
            match target {
                Some(p) if p.exists() => {
                    let pane = inner.pane.clone();
                    glib::idle_add_local_once(move || {
                        if let Some(app) = try_app() {
                            let _ = app.update(|m| m.viewer_navigate(&pane, &p, None));
                        }
                    });
                }
                Some(p) => eprintln!("rade: link to missing file {}", p.display()),
                None => launch_uri(&uri),
            }
            true
        });
        let weak = Rc::downgrade(&self.0);
        self.0.web.connect_load_changed(move |w, event| {
            let Some(inner) = weak.upgrade() else { return };
            if event == webkit6::LoadEvent::Finished {
                if let Some(y) = inner.scroll_after_load.take() {
                    w.evaluate_javascript(
                        &format!("window.scrollTo(0, {y})"),
                        None,
                        None,
                        gio::Cancellable::NONE,
                        |_| {},
                    );
                }
                fix_scale(w);
            }
        });
    }

    pub fn widget(&self) -> &webkit6::WebView {
        &self.0.web
    }

    /// Load `path` (Markdown); `keep_place` = a live reload or theme change:
    /// keep the scroll position.
    pub fn load(&self, path: &Path, root: &Path, keep_place: bool) {
        let web = self.0.web.clone();
        ROOTS.with(|r| r.borrow_mut().insert(key(&web), root.to_path_buf()));
        let bg = crate::tokens::get("bg0", is_dark());
        web.set_background_color(&gdk::RGBA::parse(bg).expect("color"));
        *self.0.current.borrow_mut() = path.to_path_buf();
        let uri = file_uri(path);
        if !keep_place {
            web.load_uri(&uri);
            return;
        }
        let weak = Rc::downgrade(&self.0);
        web.evaluate_javascript(
            "window.scrollY",
            None,
            None,
            gio::Cancellable::NONE,
            move |r| {
                let Some(inner) = weak.upgrade() else { return };
                inner.scroll_after_load.set(r.ok().map(|v| v.to_double()));
                inner.web.load_uri(&uri);
            },
        );
    }

    /// Run `js` in the page; `done` gets its result as a string.
    pub fn eval(&self, js: &str, done: Box<dyn FnOnce(Result<String, String>)>) {
        self.0
            .web
            .evaluate_javascript(js, None, None, gio::Cancellable::NONE, move |r| {
                done(r.map(|v| v.to_str().to_string()).map_err(|e| e.to_string()))
            });
    }

    /// Debug summary of the rendered page (`rade debug viewer`).
    pub fn info(&self, mut base: Value, done: Box<dyn FnOnce(Value)>) {
        self.eval(
            INFO_JS,
            Box::new(move |r| {
                match r.map(|s| serde_json::from_str::<Value>(&s)) {
                    Ok(Ok(Value::Object(page))) => base.as_object_mut().unwrap().extend(page),
                    other => base["error"] = format!("{other:?}").into(),
                }
                done(base)
            }),
        );
    }
}

/// Every WebView after the first can get a device scale derived from the
/// monitor's reported DPI instead of the window's scale (headless Weston:
/// 1400 px = "1400 mm" → devicePixelRatio 0.26, the page laid out 3.8×
/// too wide and drawn tiny). Once loaded, compare with the GTK surface's
/// scale and correct through the zoom level.
pub(crate) fn fix_scale(w: &webkit6::WebView) {
    let Some(target) = w.native().and_then(|n| n.surface()).map(|s| s.scale()) else {
        return;
    };
    let w = w.clone();
    w.clone().evaluate_javascript(
        "devicePixelRatio",
        // Not the page's world: a page could redefine it there.
        Some(crate::browser_view::WORLD),
        None,
        gio::Cancellable::NONE,
        move |r| {
            let Ok(dpr) = r.map(|v| v.to_double()) else {
                return;
            };
            if dpr > 0.0 && (dpr - target).abs() > 0.01 {
                w.set_zoom_level(w.zoom_level() * target / dpr);
            }
        },
    );
}

impl Drop for Inner {
    fn drop(&mut self) {
        ROOTS.with(|r| r.borrow_mut().remove(&key(&self.web)));
    }
}

/// The page's design (R112.5): `markdown.css`'s variables from the tokens
/// of both themes, the code colors of the theme shown and the code font.
fn page_css(dark: bool) -> String {
    let vars = |dark: bool| {
        [
            ("bg", "bg0"),
            ("fg", "fg"),
            ("muted", "fg2"),
            ("faint", "fg3"),
            ("hair", "line"),
            ("border", "line2"),
            ("soft", "bg3"),
            ("code-bg", "bg2"),
            ("pre-bg", "bg1"),
            ("accent", "acc"),
            ("accent-line", "acc-line"),
            ("accent-dim", "acc-dim"),
            ("mark", "wait-dim"),
            ("sel", "term-sel"),
            ("note", "run"),
            ("tip", "ok"),
            ("important", "wt"),
            ("warning", "wait"),
            ("caution", "err"),
        ]
        .iter()
        .map(|(var, token)| format!("--{var}: {};", crate::tokens::get(token, dark)))
        .collect::<Vec<_>>()
        .join(" ")
    };
    let font = crate::app::try_app()
        .map(|a| a.config.borrow().code_font())
        .unwrap_or_else(|| rade_core::config::Config::default().code_font());
    format!(
        ":root {{ {} }}\nhtml.dark {{ {} }}\n{}\n{}",
        vars(false),
        vars(true),
        crate::syntax::markdown_css(dark),
        crate::fonts::code_css_web(&font)
    )
}
