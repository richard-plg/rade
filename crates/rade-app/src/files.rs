//! Right sidebar file tree (R8): the active Worktree's files in a
//! GtkListView over a lazy GtkTreeListModel. Each listed folder is watched
//! (gio::FileMonitor) and re-listed on change; git status is refreshed
//! (debounced, off the UI thread) on any change, including `.git`. Rows are
//! `gtk::StringObject`s holding the absolute path; entry facts and git
//! state live in maps here. Clicking a file opens it in a Viewer (R8.4).
//! Row icons are PKief's Material Icon Theme (VS Code), bundled as a
//! GResource (`build.rs`); which icon is `rade_core::filetree::Icons`.

use crate::app::try_app;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::filetree::{git_status, list_dir, Entry, GitState, Icons};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::LazyLock;
use std::time::Duration;

static ICONS: LazyLock<Icons> = LazyLock::new(|| {
    Icons::from_json(include_str!("../assets/file-icons/map.json")).expect("map.json")
});
const ICON_SIZE: i32 = 16;

thread_local! {
    static PAINTABLES: RefCell<HashMap<(String, i32, bool), gtk::IconPaintable>> = RefCell::default();
}

/// The SVG `stem` of the theme shown, rendered at `ICON_SIZE` × the
/// widget's scale (crisp on HiDPI).
pub fn set_icon(image: &gtk::Image, stem: &str) {
    register_icons();
    let scale = image.scale_factor();
    let dark = adw::StyleManager::default().is_dark();
    let p = PAINTABLES.with_borrow_mut(|c| {
        c.entry((stem.to_string(), scale, dark))
            .or_insert_with(|| {
                let theme = if dark { "dark" } else { "light" };
                let uri = format!("resource:///rade/file-icons/{theme}/{stem}.svg");
                gtk::IconPaintable::for_file(&gio::File::for_uri(&uri), ICON_SIZE, scale)
            })
            .clone()
    });
    image.set_paintable(Some(&p));
    image.set_widget_name(stem); // `rade debug tree rows` reports it
}

/// The icon stem of a file or folder name (Catppuccin's map, R8).
pub fn icon_for(name: &str, dir: bool, expanded: bool) -> String {
    if dir {
        ICONS.folder(name, expanded, false).to_string()
    } else {
        ICONS.file(name, false).to_string()
    }
}

fn register_icons() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        gio::resources_register_include!("file-icons-dark.gresource").expect("file icons");
        gio::resources_register_include!("file-icons-light.gresource").expect("file icons");
    });
}

/// A row on screen.
struct Bound {
    icon: gtk::Image,
    name: gtk::Label,
    mark: gtk::Label,
    row: gtk::TreeListRow,
    /// Folders swap the open/closed icon.
    expanded: Option<glib::SignalHandlerId>,
}

impl Bound {
    fn drop_handler(self) {
        if let Some(h) = self.expanded {
            self.row.disconnect(h);
        }
    }
}

fn icon_stem(path: &Path, dir: bool, expanded: bool) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let light = !adw::StyleManager::default().is_dark();
    if dir {
        ICONS.folder(&name, expanded, light)
    } else {
        ICONS.file(&name, light)
    }
    .to_string()
}

const RELIST_DELAY: Duration = Duration::from_millis(80);
const GIT_DELAY: Duration = Duration::from_millis(250);

#[derive(Default)]
struct State {
    root: Option<PathBuf>,
    entries: HashMap<PathBuf, Entry>,
    stores: HashMap<PathBuf, gio::ListStore>,
    monitors: HashMap<PathBuf, gio::FileMonitor>,
    git: HashMap<PathBuf, GitState>,
    /// Rows on screen.
    bound: HashMap<PathBuf, Bound>,
    dirty: HashSet<PathBuf>,
}

pub struct Inner {
    pub root: gtk::Box,
    title: gtk::Label,
    refresh: gtk::Button,
    collapse: gtk::Button,
    list: gtk::ListView,
    scroll: gtk::ScrolledWindow,
    /// An empty folder's empty state (R103.6).
    empty: gtk::Box,
    show_ignored: gtk::ToggleButton,
    tree: RefCell<Option<gtk::TreeListModel>>,
    state: RefCell<State>,
    relist_queued: Cell<bool>,
    git_queued: Cell<bool>,
    /// Bumped on every rebuild; stale git results are dropped.
    generation: Cell<u64>,
}

#[derive(Clone)]
pub struct FileTree(Rc<Inner>);

fn path_of(item: &glib::Object) -> PathBuf {
    item.downcast_ref::<gtk::StringObject>()
        .map(|s| PathBuf::from(s.string().as_str()))
        .unwrap_or_default()
}

fn letter(s: GitState) -> &'static str {
    match s {
        GitState::Untracked => "U",
        GitState::Added => "A",
        GitState::Modified => "M",
        GitState::Deleted => "D",
        GitState::Conflicted => "!",
    }
}

const GIT_CLASSES: [&str; 5] = [
    "rade-git-untracked",
    "rade-git-added",
    "rade-git-modified",
    "rade-git-deleted",
    "rade-git-conflicted",
];

impl FileTree {
    pub fn new() -> Self {
        register_icons();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("rade-files");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.add_css_class("rade-files-header");
        let title = gtk::Label::new(Some("Files"));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("heading");
        let show_ignored = gtk::ToggleButton::new();
        show_ignored.set_icon_name("view-reveal-symbolic");
        show_ignored.set_tooltip_text(Some("Show files ignored by .gitignore"));
        show_ignored.add_css_class("flat");
        // R129.1: right to left — Show ignored, Refresh, Collapse All.
        let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh"));
        refresh.add_css_class("flat");
        let collapse = gtk::Button::from_icon_name("ph-arrows-in-line-vertical-symbolic");
        collapse.set_tooltip_text(Some("Collapse All"));
        collapse.add_css_class("flat");
        header.append(&title);
        header.append(&collapse);
        header.append(&refresh);
        header.append(&show_ignored);

        let list = gtk::ListView::new(None::<gtk::NoSelection>, None::<gtk::ListItemFactory>);
        list.set_single_click_activate(true);
        list.add_css_class("navigation-sidebar");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_child(Some(&list));
        let empty = crate::chrome::empty_state(&rade_core::chrome::EMPTY_FILES);
        empty.set_visible(false);
        root.append(&header);
        root.append(&empty);
        root.append(&scroll);

        let tree = Self(Rc::new(Inner {
            root,
            title,
            refresh,
            collapse,
            list,
            scroll,
            empty,
            show_ignored,
            tree: RefCell::default(),
            state: RefCell::default(),
            relist_queued: Cell::new(false),
            git_queued: Cell::new(false),
            generation: Cell::new(0),
        }));
        tree.0.list.set_factory(Some(&tree.factory()));
        let weak = Rc::downgrade(&tree.0);
        adw::StyleManager::default().connect_dark_notify(move |_| {
            let Some(inner) = weak.upgrade() else { return };
            let st = inner.state.borrow();
            for (p, b) in &st.bound {
                let dir = st.entries.get(p).is_some_and(|e| e.dir);
                set_icon(&b.icon, &icon_stem(p, dir, b.row.is_expanded()));
            }
        });
        let weak = Rc::downgrade(&tree.0);
        tree.0.list.connect_activate(move |_, pos| {
            if let Some(inner) = weak.upgrade() {
                FileTree(inner).activate_at(pos);
            }
        });
        let weak = Rc::downgrade(&tree.0);
        tree.0.show_ignored.connect_toggled(move |_| {
            if let Some(inner) = weak.upgrade() {
                FileTree(inner).rebuild();
            }
        });
        let weak = Rc::downgrade(&tree.0);
        tree.0.refresh.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                FileTree(inner).refresh();
            }
        });
        let weak = Rc::downgrade(&tree.0);
        tree.0.collapse.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                FileTree(inner).collapse_all();
            }
        });
        // R129.2: the title copies the full root path.
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(&tree.0);
        click.connect_released(move |_, _, _, _| {
            if let Some(inner) = weak.upgrade() {
                FileTree(inner).copy_root();
            }
        });
        tree.0.title.add_controller(click);
        tree.0.title.set_cursor_from_name(Some("pointer"));
        tree
    }

    /// R129.2: copy the root's full path and say so.
    fn copy_root(&self) {
        let Some(root) = self.0.state.borrow().root.clone() else {
            return;
        };
        let text = root.display().to_string();
        self.0.root.display().clipboard().set_text(&text);
        toast(&format!("Copied {}", glib::markup_escape_text(&text)));
    }

    /// R129.1: relist every listed folder and the git status; expanded
    /// folders and the scroll position stay (no rebuild).
    fn refresh(&self) {
        if self.0.tree.borrow().is_none() {
            return self.rebuild();
        }
        {
            let mut st = self.0.state.borrow_mut();
            let all: Vec<PathBuf> = st.stores.keys().cloned().collect();
            st.dirty.extend(all);
        }
        self.relist();
        self.queue_git();
    }

    /// R129.1: fold every expanded folder.
    fn collapse_all(&self) {
        let Some(tree) = self.0.tree.borrow().clone() else {
            return;
        };
        let rows: Vec<gtk::TreeListRow> = (0..tree.n_items())
            .filter_map(|i| tree.item(i).and_downcast::<gtk::TreeListRow>())
            .filter(|r| r.depth() == 0 && r.is_expanded())
            .collect();
        for r in rows {
            r.set_expanded(false);
        }
    }

    fn show_empty(&self, empty: bool) {
        self.0.empty.set_visible(empty);
        self.0.scroll.set_visible(!empty);
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    fn factory(&self) -> gtk::SignalListItemFactory {
        let f = gtk::SignalListItemFactory::new();
        f.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let icon = gtk::Image::new();
            icon.set_pixel_size(ICON_SIZE);
            let name = gtk::Label::new(None);
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            let mark = gtk::Label::new(None);
            mark.add_css_class("rade-git-mark");
            row.append(&icon);
            row.append(&name);
            row.append(&mark);
            let expander = gtk::TreeExpander::new();
            expander.set_child(Some(&row));
            expander.set_indent_for_icon(true);
            item.set_child(Some(&expander));
            // Middle-click: Open to the Side; right-click: a menu (R113.2).
            let click = gtk::GestureClick::new();
            click.set_button(0);
            let li = item.downgrade();
            let at = expander.clone();
            click.connect_pressed(move |g, _, x, y| {
                let Some(li) = li.upgrade() else { return };
                let Some(path) = li
                    .item()
                    .and_downcast::<gtk::TreeListRow>()
                    .and_then(|r| r.item())
                    .map(|o| path_of(&o))
                else {
                    return;
                };
                if path.is_dir() {
                    return;
                }
                match g.current_button() {
                    2 => {
                        g.set_state(gtk::EventSequenceState::Claimed);
                        open_side(&path);
                    }
                    3 => {
                        g.set_state(gtk::EventSequenceState::Claimed);
                        file_menu(&at, &path, x, y);
                    }
                    _ => {}
                }
            });
            expander.add_controller(click);
        });
        let weak = Rc::downgrade(&self.0);
        f.connect_bind(move |_, item| {
            let Some(inner) = weak.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let expander = item.child().and_downcast::<gtk::TreeExpander>().unwrap();
            expander.set_list_row(Some(&row));
            let b = expander.child().and_downcast::<gtk::Box>().unwrap();
            let icon = b.first_child().and_downcast::<gtk::Image>().unwrap();
            let name = icon.next_sibling().and_downcast::<gtk::Label>().unwrap();
            let mark = name.next_sibling().and_downcast::<gtk::Label>().unwrap();
            let path = row.item().map(|o| path_of(&o)).unwrap_or_default();
            let mut st = inner.state.borrow_mut();
            let (dir, ignored) = st
                .entries
                .get(&path)
                .map_or((false, false), |e| (e.dir, e.ignored));
            set_icon(&icon, &icon_stem(&path, dir, row.is_expanded()));
            let expanded = dir.then(|| {
                let (icon, p) = (icon.clone(), path.clone());
                row.connect_expanded_notify(move |r| {
                    set_icon(&icon, &icon_stem(&p, true, r.is_expanded()))
                })
            });
            name.set_label(&path.file_name().unwrap_or_default().to_string_lossy());
            name.set_tooltip_text(Some(&path.display().to_string()));
            b.set_opacity(if ignored { 0.5 } else { 1.0 });
            style(&name, &mark, st.git.get(&path).copied());
            let old = st.bound.insert(
                path,
                Bound {
                    icon,
                    name,
                    mark,
                    row,
                    expanded,
                },
            );
            if let Some(b) = old {
                b.drop_handler();
            }
        });
        let weak = Rc::downgrade(&self.0);
        f.connect_unbind(move |_, item| {
            let Some(inner) = weak.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            if let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() {
                if let Some(p) = row.item().map(|o| path_of(&o)) {
                    let mut st = inner.state.borrow_mut();
                    if st.bound.get(&p).is_some_and(|b| b.row == row) {
                        st.bound.remove(&p).unwrap().drop_handler();
                    }
                }
            }
        });
        f
    }

    /// Follow the active Worktree (R8.2); builds only while visible.
    pub fn set_root(&self, root: &Path, visible: bool) {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let name = root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.0.title.set_label(&name);
        self.0
            .title
            .set_tooltip_text(Some(&root.display().to_string()));
        let same = self.0.state.borrow().root.as_deref() == Some(root.as_path());
        if !same {
            self.0.state.borrow_mut().root = Some(root);
            *self.0.tree.borrow_mut() = None;
        }
        if visible && self.0.tree.borrow().is_none() {
            self.rebuild();
        }
    }

    fn rebuild(&self) {
        let inner = &self.0;
        inner.generation.set(inner.generation.get() + 1);
        let root = {
            let mut st = inner.state.borrow_mut();
            st.entries.clear();
            st.stores.clear();
            st.monitors.clear();
            st.git.clear();
            st.root.clone()
        };
        let Some(root) = root else { return };
        let store = self.store_for(&root);
        // An empty folder shows its empty state instead of the list.
        let weak = Rc::downgrade(inner);
        store.connect_items_changed(move |st, _, _, _| {
            if let Some(inner) = weak.upgrade() {
                FileTree(inner).show_empty(st.n_items() == 0);
            }
        });
        self.show_empty(store.n_items() == 0);
        let weak: Weak<Inner> = Rc::downgrade(inner);
        let tree = gtk::TreeListModel::new(store, false, false, move |item| {
            let inner = weak.upgrade()?;
            let path = path_of(item);
            let dir = inner
                .state
                .borrow()
                .entries
                .get(&path)
                .is_some_and(|e| e.dir);
            dir.then(|| FileTree(inner).store_for(&path).upcast())
        });
        inner
            .list
            .set_model(Some(&gtk::NoSelection::new(Some(tree.clone()))));
        *inner.tree.borrow_mut() = Some(tree);
        let git_dir = root.join(".git");
        if git_dir.is_dir() {
            self.watch(&git_dir, false);
        }
        self.queue_git();
    }

    /// Children of `dir` (listed and watched on first use).
    fn store_for(&self, dir: &Path) -> gio::ListStore {
        if let Some(s) = self.0.state.borrow().stores.get(dir) {
            return s.clone();
        }
        let store = gio::ListStore::new::<gtk::StringObject>();
        let entries = list_dir(dir, self.0.show_ignored.is_active());
        for e in &entries {
            store.append(&gtk::StringObject::new(&e.path.to_string_lossy()));
        }
        {
            let mut st = self.0.state.borrow_mut();
            st.entries
                .extend(entries.into_iter().map(|e| (e.path.clone(), e)));
            st.stores.insert(dir.to_path_buf(), store.clone());
        }
        self.watch(dir, true);
        store
    }

    /// ponytail: one inotify watch per listed folder, dropped only on
    /// rebuild; prune on collapse if huge trees ever hit the watch limit.
    fn watch(&self, dir: &Path, relist: bool) {
        // Test only (R129-e): as if inotify watches were exhausted.
        if no_watch() {
            return;
        }
        let Ok(m) = gio::File::for_path(dir)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        let d = dir.to_path_buf();
        m.connect_changed(move |_, _, _, _| {
            let Some(inner) = weak.upgrade() else { return };
            let t = FileTree(inner);
            if relist {
                t.0.state.borrow_mut().dirty.insert(d.clone());
                t.queue_relist();
            }
            t.queue_git();
        });
        self.0
            .state
            .borrow_mut()
            .monitors
            .insert(dir.to_path_buf(), m);
    }

    fn queue_relist(&self) {
        if self.0.relist_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        glib::timeout_add_local_once(RELIST_DELAY, move || {
            if let Some(inner) = weak.upgrade() {
                inner.relist_queued.set(false);
                FileTree(inner).relist();
            }
        });
    }

    /// Re-list changed folders, keeping unchanged rows (and their expansion).
    fn relist(&self) {
        let dirty: Vec<PathBuf> = self.0.state.borrow_mut().dirty.drain().collect();
        let show = self.0.show_ignored.is_active();
        for dir in dirty {
            let Some(store) = self.0.state.borrow().stores.get(&dir).cloned() else {
                continue;
            };
            let fresh = list_dir(&dir, show);
            let keep: HashSet<&PathBuf> = fresh.iter().map(|e| &e.path).collect();
            for i in (0..store.n_items()).rev() {
                let p = path_of(&store.item(i).unwrap());
                if !keep.contains(&p) {
                    store.remove(i);
                    let mut st = self.0.state.borrow_mut();
                    st.entries.remove(&p);
                    st.stores.retain(|k, _| !k.starts_with(&p));
                    st.monitors.retain(|k, _| !k.starts_with(&p));
                }
            }
            for (i, e) in fresh.iter().enumerate() {
                let at = store.item(i as u32).map(|o| path_of(&o));
                if at.as_ref() != Some(&e.path) {
                    store.insert(i as u32, &gtk::StringObject::new(&e.path.to_string_lossy()));
                }
            }
            let mut st = self.0.state.borrow_mut();
            st.entries
                .extend(fresh.into_iter().map(|e| (e.path.clone(), e)));
        }
    }

    fn queue_git(&self) {
        if self.0.git_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        glib::timeout_add_local_once(GIT_DELAY, move || {
            let Some(inner) = weak.upgrade() else { return };
            inner.git_queued.set(false);
            let Some(root) = inner.state.borrow().root.clone() else {
                return;
            };
            let generation = inner.generation.get();
            let weak = Rc::downgrade(&inner);
            glib::spawn_future_local(async move {
                let status = gio::spawn_blocking(move || git_status(&root))
                    .await
                    .ok()
                    .flatten();
                let Some(inner) = weak.upgrade() else { return };
                if inner.generation.get() != generation {
                    return;
                }
                let mut st = inner.state.borrow_mut();
                st.git = status.unwrap_or_default();
                for (p, b) in &st.bound {
                    style(&b.name, &b.mark, st.git.get(p).copied());
                }
            });
        });
    }

    fn row_at(&self, pos: u32) -> Option<gtk::TreeListRow> {
        self.0.tree.borrow().as_ref()?.item(pos).and_downcast()
    }

    fn activate_at(&self, pos: u32) {
        let Some(row) = self.row_at(pos) else { return };
        let path = row.item().map(|o| path_of(&o)).unwrap_or_default();
        if self
            .0
            .state
            .borrow()
            .entries
            .get(&path)
            .is_some_and(|e| e.dir)
        {
            row.set_expanded(!row.is_expanded());
        } else {
            open_in_viewer(&path);
        }
    }

    fn find(&self, path: &Path) -> Option<u32> {
        let tree = self.0.tree.borrow().clone()?;
        (0..tree.n_items()).find(|i| {
            self.row_at(*i)
                .and_then(|r| r.item())
                .is_some_and(|o| path_of(&o) == path)
        })
    }

    /// `debug.ui files [arg]` (R129): the header — title, its tooltip, the
    /// buttons right to left as shown — after `arg` = refresh | collapse |
    /// title | ignored pressed it.
    pub fn debug_header(&self, arg: Option<&str>) -> Value {
        match arg {
            Some("refresh") => self.0.refresh.emit_clicked(),
            Some("collapse") => self.0.collapse.emit_clicked(),
            Some("title") => self.copy_root(),
            Some("ignored") => self
                .0
                .show_ignored
                .set_active(!self.0.show_ignored.is_active()),
            _ => {}
        }
        let b = |name: &str, w: &gtk::Widget, icon: Option<glib::GString>| {
            json!({"name": name, "tooltip": w.tooltip_text().map(|t| t.to_string()),
                   "icon": icon.map(|i| i.to_string()), "visible": w.is_visible()})
        };
        let (r, c, i) = (&self.0.refresh, &self.0.collapse, &self.0.show_ignored);
        json!({"title": self.0.title.label().to_string(),
               "title_tooltip": self.0.title.tooltip_text().map(|t| t.to_string()),
               "buttons": [b("ignored", i.upcast_ref(), i.icon_name()),
                           b("refresh", r.upcast_ref(), r.icon_name()),
                           b("collapse", c.upcast_ref(), c.icon_name())]})
    }

    /// `rade debug tree <op> [path]`: rows | expand | collapse | activate |
    /// activate_side | toggle_ignored.
    pub fn debug(&self, op: &str, path: Option<&Path>) -> Result<Value, String> {
        let pos = || path.and_then(|p| self.find(p)).ok_or("no such row");
        match op {
            "rows" => {}
            "expand" | "collapse" => self.row_at(pos()?).unwrap().set_expanded(op == "expand"),
            "activate" => self.activate_at(pos()?),
            // A middle-click on a file row (R113.2).
            "activate_side" => {
                pos()?;
                open_side(path.unwrap());
            }
            "toggle_ignored" => self
                .0
                .show_ignored
                .set_active(!self.0.show_ignored.is_active()),
            _ => return Err(format!("unknown op '{op}'")),
        }
        let st = self.0.state.borrow();
        let rows: Vec<Value> = (0..self.0.tree.borrow().as_ref().map_or(0, |t| t.n_items()))
            .filter_map(|i| self.row_at(i))
            .map(|r| {
                let p = r.item().map(|o| path_of(&o)).unwrap_or_default();
                let e = st.entries.get(&p);
                let icon = st.bound.get(&p).map(|b| b.icon.widget_name().to_string());
                json!({"path": p, "depth": r.depth(), "dir": e.is_some_and(|e| e.dir), "icon": icon,
                       "expanded": r.is_expanded(), "ignored": e.is_some_and(|e| e.ignored),
                       "git": st.git.get(&p).map(|g| g.as_str())})
            })
            .collect();
        Ok(json!({"root": st.root, "visible": self.0.root.is_mapped(),
                  "show_ignored": self.0.show_ignored.is_active(), "rows": rows}))
    }
}

/// `RADE_FILES_NO_WATCH=1` turns the tree's watches off, in test mode only.
fn no_watch() -> bool {
    crate::handlers::DEBUG_SOCKET
        .get()
        .copied()
        .unwrap_or(false)
        && std::env::var_os("RADE_FILES_NO_WATCH").is_some_and(|v| v == "1")
}

fn toast(markup: &str) {
    if let Some(app) = try_app() {
        app.chrome.toast(markup, None);
    }
}

fn style(name: &gtk::Label, mark: &gtk::Label, git: Option<GitState>) {
    for c in GIT_CLASSES {
        name.remove_css_class(c);
        mark.remove_css_class(c);
    }
    mark.set_label(git.map_or("", letter));
    if let Some(g) = git {
        let c = GIT_CLASSES[g as usize];
        name.add_css_class(c);
        mark.add_css_class(c);
    }
}

/// A file row's right-click menu: Open, Open to the Side (R113.2).
fn file_menu(at: &impl IsA<gtk::Widget>, path: &Path, x: f64, y: f64) {
    let menu = gio::Menu::new();
    let target = path.display().to_string().to_variant();
    for (label, action) in [
        ("Open", "files.open"),
        ("Open to the Side", "files.open-side"),
    ] {
        let i = gio::MenuItem::new(Some(label), None);
        i.set_action_and_target_value(Some(action), Some(&target));
        menu.append_item(&i);
    }
    let group = gio::SimpleActionGroup::new();
    for (name, side) in [("open", false), ("open-side", true)] {
        let a = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        a.connect_activate(move |_, v| {
            let p: String = v.and_then(|v| v.get()).unwrap_or_default();
            glib::idle_add_local_once(move || open_at(Path::new(&p), side));
        });
        group.add_action(&a);
    }
    at.insert_action_group("files", Some(&group));
    let pop = gtk::PopoverMenu::from_model(Some(&menu));
    pop.set_parent(at);
    pop.set_has_arrow(false);
    pop.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
}

/// Open to the Side from the tree (middle-click, its menu).
pub(crate) fn open_side(path: &Path) {
    open_at(path, true);
}

/// A file clicked in the tree opens in the active Worktree's Viewer (R8.4).
pub(crate) fn open_in_viewer(path: &Path) {
    open_at(path, false);
}

fn open_at(path: &Path, side: bool) {
    let Some(app) = try_app() else { return };
    let (p, w) = {
        let m = app.model.borrow();
        let (p, w) = m.active();
        (p.id.clone(), w.id.clone())
    };
    let path = path.to_path_buf();
    if let Err(e) = app.update(|m| m.open_viewer_at(&p, &w, &path, None, true, side)) {
        eprintln!("rade: open {}: {e}", path.display());
    }
}
