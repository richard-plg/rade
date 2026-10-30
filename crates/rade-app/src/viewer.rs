//! Viewer Panes (R7.2): a read-only file view with a header (path, Back,
//! Copy path, Open in VS Code — `[viewer] editor`, else $EDITOR / default
//! app — close). Code and text use
//! GtkSourceView (highlighting, line numbers, scroll to `line`); Markdown
//! renders in a WebView (`markdown_view.rs`); images show as a picture;
//! large or binary files show a message. The file is watched and reloaded
//! on change; `Esc` closes the Pane, `Alt+Left` / `Alt+Right` go Back /
//! Forward. Code navigation (R84): F12 or Ctrl+click → definition,
//! Shift+F12 → references, Alt+Shift+H → callers, Ctrl+F12 →
//! implementations, at the caret (`crate::code`); M30: Ctrl+I or Ctrl
//! with the pointer resting on a name → hover, Alt+F12 → peek
//! (`crate::code_pop`), Alt+Shift+O or the header's Outline button → the
//! file's symbols in the results popover.

use crate::app::try_app;
use crate::markdown_view::MarkdownView;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::code::nav::Query;
use rade_core::viewer::{classify, Content, MAX_BYTES, PDF_MAX_BYTES};
use serde_json::{json, Value};
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// What is loaded: (file, line). A changed line re-scrolls, a changed file reloads.
type Shown = (PathBuf, Option<u32>);

pub struct Viewer {
    pub id: String,
    pub root: gtk::Box,
    title: gtk::Label,
    back: gtk::Button,
    forward: gtk::Button,
    /// Outline (code files only).
    outline: gtk::Button,
    open: gtk::Button,
    stack: gtk::Stack,
    source: sourceview5::View,
    buffer: sourceview5::Buffer,
    picture: gtk::Picture,
    message: adw::StatusPage,
    markdown: RefCell<Option<MarkdownView>>,
    /// HTML and PDF, rendered (R114).
    doc: RefCell<Option<crate::doc_view::DocView>>,
    /// An HTML file shows its source instead (the header toggle, R114.2).
    html_source: Cell<bool>,
    /// The header's Rendered / Source toggle (HTML only).
    source_toggle: gtk::ToggleButton,
    /// The header's Zoom / Restore button (R113.1).
    zoom: gtk::Button,
    /// Content zoom (R113.5): `−  100%  +` in the header.
    zoom_box: gtk::Box,
    zoom_label: gtk::Button,
    content_zoom: Cell<f64>,
    /// An image fits the Pane (its zoom follows the size).
    image_fit: Cell<bool>,
    image_scroll: gtk::ScrolledWindow,
    /// The code view's zoomed font size (`#viewer-src-<id>`).
    zoom_css: gtk::CssProvider,
    /// The zoom applied to the Markdown / document WebViews so far.
    web_zoom: Cell<(f64, f64)>,
    /// A PDF is shown: its own viewer zooms.
    pdf: Cell<bool>,
    shown: RefCell<Option<Shown>>,
    root_dir: RefCell<PathBuf>,
    worktree: RefCell<PathBuf>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    reload_pending: Cell<bool>,
    /// The caret column for the next scroll to that line (a code jump).
    pending_col: Cell<Option<(u32, u32)>>,
    /// The pointer-rest timer of a Ctrl+hover.
    rest: Cell<Option<glib::SourceId>>,
    /// The pointer's last place over the source view.
    pointer: Cell<Option<(f64, f64)>>,
    /// The name shown as a link while Ctrl is held over it: (0-based line,
    /// first and past-the-end character).
    link: Cell<Option<(i32, i32, i32)>>,
    /// Gutter marks of the lines changed against the index (R123).
    quick: Rc<crate::quick_diff::QuickDiff>,
}

pub fn is_dark() -> bool {
    adw::StyleManager::default().is_dark()
}

/// The link color of a name under Ctrl: the system accent as text color.
pub fn link_rgba() -> gdk::RGBA {
    let sm = adw::StyleManager::default();
    sm.accent_color().to_standalone_rgba(sm.is_dark())
}

/// Open a URI with the desktop's default handler (browser for http(s)).
pub fn launch_uri(uri: &str) {
    if let Err(e) = gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>) {
        eprintln!("rade: opening {uri}: {e}");
    }
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("flat");
    b
}

/// `[viewer] editor` when it is on PATH (read per click: config reloads live).
pub(crate) fn gui_editor() -> Option<String> {
    let app = try_app()?;
    // The Source on screen's layered `[viewer] editor` (R29.2).
    let (s, w) = {
        let m = app.model.borrow();
        let (s, w) = m.active();
        (s.id.clone(), w.id.clone())
    };
    let editor = app.config_for(&s, Some(&w)).viewer.editor;
    let bin = editor.split_whitespace().next()?;
    glib::find_program_in_path(bin).map(|_| editor)
}

/// (icon, tooltip) for the editor button.
fn open_action() -> (String, String) {
    let Some(editor) = gui_editor() else {
        let tip = match editor() {
            Some(e) => format!("Open in {e}"),
            None => "Open with the default app".into(),
        };
        return ("ph-arrow-square-out-symbolic".into(), tip);
    };
    let bin = Path::new(editor.split_whitespace().next().unwrap_or_default())
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let (names, tip): (&[&str], String) = if bin == "code" {
        // VS Code's .deb installs `Icon=vscode` (/usr/share/pixmaps).
        (
            &["vscode", "com.visualstudio.code", "code"],
            "Open in VS Code".into(),
        )
    } else {
        (&[bin.as_str()], format!("Open in {bin}"))
    };
    let theme = gtk::IconTheme::for_display(&gdk::Display::default().expect("display"));
    let icon = names
        .iter()
        .find(|n| theme.has_icon(n))
        .map_or("document-edit-symbolic", |n| n);
    (icon.into(), tip)
}

fn editor() -> Option<String> {
    ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
}

pub(crate) fn scheme_for(dark: bool) -> Option<sourceview5::StyleScheme> {
    let m = sourceview5::StyleSchemeManager::default();
    let ids: &[&str] = if dark {
        &[crate::syntax::DARK, "Adwaita-dark", "classic-dark"]
    } else {
        &[crate::syntax::LIGHT, "Adwaita", "classic"]
    };
    ids.iter().find_map(|id| m.scheme(id))
}

impl Viewer {
    pub fn new(id: &str) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.add_css_class("rade-viewer");
        root.add_css_class("view");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        header.add_css_class("rade-viewer-header");
        let back = icon_button("ph-caret-left-symbolic", "Back (Alt+Left)");
        let forward = icon_button("ph-caret-right-symbolic", "Forward (Alt+Right)");
        let icon = gtk::Image::from_icon_name("ph-file-text-symbolic");
        icon.add_css_class("rade-viewer-icon");
        let title = gtk::Label::new(None);
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        title.add_css_class("rade-viewer-title");
        let outline = icon_button("ph-list-bullets-symbolic", "Outline (Alt+Shift+O)");
        outline.add_css_class("rade-viewer-outline");
        let source_toggle = gtk::ToggleButton::new();
        source_toggle.set_icon_name("ph-file-code-symbolic");
        source_toggle.set_tooltip_text(Some("Show the source"));
        source_toggle.add_css_class("flat");
        source_toggle.add_css_class("rade-viewer-source");
        source_toggle.set_focus_on_click(false);
        source_toggle.set_visible(false);
        let copy = icon_button("ph-copy-symbolic", "Copy path");
        let open = icon_button("document-edit-symbolic", "");
        let zoom_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        zoom_box.add_css_class("rade-viewer-zoombox");
        let zoom_out = icon_button("ph-minus-symbolic", "Zoom out (Ctrl+−, Ctrl+scroll)");
        let zoom_label = gtk::Button::with_label("100%");
        zoom_label.add_css_class("flat");
        zoom_label.add_css_class("rade-viewer-zoomlabel");
        zoom_label.set_focus_on_click(false);
        zoom_label.set_tooltip_text(Some("Actual size / fit (Ctrl+0)"));
        let zoom_in = icon_button("ph-plus-symbolic", "Zoom in (Ctrl++, Ctrl+scroll)");
        zoom_box.append(&zoom_out);
        zoom_box.append(&zoom_label);
        zoom_box.append(&zoom_in);
        let zoom = icon_button("view-fullscreen-symbolic", "Zoom (Ctrl+Shift+Z)");
        zoom.add_css_class("rade-viewer-zoom");
        let close = icon_button("ph-x-symbolic", "Close (Esc)");
        for w in [
            back.upcast_ref::<gtk::Widget>(),
            forward.upcast_ref(),
            icon.upcast_ref(),
            title.upcast_ref(),
        ] {
            header.append(w);
        }
        header.append(&zoom_box);
        header.append(&source_toggle);
        for w in [&outline, &copy, &open, &zoom, &close] {
            header.append(w);
        }

        let buffer = sourceview5::Buffer::new(None);
        buffer.set_highlight_matching_brackets(false);
        let source = sourceview5::View::with_buffer(&buffer);
        source.set_editable(false);
        // A caret: code navigation asks about the name under it.
        source.set_cursor_visible(true);
        source.set_monospace(true);
        source.set_show_line_numbers(true);
        source.set_highlight_current_line(true);
        source.set_tab_width(4);
        source.set_left_margin(12);
        source.set_top_margin(10);
        source.set_bottom_margin(24);
        source.set_widget_name(&format!("viewer-src-{id}"));
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&source));
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::ScaleDown);
        picture.set_margin_top(12);
        picture.set_margin_bottom(12);
        let message = adw::StatusPage::new();
        message.set_icon_name(Some("dialog-information-symbolic"));

        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&scroll, Some("text"));
        let image_scroll = gtk::ScrolledWindow::new();
        image_scroll.set_child(Some(&picture));
        stack.add_named(&image_scroll, Some("image"));
        stack.add_named(&message, Some("message"));
        root.append(&header);
        root.append(&stack);

        let v = Rc::new(Self {
            id: id.into(),
            root,
            title,
            back,
            forward,
            outline: outline.clone(),
            open: open.clone(),
            stack,
            quick: crate::quick_diff::QuickDiff::attach(id, &source, &buffer),
            source,
            buffer,
            picture,
            message,
            markdown: RefCell::default(),
            doc: RefCell::default(),
            html_source: Cell::new(false),
            source_toggle: source_toggle.clone(),
            zoom: zoom.clone(),
            zoom_box: zoom_box.clone(),
            zoom_label: zoom_label.clone(),
            content_zoom: Cell::new(1.0),
            image_fit: Cell::new(true),
            image_scroll,
            zoom_css: gtk::CssProvider::new(),
            web_zoom: Cell::new((1.0, 1.0)),
            pdf: Cell::new(false),
            shown: RefCell::default(),
            root_dir: RefCell::default(),
            worktree: RefCell::default(),
            monitor: RefCell::default(),
            reload_pending: Cell::new(false),
            pending_col: Cell::new(None),
            rest: Cell::new(None),
            pointer: Cell::new(None),
            link: Cell::new(None),
        });
        v.connect(&copy, &open, &close);
        v.connect_zoom(&zoom_out, &zoom_in);
        v
    }

    /// Content zoom (R113.5): the header's buttons, Ctrl+scroll.
    fn connect_zoom(self: &Rc<Self>, out: &gtk::Button, inn: &gtk::Button) {
        if let Some(d) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &d,
                &self.zoom_css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
            );
        }
        for (b, dir) in [(out, -1), (&self.zoom_label, 0), (inn, 1)] {
            let this = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(v) = this.upgrade() {
                    v.step_zoom(dir);
                }
            });
        }
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = Rc::downgrade(self);
        scroll.connect_scroll(move |c, _, dy| {
            let ctrl = c
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK);
            match this.upgrade() {
                Some(v) if ctrl && v.zoom_box.is_visible() && dy != 0.0 => {
                    v.step_zoom(if dy < 0.0 { 1 } else { -1 });
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.root.add_controller(scroll);
    }

    /// One zoom step in (1), out (-1) or back (0: 100 %, an image fits).
    pub fn step_zoom(&self, dir: i32) {
        let view = self.stack.visible_child_name();
        let image = view.as_deref() == Some("image");
        if image && dir == 0 {
            self.image_fit.set(true);
        } else {
            let from = if image && self.image_fit.get() {
                self.fit_scale()
            } else {
                self.content_zoom.get()
            };
            self.image_fit.set(false);
            self.content_zoom
                .set(rade_core::viewer::zoom_step(from, dir));
        }
        self.apply_zoom();
    }

    /// The scale at which the image fits the Pane (at most 1).
    fn fit_scale(&self) -> f64 {
        let Some(p) = self.picture.paintable() else {
            return 1.0;
        };
        let (w, h) = (p.intrinsic_width() as f64, p.intrinsic_height() as f64);
        if w <= 0.0 || h <= 0.0 {
            return 1.0;
        }
        let (aw, ah) = (
            self.image_scroll.width() as f64 - 24.0,
            self.image_scroll.height() as f64 - 24.0,
        );
        (aw / w).min(ah / h).clamp(0.01, 1.0)
    }

    /// Show the current zoom on what is visible.
    fn apply_zoom(&self) {
        let z = self.content_zoom.get();
        let view = self.stack.visible_child_name();
        self.zoom_box.set_visible(
            matches!(view.as_deref(), Some("text" | "markdown" | "image"))
                || (view.as_deref() == Some("doc") && !self.pdf.get()),
        );
        let fit = view.as_deref() == Some("image") && self.image_fit.get();
        self.zoom_label.set_label(&if fit {
            "Fit".to_string()
        } else {
            rade_core::viewer::zoom_label(z)
        });
        let size = try_app()
            .map(|a| a.config.borrow().code_font().size)
            .unwrap_or(13.0) as f64;
        self.zoom_css.load_from_string(&if (z - 1.0).abs() < 1e-6 {
            String::new()
        } else {
            format!(
                "#viewer-src-{} {{ font-size: {:.1}px; }}",
                self.id,
                size * z
            )
        });
        let web = |w: &webkit6::WebView, applied: f64| {
            if (applied - z).abs() > 1e-6 {
                use webkit6::prelude::WebViewExt;
                w.set_zoom_level(w.zoom_level() / applied * z);
            }
        };
        let (md_z, doc_z) = self.web_zoom.get();
        if let Some(md) = self.markdown.borrow().as_ref() {
            web(md.widget(), md_z);
        }
        let doc_now = if self.pdf.get() { doc_z } else { z };
        if let Some(d) = self.doc.borrow().as_ref() {
            if !self.pdf.get() {
                web(d.widget(), doc_z);
            }
        }
        self.web_zoom.set((z, doc_now));
        if fit {
            self.picture.set_content_fit(gtk::ContentFit::ScaleDown);
            self.picture.set_halign(gtk::Align::Fill);
            self.picture.set_valign(gtk::Align::Fill);
            self.picture.set_size_request(-1, -1);
            self.picture.set_can_shrink(true);
            self.image_scroll
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
        } else if let Some(p) = self.picture.paintable() {
            let s = if view.as_deref() == Some("image") {
                z
            } else {
                1.0
            };
            // Its own size, centred while smaller than the Pane.
            self.picture.set_content_fit(gtk::ContentFit::Contain);
            self.picture.set_halign(gtk::Align::Center);
            self.picture.set_valign(gtk::Align::Center);
            self.picture.set_can_shrink(false);
            self.picture.set_size_request(
                (p.intrinsic_width() as f64 * s) as i32,
                (p.intrinsic_height() as f64 * s) as i32,
            );
            self.image_scroll
                .set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        }
    }

    fn connect(self: &Rc<Self>, copy: &gtk::Button, open: &gtk::Button, close: &gtk::Button) {
        let id = self.id.clone();
        let this = Rc::downgrade(self);
        copy.connect_clicked(move |b| {
            if let Some(v) = this.upgrade() {
                if let Some(p) = v.path() {
                    b.clipboard().set_text(&p.display().to_string());
                }
            }
        });
        let this = Rc::downgrade(self);
        open.connect_clicked(move |_| {
            if let Some(v) = this.upgrade() {
                v.open_external();
            }
        });
        let i = id.clone();
        close.connect_clicked(move |_| close_viewer(&i));
        // Outline only for code and text (not Markdown, images, messages).
        let o = self.outline.clone();
        self.stack.connect_visible_child_name_notify(move |st| {
            o.set_visible(st.visible_child_name().as_deref() == Some("text"));
        });
        let this = Rc::downgrade(self);
        self.outline.connect_clicked(move |_| {
            if let Some(v) = this.upgrade() {
                v.nav(Query::Symbols);
            }
        });
        let this = Rc::downgrade(self);
        self.source_toggle.connect_toggled(move |t| {
            if let Some(v) = this.upgrade() {
                if v.html_source.replace(t.is_active()) != t.is_active() {
                    v.load(false);
                }
            }
        });
        let i = id.clone();
        self.zoom.connect_clicked(move |_| toggle_zoom(&i));
        let i = id.clone();
        self.back.connect_clicked(move |_| go_back(&i));
        let i = id.clone();
        self.forward.connect_clicked(move |_| go_forward(&i));

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| match this.upgrade() {
            Some(v) if v.on_key(key, state) => glib::Propagation::Stop,
            _ => glib::Propagation::Proceed,
        });
        // Ctrl let go: the name is plain text again.
        let this = Rc::downgrade(self);
        keys.connect_key_released(move |_, key, _, _| {
            if let (Some(v), gdk::Key::Control_L | gdk::Key::Control_R) = (this.upgrade(), key) {
                v.set_link(None);
            }
        });
        self.root.add_controller(keys);

        // Ctrl+click on a name: its definition (R84.1). In the capture
        // phase, claimed: the text view never sees the press, so the
        // selection stays and no drag-select starts (0.7.1).
        let ctrl = gtk::GestureClick::new();
        ctrl.set_button(gdk::BUTTON_PRIMARY);
        ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = Rc::downgrade(self);
        ctrl.connect_pressed(move |g, _, x, y| {
            if !g
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                return;
            }
            if let Some(v) = this.upgrade() {
                if v.ctrl_click(x, y) {
                    g.set_state(gtk::EventSequenceState::Claimed);
                }
            }
        });
        self.source.add_controller(ctrl);

        // Ctrl with the pointer resting on a name: its hover (R90.1).
        let motion = gtk::EventControllerMotion::new();
        let this = Rc::downgrade(self);
        motion.connect_motion(move |m, x, y| {
            let Some(v) = this.upgrade() else { return };
            crate::code_pop::pointer_moved(v.source.upcast_ref(), x, y);
            v.pointer.set(Some((x, y)));
            if m.current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                v.set_link(Some((x, y)));
                v.rest_then_hover(x, y);
            } else {
                v.set_link(None);
                if let Some(t) = v.rest.take() {
                    t.remove();
                }
            }
        });
        let this = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(v) = this.upgrade() {
                v.pointer.set(None);
                v.set_link(None);
                if let Some(t) = v.rest.take() {
                    t.remove();
                }
            }
        });
        self.source.add_controller(motion);

        // Clicking into the Viewer focuses its Pane in the model.
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            if let Some(app) = try_app() {
                app.focus_entered(&id);
            }
        });
        self.root.add_controller(focus);
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = Rc::downgrade(self);
        click.connect_pressed(move |_, _, _, _| {
            crate::code_pop::close();
            if let Some(v) = this.upgrade() {
                v.focus();
            }
        });
        self.root.add_controller(click);
    }

    /// The Viewer's keys; true = handled. Code navigation keys only act
    /// on a code / text file (not Markdown, images).
    pub fn on_key(&self, key: gdk::Key, state: gdk::ModifierType) -> bool {
        use gdk::ModifierType as M;
        let mods = state & (M::CONTROL_MASK | M::SHIFT_MASK | M::ALT_MASK);
        let text = self.stack.visible_child_name().as_deref() == Some("text");
        if key == gdk::Key::Escape && (crate::code_pop::close() || self.quick.close_peek()) {
            return true;
        }
        // Ctrl pressed with the pointer already resting on a name (R90.1).
        if matches!(key, gdk::Key::Control_L | gdk::Key::Control_R) {
            if let Some((x, y)) = self.pointer.get() {
                self.set_link(Some((x, y)));
                self.rest_then_hover(x, y);
            }
            return false;
        }
        let q = match (key.to_lower(), mods) {
            // Zoomed: Esc restores the Layout first (R113.1).
            (gdk::Key::Escape, m) if m.is_empty() => {
                if self.zoomed() {
                    toggle_zoom(&self.id);
                } else {
                    close_viewer(&self.id);
                }
                return true;
            }
            // Content zoom (R113.5); PDF has its own.
            (gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add, m)
                if m.contains(M::CONTROL_MASK) && self.zoom_box.is_visible() =>
            {
                self.step_zoom(1);
                return true;
            }
            (gdk::Key::minus | gdk::Key::KP_Subtract, M::CONTROL_MASK)
                if self.zoom_box.is_visible() =>
            {
                self.step_zoom(-1);
                return true;
            }
            (gdk::Key::_0 | gdk::Key::KP_0, M::CONTROL_MASK) if self.zoom_box.is_visible() => {
                self.step_zoom(0);
                return true;
            }
            (gdk::Key::i, M::CONTROL_MASK) if text => {
                self.pop_at_caret(false);
                return true;
            }
            (gdk::Key::F12, M::ALT_MASK) if text => {
                self.pop_at_caret(true);
                return true;
            }
            (gdk::Key::o, m) if m == M::ALT_MASK | M::SHIFT_MASK => Query::Symbols,
            (gdk::Key::Left, M::ALT_MASK) => {
                go_back(&self.id);
                return true;
            }
            (gdk::Key::Right, M::ALT_MASK) => {
                go_forward(&self.id);
                return true;
            }
            (gdk::Key::F12, m) if m.is_empty() => Query::Def,
            (gdk::Key::F12, M::SHIFT_MASK) => Query::Refs,
            (gdk::Key::F12, M::CONTROL_MASK) => Query::Impl,
            (gdk::Key::h, m) if m == M::ALT_MASK | M::SHIFT_MASK => Query::Callers,
            _ => return false,
        };
        if text {
            crate::code_pop::close();
            self.nav(q);
        }
        text
    }

    /// The caret's character as a rectangle in the source view.
    fn caret_rect(&self) -> gdk::Rectangle {
        let it = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let r = self.source.iter_location(&it);
        let (x, y) = self
            .source
            .buffer_to_window_coords(gtk::TextWindowType::Widget, r.x(), r.y());
        gdk::Rectangle::new(x, y, r.width().max(8), r.height())
    }

    /// The header's Outline button (`debug.viewer outline`); a no-op
    /// when it is hidden.
    pub fn outline_click(&self) {
        if self.outline.is_visible() {
            self.outline.emit_clicked();
        }
    }

    /// Where a popover at the caret points.
    pub fn caret_anchor(&self) -> crate::code_pop::Anchor {
        crate::code_pop::Anchor {
            widget: self.source.clone().upcast(),
            rect: self.caret_rect(),
            pointer: None,
        }
    }

    /// Hover (or peek) at the caret (Ctrl+I, Alt+F12).
    pub fn pop_at_caret(&self, peek: bool) {
        let Some(path) = self.path() else { return };
        let (line, col) = self.caret();
        crate::code_pop::ask(&self.id, peek, self.caret_anchor(), &path, line, col);
    }

    /// Hover at (x, y) after 500 ms of rest (a new move restarts it).
    fn rest_then_hover(&self, x: f64, y: f64) {
        if let Some(t) = self.rest.take() {
            t.remove();
        }
        let id = self.id.clone();
        self.rest.set(Some(glib::timeout_add_local_once(
            Duration::from_millis(500),
            move || {
                let v = try_app().and_then(|a| a.viewers.borrow().get(&id).cloned());
                if let Some(v) = v {
                    v.rest.set(None);
                    v.hover_at(x, y);
                }
            },
        )));
    }

    /// The pointer rested with Ctrl at (x, y) of the source view: the
    /// hover of the name there (nothing over blank space).
    pub fn hover_at(&self, x: f64, y: f64) {
        if self.stack.visible_child_name().as_deref() != Some("text") {
            return;
        }
        let Some(path) = self.path() else { return };
        let (bx, by) =
            self.source
                .window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let Some(it) = self.source.iter_at_location(bx, by) else {
            return;
        };
        let mut end = it;
        end.forward_to_line_end();
        let mut start = it;
        start.set_line_offset(0);
        let line_text = self.buffer.text(&start, &end, false);
        let col = it.line_offset() as usize;
        // Only on a name (not just before one, as for the caret).
        let on_name = line_text
            .chars()
            .nth(col)
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
        if !on_name {
            return;
        }
        let r = self.source.iter_location(&it);
        let (rx, ry) =
            self.source
                .buffer_to_window_coords(gtk::TextWindowType::Widget, r.x(), r.y());
        let at = crate::code_pop::Anchor {
            widget: self.source.clone().upcast(),
            rect: gdk::Rectangle::new(rx, ry, r.width().max(8), r.height()),
            pointer: Some((x, y)),
        };
        crate::code_pop::ask(
            &self.id,
            false,
            at,
            &path,
            it.line() as u32 + 1,
            col as u32 + 1,
        );
    }

    /// The caret as (1-based line, 1-based character column).
    pub fn caret(&self) -> (u32, u32) {
        let it = self.buffer.iter_at_mark(&self.buffer.get_insert());
        (it.line() as u32 + 1, it.line_offset() as u32 + 1)
    }

    /// Ask about the name at the caret (R84).
    pub fn nav(&self, q: Query) {
        let Some(path) = self.path() else { return };
        let (line, col) = self.caret();
        crate::code::navigate(&self.id, q, &path, line, col, Duration::ZERO);
    }

    /// Select from `a` to `b` (1-based line, column).
    pub fn select(&self, a: (u32, u32), b: (u32, u32)) {
        let at = |(l, c): (u32, u32)| {
            self.buffer
                .iter_at_line_offset(l.max(1) as i32 - 1, c.max(1) as i32 - 1)
        };
        if let (Some(a), Some(b)) = (at(a), at(b)) {
            self.buffer.select_range(&a, &b);
        }
    }

    /// Copy the selection to the clipboard, as Ctrl+C does (R135, test
    /// driver): the code view's or the WebView's.
    pub fn copy(&self) {
        match self
            .markdown()
            .filter(|_| self.stack.visible_child_name().as_deref() != Some("text"))
        {
            Some(md) => webkit6::prelude::WebViewExt::execute_editing_command(md.widget(), "Copy"),
            None => self.source.emit_copy_clipboard(),
        }
    }

    /// Put the caret at `line:col` (1-based).
    pub fn place(&self, line: u32, col: u32) {
        if let Some(it) = self
            .buffer
            .iter_at_line_offset(line.max(1) as i32 - 1, col.max(1) as i32 - 1)
        {
            self.buffer.place_cursor(&it);
        }
    }

    /// A Ctrl+click at widget coordinates on a name: its definition, the
    /// caret and selection left as they are (0.7.1). False = not on a name.
    pub fn ctrl_click(&self, x: f64, y: f64) -> bool {
        let (Some((line, start, _)), Some(path)) = (self.name_at(x, y), self.path()) else {
            return false;
        };
        crate::code_pop::close();
        crate::code::navigate(
            &self.id,
            Query::Def,
            &path,
            line as u32 + 1,
            start as u32 + 1,
            Duration::ZERO,
        );
        true
    }

    /// The name under widget coordinates (x, y) of the text: (0-based
    /// line, first and past-the-end character) — from the buffer, no server.
    fn name_at(&self, x: f64, y: f64) -> Option<(i32, i32, i32)> {
        if self.stack.visible_child_name().as_deref() != Some("text") {
            return None;
        }
        let (bx, by) =
            self.source
                .window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let it = self.source.iter_at_location(bx, by)?;
        // Past the line's end, iter_at_location gives its last character.
        let r = self.source.iter_location(&it);
        if bx < r.x() || bx >= r.x() + r.width().max(1) {
            return None;
        }
        let mut start = it;
        start.set_line_offset(0);
        let mut end = it;
        end.forward_to_line_end();
        let text = self.buffer.text(&start, &end, false);
        let col = it.line_offset() as usize;
        let on = text
            .chars()
            .nth(col)
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
        let (w, s) = on
            .then(|| rade_core::code::nav::word_at(&text, col))
            .flatten()
            .filter(|(w, _)| !rade_core::code::nav::is_keyword(w))?;
        Some((it.line(), s as i32, (s + w.chars().count()) as i32))
    }

    /// Ctrl held over a name (0.7.1, as VS Code): underlined in the link
    /// color, the pointer a hand; `None` (or not on a name) = plain again.
    pub fn set_link(&self, at: Option<(f64, f64)>) {
        let name = at.and_then(|(x, y)| self.name_at(x, y));
        let old = self.link.get();
        // The pointer always follows (a jump to another file clears the
        // link, not the hand).
        self.source
            .set_cursor_from_name(Some(if name.is_some() { "pointer" } else { "text" }));
        if name == old {
            return;
        }
        let tags = self.buffer.tag_table();
        let tag = tags.lookup("rade-link").unwrap_or_else(|| {
            let t = gtk::TextTag::new(Some("rade-link"));
            t.set_underline(gtk::pango::Underline::Single);
            tags.add(&t);
            t
        });
        tag.set_foreground_rgba(Some(&link_rgba()));
        let range = |(l, a, b): (i32, i32, i32)| {
            Some((
                self.buffer.iter_at_line_offset(l, a)?,
                self.buffer.iter_at_line_offset(l, b)?,
            ))
        };
        if let Some((a, b)) = old.and_then(range) {
            self.buffer.remove_tag(&tag, &a, &b);
        }
        if let Some((a, b)) = name.and_then(range) {
            self.buffer.apply_tag(&tag, &a, &b);
        }
        self.link.set(name);
    }

    /// The name shown as a link and the pointer's cursor (`debug.viewer info`).
    fn link_info(&self) -> Value {
        let word = self.link.get().and_then(|(l, a, b)| {
            let s = self.buffer.iter_at_line_offset(l, a)?;
            let e = self.buffer.iter_at_line_offset(l, b)?;
            Some(json!({"line": l + 1, "col": a + 1, "word": self.buffer.text(&s, &e, false).to_string(),
                        "underlined": s.has_tag(&self.buffer.tag_table().lookup("rade-link")?)}))
        });
        json!({"link": word, "cursor": self.source.cursor().and_then(|c| c.name()).map(|n| n.to_string())})
    }

    /// Widget coordinates of `line:col` (the middle of its character), for
    /// `debug.viewer ctrl_click`.
    pub fn coords_of(&self, line: u32, col: u32) -> Option<(f64, f64)> {
        let it = self
            .buffer
            .iter_at_line_offset(line.max(1) as i32 - 1, col.max(1) as i32 - 1)?;
        let r = self.source.iter_location(&it);
        let (x, y) = self.source.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            r.x() + r.width().max(4) / 4,
            r.y() + r.height() / 2,
        );
        Some((x as f64, y as f64))
    }

    /// `line:col` in the coordinates of `win` (live tests aim a real click).
    pub fn window_point(
        &self,
        win: &impl IsA<gtk::Widget>,
        line: u32,
        col: u32,
    ) -> Option<(f32, f32)> {
        let (x, y) = self.coords_of(line, col)?;
        let p = self
            .source
            .compute_point(win, &gtk::graphene::Point::new(x as f32, y as f32))?;
        Some((p.x(), p.y()))
    }

    /// The caret column to use when the next scroll lands on `line`.
    pub fn set_pending_col(&self, line: u32, col: u32) {
        self.pending_col.set(Some((line, col)));
    }

    /// Quick Diff (R123): the gutter marks and their peek.
    pub fn quick_diff(&self) -> &Rc<crate::quick_diff::QuickDiff> {
        &self.quick
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.shown.borrow().as_ref().map(|s| s.0.clone())
    }

    /// Show `path` at `line` (from the model); reloads only what changed.
    pub fn show(
        self: &Rc<Self>,
        path: &Path,
        line: Option<u32>,
        (can_go_back, can_go_forward): (bool, bool),
        worktree: &Path,
    ) {
        self.back.set_visible(can_go_back || can_go_forward);
        self.back.set_sensitive(can_go_back);
        self.forward.set_visible(can_go_back || can_go_forward);
        self.forward.set_sensitive(can_go_forward);
        let (icon, tip) = open_action();
        self.open.set_icon_name(&icon);
        self.open.set_tooltip_text(Some(&tip));
        *self.worktree.borrow_mut() = worktree.to_path_buf();
        *self.root_dir.borrow_mut() = rade_core::viewer::access_root(path, worktree);
        let prev = self.shown.borrow().clone();
        let rel = path.strip_prefix(worktree).unwrap_or(path);
        let label = match line {
            Some(l) => format!("{}:{l}", rel.display()),
            None => rel.display().to_string(),
        };
        // Folder dimmed, the file's name in full ink (R102 Pane header).
        let (dir, name) = match label.rfind('/') {
            Some(i) => label.split_at(i + 1),
            None => ("", label.as_str()),
        };
        self.title.set_markup(&format!(
            "<span alpha=\"62%\">{}</span>{}",
            glib::markup_escape_text(dir),
            glib::markup_escape_text(name)
        ));
        self.title
            .set_tooltip_text(Some(&path.display().to_string()));
        match prev {
            Some((p, l)) if p == path && l == line => {}
            Some((p, _)) if p == path => {
                *self.shown.borrow_mut() = Some((path.into(), line));
                self.scroll_to(line);
            }
            _ => {
                *self.shown.borrow_mut() = Some((path.into(), line));
                self.watch(path);
                self.load(false);
            }
        }
    }

    fn watch(self: &Rc<Self>, path: &Path) {
        let monitor = gio::File::for_path(path)
            .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            .ok();
        if let Some(m) = &monitor {
            let this = Rc::downgrade(self);
            m.connect_changed(move |_, _, _, event| {
                if event == gio::FileMonitorEvent::AttributeChanged {
                    return;
                }
                let Some(v) = this.upgrade() else { return };
                if v.reload_pending.replace(true) {
                    return;
                }
                // Writers emit bursts (truncate, write, close): reload once.
                let this = Rc::downgrade(&v);
                glib::timeout_add_local_once(Duration::from_millis(150), move || {
                    if let Some(v) = this.upgrade() {
                        v.reload_pending.set(false);
                        v.load(true);
                    }
                });
            });
        }
        *self.monitor.borrow_mut() = monitor;
    }

    /// (Re)load the file. `keep_place` = a live reload: keep the scroll
    /// position instead of jumping to `line`.
    pub fn load(self: &Rc<Self>, keep_place: bool) {
        // Another file, Back / Forward, a jump: the popover is about the old place.
        crate::code_pop::close_on(self.source.upcast_ref());
        let Some((path, line)) = self.shown.borrow().clone() else {
            return;
        };
        let message = |icon: &str, title: &str, body: String| {
            self.message.set_icon_name(Some(icon));
            self.message.set_title(title);
            self.message.set_description(Some(&body));
            self.stack.set_visible_child_name("message");
        };
        let kind = classify(&path);
        // Quick Diff marks a code file only (R123.1); set again below.
        if !matches!(kind, Content::Text) {
            self.quick.set_file(None);
        }
        self.pdf.set(kind == Content::Pdf);
        self.source_toggle.set_visible(kind == Content::Html);
        let show_source = kind == Content::Html && self.html_source.get();
        self.source_toggle.set_tooltip_text(Some(if show_source {
            "Show it rendered"
        } else {
            "Show the source"
        }));
        if kind == Content::Pdf || (kind == Content::Html && !show_source) {
            let doc = self
                .doc
                .borrow_mut()
                .get_or_insert_with(|| {
                    let d = crate::doc_view::DocView::new(&self.id);
                    self.stack.add_named(d.widget(), Some("doc"));
                    d
                })
                .clone();
            let scripts = try_app().is_none_or(|a| a.config.borrow().viewer.html_scripts);
            // Shown first, like Markdown (a WebView loading unmapped
            // keeps a wrongly scaled first frame).
            self.stack.set_visible_child_name("doc");
            doc.load(&path, keep_place, scripts);
            self.apply_zoom();
            return;
        }
        match kind {
            // Shown rendered above.
            Content::Pdf => {}
            Content::Markdown => {
                let md = self
                    .markdown
                    .borrow_mut()
                    .get_or_insert_with(|| {
                        let md = MarkdownView::new(&self.id);
                        self.stack.add_named(md.widget(), Some("markdown"));
                        md
                    })
                    .clone();
                // Shown first: a WebView that loads while unmapped keeps a
                // wrongly scaled first frame.
                self.stack.set_visible_child_name("markdown");
                md.load(&path, &self.root_dir.borrow(), keep_place);
            }
            Content::Text | Content::Html => {
                let text = std::fs::read(&path)
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                let vadj = self.source.vadjustment().map(|a| a.value());
                let lang = sourceview5::LanguageManager::default()
                    .guess_language(path.file_name().and_then(|n| n.to_str()), None);
                self.buffer.set_language(lang.as_ref());
                self.buffer.set_highlight_syntax(true);
                self.buffer.set_style_scheme(scheme_for(is_dark()).as_ref());
                self.link.set(None);
                self.buffer.set_text(&text);
                self.quick.set_file(Some(&path));
                self.stack.set_visible_child_name("text");
                match (keep_place, vadj) {
                    (true, Some(y)) => {
                        let source = self.source.clone();
                        glib::idle_add_local_once(move || {
                            if let Some(a) = source.vadjustment() {
                                a.set_value(y);
                            }
                        });
                    }
                    _ => self.scroll_to(line),
                }
            }
            Content::Image => {
                // Another image starts fitted; a live reload keeps the zoom.
                if !keep_place {
                    self.image_fit.set(true);
                }
                self.picture.set_filename(Some(&path));
                self.stack.set_visible_child_name("image");
            }
            Content::TooLarge(n) => message(
                "dialog-information-symbolic",
                "File too large to show",
                format!(
                    "{:.1} MB — the Viewer shows {} up to {} MB. Use “Open” above.",
                    n as f64 / 1e6,
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                    {
                        "PDFs"
                    } else {
                        "files"
                    },
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                    {
                        PDF_MAX_BYTES
                    } else {
                        MAX_BYTES
                    } / (1024 * 1024)
                ),
            ),
            Content::Binary => message(
                "application-x-executable-symbolic",
                "Binary file",
                "It can't be shown as text. Use “Open” above.".into(),
            ),
            Content::Directory => message("folder-symbolic", "This is a folder", String::new()),
            Content::Missing => message(
                "dialog-warning-symbolic",
                "File not found",
                "It may have been moved or deleted.".into(),
            ),
        }
        self.apply_zoom();
    }

    fn scroll_to(&self, line: Option<u32>) {
        // The target line gets a soft accent band (removed when there is none).
        let tags = self.buffer.tag_table();
        let tag = tags.lookup("rade-target").unwrap_or_else(|| {
            let t = gtk::TextTag::new(Some("rade-target"));
            t.set_paragraph_background_rgba(Some(&gdk::RGBA::new(0.21, 0.52, 0.89, 0.16)));
            tags.add(&t);
            t
        });
        let (start, end) = self.buffer.bounds();
        self.buffer.remove_tag(&tag, &start, &end);
        if let Some(it) = line.and_then(|l| self.buffer.iter_at_line(l.max(1) as i32 - 1)) {
            let mut next = it;
            next.forward_line();
            self.buffer.apply_tag(&tag, &it, &next);
        }
        let line = line.unwrap_or(1).max(1) as i32 - 1;
        let Some(mut it) = self.buffer.iter_at_line(line) else {
            return;
        };
        // A code jump puts the caret on the name.
        if let Some((l, c)) = self.pending_col.take() {
            if l as i32 - 1 == line {
                if let Some(x) = self.buffer.iter_at_line_offset(line, c.max(1) as i32 - 1) {
                    it = x;
                }
            }
        }
        self.buffer.place_cursor(&it);
        let mark = self.buffer.create_mark(None, &it, true);
        let source = self.source.clone();
        // After layout, so the line's position is known.
        glib::idle_add_local_once(move || {
            source.scroll_to_mark(&mark, 0.0, true, 0.0, 0.25);
            source.buffer().delete_mark(&mark);
        });
    }

    /// Re-apply the light/dark appearance (adw StyleManager changed).
    pub fn restyle(self: &Rc<Self>) {
        self.buffer.set_style_scheme(scheme_for(is_dark()).as_ref());
        self.quick.restyle();
        if self.stack.visible_child_name().as_deref() == Some("markdown") {
            self.load(true);
        }
    }

    pub fn focus(&self) {
        if self
            .root
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN)
        {
            return;
        }
        match self.stack.visible_child_name().as_deref() {
            Some("text") => self.source.grab_focus(),
            Some("markdown") => self
                .markdown
                .borrow()
                .as_ref()
                .is_some_and(|m| m.widget().grab_focus()),
            _ => self.root.grab_focus(),
        };
    }

    pub fn open_external(&self) {
        let Some((path, line)) = self.shown.borrow().clone() else {
            return;
        };
        if let Some(ed) = gui_editor() {
            let argv = rade_core::viewer::editor_argv(&ed, &path, line, &self.worktree.borrow());
            let argv: Vec<&std::ffi::OsStr> = argv.iter().map(|a| a.as_ref()).collect();
            // GSubprocess reaps the child; `code` itself returns at once.
            if let Err(e) = gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
                eprintln!("rade: {}: {e}", argv[0].to_string_lossy());
            }
            return;
        }
        match editor() {
            // A terminal editor needs a Terminal: split one below the Viewer.
            Some(ed) => {
                let q = |s: &str| {
                    String::from_utf8_lossy(&shell_quote::Bash::quote_vec(s)).into_owned()
                };
                let cmd = match line {
                    Some(l) => format!("{ed} +{l} {}", q(&path.display().to_string())),
                    None => format!("{ed} {}", q(&path.display().to_string())),
                };
                if let Some(app) = try_app() {
                    let id = self.id.clone();
                    let _ =
                        app.update(|m| m.split(&id, rade_core::SplitDir::Down, Some(cmd), None));
                }
            }
            None => launch_uri(&gio::File::for_path(&path).uri()),
        }
    }

    /// Debug summary (`rade debug viewer`); Markdown asks the page.
    pub fn info(&self, done: Box<dyn FnOnce(Value)>) {
        let (path, line) = self.shown.borrow().clone().unzip();
        let view = self.stack.visible_child_name().map(|s| s.to_string());
        let s = |g: Option<glib::GString>| g.map(|g| g.to_string());
        let open = (s(self.open.icon_name()), s(self.open.tooltip_text()));
        let base = json!({"pane": self.id, "path": path, "line": line.flatten(), "view": view,
                          "back": self.back.is_visible() && self.back.is_sensitive(), "forward": self.forward.is_visible() && self.forward.is_sensitive(),
                          "open_action": {"icon": open.0, "tooltip": open.1},
                          "zoomed": self.zoomed(), "source_toggle": self.source_toggle.is_visible(),
                          "content_zoom": self.zoom_box.is_visible().then(|| self.zoom_label.label().map(|l| l.to_string())).flatten(),
                          "image_size": self.picture.size_request()});
        match view.as_deref() {
            Some("markdown") => match self.markdown.borrow().as_ref() {
                Some(md) => md.info(base, done),
                None => done(base),
            },
            Some("doc") => match self.doc.borrow().as_ref() {
                Some(d) => d.info(base, done),
                None => done(base),
            },
            Some("text") => {
                let rect = self.source.visible_rect();
                let (top, _) = self.source.line_at_y(rect.y());
                let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
                let mut v = base;
                v["top_line"] = json!(top.line() + 1);
                v["cursor_line"] = json!(cursor.line() + 1);
                v["cursor_col"] = json!(cursor.line_offset() + 1);
                let (a, b) = self.buffer.selection_bounds().unzip();
                v["selection"] = json!(a.zip(b).map(|(a, b)| format!(
                    "{}:{}-{}:{}",
                    a.line() + 1,
                    a.line_offset() + 1,
                    b.line() + 1,
                    b.line_offset() + 1
                )));
                let l = self.link_info();
                v["link"] = l["link"].clone();
                v["pointer_cursor"] = l["cursor"].clone();
                v["lines"] = json!(self.buffer.line_count());
                v["quick_diff"] = self.quick.marks();
                v["language"] = json!(self.buffer.language().map(|l| l.id().to_string()));
                v["scheme"] = json!(self.buffer.style_scheme().map(|s| s.id().to_string()));
                done(v)
            }
            _ => {
                let mut v = base;
                v["message"] = json!(self.message.title().to_string());
                done(v)
            }
        }
    }

    /// The header's Zoom button follows the Tab's Zoom (R113.1).
    pub fn set_zoomed(&self, zoomed: bool) {
        self.zoom.set_icon_name(if zoomed {
            "view-restore-symbolic"
        } else {
            "view-fullscreen-symbolic"
        });
        self.zoom.set_tooltip_text(Some(if zoomed {
            "Restore the Layout (Ctrl+Shift+Z, Esc)"
        } else {
            "Zoom (Ctrl+Shift+Z)"
        }));
        if zoomed {
            self.zoom.add_css_class("zoomed");
        } else {
            self.zoom.remove_css_class("zoomed");
        }
    }

    fn zoomed(&self) -> bool {
        self.zoom.has_css_class("zoomed")
    }

    /// The header's Rendered / Source toggle (`debug.viewer source`).
    pub fn toggle_source(&self) {
        if self.source_toggle.is_visible() {
            self.source_toggle
                .set_active(!self.source_toggle.is_active());
        }
    }

    pub fn markdown(&self) -> Option<MarkdownView> {
        self.markdown.borrow().clone()
    }
}

/// Zoom Viewer `id` (it takes the Tab), or restore the Layout (R113.1).
pub fn toggle_zoom(id: &str) {
    let id = id.to_string();
    glib::idle_add_local_once(move || {
        let Some(app) = try_app() else { return };
        let _ = app.update(|m| {
            m.focus_pane(&id)?;
            let tab = m
                .locate(&id)
                .ok_or_else(|| rade_core::rpc::Error::not_found("no such Viewer"))?
                .tab;
            m.toggle_zoom(&tab)
        });
    });
}

pub fn close_viewer(id: &str) {
    crate::code_pop::close();
    let id = id.to_string();
    // Not inside the key handler: closing destroys this widget.
    glib::idle_add_local_once(move || {
        if let Some(app) = try_app() {
            let _ = app.close_pane(&id, false);
        }
    });
}

pub fn go_back(id: &str) {
    if let Some(app) = try_app() {
        let _ = app.update(|m| m.viewer_back(id));
    }
}

pub fn go_forward(id: &str) {
    if let Some(app) = try_app() {
        let _ = app.update(|m| m.viewer_forward(id));
    }
}
