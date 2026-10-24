//! The results popover of a code navigation (PRD R85, R84.2 / R87.3 / R91
//! 1.0.0 amendments), shown by [`crate::code_pop`] on the Viewer or Review
//! Pane that asked: the last navigation's results grouped by file (a
//! click opens the Viewer at the line; Back returns), an outline,
//! where they come from (the server's name, or "text matches" with a
//! pill on each row), "indexing…" with an automatic retry, the hint when
//! a server is missing (its install command), Restart after crashes, and
//! a call hierarchy as a tree (0.7.1): callers (or callees, the switch)
//! with their call-site counts, each opening to its call sites and its
//! own callers (asked for when opened, never twice into a recursion),
//! then "Other references (not calls)".

use crate::app::try_app;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::code::nav::{self, Answer, Query};
use rade_core::code::Ctx;
use rade_core::rpc::{Error, Result};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
enum Row {
    File(usize),
    Item(usize),
    /// A call hierarchy node at its path from the top level.
    Call(Vec<usize>),
    /// Call site `j` of a node.
    Site(Vec<usize>, usize),
    /// "Loading callers…" / why they could not be loaded, under a node.
    Note(Vec<usize>),
    OthersHead,
    Other(usize),
}

/// The node at `path` of a call tree.
fn node<'a>(calls: &'a [nav::Call], path: &[usize]) -> Option<&'a nav::Call> {
    let (first, rest) = path.split_first()?;
    let c = calls.get(*first)?;
    if rest.is_empty() {
        Some(c)
    } else {
        node(&c.children, rest)
    }
}

fn node_mut<'a>(calls: &'a mut [nav::Call], path: &[usize]) -> Option<&'a mut nav::Call> {
    let (first, rest) = path.split_first()?;
    let c = calls.get_mut(*first)?;
    if rest.is_empty() {
        Some(c)
    } else {
        node_mut(&mut c.children, rest)
    }
}

/// `preview` as markup with the `k`-th whole-word `name` in bold (a line
/// with several calls tells them apart); a late one is brought into view.
fn bold_nth(preview: &str, name: &str, k: usize) -> String {
    let esc = |s: &str| glib::markup_escape_text(s).to_string();
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let Some(start) = preview
        .match_indices(name)
        .filter(|(i, _)| {
            !preview[..*i].chars().next_back().is_some_and(word)
                && !preview[i + name.len()..].chars().next().is_some_and(word)
        })
        .nth(k)
        .map(|(i, _)| i)
    else {
        return esc(preview);
    };
    let end = start + name.len();
    // Past 12 characters in: start a little before it.
    let before = &preview[..start];
    let lead = before.chars().count();
    let shown = if lead > 12 {
        let skip = before.char_indices().nth(lead - 8).map_or(0, |(i, _)| i);
        format!("…{}", esc(&before[skip..]))
    } else {
        esc(before)
    };
    format!("{shown}<b>{}</b>{}", esc(name), esc(&preview[end..]))
}

/// Open the top level by itself up to this many nodes.
const AUTO_OPEN: usize = 10;

/// What was asked: (query, Viewer pane, file, line, col).
type Asked = (Query, String, PathBuf, u32, u32);

#[derive(Default)]
struct State {
    answer: Option<Answer>,
    groups: Vec<(PathBuf, Vec<usize>)>,
    rows: Vec<Row>,
    folded: HashSet<PathBuf>,
    asked: Option<Asked>,
    /// Open call hierarchy nodes.
    open: HashSet<Vec<usize>>,
    /// Nodes whose next level is loading (or failed: the message).
    notes: std::collections::HashMap<Vec<usize>, String>,
    others_folded: bool,
    /// Bumped by each answer: a late expansion of an older one is dropped.
    generation: u64,
}

pub struct CodeView {
    pub root: gtk::Box,
    title: gtk::Label,
    modes: gtk::Box,
    callers: gtk::ToggleButton,
    callees: gtk::ToggleButton,
    summary: gtk::Label,
    hint: gtk::Label,
    retry: gtk::Button,
    spinner: adw::Spinner,
    store: gtk::StringList,
    list: gtk::ListView,
    state: RefCell<State>,
    running: Cell<bool>,
    /// An automatic retry is queued while the server indexes.
    retry_queued: Cell<bool>,
}

thread_local! {
    static CODE: RefCell<Option<Rc<CodeView>>> = const { RefCell::new(None) };
}

pub fn get() -> Option<Rc<CodeView>> {
    CODE.with(|c| c.borrow().clone())
}

pub fn install() {
    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("heading");
    let callers = gtk::ToggleButton::with_label("Callers");
    let callees = gtk::ToggleButton::with_label("Callees");
    callees.set_group(Some(&callers));
    callers.set_tooltip_text(Some("Who calls it (Alt+Shift+H)"));
    callees.set_tooltip_text(Some("What it calls"));
    for b in [&callers, &callees] {
        b.add_css_class("flat");
    }
    let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    modes.add_css_class("linked");
    modes.add_css_class("rade-code-modes");
    modes.append(&callers);
    modes.append(&callees);
    modes.set_visible(false);
    let close = gtk::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.set_tooltip_text(Some("Close (Esc)"));
    close.connect_clicked(|_| {
        crate::code_pop::close();
    });
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    head.append(&title);
    head.append(&modes);
    head.append(&close);
    let summary = gtk::Label::new(None);
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.set_wrap(true);
    summary.add_css_class("rade-meta");
    let spinner = adw::Spinner::new();
    spinner.set_visible(false);
    let retry = gtk::Button::with_label("Retry");
    retry.add_css_class("flat");
    retry.set_visible(false);
    let status = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    status.append(&summary);
    status.append(&spinner);
    status.append(&retry);
    let hint = gtk::Label::new(None);
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_selectable(true);
    hint.add_css_class("rade-code-hint");
    hint.set_visible(false);
    let top = gtk::Box::new(gtk::Orientation::Vertical, 6);
    top.add_css_class("rade-search-top");
    top.append(&head);
    top.append(&status);
    top.append(&hint);

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
    // As tall as its rows, up to a limit; longer lists scroll.
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(380);
    scroll.set_child(Some(&list));
    let results = gtk::Box::new(gtk::Orientation::Vertical, 0);
    results.append(&top);
    results.append(&scroll);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("rade-search");
    root.add_css_class("rade-code");
    // A popover's content: a fixed width, the list scrolls past its limit.
    root.set_size_request(460, -1);
    root.append(&results);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(|_, key, _, _| {
        if key == gtk::gdk::Key::Escape && crate::code_pop::close() {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    root.add_controller(keys);

    let v = Rc::new(CodeView {
        root,
        title,
        modes,
        callers,
        callees,
        summary,
        hint,
        retry,
        spinner,
        store,
        list,
        state: RefCell::default(),
        running: Cell::new(false),
        retry_queued: Cell::new(false),
    });
    v.list.set_factory(Some(&v.factory()));
    v.connect();
    CODE.with(|x| *x.borrow_mut() = Some(v.clone()));
}

impl CodeView {
    fn connect(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        self.list.connect_activate(move |_, pos| {
            if let Some(v) = w.upgrade() {
                v.activate(pos as usize);
            }
        });
        let w = Rc::downgrade(self);
        self.retry.connect_clicked(move |_| {
            if let Some(v) = w.upgrade() {
                v.retry_now();
            }
        });
        for (b, q) in [
            (&self.callers, Query::Callers),
            (&self.callees, Query::Callees),
        ] {
            let w = Rc::downgrade(self);
            b.connect_toggled(move |b| {
                let Some(v) = w.upgrade() else { return };
                if !b.is_active() || v.running.get() {
                    return;
                }
                let asked = v.state.borrow().asked.clone();
                if let Some((was, pane, path, line, col)) = asked {
                    if was != q {
                        crate::code::navigate(&pane, q, &path, line, col, Duration::ZERO);
                    }
                }
            });
        }
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
            let pill = gtk::Label::new(None);
            pill.add_css_class("rade-search-count-pill");
            // A call hierarchy node: go to the function itself.
            let goto = gtk::Button::from_icon_name("go-jump-symbolic");
            goto.add_css_class("flat");
            goto.add_css_class("rade-code-goto");
            goto.set_valign(gtk::Align::Center);
            for w in [
                icon.upcast_ref::<gtk::Widget>(),
                num.upcast_ref(),
                text.upcast_ref(),
                dir.upcast_ref(),
                pill.upcast_ref(),
                goto.upcast_ref(),
            ] {
                b.append(w);
            }
            let it = item.downgrade();
            goto.connect_clicked(move |_| {
                if let (Some(v), Some(it)) = (get(), it.upgrade()) {
                    v.goto(it.position() as usize);
                }
            });
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
            let pill = dir.next_sibling().and_downcast::<gtk::Label>().unwrap();
            let goto = pill.next_sibling().unwrap();
            let st = v.state.borrow();
            let (Some(a), Some(row)) = (&st.answer, st.rows.get(item.position() as usize)) else {
                return;
            };
            let esc = |s: &str| glib::markup_escape_text(s).to_string();
            // In a call hierarchy every row keeps the Go to slot, so pills line up.
            let tree = !matches!(row, Row::File(_) | Row::Item(_));
            goto.set_visible(tree);
            goto.set_opacity(0.0);
            goto.set_sensitive(false);
            pill.set_valign(gtk::Align::Center);
            b.set_margin_start(0);
            b.set_margin_top(0);
            text.set_margin_start(0);
            dir.set_visible(false);
            pill.set_visible(false);
            text.remove_css_class("heading");
            text.remove_css_class("rade-meta");
            text.set_hexpand(true);
            // A call site, a note or another reference sits one level in:
            // its line number where a child's chevron is (chevron 16 + gap 6,
            // less the hit rows' own 14 px padding).
            let inner = |depth: usize| 16 * depth as i32 + 8;
            let leaf = || {
                icon.set_visible(false);
                num.set_visible(true);
                b.remove_css_class("rade-search-file");
                b.add_css_class("rade-search-hit");
            };
            match row {
                Row::Call(path) => {
                    let Some(c) = node(&a.calls, path) else {
                        return;
                    };
                    b.set_margin_start(16 * (path.len() as i32 - 1));
                    icon.set_icon_name(Some(if c.recursive {
                        "media-playlist-repeat-symbolic"
                    } else if st.open.contains(path) {
                        "pan-down-symbolic"
                    } else {
                        "pan-end-symbolic"
                    }));
                    icon.set_visible(true);
                    num.set_visible(false);
                    let rel = nav::rel(&c.path, &a.root);
                    text.set_markup(&if c.recursive {
                        format!(
                            "<b>{}</b>  <span alpha=\"70%\">recursive</span>",
                            esc(&c.name)
                        )
                    } else {
                        format!("<b>{}</b>", esc(&c.name))
                    });
                    text.set_hexpand(false);
                    let n = c.from_ranges.len();
                    let calls = if n == 1 {
                        "1 call".to_string()
                    } else {
                        format!("{n} calls")
                    };
                    text.set_tooltip_text(Some(&if c.recursive {
                        format!("{} calls itself (not expanded again)", c.name)
                    } else {
                        format!("{} · {calls} · {rel}:{}", c.name, c.line)
                    }));
                    // Long paths shrink from the left; the line stays.
                    dir.set_label(&format!("{rel}:{}", c.line));
                    dir.set_visible(true);
                    pill.set_label(&n.to_string());
                    pill.set_tooltip_text(Some(&calls));
                    pill.remove_css_class("rade-code-textmatch");
                    pill.add_css_class("rade-search-count-pill");
                    pill.set_visible(true);
                    goto.set_opacity(1.0);
                    goto.set_sensitive(true);
                    goto.set_tooltip_text(Some(&format!("Go to {}", c.name)));
                    b.remove_css_class("rade-search-hit");
                    b.add_css_class("rade-search-file");
                    return;
                }
                Row::Site(path, j) => {
                    let Some(c) = node(&a.calls, path) else {
                        return;
                    };
                    let Some(s) = c.from_ranges.get(*j) else {
                        return;
                    };
                    leaf();
                    b.set_margin_start(inner(path.len() - 1));
                    num.set_label(&s.line.to_string());
                    // Several calls on one line: the k-th one is in bold.
                    let name = if a.query == Some(Query::Callees) {
                        &c.name
                    } else {
                        &a.word
                    };
                    let k = c.from_ranges[..*j]
                        .iter()
                        .filter(|o| o.path == s.path && o.line == s.line)
                        .count();
                    text.set_markup(&bold_nth(&s.preview, name, k));
                    text.set_tooltip_text(Some(&format!(
                        "{}:{}:{}",
                        nav::rel(&s.path, &a.root),
                        s.line,
                        s.col
                    )));
                    return;
                }
                Row::Note(path) => {
                    leaf();
                    b.set_margin_start(inner(path.len() - 1));
                    num.set_label("");
                    let what = if a.query == Some(Query::Callees) {
                        "callees"
                    } else {
                        "callers"
                    };
                    let note = st
                        .notes
                        .get(path)
                        .cloned()
                        .unwrap_or_else(|| format!("No {what}"));
                    text.set_markup(&format!("<span alpha=\"70%\"><i>{}</i></span>", esc(&note)));
                    text.set_tooltip_text(None);
                    return;
                }
                Row::OthersHead => {
                    icon.set_icon_name(Some(if st.others_folded {
                        "pan-end-symbolic"
                    } else {
                        "pan-down-symbolic"
                    }));
                    icon.set_visible(true);
                    num.set_visible(false);
                    // A section, not a caller.
                    text.set_label("Other references (not calls)");
                    text.add_css_class("rade-meta");
                    text.set_tooltip_text(Some(
                        "Uses of the name that are not calls: passed or stored as a value",
                    ));
                    pill.set_label(&a.others.len().to_string());
                    pill.set_tooltip_text(None);
                    pill.remove_css_class("rade-code-textmatch");
                    pill.add_css_class("rade-search-count-pill");
                    pill.set_visible(true);
                    b.set_margin_top(6);
                    b.remove_css_class("rade-search-hit");
                    b.add_css_class("rade-search-file");
                    return;
                }
                Row::Other(i) => {
                    let it = &a.others[*i];
                    leaf();
                    b.set_margin_start(inner(0));
                    num.set_label(&it.line.to_string());
                    let rel = nav::rel(&it.path, &a.root);
                    text.set_markup(&bold_nth(&it.preview, &a.word, 0));
                    text.set_hexpand(false);
                    text.set_tooltip_text(Some(&format!("{rel}:{}", it.line)));
                    dir.set_label(rel.rsplit('/').next().unwrap_or(&rel));
                    dir.set_visible(true);
                    return;
                }
                _ => {}
            }
            match *row {
                Row::File(g) => {
                    let (path, idx) = &st.groups[g];
                    let rel = nav::rel(path, &a.root);
                    let (d, n) = rel.rsplit_once('/').unwrap_or(("", &rel));
                    icon.set_icon_name(Some(if st.folded.contains(path) {
                        "pan-end-symbolic"
                    } else {
                        "pan-down-symbolic"
                    }));
                    icon.set_visible(true);
                    num.set_visible(false);
                    text.set_label(n);
                    text.add_css_class("heading");
                    text.set_tooltip_text(Some(&rel));
                    text.set_hexpand(false);
                    dir.set_label(d);
                    dir.set_visible(true);
                    pill.set_label(&idx.len().to_string());
                    pill.remove_css_class("rade-code-textmatch");
                    pill.add_css_class("rade-search-count-pill");
                    pill.set_visible(true);
                    b.remove_css_class("rade-search-hit");
                    b.add_css_class("rade-search-file");
                }
                Row::Item(i) => {
                    let it = &a.items[i];
                    icon.set_visible(false);
                    num.set_visible(true);
                    num.set_label(&it.line.to_string());
                    // An outline (R91): the short name, indented by depth, and its kind.
                    let outline = a.query == Some(Query::Symbols);
                    text.set_margin_start(if outline { 16 * it.depth as i32 } else { 0 });
                    // Its own name: the parent's full name (the nearest item
                    // above, one level up) is a prefix of it.
                    let short = |n: &str| -> String {
                        let parent = a.items[..i]
                            .iter()
                            .rev()
                            .find(|p| it.depth > 0 && p.depth + 1 == it.depth)
                            .and_then(|p| p.name.as_deref());
                        parent
                            .and_then(|p| n.strip_prefix(p)?.strip_prefix('.'))
                            .unwrap_or(n)
                            .to_string()
                    };
                    text.set_markup(&match (&it.name, outline) {
                        (Some(n), true) => format!(
                            "<b>{}</b>  <span alpha=\"70%\">{}</span>",
                            esc(&short(n)),
                            esc(it.kind.as_deref().unwrap_or(""))
                        ),
                        (Some(n), false) => format!(
                            "<b>{}</b>  <span alpha=\"70%\">{}</span>",
                            esc(n),
                            esc(&it.preview)
                        ),
                        (None, _) => esc(&it.preview),
                    });
                    text.remove_css_class("heading");
                    text.set_tooltip_text(Some(&it.preview));
                    text.set_hexpand(true);
                    dir.set_visible(false);
                    pill.set_label("text match");
                    pill.remove_css_class("rade-search-count-pill");
                    pill.add_css_class("rade-code-textmatch");
                    pill.set_visible(a.text());
                    b.remove_css_class("rade-search-file");
                    b.add_css_class("rade-search-hit");
                }
                _ => {}
            }
        });
        f
    }

    /// A query started from Viewer `pane`.
    pub fn running(&self, q: Query, pane: &str, ctx: &Ctx) {
        self.running.set(true);
        self.spinner.set_visible(true);
        self.retry.set_visible(false);
        self.state.borrow_mut().asked = Some((q, pane.into(), ctx.path.clone(), ctx.line, ctx.col));
        self.sync_modes(q);
    }

    fn sync_modes(&self, q: Query) {
        let call = matches!(q, Query::Callers | Query::Callees);
        self.modes.set_visible(call);
        if call {
            // Not a click: the toggle handler ignores it while running.
            self.callers.set_active(q == Query::Callers);
            self.callees.set_active(q == Query::Callees);
        }
    }

    /// The answer arrived; `reveal` shows the page (not for a jump).
    pub fn show(self: &Rc<Self>, a: Answer, reveal: bool) {
        self.running.set(false);
        self.spinner.set_visible(false);
        self.title
            .set_label(&a.query.map_or(String::new(), |q| q.title(&a.word)));
        self.summary.set_label(&a.summary());
        // A missing server: its install command on a line of its own, monospace.
        let hint = a.hint.as_deref().unwrap_or("");
        let esc = |s: &str| glib::markup_escape_text(s).to_string();
        self.hint.set_markup(&match hint.split_once(": ") {
            Some((why, cmd)) if a.state == "no-server" => {
                format!("{}:\n<tt>{}</tt>", esc(why), esc(cmd))
            }
            _ => esc(hint),
        });
        self.hint.set_visible(a.hint.is_some());
        self.retry.set_label(match a.state.as_str() {
            "crashed" => "Restart",
            "no-server" => "Check again",
            _ => "Retry",
        });
        self.retry.set_visible(matches!(
            a.state.as_str(),
            "indexing" | "error" | "crashed" | "no-server"
        ));
        if let Some(q) = a.query {
            self.sync_modes(q);
        }
        let indexing = a.state == "indexing";
        let auto: Vec<Vec<usize>> = if a.calls.len() <= AUTO_OPEN {
            (0..a.calls.len()).map(|i| vec![i]).collect()
        } else {
            vec![]
        };
        {
            let mut st = self.state.borrow_mut();
            st.groups = nav::group(&a.items);
            st.folded.clear();
            st.open.clear();
            st.notes.clear();
            st.others_folded = false;
            st.generation += 1;
            st.answer = Some(a);
        }
        // A short list opens by itself: each caller's calls and callers.
        for p in auto {
            self.toggle(&p);
        }
        self.rebuild();
        if reveal {
            self.reveal();
        }
        // Indexing: ask again, waiting for the server this time.
        if indexing && !self.retry_queued.replace(true) {
            let w = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                if let Some(v) = w.upgrade() {
                    v.retry_queued.set(false);
                    v.ask_again(Duration::from_secs(30));
                }
            });
        }
    }

    fn ask_again(&self, wait: Duration) {
        let asked = self.state.borrow().asked.clone();
        if let Some((q, pane, path, line, col)) = asked {
            crate::code::navigate(&pane, q, &path, line, col, wait);
        }
    }

    fn retry_now(self: &Rc<Self>) {
        let (crashed, path) = {
            let st = self.state.borrow();
            (
                st.answer.as_ref().is_some_and(|a| a.state == "crashed"),
                st.asked.as_ref().map(|a| a.2.clone()),
            )
        };
        match (crashed, path) {
            // Restart: forget the crashes and stop it, then ask again.
            (true, Some(path)) => {
                let w = Rc::downgrade(self);
                crate::worktrees::in_background(
                    move || crate::code::dispatch("code.restart", json!({"path": path})),
                    move |_| {
                        if let Some(v) = w.upgrade() {
                            v.ask_again(Duration::from_secs(10));
                        }
                    },
                );
            }
            _ => self.ask_again(Duration::from_secs(10)),
        }
    }

    /// Show the results in a popover on the Pane that asked (at its caret
    /// or, in a Review, at the clicked name).
    pub fn reveal(&self) {
        let Some(app) = try_app() else { return };
        let Some(pane) = self.state.borrow().asked.as_ref().map(|a| a.1.clone()) else {
            return;
        };
        let viewer = app.viewers.borrow().get(&pane).cloned();
        let at = match viewer {
            Some(v) => v.caret_anchor(),
            None => match app.reviews.borrow().get(&pane).cloned() {
                Some(r) => r.target_anchor(),
                None => return,
            },
        };
        crate::code_pop::open_results(at, self.root.upcast_ref(), self.title.label().as_str());
    }

    fn rebuild(&self) {
        let rows = {
            let st = self.state.borrow();
            let mut rows = Vec::new();
            // A call hierarchy: a tree, then the references that are not calls.
            if let Some(a) = st
                .answer
                .as_ref()
                .filter(|a| !a.calls.is_empty() || !a.others.is_empty())
            {
                fn walk(st: &State, calls: &[nav::Call], at: &[usize], rows: &mut Vec<Row>) {
                    for (i, c) in calls.iter().enumerate() {
                        let p: Vec<usize> = at.iter().copied().chain([i]).collect();
                        rows.push(Row::Call(p.clone()));
                        if !st.open.contains(&p) {
                            continue;
                        }
                        rows.extend((0..c.from_ranges.len()).map(|j| Row::Site(p.clone(), j)));
                        if st.notes.contains_key(&p) || (c.expanded && c.children.is_empty()) {
                            rows.push(Row::Note(p.clone()));
                        }
                        walk(st, &c.children, &p, rows);
                    }
                }
                walk(&st, &a.calls, &[], &mut rows);
                if !a.others.is_empty() {
                    rows.push(Row::OthersHead);
                    if !st.others_folded {
                        rows.extend((0..a.others.len()).map(Row::Other));
                    }
                }
                rows
            } else {
                // An outline is one file, named in the title: no file row.
                let outline = st.answer.as_ref().and_then(|a| a.query) == Some(Query::Symbols);
                for (g, (path, idx)) in st.groups.iter().enumerate() {
                    if outline {
                        rows.extend(idx.iter().map(|&i| Row::Item(i)));
                        continue;
                    }
                    rows.push(Row::File(g));
                    if !st.folded.contains(path) {
                        rows.extend(idx.iter().map(|&i| Row::Item(i)));
                    }
                }
                rows
            }
        };
        let n = rows.len();
        self.state.borrow_mut().rows = rows;
        let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        self.store.splice(0, self.store.n_items(), &refs);
    }

    /// Open or close a call hierarchy node; opening one whose next level
    /// was never asked for asks for it (off the GTK thread).
    fn toggle(&self, path: &[usize]) {
        let ask = {
            let mut st = self.state.borrow_mut();
            if st.open.remove(path) {
                return;
            }
            st.open.insert(path.to_vec());
            let gen = st.generation;
            let Some(a) = st.answer.as_ref() else { return };
            let incoming = a.query != Some(Query::Callees);
            match node(&a.calls, path) {
                // Not while loading; after a failure, asked again.
                Some(c)
                    if !c.expanded
                        && !c.recursive
                        && !st.notes.get(path).is_some_and(|n| n.starts_with("Loading")) =>
                {
                    let item = c.item.clone();
                    let what = if incoming { "callers" } else { "callees" };
                    st.notes.insert(path.to_vec(), format!("Loading {what}…"));
                    st.asked.clone().map(|asked| (asked, item, incoming, gen))
                }
                _ => None,
            }
        };
        let Some(((q, _, file, line, col), item, incoming, gen)) = ask else {
            return;
        };
        let Some(app) = try_app() else { return };
        let path = path.to_vec();
        let ctx = crate::code::context(&app, q, &file, line, col);
        crate::worktrees::in_background(
            move || {
                let (ctx, user) = ctx.map_err(|e| e.message)?;
                crate::code::expand(&ctx, &user, &item, incoming)
            },
            move |r| {
                let Some(v) = get() else { return };
                {
                    let mut st = v.state.borrow_mut();
                    if st.generation != gen {
                        return;
                    }
                    let Some(a) = st.answer.as_mut() else { return };
                    // Its ancestors (and itself): a child among them is a recursion.
                    let up: Vec<nav::ItemKey> = (1..=path.len())
                        .filter_map(|k| node(&a.calls, &path[..k]).map(|c| nav::item_key(&c.item)))
                        .collect();
                    let note = match r {
                        Ok(mut children) => {
                            for c in &mut children {
                                c.recursive = up.contains(&nav::item_key(&c.item));
                            }
                            if let Some(n) = node_mut(&mut a.calls, &path) {
                                n.children = children;
                                n.expanded = true;
                            }
                            None
                        }
                        Err(e) => Some(format!("Could not load them: {e}")),
                    };
                    match note {
                        Some(n) => st.notes.insert(path.clone(), n),
                        None => st.notes.remove(&path),
                    };
                }
                v.rebuild();
            },
        );
    }

    /// A call hierarchy node's row: go to the function.
    fn goto(&self, pos: usize) {
        let (pane, at) = {
            let st = self.state.borrow();
            let at = match (st.rows.get(pos), &st.answer) {
                (Some(Row::Call(p)), Some(a)) => {
                    node(&a.calls, p).map(|c| (c.path.clone(), c.line, c.col))
                }
                _ => None,
            };
            (st.asked.as_ref().map(|a| a.1.clone()), at)
        };
        if let Some((path, line, col)) = at {
            crate::code::jump_to(&pane.unwrap_or_default(), &path, line, col);
        }
    }

    fn activate(&self, pos: usize) {
        let row = self.state.borrow().rows.get(pos).cloned();
        let jump = |it: Option<nav::Item>| {
            let pane = self.state.borrow().asked.as_ref().map(|a| a.1.clone());
            if let Some(it) = it {
                crate::code::jump_to(&pane.unwrap_or_default(), &it.path, it.line, it.col);
            }
        };
        match row {
            Some(Row::Call(p)) => {
                self.toggle(&p);
                self.rebuild();
            }
            Some(Row::Site(p, j)) => {
                let it = {
                    let st = self.state.borrow();
                    st.answer
                        .as_ref()
                        .and_then(|a| node(&a.calls, &p)?.from_ranges.get(j).cloned())
                };
                jump(it);
            }
            Some(Row::Other(i)) => {
                let it = {
                    let st = self.state.borrow();
                    st.answer.as_ref().and_then(|a| a.others.get(i).cloned())
                };
                jump(it);
            }
            Some(Row::OthersHead) => {
                {
                    let mut st = self.state.borrow_mut();
                    st.others_folded = !st.others_folded;
                }
                self.rebuild();
            }
            Some(Row::Note(_)) => {}
            Some(Row::File(g)) => {
                {
                    let mut st = self.state.borrow_mut();
                    let p = st.groups[g].0.clone();
                    if !st.folded.remove(&p) {
                        st.folded.insert(p);
                    }
                }
                self.rebuild();
            }
            Some(Row::Item(i)) => {
                let (pane, it) = {
                    let st = self.state.borrow();
                    let it = st.answer.as_ref().map(|a| a.items[i].clone());
                    (st.asked.as_ref().map(|a| a.1.clone()), it)
                };
                if let Some(it) = it {
                    crate::code::jump_to(&pane.unwrap_or_default(), &it.path, it.line, it.col);
                }
            }
            None => {}
        }
    }

    fn info(&self) -> Value {
        let st = self.state.borrow();
        let rows: Vec<Value> = match &st.answer {
            Some(a) => st
                .rows
                .iter()
                .map(|r| match r {
                    Row::Call(p) => {
                        let c = node(&a.calls, p).cloned().unwrap_or_default();
                        json!({"kind": "call", "name": c.name, "path": nav::rel(&c.path, &a.root), "line": c.line,
                               "depth": p.len(), "count": c.from_ranges.len(), "open": st.open.contains(p),
                               "recursive": c.recursive})
                    }
                    Row::Site(p, j) => {
                        let s = node(&a.calls, p).and_then(|c| c.from_ranges.get(*j).cloned()).unwrap_or_default();
                        json!({"kind": "site", "path": nav::rel(&s.path, &a.root), "line": s.line, "col": s.col,
                               "depth": p.len(), "preview": s.preview})
                    }
                    Row::Note(p) => json!({"kind": "note", "depth": p.len(), "text": st.notes.get(p).cloned().unwrap_or_else(|| {
                        if a.query == Some(Query::Callees) { "No callees" } else { "No callers" }.to_string()
                    })}),
                    Row::OthersHead => json!({"kind": "others", "count": a.others.len(), "open": !st.others_folded}),
                    Row::Other(i) => {
                        let it = &a.others[*i];
                        json!({"kind": "other", "path": nav::rel(&it.path, &a.root), "line": it.line, "col": it.col, "preview": it.preview})
                    }
                    &Row::File(g) => json!({"kind": "file", "path": nav::rel(&st.groups[g].0, &a.root), "count": st.groups[g].1.len()}),
                    &Row::Item(i) => {
                        let it = &a.items[i];
                        json!({"kind": "item", "path": nav::rel(&it.path, &a.root), "line": it.line, "col": it.col,
                               "name": it.name, "symbol_kind": it.kind, "preview": it.preview, "text_match": a.text()})
                    }
                })
                .collect(),
            None => vec![],
        };
        json!({
            "visible": crate::code_pop::results_open() && self.root.is_mapped(),
            "title": self.title.label().as_str(),
            "summary": self.summary.label().as_str(),
            "hint": self.hint.is_visible().then(|| self.hint.text().replace(":\n", ": ")),
            "retry": self.retry.is_visible().then(|| self.retry.label().map(|l| l.to_string())).flatten(),
            "running": self.running.get(),
            "modes": self.modes.is_visible().then(|| if self.callers.is_active() { "callers" } else { "callees" }),
            "state": st.answer.as_ref().map(|a| a.state.clone()),
            "source": st.answer.as_ref().map(|a| a.source.clone()),
            "rows": rows,
        })
    }
}

/// `debug.code` (E2E): `{"op": "info" | "activate" | "goto" | "retry" | "mode" | "show", …}`;
/// `activate` takes `index`, or `path` (+ `line` for an item row), or
/// `name` (+ `depth`) for a call hierarchy node, or `kind: "others"`;
/// `goto` = a node's Go to button.
pub fn debug(p: &Value) -> Result<Value> {
    let v = get().ok_or_else(|| Error::internal("no code results"))?;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    match s("op").unwrap_or("info") {
        "info" => {}
        // The hover / peek popover (R90).
        "popover" => return Ok(crate::code_pop::info()),
        "popover_open" => return crate::code_pop::click_open(),
        "popover_close" => return Ok(json!({"closed": crate::code_pop::close()})),
        "show" => v.reveal(),
        "retry" => v.retry.emit_clicked(),
        "mode" => match s("arg") {
            Some("callees") => v.callees.set_active(true),
            _ => v.callers.set_active(true),
        },
        "activate" | "goto" => {
            let line = p.get("line").and_then(Value::as_u64);
            let depth = p.get("depth").and_then(Value::as_u64);
            let i = p
                .get("index")
                .and_then(Value::as_u64)
                .map(|i| i as usize)
                .or_else(|| {
                    let info = v.info();
                    info["rows"].as_array()?.iter().position(|r| {
                        // A call hierarchy node by name (and depth).
                        if let Some(n) = s("name") {
                            return r["kind"] == "call"
                                && r["name"].as_str() == Some(n)
                                && depth.is_none_or(|d| r["depth"].as_u64() == Some(d));
                        }
                        if s("kind") == Some("others") {
                            return r["kind"] == "others";
                        }
                        r["path"].as_str() == s("path")
                            && match line {
                                Some(l) => r["line"].as_u64() == Some(l),
                                None => r["kind"] == "file",
                            }
                    })
                });
            let i = i.ok_or_else(|| Error::not_found("no such row"))?;
            if s("op") == Some("goto") {
                v.goto(i);
            } else {
                v.activate(i);
            }
        }
        o => return Err(Error::usage(format!("unknown op '{o}'"))),
    }
    Ok(v.info())
}
