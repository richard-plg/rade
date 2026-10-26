//! The Update's widgets (PRD R127, ADR-0048): the Statusline cell, the
//! About block at the top of Settings › Updates and the Restart dialog.
//! Both widgets are persistent: built once and updated in place on every
//! phase or progress change, so a 250 ms download tick never rebuilds the
//! Settings page or the Statusline. The texts come from
//! `rade_core::update`; the controller is `update.rs`.

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::prelude::*;
use libadwaita as adw;
use rade_core::update::{AboutView, CellView};
use std::cell::RefCell;

/// The classes the cell's state sets (D16).
const STATE_CLASSES: [&str; 3] = ["wait", "acc", "err"];

/// The Statusline's Update cell (D16): a flat button, hidden while there
/// is nothing to say.
pub struct UpdateCell {
    pub button: gtk::Button,
    icon: gtk::Image,
    label: gtk::Label,
    pub last: RefCell<Option<CellView>>,
}

impl UpdateCell {
    pub fn new() -> Self {
        let icon = gtk::Image::new();
        icon.add_css_class("status-icon");
        let label = gtk::Label::new(None);
        label.add_css_class("status-text");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        inner.append(&icon);
        inner.append(&label);
        let button = gtk::Button::new();
        button.set_child(Some(&inner));
        button.add_css_class("flat");
        button.add_css_class("status-cell");
        button.add_css_class("status-cell-update");
        button.set_focus_on_click(false);
        button.set_visible(false);
        button.connect_clicked(|_| crate::update::cell_clicked());
        Self {
            button,
            icon,
            label,
            last: RefCell::new(None),
        }
    }

    /// Show `v` (or hide the cell), in place; then fit the Statusline again
    /// so a cell that appears or changes its text is judged by its width.
    pub fn set(&self, v: Option<CellView>) {
        if *self.last.borrow() == v {
            return;
        }
        for c in STATE_CLASSES {
            self.button.remove_css_class(c);
        }
        match &v {
            Some(c) => {
                self.icon.set_icon_name(Some(c.icon));
                self.label.set_label(&c.text);
                if let Some(class) = c.class {
                    self.button.add_css_class(class);
                }
                self.button.set_tooltip_text(Some(&c.tooltip));
                self.button.set_visible(true);
            }
            None => self.button.set_visible(false),
        }
        *self.last.borrow_mut() = v;
        if let Some(app) = crate::app::try_app() {
            app.chrome.update_changed();
        }
    }

    /// The text shown, or `None` when the cell is absent or the fit dropped
    /// it (`debug.ui chrome` → `statusline.update`).
    pub fn shown_text(&self) -> Option<String> {
        let last = self.last.borrow();
        let c = last.as_ref()?;
        self.button.is_visible().then(|| c.text.clone())
    }
}

// ------------------------------------------------------------- About block

struct About {
    root: gtk::Box,
    title: gtk::Label,
    status: gtk::Label,
    sub: gtk::Label,
    progress: gtk::ProgressBar,
    actions: gtk::Box,
    notes: gtk::Button,
    /// The labels of the buttons built now.
    labels: RefCell<Vec<String>>,
    last: RefCell<Option<AboutView>>,
}

thread_local! {
    static ABOUT: RefCell<Option<std::rc::Rc<About>>> = const { RefCell::new(None) };
}

fn about() -> std::rc::Rc<About> {
    if let Some(a) = ABOUT.with(|a| a.borrow().clone()) {
        return a;
    }
    let label = |classes: &[&str]| {
        let l = gtk::Label::new(None);
        l.set_xalign(0.0);
        for c in classes {
            l.add_css_class(c);
        }
        l
    };
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.add_css_class("settings-about");
    root.set_margin_top(4);
    root.set_margin_bottom(8);
    root.set_margin_start(10);
    root.set_margin_end(8);

    let title = label(&["settings-about-title"]);
    title.set_hexpand(true);
    let status = label(&["settings-about-status"]);
    status.set_halign(gtk::Align::End);
    status.set_xalign(1.0);
    let line1 = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line1.append(&title);
    line1.append(&status);
    root.append(&line1);

    let sub = label(&["settings-about-sub"]);
    sub.set_wrap(true);
    root.append(&sub);

    let progress = gtk::ProgressBar::new();
    progress.add_css_class("settings-about-progress");
    progress.set_visible(false);
    root.append(&progress);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_hexpand(true);
    let notes = gtk::Button::with_label("Release notes ›");
    notes.add_css_class("flat");
    notes.set_focus_on_click(false);
    notes.set_visible(false);
    notes.connect_clicked(|_| crate::update::about_click("Release notes ›"));
    let line4 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line4.append(&actions);
    line4.append(&notes);
    root.append(&line4);

    let privacy = label(&["settings-about-privacy"]);
    privacy.set_text(rade_core::update::PRIVACY);
    privacy.set_wrap(true);
    root.append(&privacy);

    let a = std::rc::Rc::new(About {
        root,
        title,
        status,
        sub,
        progress,
        actions,
        notes,
        labels: RefCell::new(Vec::new()),
        last: RefCell::new(None),
    });
    ABOUT.with(|x| *x.borrow_mut() = Some(a.clone()));
    a
}

/// The one persistent About widget (§5.1), taken out of its previous parent
/// so Settings can append it again after a re-render.
pub fn about_block() -> gtk::Widget {
    let a = about();
    if let Some(parent) = a.root.parent().and_downcast::<gtk::Box>() {
        parent.remove(&a.root);
    }
    a.root.clone().upcast()
}

/// Update the About block in place (buttons are rebuilt only when their
/// labels change).
pub fn render_about(v: &AboutView) {
    let a = about();
    if a.last.borrow().as_ref() == Some(v) {
        return;
    }
    a.title.set_label(&v.title);
    a.status.set_label(&v.status);
    for c in ["ok", "wait", "err", "acc"] {
        a.status.remove_css_class(c);
    }
    if let Some(c) = v.status_class {
        a.status.add_css_class(c);
    }
    a.sub.set_label(&v.sub);
    match v.progress {
        Some(f) => {
            a.progress.set_fraction(f);
            a.progress.set_visible(true);
        }
        None => a.progress.set_visible(false),
    }
    if *a.labels.borrow() != v.buttons {
        while let Some(c) = a.actions.first_child() {
            a.actions.remove(&c);
        }
        for (i, text) in v.buttons.iter().enumerate() {
            let b = gtk::Button::with_label(text);
            if i == 0 {
                b.add_css_class("suggested-action");
            } else {
                b.add_css_class("flat");
            }
            b.set_focus_on_click(false);
            let text = text.clone();
            b.connect_clicked(move |_| crate::update::about_click(&text));
            a.actions.append(&b);
        }
        *a.labels.borrow_mut() = v.buttons.clone();
    }
    if let Some(b) = a.actions.first_child() {
        b.set_sensitive(v.primary_enabled);
    }
    a.notes.set_visible(v.notes);
    *a.last.borrow_mut() = Some(v.clone());
}

// ---------------------------------------------------------- Restart dialog

/// D20: the dialog; `answer` gets the response id (`not_now` | `restart`).
pub fn restart_dialog(
    body: &str,
    agents_run: bool,
    answer: impl Fn(&str) + 'static,
) -> adw::AlertDialog {
    let d = adw::AlertDialog::new(Some("Restart RADE to finish updating?"), Some(body));
    d.add_response("not_now", "Not now");
    d.add_response("restart", "Restart");
    d.set_response_appearance(
        "restart",
        if agents_run {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    d.set_default_response(Some("not_now"));
    d.set_close_response("not_now");
    d.connect_response(None, move |_, id| answer(id));
    d
}
