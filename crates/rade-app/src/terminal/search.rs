// Search bar wiring follows limux (MIT), https://github.com/am-will/limux @ ae67aae
// (terminal.rs: `search:` / `navigate_search:` / `end_search` binding actions).

//! RADE (R19.5): the search bar over a Terminal. Ghostty searches (its own
//! `Ctrl+Shift+F` = `start_search`, its scrollback, its match highlights);
//! this bar is the entry it asks the app runtime for: `START_SEARCH` shows
//! it, typing sends `search:<text>`, Enter / Shift+Enter send
//! `navigate_search:next|previous`, Esc sends `end_search`; Ghostty reports
//! `SEARCH_TOTAL` / `SEARCH_SELECTED` for the `3/17` count.

use super::*;

#[derive(Clone)]
pub struct SearchBar {
    pub root: gtk::Box,
    pub entry: gtk::SearchEntry,
    count: gtk::Label,
    total: Rc<Cell<isize>>,
    selected: Rc<Cell<isize>>,
    surface_cell: Rc<RefCell<Option<ghostty_surface_t>>>,
    gl_area: gtk::GLArea,
}

fn icon(name: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(name);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("flat");
    b
}

impl SearchBar {
    pub(super) fn new(
        surface_cell: Rc<RefCell<Option<ghostty_surface_t>>>,
        gl_area: gtk::GLArea,
    ) -> Self {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find in terminal")
            .width_chars(22)
            .build();
        let count = gtk::Label::new(None);
        count.add_css_class("rade-search-count");
        count.add_css_class("dim-label");
        let prev = icon("go-up-symbolic", "Previous match (Shift+Enter)");
        let next = icon("go-down-symbolic", "Next match (Enter)");
        let close = icon("window-close-symbolic", "Close (Esc)");
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        root.add_css_class("card");
        root.add_css_class("rade-search");
        root.set_halign(gtk::Align::End);
        root.set_valign(gtk::Align::Start);
        root.set_margin_top(8);
        root.set_margin_end(20); // clear of the scrollbar
        root.append(&entry);
        root.append(&count);
        root.append(&prev);
        root.append(&next);
        root.append(&close);
        root.set_visible(false);
        let bar = Self {
            root,
            entry: entry.clone(),
            count,
            total: Rc::new(Cell::new(-1)),
            selected: Rc::new(Cell::new(-1)),
            surface_cell,
            gl_area,
        };
        {
            let b = bar.clone();
            entry.connect_search_changed(move |e| {
                // An empty needle stops Ghostty's search (its count goes away).
                b.action(&format!("search:{}", e.text()));
            });
        }
        {
            let b = bar.clone();
            entry.connect_activate(move |_| b.step(true));
        }
        // Shift+Enter: previous (the entry's own activate is Enter only).
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let b = bar.clone();
            keys.connect_key_pressed(move |_, key, _, state| {
                let enter = matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter);
                let up = matches!(key, gtk::gdk::Key::Up | gtk::gdk::Key::KP_Up);
                let down = matches!(key, gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down);
                if up || down || (enter && state.contains(gtk::gdk::ModifierType::SHIFT_MASK)) {
                    b.step(down);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        }
        entry.add_controller(keys);
        {
            let b = bar.clone();
            entry.connect_stop_search(move |_| b.close());
        }
        {
            let b = bar.clone();
            prev.connect_clicked(move |_| b.step(false));
        }
        {
            let b = bar.clone();
            next.connect_clicked(move |_| b.step(true));
        }
        {
            let b = bar.clone();
            close.connect_clicked(move |_| b.close());
        }
        bar
    }

    fn action(&self, action: &str) {
        surface_action(*self.surface_cell.borrow(), action);
    }

    /// Ghostty asked for the bar (`START_SEARCH`): show it, focused, with
    /// `needle` (search_selection) or the last text, all selected.
    pub(super) fn open(&self, needle: &str) {
        self.root.set_visible(true);
        if !needle.is_empty() {
            self.entry.set_text(needle);
        } else if !self.entry.text().is_empty() {
            self.action(&format!("search:{}", self.entry.text()));
        }
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    /// Esc, the close button: end Ghostty's search, keys back to the Terminal.
    pub fn close(&self) {
        self.action("end_search");
        self.ended();
        self.gl_area.grab_focus();
    }

    /// Ghostty ended the search (`END_SEARCH`).
    pub(super) fn ended(&self) {
        // Ended by Ghostty while typing here: keys back to the Terminal.
        if self.entry.has_focus() || self.entry.focus_child().is_some() {
            self.gl_area.grab_focus();
        }
        self.root.set_visible(false);
        self.set_total(-1);
        self.set_selected(-1);
    }

    pub fn step(&self, forward: bool) {
        if self.entry.text().is_empty() {
            return;
        }
        self.action(if forward {
            "navigate_search:next"
        } else {
            "navigate_search:previous"
        });
    }

    pub(super) fn set_total(&self, total: isize) {
        self.total.set(total);
        self.update_count();
    }

    pub(super) fn set_selected(&self, selected: isize) {
        self.selected.set(selected);
        self.update_count();
    }

    fn update_count(&self) {
        self.count.set_label(&rade_core::basic::search_count(
            self.total.get(),
            self.selected.get(),
        ));
    }

    /// (open, text, count label) for `debug.search`.
    pub fn info(&self) -> (bool, String, String) {
        (
            self.root.is_visible(),
            self.entry.text().to_string(),
            self.count.label().to_string(),
        )
    }
}
