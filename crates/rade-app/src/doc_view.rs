//! Rendered HTML and PDF files in a Viewer (R114). A WebView on the file's
//! `file://` URI in an ephemeral NetworkSession of its own: a PDF shows in
//! WebKitGTK's built-in PDF viewer (pages, zoom, search, copy); an HTML
//! page loads its relative CSS, images and scripts from its folder and runs
//! its JavaScript unless `[viewer] html_scripts = false`. Like Markdown
//! (R7.3), a clicked link to another existing file opens in the same Viewer
//! through the model (with Back), a web link in the system browser,
//! `#anchors` scroll; nothing else may navigate the page away.

use crate::app::try_app;
use crate::viewer::{is_dark, launch_uri};
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use serde_json::Value;
use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use webkit6::prelude::*;

thread_local! {
    static SESSION: OnceCell<webkit6::NetworkSession> = const { OnceCell::new() };
}

/// One ephemeral session for every rendered document: no cookies or
/// storage kept, nothing shared with the Browser Panes.
fn session() -> webkit6::NetworkSession {
    SESSION.with(|s| {
        s.get_or_init(webkit6::NetworkSession::new_ephemeral)
            .clone()
    })
}

/// The local file a `file://` URI names (query and fragment dropped).
fn file_path(uri: &str) -> Option<PathBuf> {
    let base = uri.split(['?', '#']).next()?;
    gio::File::for_uri(base)
        .path()
        .filter(|_| base.starts_with("file://"))
}

struct Inner {
    web: webkit6::WebView,
    pane: String,
    current: RefCell<PathBuf>,
    scroll_after_load: Cell<Option<f64>>,
}

#[derive(Clone)]
pub struct DocView(Rc<Inner>);

impl DocView {
    pub fn new(pane: &str) -> Self {
        let settings = webkit6::Settings::new();
        settings.set_enable_javascript(true);
        settings.set_javascript_can_open_windows_automatically(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
        settings.set_enable_developer_extras(false);
        let web = webkit6::WebView::builder()
            .network_session(&session())
            .settings(&settings)
            .build();
        web.set_vexpand(true);
        web.set_hexpand(true);
        let v = Self(Rc::new(Inner {
            web,
            pane: pane.into(),
            current: RefCell::default(),
            scroll_after_load: Cell::new(None),
        }));
        v.connect();
        v
    }

    fn connect(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.web.connect_decide_policy(move |_, decision, kind| {
            let Some(inner) = weak.upgrade() else {
                return false;
            };
            let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
                return false; // responses (a PDF): default handling
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
            let target = file_path(&uri);
            if target.as_deref() == Some(inner.current.borrow().as_path()) {
                decision.use_(); // this document: #anchors, a reload
                return true;
            }
            if !clicked {
                // Frames, the PDF viewer's own pages: local only.
                let local = target.is_some()
                    || uri.starts_with("about:")
                    || uri.starts_with("webkit-pdfjs-viewer:");
                if local {
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
                crate::markdown_view::fix_scale(w);
            }
        });
    }

    pub fn widget(&self) -> &webkit6::WebView {
        &self.0.web
    }

    /// Load `path`; `keep_place` = a live reload: keep the scroll position
    /// (an HTML page; the PDF viewer keeps its own page on reload).
    pub fn load(&self, path: &Path, keep_place: bool, scripts: bool) {
        let web = self.0.web.clone();
        if let Some(s) = WebViewExt::settings(&web) {
            s.set_enable_javascript(
                scripts || rade_core::viewer::classify(path) == rade_core::viewer::Content::Pdf,
            );
        }
        let bg = crate::tokens::get("bg0", is_dark());
        web.set_background_color(&gdk::RGBA::parse(bg).expect("color"));
        *self.0.current.borrow_mut() = path.to_path_buf();
        let uri = gio::File::for_path(path).uri().to_string();
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

    /// Debug summary of the page (`rade debug viewer`): URL, title, the
    /// first text, loading state.
    pub fn info(&self, mut base: Value, done: Box<dyn FnOnce(Value)>) {
        let web = self.0.web.clone();
        base["doc"] = serde_json::json!({
            "uri": web.uri().map(|u| u.to_string()),
            "loading": web.is_loading(),
            "title": web.title().map(|t| t.to_string()),
        });
        web.evaluate_javascript(
            "JSON.stringify({text: document.body ? document.body.innerText.slice(0, 400) : '', \
             images: Array.from(document.images).map(i => i.complete && i.naturalWidth > 0), \
             scroll_y: window.scrollY})",
            None,
            None,
            gio::Cancellable::NONE,
            move |r| {
                match r.map(|v| serde_json::from_str::<Value>(v.to_str().as_str())) {
                    Ok(Ok(Value::Object(page))) => {
                        base["doc"].as_object_mut().unwrap().extend(page)
                    }
                    other => base["doc"]["error"] = format!("{other:?}").into(),
                }
                done(base)
            },
        );
    }
}
