//! The Right Panel's Search page (PRD R22): a query with Match Case /
//! Whole Word / Regular Expression toggles and include / exclude globs,
//! results grouped by file in a GtkListView; a line opens the Viewer there.
//! The search (`rade_core::search`) runs off the GTK thread 200 ms after
//! typing stops; a newer query or Worktree cancels the one running. In a
//! Project (R107.4) a scope toggle picks `link` (the focused link's
//! checkout or Worktree) or `Project` (every link's, results grouped by
//! link under a header row).

use crate::app::try_app;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::rpc::{Error, Result};
use rade_core::search::{self, Options, Results, Row, Scope};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(200);

/// A list row: a link's header (Project scope) or a result row.
#[derive(Clone, Copy, Debug, PartialEq)]
enum VRow {
    Group(usize),
    Row(Row),
}

#[derive(Default)]
struct State {
    root: Option<PathBuf>,
    results: Results,
    rows: Vec<VRow>,
    /// The focused Project's groups ((source, folder)) and the focused
    /// link's Source, when the focused checkout is in a Project.
    groups: Vec<(String, PathBuf)>,
    focused: String,
    project: bool,
    /// The labelled folders the last search covered: (label, folder,
    /// first file index, file count).
    searched: Vec<(String, PathBuf, usize, usize)>,
    folded: HashSet<String>,
    /// The last search: its options and how long it took (`debug.find`).
    last: Option<(Options, u128)>,
}

pub struct SearchView {
    pub root: gtk::Box,
    entry: gtk::SearchEntry,
    case: gtk::ToggleButton,
    word: gtk::ToggleButton,
    regex: gtk::ToggleButton,
    include: gtk::Entry,
    exclude: gtk::Entry,
    summary: gtk::Label,
    spinner: adw::Spinner,
    store: gtk::StringList,
    list: gtk::ListView,
    empty: gtk::Box,
    scroll: gtk::ScrolledWindow,
    /// `link` / `Project` (R107.4), shown in a Project only.
    scope_box: gtk::Box,
    scope_link: gtk::ToggleButton,
    scope_project: gtk::ToggleButton,
    state: RefCell<State>,
    /// The search allowed to land; bumping it cancels the running one.
    generation: Arc<AtomicU64>,
    pending: RefCell<Option<glib::SourceId>>,
    running: Cell<bool>,
}

thread_local! {
    static SEARCH: RefCell<Option<Rc<SearchView>>> = const { RefCell::new(None) };
}

pub fn get() -> Option<Rc<SearchView>> {
    SEARCH.with(|c| c.borrow().clone())
}

fn toggle(label: &str, tip: &str) -> gtk::ToggleButton {
    let t = gtk::ToggleButton::with_label(label);
    t.add_css_class("flat");
    t.add_css_class("rade-search-opt");
    t.set_tooltip_text(Some(tip));
    t
}

/// The preview with its matches highlighted (Pango markup).
fn markup(text: &str, ranges: &[(usize, usize)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    let esc = |s: &[char]| glib::markup_escape_text(&s.iter().collect::<String>()).to_string();
    for &(s, e) in ranges {
        if s < at || e > chars.len() {
            continue;
        }
        out += &esc(&chars[at..s]);
        out += &format!(
            "<span bgcolor=\"{}\" bgalpha=\"38%\" weight=\"bold\">{}</span>",
            crate::tokens::rgb_now("wait"),
            esc(&chars[s..e])
        );
        at = e;
    }
    out += &esc(&chars[at..]);
    out
}

pub fn install() -> gtk::Widget {
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Search"));
    entry.set_hexpand(true);
    let case = toggle("Aa", "Match Case");
    let word = toggle("ab", "Match Whole Word");
    let regex = toggle(".*", "Use Regular Expression");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    row.append(&entry);
    row.append(&case);
    row.append(&word);
    row.append(&regex);
    let include = gtk::Entry::new();
    include.set_placeholder_text(Some("files to include (e.g. *.rs, src/**)"));
    let exclude = gtk::Entry::new();
    exclude.set_placeholder_text(Some("files to exclude"));
    for e in [&include, &exclude] {
        e.add_css_class("rade-search-glob");
    }
    let summary = gtk::Label::new(None);
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.set_wrap(true);
    summary.add_css_class("rade-meta");
    let spinner = adw::Spinner::new();
    spinner.set_visible(false);
    let status = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    status.append(&summary);
    status.append(&spinner);
    let scope_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    scope_box.add_css_class("search-scope");
    scope_box.add_css_class("linked");
    let scope_link = gtk::ToggleButton::with_label("link");
    scope_link.add_css_class("search-scope-link");
    scope_link.set_tooltip_text(Some("The focused link's checkout or Worktree"));
    let scope_project = gtk::ToggleButton::with_label("Project");
    scope_project.add_css_class("search-scope-project");
    scope_project.set_tooltip_text(Some("Every link of the Project, grouped by link"));
    scope_project.set_group(Some(&scope_link));
    scope_link.set_active(true);
    scope_box.append(&scope_link);
    scope_box.append(&scope_project);
    scope_box.set_visible(false);
    let top = gtk::Box::new(gtk::Orientation::Vertical, 6);
    top.add_css_class("rade-search-top");
    top.append(&scope_box);
    top.append(&row);
    top.append(&include);
    top.append(&exclude);
    top.append(&status);

    let store = gtk::StringList::new(&[]);
    let list = gtk::ListView::new(
        Some(gtk::NoSelection::new(Some(store.clone()))),
        None::<gtk::ListItemFactory>,
    );
    list.set_single_click_activate(true);
    list.add_css_class("navigation-sidebar");
    list.add_css_class("rade-search-list");
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(&list));
    // No query: one line and key hints (R103.6).
    let empty = crate::chrome::empty_state(&rade_core::chrome::EMPTY_SEARCH);
    scroll.set_visible(false);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("rade-search");
    root.append(&top);
    root.append(&empty);
    root.append(&scroll);

    let v = Rc::new(SearchView {
        root,
        entry,
        case,
        word,
        regex,
        include,
        exclude,
        summary,
        spinner,
        store,
        list,
        empty,
        scroll,
        scope_box,
        scope_link,
        scope_project,
        state: RefCell::default(),
        generation: Arc::default(),
        pending: RefCell::default(),
        running: Cell::new(false),
    });
    v.list.set_factory(Some(&v.factory()));
    v.connect();
    SEARCH.with(|x| *x.borrow_mut() = Some(v.clone()));
    v.root.clone().upcast()
}

impl SearchView {
    fn connect(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        let changed = move || {
            if let Some(v) = w.upgrade() {
                v.queue();
            }
        };
        let c = changed.clone();
        self.entry.connect_search_changed(move |_| c());
        for t in [&self.case, &self.word, &self.regex] {
            let c = changed.clone();
            t.connect_toggled(move |_| c());
        }
        for e in [&self.include, &self.exclude] {
            let c = changed.clone();
            e.connect_changed(move |_| c());
        }
        let c = changed.clone();
        self.scope_project.connect_toggled(move |_| c());
        let w = Rc::downgrade(self);
        self.entry.connect_stop_search(move |_| {
            if let Some(app) = try_app() {
                app.restore_focus();
            }
            let _ = w;
        });
        let w = Rc::downgrade(self);
        self.list.connect_activate(move |_, pos| {
            if let Some(v) = w.upgrade() {
                v.activate(pos as usize);
            }
        });
    }

    fn factory(self: &Rc<Self>) -> gtk::SignalListItemFactory {
        let f = gtk::SignalListItemFactory::new();
        f.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let icon = gtk::Image::new();
            icon.add_css_class("rade-dim");
            let num = gtk::Label::new(None);
            num.add_css_class("rade-meta");
            num.set_xalign(1.0);
            num.set_width_chars(4);
            let text = gtk::Label::new(None);
            text.set_xalign(0.0);
            text.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let dir = gtk::Label::new(None);
            dir.set_xalign(0.0);
            dir.set_hexpand(true);
            dir.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            dir.add_css_class("rade-meta");
            let count = gtk::Label::new(None);
            count.add_css_class("rade-search-count-pill");
            for w in [
                icon.upcast_ref::<gtk::Widget>(),
                num.upcast_ref(),
                text.upcast_ref(),
                dir.upcast_ref(),
                count.upcast_ref(),
            ] {
                b.append(w);
            }
            item.set_child(Some(&b));
        });
        let w = Rc::downgrade(self);
        f.connect_bind(move |_, item| {
            let Some(v) = w.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let b = item.child().and_downcast::<gtk::Box>().unwrap();
            let icon = b.first_child().and_downcast::<gtk::Image>().unwrap();
            let num = icon.next_sibling().and_downcast::<gtk::Label>().unwrap();
            let text = num.next_sibling().and_downcast::<gtk::Label>().unwrap();
            let dir = text.next_sibling().and_downcast::<gtk::Label>().unwrap();
            let count = dir.next_sibling().and_downcast::<gtk::Label>().unwrap();
            let st = v.state.borrow();
            let Some(vrow) = st.rows.get(item.position() as usize) else {
                return;
            };
            let row = match *vrow {
                VRow::Group(gi) => {
                    let (label, _, _, n) = &st.searched[gi];
                    icon.set_icon_name(Some("ph-git-fork-symbolic"));
                    icon.set_visible(true);
                    num.set_visible(false);
                    text.set_label(label);
                    text.add_css_class("heading");
                    text.set_tooltip_text(None);
                    text.set_hexpand(true);
                    dir.set_visible(false);
                    count.set_label(&n.to_string());
                    count.set_visible(true);
                    b.remove_css_class("rade-search-hit");
                    b.remove_css_class("rade-search-file");
                    b.add_css_class("search-group");
                    return;
                }
                VRow::Row(r) => r,
            };
            b.remove_css_class("search-group");
            match row {
                Row::File(fi) => {
                    let f = &st.results.files[fi];
                    let folded = st.folded.contains(&f.path);
                    icon.set_icon_name(Some(if folded {
                        "pan-end-symbolic"
                    } else {
                        "pan-down-symbolic"
                    }));
                    icon.set_visible(true);
                    num.set_visible(false);
                    let (d, n) = f.path.rsplit_once('/').unwrap_or(("", &f.path));
                    text.set_label(n);
                    text.add_css_class("heading");
                    text.set_tooltip_text(Some(&f.path));
                    text.set_hexpand(false);
                    dir.set_label(d);
                    dir.set_visible(true);
                    count.set_label(&f.matches.to_string());
                    count.set_visible(true);
                    b.remove_css_class("rade-search-hit");
                    b.add_css_class("rade-search-file");
                }
                Row::Hit(fi, hi) => {
                    let h = &st.results.files[fi].hits[hi];
                    icon.set_visible(false);
                    num.set_visible(true);
                    num.set_label(&h.line.to_string());
                    text.set_markup(&markup(&h.preview, &h.ranges));
                    text.remove_css_class("heading");
                    text.set_tooltip_text(None);
                    text.set_hexpand(true);
                    dir.set_visible(false);
                    count.set_visible(false);
                    b.remove_css_class("rade-search-file");
                    b.add_css_class("rade-search-hit");
                }
            }
        });
        f
    }

    fn options(&self) -> Options {
        Options {
            query: self.entry.text().to_string(),
            case: self.case.is_active(),
            word: self.word.is_active(),
            regex: self.regex.is_active(),
            include: self.include.text().to_string(),
            exclude: self.exclude.text().to_string(),
        }
    }

    /// Follow the active Worktree (every sync); a changed one re-runs.
    pub fn follow(self: &Rc<Self>, root: &Path) {
        if self.state.borrow().root.as_deref() == Some(root) {
            return;
        }
        self.state.borrow_mut().root = Some(root.to_path_buf());
        if !self.entry.text().is_empty() {
            self.queue();
        }
    }

    /// Every sync: the focused Project's links (R107.4); the scope toggle
    /// shows only in a Project.
    pub fn follow_scope(self: &Rc<Self>, ws: &rade_core::State) {
        let now = crate::changes_project::focused(ws);
        let (groups, focused) = match &now {
            Some((_, _, gs)) => (
                gs.iter()
                    .map(|g| (g.source.clone(), g.path.clone()))
                    .collect::<Vec<_>>(),
                ws.active().0.id.clone(),
            ),
            None => (Vec::new(), String::new()),
        };
        let changed = {
            let st = self.state.borrow();
            st.groups != groups || st.focused != focused
        };
        self.scope_box.set_visible(now.is_some());
        if changed {
            {
                let mut st = self.state.borrow_mut();
                st.groups = groups;
                st.focused = focused;
                st.project = now.is_some();
            }
            if self.scope_project.is_active() && !self.entry.text().is_empty() {
                self.queue();
            }
        }
    }

    fn scope(&self) -> Scope {
        if self.scope_project.is_active() && self.state.borrow().project {
            Scope::Project
        } else {
            Scope::Link
        }
    }

    /// Search 200 ms after the last change (R22.4); cancels the running one.
    fn queue(self: &Rc<Self>) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let w = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            if let Some(v) = w.upgrade() {
                v.pending.borrow_mut().take();
                v.start();
            }
        });
        *self.pending.borrow_mut() = Some(id);
    }

    fn start(self: &Rc<Self>) {
        let Some(root) = self.state.borrow().root.clone() else {
            return;
        };
        // The labelled folders of the scope: the focused link's checkout
        // or Worktree with scope `link`, the focused checkout outside a
        // Project.
        let roots: Vec<(String, PathBuf)> = {
            let st = self.state.borrow();
            let scope = self.scope();
            let groups: Vec<rade_core::home::Group> = st
                .groups
                .iter()
                .map(|(s, p)| rade_core::home::Group {
                    project: String::new(),
                    source: s.clone(),
                    path: p.clone(),
                    worktree: None,
                    label: s.clone(),
                    base: String::new(),
                })
                .collect();
            let mut r = search::scope_roots(scope, &groups, &st.focused, &root);
            let name = |id: &str| {
                try_app()
                    .and_then(|a| a.model.borrow().source(id).map(|s| s.name.clone()))
                    .unwrap_or_else(|| id.to_string())
            };
            r.iter_mut()
                .filter(|(l, _)| !l.is_empty())
                .for_each(|(l, _)| *l = name(l));
            r
        };
        let o = self.options();
        let my = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        if let Err(e) = search::matcher(&o) {
            self.show(Results::default(), Some(e), o, 0);
            return;
        }
        if o.query.is_empty() {
            self.show(Results::default(), None, o, 0);
            return;
        }
        self.running.set(true);
        self.spinner.set_visible(true);
        let gen = self.generation.clone();
        let w = Rc::downgrade(self);
        let t0 = Instant::now();
        let opts = o.clone();
        crate::worktrees::in_background(
            move || {
                let groups = search::search_roots(&roots, &opts, search::MAX_MATCHES, &|| {
                    gen.load(Ordering::Relaxed) != my
                })?;
                // One Results, files in group order; the groups remembered.
                let mut all = Results::default();
                let mut searched = Vec::new();
                for (label, r) in groups {
                    let folder = roots
                        .iter()
                        .find(|(l, _)| *l == label)
                        .map(|(_, p)| p.clone())
                        .unwrap_or_default();
                    searched.push((label, folder, all.files.len(), r.files.len()));
                    all.matches += r.matches;
                    all.capped |= r.capped;
                    all.cancelled |= r.cancelled;
                    all.files.extend(r.files);
                }
                Ok::<_, String>((all, searched))
            },
            move |r| {
                let Some(v) = w.upgrade() else { return };
                if v.generation.load(Ordering::Relaxed) != my {
                    return; // a newer search owns the page
                }
                v.running.set(false);
                v.spinner.set_visible(false);
                let ms = t0.elapsed().as_millis();
                match r {
                    Ok((res, searched)) => {
                        v.state.borrow_mut().searched = searched;
                        v.show(res, None, o, ms)
                    }
                    Err(e) => {
                        v.state.borrow_mut().searched.clear();
                        v.show(Results::default(), Some(e), o, ms)
                    }
                }
            },
        );
    }

    fn show(&self, r: Results, err: Option<String>, o: Options, ms: u128) {
        self.running.set(false);
        self.spinner.set_visible(false);
        self.empty.set_visible(o.query.is_empty() && err.is_none());
        self.scroll.set_visible(!self.empty.is_visible());
        match &err {
            Some(e) => {
                self.summary.set_label(e);
                self.summary.add_css_class("rade-scm-error");
            }
            None => {
                self.summary.remove_css_class("rade-scm-error");
                self.summary.set_label(&if o.query.is_empty() {
                    String::new()
                } else if r.files.is_empty() {
                    "No results".into()
                } else {
                    r.summary()
                });
            }
        }
        {
            let mut st = self.state.borrow_mut();
            st.results = r;
            st.last = Some((o, ms));
        }
        self.rebuild();
    }

    fn rebuild(&self) {
        let grouped = self.scope() == Scope::Project;
        let rows: Vec<VRow> = {
            let st = self.state.borrow();
            let plain = search::rows(&st.results, &st.folded);
            let mut out = Vec::new();
            for r in plain {
                if let (true, Row::File(fi)) = (grouped, r) {
                    if let Some(gi) = st.searched.iter().position(|g| g.2 == fi && g.3 > 0) {
                        out.push(VRow::Group(gi));
                    }
                }
                out.push(VRow::Row(r));
            }
            out
        };
        let n = rows.len();
        self.state.borrow_mut().rows = rows;
        let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        self.store.splice(0, self.store.n_items(), &refs);
    }

    /// The folder a file index was found in.
    fn folder_of(&self, fi: usize) -> PathBuf {
        let st = self.state.borrow();
        st.searched
            .iter()
            .find(|g| fi >= g.2 && fi < g.2 + g.3)
            .map(|g| g.1.clone())
            .or_else(|| st.root.clone())
            .unwrap_or_default()
    }

    fn activate(self: &Rc<Self>, pos: usize) {
        let row = match self.state.borrow().rows.get(pos).copied() {
            Some(VRow::Row(r)) => Some(r),
            _ => None,
        };
        match row {
            Some(Row::File(fi)) => {
                let p = self.state.borrow().results.files[fi].path.clone();
                {
                    let mut st = self.state.borrow_mut();
                    if !st.folded.remove(&p) {
                        st.folded.insert(p);
                    }
                }
                self.rebuild();
            }
            Some(Row::Hit(fi, hi)) => {
                let folder = self.folder_of(fi);
                let (path, line) = {
                    let st = self.state.borrow();
                    let f = &st.results.files[fi];
                    (folder.join(&f.path), f.hits[hi].line)
                };
                open_at(&path, line);
            }
            None => {}
        }
    }

    /// Show the page with the keyboard in its entry (R22.5).
    pub fn show_page(self: &Rc<Self>) {
        let Some(app) = try_app() else { return };
        if !app.model.borrow().file_tree {
            app.update(|m| m.file_tree = true);
        }
        app.chrome.right.show("search");
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    fn info(&self) -> Value {
        let st = self.state.borrow();
        let link = |fi: usize| {
            st.searched
                .iter()
                .find(|g| fi >= g.2 && fi < g.2 + g.3)
                .map(|g| g.0.clone())
        };
        let rows: Vec<Value> = st
            .rows
            .iter()
            .map(|r| match *r {
                VRow::Group(gi) => {
                    json!({"kind": "group", "link": st.searched[gi].0, "files": st.searched[gi].3})
                }
                VRow::Row(Row::File(fi)) => {
                    let f = &st.results.files[fi];
                    json!({"kind": "file", "path": f.path, "matches": f.matches, "folded": st.folded.contains(&f.path), "link": link(fi)})
                }
                VRow::Row(Row::Hit(fi, hi)) => {
                    let h = &st.results.files[fi].hits[hi];
                    json!({"kind": "hit", "path": st.results.files[fi].path, "line": h.line, "preview": h.preview, "ranges": h.ranges, "link": link(fi)})
                }
            })
            .collect();
        json!({
            "root": st.root,
            "options": self.options(),
            "summary": self.summary.label().as_str(),
            "error": self.summary.has_css_class("rade-scm-error"),
            "running": self.running.get() || self.pending.borrow().is_some(),
            "matches": st.results.matches,
            "files": st.results.files.len(),
            "capped": st.results.capped,
            "took_ms": st.last.as_ref().map(|l| l.1),
            "searched": st.last.as_ref().map(|l| &l.0.query),
            "rows": rows,
            "visible": self.root.is_mapped(),
            "scope": match self.scope() { Scope::Link => "link", Scope::Project => "project" },
            "scope_shown": self.scope_box.is_visible(),
            "entry_focused": self.entry.has_focus() || self.entry.focus_child().is_some(),
        })
    }
}

/// A result's line in the active Worktree's Viewer (R22.3, R7.2).
fn open_at(path: &Path, line: u32) {
    let Some(app) = try_app() else { return };
    let (p, w) = {
        let m = app.model.borrow();
        let (p, w) = m.active();
        (p.id.clone(), w.id.clone())
    };
    let path = path.to_path_buf();
    if let Err(e) = app.update(|m| m.open_viewer_in(&p, &w, &path, Some(line), true)) {
        eprintln!("rade: open {}: {e}", path.display());
    }
}

/// `debug.find` (E2E): `{"op": "info" | "set" | "activate" | "show", …}`.
/// `set` takes any of `query`, `case`, `word`, `regex`, `include`,
/// `exclude` and searches as typing does; `activate` (`index`, or `path` +
/// `line`, or `path` for its file row) clicks a row.
pub fn debug(p: &Value) -> Result<Value> {
    let v = get().ok_or_else(|| Error::internal("no Search page"))?;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    match s("op").unwrap_or("info") {
        "info" => {}
        "show" => v.show_page(),
        "set" => {
            if let Some(q) = s("query") {
                v.entry.set_text(q);
            }
            for (k, t) in [("case", &v.case), ("word", &v.word), ("regex", &v.regex)] {
                if let Some(b) = p.get(k).and_then(Value::as_bool) {
                    t.set_active(b);
                }
            }
            for (k, e) in [("include", &v.include), ("exclude", &v.exclude)] {
                if let Some(t) = s(k) {
                    e.set_text(t);
                }
            }
            match s("scope") {
                Some("project") => v.scope_project.set_active(true),
                Some("link") => v.scope_link.set_active(true),
                _ => {}
            }
            v.queue();
        }
        "activate" => {
            let line = p.get("line").and_then(Value::as_u64);
            let index = p
                .get("index")
                .and_then(Value::as_u64)
                .map(|i| i as usize)
                .or_else(|| {
                    let st = v.state.borrow();
                    st.rows.iter().position(|r| match (*r, line) {
                        (VRow::Row(Row::File(fi)), None) => {
                            Some(st.results.files[fi].path.as_str()) == s("path")
                        }
                        (VRow::Row(Row::Hit(fi, hi)), Some(l)) => {
                            Some(st.results.files[fi].path.as_str()) == s("path")
                                && st.results.files[fi].hits[hi].line as u64 == l
                        }
                        _ => false,
                    })
                });
            let i = index.ok_or_else(|| Error::not_found("no such result row"))?;
            v.activate(i);
        }
        o => return Err(Error::usage(format!("unknown op '{o}'"))),
    }
    Ok(v.info())
}
