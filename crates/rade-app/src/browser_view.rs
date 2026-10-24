//! Browser Pane (PRD R18, ADR-0011): a WebKitGTK 6 WebView under a header
//! with Back / Forward / Reload, a URL bar (localhost first), the Web
//! Inspector and Close. Every Browser shares one NetworkSession whose
//! cookies and storage live in this flavor's data dir (ADR-0010). Page
//! console messages and uncaught errors are kept for `rade browser
//! console`; the automation library (`assets/browser.js`) runs in the
//! isolated script world [`WORLD`]. Light/dark follows the system theme
//! (`prefers-color-scheme`). Prior art: limux's WebKitGTK browser tab (MIT).

use crate::app::try_app;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use rade_core::browser::normalize_url;
use serde_json::{json, Value};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use webkit6::prelude::*;

/// Script world of the automation library: page scripts can't reach it.
pub const WORLD: &str = "rade";
use rade_core::browser::AUTOMATION_JS;
/// Page world, document start: forward console calls and uncaught errors.
/// (Console methods live in the page's world, so this hook must too; a
/// page can only fake or hide its own messages.)
const CONSOLE_JS: &str = r#"(function () {
  var h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.radeConsole;
  if (!h) return;
  var post = h.postMessage.bind(h), S = JSON.stringify;
  function fmt(a) {
    if (typeof a === 'string') return a;
    if (a instanceof Error) return a.stack ? a + '\n' + a.stack : String(a);
    try { var s = S(a); return s === undefined ? String(a) : s; } catch (e) { return String(a); }
  }
  function send(level, text, src, line) {
    try { post(S({ level: level, text: String(text).slice(0, 4000), source: src || location.href, line: line || 0 })); } catch (e) {}
  }
  ['log', 'info', 'warn', 'error', 'debug'].forEach(function (level) {
    var orig = console[level];
    console[level] = function () {
      send(level, Array.prototype.map.call(arguments, fmt).join(' '));
      return orig.apply(this, arguments);
    };
  });
  addEventListener('error', function (e) {
    // A muted error (cross-origin, or `rade browser eval`'s own) says nothing.
    if (e instanceof ErrorEvent && !(e.message === 'Script error.' && !e.filename)) send('error', 'Uncaught ' + (e.error ? fmt(e.error) : e.message), e.filename, e.lineno);
  }, true);
  addEventListener('unhandledrejection', function (e) { send('error', 'Unhandled rejection: ' + fmt(e.reason)); });
})();"#;

/// Narrow support safeguard for the reproducible in-process WebKit crash (ADR-0032).
pub(crate) fn unsupported_embedded_url(url: &str) -> bool {
    let Ok(uri) = glib::Uri::parse(url, glib::UriFlags::NONE) else {
        return false;
    };
    let Some(host) = uri.host() else { return false };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    ["youtube.com", "youtu.be", "youtube-nocookie.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

pub(crate) const UNSUPPORTED_MESSAGE: &str = "YouTube is unavailable in the embedded WebKit preview because it can crash RADE. Open this URL in Chromium or your default browser.";

/// Console entries kept per Browser between two `rade browser console`.
const MAX_CONSOLE: usize = 500;

thread_local! {
    static SESSION: OnceCell<webkit6::NetworkSession> = const { OnceCell::new() };
}

/// The Browsers' NetworkSession: persistent, in this flavor's dirs.
fn session() -> webkit6::NetworkSession {
    SESSION.with(|s| {
        s.get_or_init(|| {
            let (data, cache) = rade_core::paths::browser_dirs(&rade_core::paths::process_env);
            let s = webkit6::NetworkSession::new(
                Some(&data.to_string_lossy()),
                Some(&cache.to_string_lossy()),
            );
            s.set_itp_enabled(false);
            // Cookies persist only with explicit storage (logins to local
            // dev apps survive a restart).
            if let Some(c) = s.cookie_manager() {
                let db = data.join("cookies.sqlite");
                c.set_persistent_storage(
                    &db.to_string_lossy(),
                    webkit6::CookiePersistentStorage::Sqlite,
                );
            }
            // HTTP auth passwords would go to the desktop keyring: no.
            s.set_persistent_credential_storage_enabled(false);
            s
        })
        .clone()
    })
}

pub struct BrowserView {
    pub id: String,
    pub root: gtk::Box,
    pub web: webkit6::WebView,
    entry: gtk::Entry,
    status: gtk::Label,
    blocked_url: RefCell<Option<String>>,
    back: gtk::Button,
    forward: gtk::Button,
    console: RefCell<VecDeque<Value>>,
    dropped: Cell<usize>,
    /// Loads finished since the view was made (`rade browser open` waits
    /// for the next one), and the last one's error.
    pub loads: Cell<u64>,
    pub load_error: RefCell<Option<String>>,
    inspector_open: Cell<bool>,
    /// Annotate mode (R71): its header toggle, the panel under the page
    /// listing the pending Annotations, and the page's recent errors.
    pub(crate) annotate: gtk::ToggleButton,
    pub(crate) panel: crate::annotate_view::Panel,
    pub(crate) errors: RefCell<VecDeque<String>>,
    /// Until when a `rade browser` action is in flight: JS dialogs it
    /// raises are answered (accept) and logged instead of waiting for a
    /// human (they would block the page).
    pub automating_until: Cell<Option<std::time::Instant>>,
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.update_property(&[gtk::accessible::Property::Label(tip)]);
    b.add_css_class("flat");
    b
}

impl BrowserView {
    pub fn new(id: &str, url: &str) -> Rc<Self> {
        let ucm = webkit6::UserContentManager::new();
        ucm.register_script_message_handler("radeConsole", None);
        // Annotations come from RADE's world only: a page can't add any.
        ucm.register_script_message_handler("radeAnnotate", Some(WORLD));
        ucm.add_script(&webkit6::UserScript::new(
            CONSOLE_JS,
            webkit6::UserContentInjectedFrames::TopFrame,
            webkit6::UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
        // The library as a user script of its world: WebKit keeps a named
        // world (and so the ref table) alive only while something uses it.
        ucm.add_script(&webkit6::UserScript::for_world(
            AUTOMATION_JS,
            webkit6::UserContentInjectedFrames::TopFrame,
            webkit6::UserScriptInjectionTime::End,
            WORLD,
            &[],
            &[],
        ));
        let settings = webkit6::Settings::new();
        settings.set_enable_developer_extras(true);
        settings.set_javascript_can_open_windows_automatically(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
        let web = webkit6::WebView::builder()
            .network_session(&session())
            .user_content_manager(&ucm)
            .settings(&settings)
            .build();
        web.set_vexpand(true);
        web.set_hexpand(true);
        web.add_css_class("rade-browser-web");

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.add_css_class("rade-browser");
        root.add_css_class("view");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        header.add_css_class("rade-viewer-header");
        let back = icon_button("go-previous-symbolic", "Back (Alt+Left)");
        let forward = icon_button("go-next-symbolic", "Forward (Alt+Right)");
        let reload = icon_button("view-refresh-symbolic", "Reload (F5)");
        let entry = gtk::Entry::new();
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some("localhost:3000 or a URL"));
        entry.set_input_purpose(gtk::InputPurpose::Url);
        entry.update_property(&[gtk::accessible::Property::Label("Preview URL")]);
        entry.add_css_class("rade-browser-url");
        let annotate = gtk::ToggleButton::new();
        annotate.set_icon_name("document-edit-symbolic");
        annotate.update_property(&[gtk::accessible::Property::Label("Annotate preview")]);
        annotate.set_tooltip_text(Some(
            "Annotate (Ctrl+Shift+A): click or drag; Ctrl+Down/Up selects, Ctrl+Enter picks, Ctrl+Esc cancels",
        ));
        annotate.add_css_class("flat");
        annotate.add_css_class("rade-browser-annotate");
        let inspect = icon_button("applications-engineering-symbolic", "Web Inspector");
        let close = icon_button("window-close-symbolic", "Close Browser");
        for w in [&back, &forward, &reload] {
            header.append(w);
        }
        let engine = gtk::Label::new(Some("WebKit"));
        engine.set_tooltip_text(Some(
            "Embedded WebKitGTK preview. For incompatible sites, use Chromium or your browser.",
        ));
        let chromium = gtk::Button::with_label("Chromium…");
        chromium.set_tooltip_text(Some(
            "Open this URL in a separate Chromium window with RADE annotations",
        ));
        let external = icon_button("web-browser-symbolic", "Open in default browser");
        header.append(&engine);
        header.append(&entry);
        header.append(&chromium);
        header.append(&external);
        header.append(&annotate);
        header.append(&inspect);
        header.append(&close);
        root.append(&header);
        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_visible(false);
        root.append(&status);
        root.append(&web);
        let panel = crate::annotate_view::Panel::new();
        root.append(&panel.revealer);

        let v = Rc::new(Self {
            id: id.into(),
            root,
            web,
            entry,
            status,
            blocked_url: RefCell::default(),
            back,
            forward,
            console: RefCell::default(),
            dropped: Cell::new(0),
            loads: Cell::new(0),
            load_error: RefCell::default(),
            inspector_open: Cell::new(false),
            annotate,
            panel,
            errors: RefCell::default(),
            automating_until: Cell::new(None),
        });
        v.connect(&ucm, &reload, &inspect, &close);
        v.connect_annotate(&ucm);
        let this = Rc::downgrade(&v);
        external.connect_clicked(move |_| {
            let Some(v) = this.upgrade() else { return };
            let uri = v.preview_url();
            if let Err(e) =
                gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
            {
                v.status.set_label(&format!("Could not open browser: {e}"));
                v.status.set_visible(true);
            }
        });
        let this = Rc::downgrade(&v);
        chromium.connect_clicked(move |button| {
            let Some(v) = this.upgrade() else { return };
            crate::browser::open_chromium(&v, button);
        });
        v.load(url);
        v
    }

    fn connect(
        self: &Rc<Self>,
        ucm: &webkit6::UserContentManager,
        reload: &gtk::Button,
        inspect: &gtk::Button,
        close: &gtk::Button,
    ) {
        let this = Rc::downgrade(self);
        ucm.connect_script_message_received(Some("radeConsole"), move |_, msg| {
            let Some(v) = this.upgrade() else { return };
            let Ok(mut m) = serde_json::from_str::<Value>(&msg.to_str()) else {
                return;
            };
            m["time"] = json!(crate::panes::now());
            v.log(m);
        });
        let w = self.web.clone();
        self.back.connect_clicked(move |_| w.go_back());
        let w = self.web.clone();
        self.forward.connect_clicked(move |_| w.go_forward());
        let w = self.web.clone();
        reload.connect_clicked(move |_| w.reload());
        let this = Rc::downgrade(self);
        inspect.connect_clicked(move |_| {
            if let Some(v) = this.upgrade() {
                v.toggle_inspector();
            }
        });
        let id = self.id.clone();
        close.connect_clicked(move |_| {
            if let Some(app) = try_app() {
                let _ = app.close_pane(&id, false);
            }
        });
        let this = Rc::downgrade(self);
        self.entry.connect_activate(move |e| {
            let Some(v) = this.upgrade() else { return };
            match normalize_url(&e.text()) {
                Ok(url) => {
                    e.remove_css_class("error");
                    v.load(&url);
                    v.web.grab_focus();
                }
                Err(_) => e.add_css_class("error"),
            }
        });

        let this = Rc::downgrade(self);
        self.web.connect_uri_notify(move |w| {
            let Some(v) = this.upgrade() else { return };
            let uri = w.uri().map(|u| u.to_string()).unwrap_or_default();
            v.entry.remove_css_class("error");
            if !v.entry.has_focus() {
                let shown = v.preview_url();
                v.entry.set_text(if shown == rade_core::browser::BLANK {
                    ""
                } else {
                    &shown
                });
            }
            v.nav_buttons();
            // Kept for Session Restore; no re-render needed. Later: a load
            // started inside App::sync notifies while the model is borrowed.
            if !uri.is_empty() {
                let id = v.id.clone();
                glib::idle_add_local_once(move || {
                    if let Some(app) = try_app() {
                        app.touch(|m| m.set_browser_url(&id, &uri).ok());
                    }
                });
            }
        });
        let entry = self.entry.clone();
        self.web.connect_estimated_load_progress_notify(move |w| {
            let p = w.estimated_load_progress();
            entry.set_progress_fraction(if p >= 1.0 { 0.0 } else { p });
        });
        let this = Rc::downgrade(self);
        self.web.connect_title_notify(move |w| {
            if let Some(v) = this.upgrade() {
                let t = w.title().map(|t| t.to_string()).unwrap_or_default();
                v.entry
                    .set_tooltip_text((!t.is_empty()).then_some(t.as_str()));
            }
        });
        let this = Rc::downgrade(self);
        self.web.connect_load_changed(move |w, event| {
            let Some(v) = this.upgrade() else { return };
            match event {
                webkit6::LoadEvent::Started => {
                    v.blocked_url.replace(None);
                    v.load_error.replace(None);
                    v.status.set_label("Loading in WebKit… If the page is incompatible, open Chromium or your default browser.");
                    v.status.set_visible(true);
                }
                webkit6::LoadEvent::Finished => {
                    v.loads.set(v.loads.get() + 1);
                    v.status.set_visible(v.load_error.borrow().is_some());
                    v.entry.set_progress_fraction(0.0);
                    v.nav_buttons();
                    crate::markdown_view::fix_scale(w);
                    v.push_overlay();
                }
                _ => {}
            }
        });
        let this = Rc::downgrade(self);
        self.web.connect_load_failed(move |_, _, uri, err| {
            // A load replaced by another one is no failure.
            let replaced = err.matches(webkit6::NetworkError::Cancelled)
                || err.matches(webkit6::PolicyError::FrameLoadInterruptedByPolicyChange);
            if let Some(v) = this.upgrade().filter(|_| !replaced) {
                let msg = format!("{uri}: {}", err.message());
                v.log(json!({"level": "error", "text": format!("Load failed: {msg}"), "source": uri, "line": 0, "time": crate::panes::now()}));
                v.status.set_label(&format!("WebKit could not load this page: {msg}. Reload (F5), check the URL, or open Chromium / your browser."));
                v.status.set_visible(true);
                v.load_error.replace(Some(msg));
            }
            false // WebKit's error page
        });
        let this = Rc::downgrade(self);
        self.web.connect_script_dialog(move |_, d| {
            let Some(v) = this.upgrade() else { return false };
            let live = v
                .automating_until
                .get()
                .is_some_and(|t| std::time::Instant::now() < t);
            if !live {
                return false; // WebKit asks the human
            }
            use webkit6::ScriptDialogType as T;
            let kind = match d.dialog_type() {
                T::Confirm | T::BeforeUnloadConfirm => {
                    d.confirm_set_confirmed(true);
                    "confirm"
                }
                T::Prompt => {
                    let text = d.prompt_get_default_text().unwrap_or_default();
                    d.prompt_set_text(&text);
                    "prompt"
                }
                _ => "alert",
            };
            let msg = d.message().unwrap_or_default();
            v.log(json!({"level": "dialog", "text": format!("{kind} \"{msg}\" → accepted"), "source": v.web.uri().map(|u| u.to_string()), "line": 0, "time": crate::panes::now()}));
            true
        });
        let this = Rc::downgrade(self);
        self.web.connect_decide_policy(move |_, decision, _| {
            let uri =
                if let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() {
                    nav.navigation_action()
                        .and_then(|a| a.request())
                        .and_then(|r| r.uri())
                } else if let Some(response) =
                    decision.downcast_ref::<webkit6::ResponsePolicyDecision>()
                {
                    response.response().and_then(|r| r.uri())
                } else {
                    None
                };
            if let (Some(v), Some(uri)) = (this.upgrade(), uri) {
                if unsupported_embedded_url(&uri) {
                    decision.ignore();
                    v.block_navigation(&uri);
                    return true;
                }
            }
            false
        });
        // target=_blank and window.open: open here, never a new window.
        self.web.connect_create(|w, action| {
            let uri = action.clone().request().and_then(|r| r.uri());
            if let Some(uri) = uri {
                w.load_uri(&uri);
            }
            None
        });
        let this = Rc::downgrade(self);
        if let Some(i) = self.web.inspector() {
            i.connect_closed(move |_| {
                if let Some(v) = this.upgrade() {
                    v.inspector_open.set(false);
                }
            });
        }

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(v) = this.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mods = state
                & (gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::ALT_MASK);
            if v.annotating() && v.web.has_focus() && mods == gdk::ModifierType::CONTROL_MASK {
                let action = match key {
                    gdk::Key::Down => Some("next"),
                    gdk::Key::Up => Some("previous"),
                    gdk::Key::Return => Some("pick"),
                    gdk::Key::Escape => Some("cancel"),
                    _ => None,
                };
                if let Some(action) = action {
                    v.automation("annotateKey", &[json!(action)], |_| {});
                    return glib::Propagation::Stop;
                }
            }
            match (key, mods) {
                (gdk::Key::l | gdk::Key::L, gdk::ModifierType::CONTROL_MASK) => v.focus_url(),
                (gdk::Key::a | gdk::Key::A, m)
                    if m == gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK =>
                {
                    v.annotate.set_active(!v.annotate.is_active())
                }
                (gdk::Key::F5, m) if m.is_empty() => v.web.reload(),
                (gdk::Key::Left, gdk::ModifierType::ALT_MASK) => v.web.go_back(),
                (gdk::Key::Right, gdk::ModifierType::ALT_MASK) => v.web.go_forward(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.root.add_controller(keys);

        // Clicking or tabbing into the Browser focuses its Pane in the model.
        let id = self.id.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            if let Some(app) = try_app() {
                app.focus_entered(&id);
            }
        });
        self.root.add_controller(focus);
    }

    pub(crate) fn browser_status(&self) -> String {
        if self.status.is_visible() {
            self.status.text().to_string()
        } else {
            String::new()
        }
    }

    pub(crate) fn fallback_status(&self, text: &str) {
        self.status.set_label(text);
        self.status.set_visible(true);
    }

    fn nav_buttons(&self) {
        self.back.set_sensitive(self.web.can_go_back());
        self.forward.set_sensitive(self.web.can_go_forward());
    }

    fn log(&self, m: Value) {
        if m["level"] == "error" {
            let mut e = self.errors.borrow_mut();
            if e.len() == 20 {
                e.pop_front();
            }
            e.push_back(
                m["text"]
                    .as_str()
                    .unwrap_or("")
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string(),
            );
        }
        let mut c = self.console.borrow_mut();
        if c.len() == MAX_CONSOLE {
            c.pop_front();
            self.dropped.set(self.dropped.get() + 1);
        }
        c.push_back(m);
    }

    /// Console messages and errors since the last call (then forgotten),
    /// and how many were dropped over [`MAX_CONSOLE`].
    pub fn take_console(&self) -> (Vec<Value>, usize) {
        (
            self.console.borrow_mut().drain(..).collect(),
            self.dropped.replace(0),
        )
    }

    fn block_navigation(&self, url: &str) {
        self.blocked_url.replace(Some(url.to_string()));
        self.entry.set_text(url);
        self.entry.set_progress_fraction(0.0);
        self.load_error.replace(Some(UNSUPPORTED_MESSAGE.into()));
        self.loads.set(self.loads.get() + 1);
        self.fallback_status(UNSUPPORTED_MESSAGE);
    }

    pub(crate) fn preview_url(&self) -> String {
        self.blocked_url.borrow().clone().unwrap_or_else(|| {
            self.web
                .uri()
                .map(|u| u.to_string())
                .unwrap_or_else(|| self.entry.text().to_string())
        })
    }

    pub fn load(&self, url: &str) {
        self.blocked_url.replace(None);
        if unsupported_embedded_url(url) {
            self.block_navigation(url);
            return;
        }
        self.entry.set_text(if url == rade_core::browser::BLANK {
            ""
        } else {
            url
        });
        self.web.load_uri(url);
    }

    pub fn focus_url(&self) {
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    /// Keyboard focus: the URL bar on a blank page, else the page.
    pub fn focus(&self) {
        let blank = self
            .web
            .uri()
            .is_none_or(|u| u.is_empty() || u == rade_core::browser::BLANK);
        if blank {
            self.focus_url();
        } else if !self.entry.has_focus() {
            self.web.grab_focus();
        }
    }

    pub fn toggle_inspector(&self) {
        let Some(i) = self.web.inspector() else {
            return;
        };
        if self.inspector_open.replace(!self.inspector_open.get()) {
            i.close();
        } else {
            i.show();
        }
    }

    /// Run `body` as an async function body in `world` (None = the page's
    /// own); `done` gets its awaited result as JSON text, serialized by
    /// JavaScriptCore itself (a page can't swap that, unlike its
    /// `JSON.stringify`); `undefined` → `null`, unserializable → its string.
    pub fn call(
        &self,
        body: &str,
        world: Option<&str>,
        done: impl FnOnce(Result<String, String>) + 'static,
    ) {
        self.web.call_async_javascript_function(
            body,
            None,
            world,
            None,
            gio::Cancellable::NONE,
            move |r| {
                done(match r {
                    Ok(v) if v.is_undefined() => Ok("null".into()),
                    Ok(v) => Ok(v.to_json(0).map_or_else(
                        || Value::String(v.to_str().to_string()).to_string(),
                        |s| s.to_string(),
                    )),
                    Err(e) => Err(e.message().to_string()),
                })
            },
        );
    }

    /// Call `__rade.<f>(<args>)` from `assets/browser.js` in [`WORLD`]. Its
    /// errors come back as values, never thrown: a throw would reach the
    /// page's own error handlers.
    pub fn automation(
        &self,
        f: &str,
        args: &[Value],
        done: impl FnOnce(Result<Value, String>) + 'static,
    ) {
        let body = rade_core::browser::automation_body(f, args);
        self.call(&body, Some(WORLD), move |r| {
            done(r.and_then(|t| {
                let v: Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
                match v["error"].as_str() {
                    Some(e) => Err(e.to_string()),
                    None => Ok(v["ok"].clone()),
                }
            }))
        });
    }
}

#[cfg(test)]
mod support_tests {
    use super::unsupported_embedded_url;

    #[test]
    fn guard_matches_parsed_hosts_not_url_text() {
        for url in [
            "https://youtube.com/watch?v=x",
            "https://WWW.YouTube.COM./watch",
            "https://m.youtube.com:443/",
            "https://youtu.be/x",
            "https://www.youtube-nocookie.com/embed/x",
            "https://user:pw@youtube.com/",
            "https://%79outube.com/",
        ] {
            assert!(unsupported_embedded_url(url), "{url}");
        }
        for url in [
            "http://localhost:3000/youtube.com",
            "https://youtube.com.evil.test/",
            "https://notyoutube.com/",
            "https://youtube.com@localhost:3000/",
            "file:///tmp/youtube.com.html",
            "about:blank",
            "not a URL",
        ] {
            assert!(!unsupported_embedded_url(url), "{url}");
        }
    }
}
