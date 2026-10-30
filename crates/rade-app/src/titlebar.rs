//! The 40 px titlebar (PRD R103.1, spec A.2–A.16): the Workspace button
//! (placed here by `main.rs`), the breadcrumb `Project / Source / Worktree
//! ← base ↑n`, the command field (a click opens the palette), `+`, split,
//! usage (`gauge`, R110: shown once a Workspace declares an Agent Account;
//! opens the Usage page), Review with the count of files Review would
//! open, the Inspector toggle and the window controls.
//!
//! The git facts behind it (ahead / behind, Review count, identity) are read
//! off the main loop for the focused checkout ([`Facts`]).

use crate::app::try_app;
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::chrome::{breadcrumb, Crumbs};
use rade_core::State;
use std::path::{Path, PathBuf};

/// The git facts of the focused checkout, read in the background.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    /// The checkout they are about.
    pub path: PathBuf,
    /// The base it is compared with (recorded, else the main branch).
    pub base: Option<String>,
    /// Commits ahead of / behind `base`.
    pub ahead_behind: Option<(u32, u32)>,
    /// Files Review would open (changes against `base`).
    pub review: Option<usize>,
    /// `git config user.email` under the Workspace env.
    pub identity: Option<String>,
}

/// Read [`Facts`] for the checkout at `path` (blocking: git).
pub fn read_facts(path: &Path, recorded: Option<String>, env: Vec<(String, String)>) -> Facts {
    let mut f = Facts {
        path: path.to_path_buf(),
        ..Default::default()
    };
    if !path.join(".git").exists() {
        return f;
    }
    let base = recorded
        .filter(|b| rade_core::git::run(path, &["rev-parse", "--verify", "--quiet", b]).is_ok())
        .or_else(|| rade_core::git::default_branch(path));
    if let Some(b) = &base {
        let range = format!("{b}...HEAD");
        if let Ok(out) = rade_core::git::run(path, &["rev-list", "--left-right", "--count", &range])
        {
            let mut it = out.split_whitespace().filter_map(|n| n.parse::<u32>().ok());
            if let (Some(behind), Some(ahead)) = (it.next(), it.next()) {
                f.ahead_behind = Some((ahead, behind));
            }
        }
    }
    f.review = rade_core::diff::changes(path, base.as_deref())
        .ok()
        .map(|c| c.files.len());
    f.base = base;
    f.identity = read_email(path, env);
    f
}

/// `git config user.email` in `path` under a Workspace env (R107.5).
pub fn read_email(path: &Path, env: Vec<(String, String)>) -> Option<String> {
    let mut c = rade_core::git::command(path);
    c.args(["config", "user.email"]);
    for (k, v) in env {
        c.env(k, v);
    }
    c.output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The Project the focused checkout is shown under: a linked Worktree's
/// recorded Project; in a main Worktree, the link row the Navigator
/// selects (by the focused Pane's cwd, R104.5), none in Scratch.
pub fn project_name(ws: &State) -> Option<String> {
    let (s, w) = ws.active();
    let projects = crate::sidebar::projects(ws);
    if !ws.is_main_worktree(&s.id, &w.id) {
        return ws
            .project_of_worktree(&s.id, w, &projects)
            .map(|p| p.name.clone());
    }
    let links = crate::navdata::links(ws);
    match rade_core::navigator::selection(ws, &links, None, &crate::projects::canon) {
        Some(rade_core::navigator::Target::Link { project, .. }) => projects
            .iter()
            .find(|p| p.id == project)
            .map(|p| p.name.clone()),
        _ => None,
    }
}

/// The breadcrumb of the focused checkout with what `facts` know.
pub fn crumbs(ws: &State, facts: &Facts) -> Crumbs {
    let (s, w) = ws.active();
    let linked = !ws.is_main_worktree(&s.id, &w.id);
    let fresh = facts.path == w.path;
    let base = w
        .base
        .clone()
        .or_else(|| fresh.then(|| facts.base.clone()).flatten());
    let ahead = fresh.then(|| facts.ahead_behind.map(|a| a.0)).flatten();
    breadcrumb(
        project_name(ws).as_deref(),
        &s.name,
        linked.then_some(w.name.as_str()),
        base.as_deref(),
        ahead,
    )
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn icon(name: &str, classes: &[&str]) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    for c in classes {
        i.add_css_class(c);
    }
    i
}

/// A 28 px icon button running `action`.
fn tb_button(icon_name: &str, tip: &str, action: &'static str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon_name);
    b.add_css_class("tb-button");
    b.add_css_class("flat");
    b.set_tooltip_text(Some(tip));
    b.set_focus_on_click(false);
    b.connect_clicked(move |_| crate::keys::run_action(action));
    b
}

pub struct Titlebar {
    pub root: gtk::WindowHandle,
    /// Where `main.rs` puts the Workspace button.
    pub ws_slot: gtk::Box,
    crumbs: gtk::Box,
    review_count: gtk::Label,
    /// Hidden on Project home (spec B.2) and the Usage page (E.1).
    pub review_button: gtk::Button,
    /// The `gauge` button (E.1: `on` while the Usage page shows).
    pub usage_button: gtk::Button,
    pub inspector: gtk::ToggleButton,
    /// What the breadcrumb shows now.
    shown: std::cell::RefCell<Crumbs>,
    bar: gtk::Box,
    command_field: gtk::Button,
    command_icon: gtk::Image,
    placeholder: gtk::Label,
    kbd: gtk::Label,
    /// (window width, text scale × 100, breadcrumb) of the last fit.
    fitted: std::cell::RefCell<(i32, i32, String)>,
}

impl Titlebar {
    pub fn new() -> Self {
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        bar.add_css_class("titlebar-box");
        let ws_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        ws_slot.set_valign(gtk::Align::Center);
        bar.append(&ws_slot);
        let crumbs = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        crumbs.add_css_class("crumbs");
        bar.append(&crumbs);
        let left = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        left.set_hexpand(true);
        bar.append(&left);

        // The command field: looks like an input, opens the palette.
        let field = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let command_icon = icon("ph-magnifying-glass-symbolic", &["command-icon"]);
        field.append(&command_icon);
        let ph = label("Search or run a command…", &["command-placeholder"]);
        ph.set_hexpand(true);
        ph.set_xalign(0.0);
        ph.set_ellipsize(gtk::pango::EllipsizeMode::End);
        field.append(&ph);
        let kbd = label("Ctrl ⇧ P", &["kbd"]);
        kbd.set_valign(gtk::Align::Center);
        field.append(&kbd);
        let command_field = gtk::Button::new();
        command_field.set_child(Some(&field));
        command_field.add_css_class("command-field");
        command_field.set_size_request(440, 26);
        // The placeholder expands inside; the field keeps its 440 px (A.11).
        command_field.set_hexpand(false);
        command_field.set_valign(gtk::Align::Center);
        command_field.set_focus_on_click(false);
        command_field.set_tooltip_text(Some("Command palette (Ctrl+Shift+P)"));
        command_field.connect_clicked(|_| crate::keys::run_action("command_palette"));
        bar.append(&command_field);
        let right = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        right.set_hexpand(true);
        bar.append(&right);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        buttons.add_css_class("tb-buttons");
        buttons.set_valign(gtk::Align::Center);
        // `+` (R108.6): the new menu (Shell, Agents by account, Fan-out, Browser).
        let new = gtk::Button::from_icon_name("ph-plus-square-symbolic");
        new.add_css_class("tb-button");
        new.add_css_class("flat");
        new.add_css_class("tb-button-new");
        new.set_tooltip_text(Some("New: Shell, Agent, Fan-out, Browser"));
        new.set_focus_on_click(false);
        new.connect_clicked(|b| crate::new_menu::toggle(b.upcast_ref()));
        buttons.append(&new);
        // Agent Overview (R117.1): every Agent of every Workspace.
        let overview = gtk::Button::from_icon_name("ph-squares-four-symbolic");
        overview.add_css_class("tb-button");
        overview.add_css_class("flat");
        overview.add_css_class("tb-button-overview");
        overview.set_tooltip_text(Some("Agent Overview (Ctrl+Shift+Space)"));
        overview.set_focus_on_click(false);
        overview.connect_clicked(|_| crate::overview_view::toggle());
        buttons.append(&overview);
        buttons.append(&tb_button(
            "ph-square-split-horizontal-symbolic",
            "Split right",
            "split_right",
        ));
        // Usage (R110.2): the Usage page.
        let usage = tb_button("ph-gauge-symbolic", "Agent usage", "open_usage");
        usage.add_css_class("usage-button");
        usage.add_css_class("tb-button-usage");
        usage.set_visible(false);
        buttons.append(&usage);
        // Schedules (R139.1): always there, so the page is one click away.
        let schedules = tb_button("ph-clock-symbolic", "Schedules", "open_schedules");
        schedules.add_css_class("tb-button-schedules");
        buttons.append(&schedules);
        let review_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        review_box.append(&icon("ph-git-diff-symbolic", &["review-icon"]));
        review_box.append(&label("Review", &["review-label"]));
        let review_count = label("", &["review-count"]);
        review_count.set_visible(false);
        review_box.append(&review_count);
        let review_button = gtk::Button::new();
        review_button.set_child(Some(&review_box));
        review_button.add_css_class("review-button");
        review_button.set_valign(gtk::Align::Center);
        review_button.set_focus_on_click(false);
        review_button.set_tooltip_text(Some("Review changes (Ctrl+Shift+R)"));
        review_button.connect_clicked(|_| crate::keys::run_action("open_review"));
        buttons.append(&review_button);
        let inspector = gtk::ToggleButton::new();
        inspector.set_icon_name("ph-sidebar-simple-symbolic");
        inspector.add_css_class("tb-button");
        inspector.add_css_class("flat");
        inspector.add_css_class("inspector-toggle");
        inspector.set_focus_on_click(false);
        inspector.set_tooltip_text(Some("Inspector: Changes, Files, Search (Ctrl+Shift+E)"));
        inspector.connect_toggled(|b| {
            let Some(app) = try_app() else { return };
            if !app.syncing.get() && app.model.borrow().file_tree != b.is_active() {
                crate::keys::run_action("toggle_file_tree");
            }
        });
        buttons.append(&inspector);
        bar.append(&buttons);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        controls.add_css_class("window-controls");
        controls.set_valign(gtk::Align::Center);
        for (name, tip, what) in [
            ("ph-minus-symbolic", "Minimize", 0),
            ("ph-corners-out-symbolic", "Maximize", 1),
            ("ph-x-symbolic", "Close", 2),
        ] {
            let b = gtk::Button::from_icon_name(name);
            b.add_css_class("tb-button");
            b.add_css_class("flat");
            b.set_tooltip_text(Some(tip));
            b.set_focus_on_click(false);
            b.connect_clicked(move |b| {
                let Some(w) = b.root().and_downcast::<gtk::Window>() else {
                    return;
                };
                match what {
                    0 => w.minimize(),
                    1 if w.is_maximized() => w.unmaximize(),
                    1 => w.maximize(),
                    _ => w.close(),
                }
            });
            controls.append(&b);
        }
        bar.append(&controls);

        let root = gtk::WindowHandle::new();
        root.set_child(Some(&bar));
        root.add_css_class("titlebar");
        Self {
            root,
            ws_slot,
            crumbs,
            review_count,
            review_button,
            usage_button: usage,
            inspector,
            shown: Default::default(),
            bar,
            command_field,
            command_icon,
            placeholder: ph,
            kbd,
            fitted: Default::default(),
        }
    }

    /// Forget the last fit (the next `fit` measures again).
    pub fn unfit(&self) {
        self.fitted.borrow_mut().0 = 0;
    }

    /// Fit the command field to a window `width` px wide (spec A "1280 px"):
    /// 440 px shrinking to 280 px before the breadcrumb ellipsizes; the
    /// placeholder shows only while it fits beside the icon and key caps.
    pub fn fit(&self, width: i32) {
        let scale = (crate::layout_mode::text_scale() * 100.0).round() as i32;
        let key = (width, scale, self.shown.borrow().text());
        if width <= 0 || *self.fitted.borrow() == key {
            return;
        }
        *self.fitted.borrow_mut() = key;
        let nat = |w: &gtk::Widget| w.measure(gtk::Orientation::Horizontal, -1).1;
        // The field's own chrome: icon, key caps, spacing, padding, border.
        let chrome = nat(self.command_icon.upcast_ref()) + nat(self.kbd.upcast_ref()) + 16 + 16;
        let (ph, _) = self
            .placeholder
            .create_pango_layout(Some(&self.placeholder.text()))
            .pixel_size();
        let field_now = nat(self.command_field.upcast_ref());
        let free = width - (nat(self.bar.upcast_ref()) - field_now);
        let w = rade_core::chrome::command_field_width(free);
        let fits = rade_core::chrome::command_placeholder_fits(w, chrome, ph);
        self.placeholder.set_visible(fits);
        // Without the placeholder the key caps keep their place at the right.
        self.kbd.set_hexpand(!fits);
        self.kbd.set_halign(gtk::Align::End);
        self.command_field.set_size_request(w, 26);
    }

    /// Show `c` (rebuilt only when it changed) and the Review count.
    pub fn show(&self, c: Crumbs, review: Option<usize>) {
        match review {
            Some(n) => {
                self.review_count.set_label(&n.to_string());
                self.review_count.set_visible(true);
            }
            None => self.review_count.set_visible(false),
        }
        if *self.shown.borrow() == c {
            return;
        }
        while let Some(w) = self.crumbs.first_child() {
            self.crumbs.remove(&w);
        }
        let mut plain: Vec<&str> = c.project.iter().map(String::as_str).collect();
        plain.push(&c.source);
        for (i, seg) in plain.iter().enumerate() {
            if i > 0 {
                self.crumbs.append(&label("/", &["crumb-sep"]));
            }
            let last = i + 1 == plain.len();
            // Project and the current segment stay whole (A "1280 px").
            let shrinks = rade_core::chrome::crumb_ellipsizes(
                i,
                plain.len() + usize::from(c.worktree.is_some()),
                c.project.is_some(),
            );
            let l = label(
                seg,
                if last {
                    &["crumb", "crumb-current"]
                } else {
                    &["crumb"]
                },
            );
            if shrinks {
                l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            }
            self.crumbs.append(&l);
        }
        if let Some(w) = &c.worktree {
            self.crumbs.append(&label("/", &["crumb-sep"]));
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            b.append(&icon("ph-git-branch-symbolic", &["crumb-branch-icon"]));
            b.append(&label(w, &["crumb-branch"]));
            self.crumbs.append(&b);
        }
        if let Some(t) = &c.tail {
            self.crumbs.append(&label(t, &["crumb-base"]));
        }
        self.crumbs.set_tooltip_text(Some(&c.text()));
        *self.shown.borrow_mut() = c;
    }

    /// `debug.ui chrome`: the breadcrumb and the Review count as shown.
    pub fn debug(&self) -> serde_json::Value {
        serde_json::json!({
            "breadcrumb": self.shown.borrow().text(),
            "review_count": self.review_count.is_visible()
                .then(|| self.review_count.label().parse::<usize>().ok()).flatten(),
            "inspector": self.inspector.is_active(),
        })
    }
}
