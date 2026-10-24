//! The Review Pane's files rail (PRD R118.2, R118.3): the changed files as
//! a folder tree in diff order (`rade_core::review_loop::rail`), lock and
//! generated files in a folded group at the end, a filter and Hide viewed;
//! each file shows its status, a viewed tick or an unread dot, its comment
//! count and `+/−`. Viewed marks live in the Worktree's `review` state.

use super::{status_class, ReviewView};
use crate::app::try_app;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::diff::Row;
use rade_core::review_loop::{rail, RailRow, GENERATED};

pub(super) struct RailUi {
    pub root: gtk::Box,
    pub filter: gtk::SearchEntry,
    pub hide_viewed: gtk::ToggleButton,
    pub list: gtk::ListBox,
}

impl RailUi {
    pub fn new() -> Self {
        let filter = gtk::SearchEntry::new();
        filter.set_placeholder_text(Some("Filter files"));
        filter.add_css_class("review-filter");
        filter.set_hexpand(true);
        filter.set_valign(gtk::Align::Center);
        let hide_viewed = gtk::ToggleButton::new();
        hide_viewed.set_icon_name("ph-check-symbolic");
        hide_viewed.set_tooltip_text(Some("Hide viewed files"));
        hide_viewed.add_css_class("flat");
        hide_viewed.add_css_class("review-hide-viewed");
        hide_viewed.set_valign(gtk::Align::Center);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        top.add_css_class("review-rail-top");
        top.append(&filter);
        top.append(&hide_viewed);
        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.add_css_class("rade-review-files");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&list));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("review-files");
        root.append(&top);
        root.append(&scroll);
        Self {
            root,
            filter,
            hide_viewed,
            list,
        }
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l
}

impl ReviewView {
    pub(super) fn connect_rail(self: &std::rc::Rc<Self>) {
        let weak = std::rc::Rc::downgrade(self);
        self.rail.list.connect_row_activated(move |_, row| {
            let Some(v) = weak.upgrade() else { return };
            let name = row.widget_name().to_string();
            match name.split_once(':') {
                Some(("file", path)) => v.select(path),
                Some(("folder", path)) => v.toggle_rail(path),
                _ if name == "generated" => v.toggle_rail(GENERATED),
                _ => {}
            }
        });
        let weak = std::rc::Rc::downgrade(self);
        self.rail.filter.connect_search_changed(move |e| {
            if let Some(v) = weak.upgrade() {
                v.state.borrow_mut().filter = e.text().to_string();
                v.render_files();
            }
        });
        let weak = std::rc::Rc::downgrade(self);
        self.rail.hide_viewed.connect_toggled(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render_files();
            }
        });
    }

    fn toggle_rail(&self, key: &str) {
        {
            let mut st = self.state.borrow_mut();
            if !st.folded_rail.remove(key) {
                st.folded_rail.insert(key.to_string());
            }
        }
        self.render_files();
    }

    /// The Worktree's Viewed marks and the files' content hashes: (path
    /// index → viewed).
    pub(super) fn viewed_flags(&self) -> Vec<bool> {
        let st = self.state.borrow();
        let Some(c) = &st.changes else { return vec![] };
        let marks = self.review_state().map(|r| r.viewed).unwrap_or_default();
        c.files
            .iter()
            .map(|f| {
                st.hashes
                    .get(&f.path)
                    .is_some_and(|h| marks.get(&f.path) == Some(h))
            })
            .collect()
    }

    /// The rail's rows, rebuilt (the diff, filter, folds or marks changed).
    pub(super) fn render_files(&self) {
        while let Some(r) = self.rail.list.row_at_index(0) {
            self.rail.list.remove(&r);
        }
        let viewed = self.viewed_flags();
        let st = self.state.borrow();
        let Some(c) = &st.changes else { return };
        let generated = |p: &str| st.generated.contains(p);
        let order: Vec<usize> = (0..c.files.len()).collect();
        let hide = self.rail.hide_viewed.is_active();
        let rows = rail(
            &c.files,
            &order,
            &generated,
            &st.folded_rail,
            &st.filter,
            &|i| hide && viewed.get(i).copied().unwrap_or(false),
        );
        let counts = self.comment_counts();
        for r in &rows {
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let row = gtk::ListBoxRow::new();
            match r {
                RailRow::Folder {
                    path,
                    name,
                    depth,
                    folded,
                } => {
                    b.set_margin_start(*depth as i32 * 12);
                    let caret = gtk::Image::from_icon_name(if *folded {
                        "ph-caret-right-symbolic"
                    } else {
                        "ph-caret-down-symbolic"
                    });
                    caret.add_css_class("review-caret");
                    let l = label(name.trim_end_matches('/'), "review-folder-name");
                    l.set_xalign(0.0);
                    l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                    b.append(&caret);
                    b.append(&l);
                    row.add_css_class("review-folder");
                    row.set_widget_name(&format!("folder:{path}"));
                }
                RailRow::File { index, name, depth } => {
                    let f = &c.files[*index];
                    b.set_margin_start(*depth as i32 * 12 + 14);
                    let s = label(&f.status.to_string(), "rade-git-mark");
                    s.add_css_class(status_class(f.status));
                    let shown = match &f.old_path {
                        Some(o) if *depth == 0 && st.filter.is_empty() => format!("{o} → {name}"),
                        _ => name.clone(),
                    };
                    let n = label(&shown, "review-file-name");
                    n.set_xalign(0.0);
                    n.set_hexpand(true);
                    n.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                    let is_viewed = viewed.get(*index).copied().unwrap_or(false);
                    let mark = if is_viewed {
                        let i = gtk::Image::from_icon_name("ph-check-symbolic");
                        i.add_css_class("review-viewed-tick");
                        i.upcast::<gtk::Widget>()
                    } else {
                        let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        d.add_css_class("review-unread-dot");
                        d.set_valign(gtk::Align::Center);
                        d.upcast()
                    };
                    b.append(&s);
                    b.append(&n);
                    b.append(&mark);
                    if let Some(k) = counts.get(&f.path).filter(|k| **k > 0) {
                        b.append(&label(&k.to_string(), "review-comment-count"));
                    }
                    let num = gtk::Label::new(None);
                    num.set_markup(&super::rows::counts(f));
                    b.append(&num);
                    row.add_css_class("review-file");
                    if is_viewed {
                        row.add_css_class("viewed");
                    }
                    row.set_widget_name(&format!("file:{}", f.path));
                    row.set_tooltip_text(Some(&f.path));
                }
                RailRow::Generated { count, folded } => {
                    let caret = gtk::Image::from_icon_name(if *folded {
                        "ph-caret-right-symbolic"
                    } else {
                        "ph-caret-down-symbolic"
                    });
                    caret.add_css_class("review-caret");
                    b.append(&caret);
                    let l = label("Lock & generated", "review-folder-name");
                    l.set_xalign(0.0);
                    l.set_hexpand(true);
                    b.append(&l);
                    b.append(&label(&count.to_string(), "review-comment-count"));
                    row.add_css_class("review-folder");
                    row.add_css_class("review-generated");
                    row.set_widget_name("generated");
                }
            }
            row.set_child(Some(&b));
            self.rail.list.append(&row);
            if let (RailRow::File { index, .. }, Some(sel)) = (r, st.selected.as_deref()) {
                if c.files[*index].path == sel {
                    self.rail.list.select_row(Some(&row));
                }
            }
        }
        drop(st);
        self.update_progress();
    }

    /// Comments per file (drafts and sent, not resolved).
    pub(super) fn comment_counts(&self) -> std::collections::HashMap<String, usize> {
        let mut m = std::collections::HashMap::new();
        for c in self.comments_now() {
            *m.entry(c.path).or_insert(0) += 1;
        }
        m
    }

    /// The header's `k/n viewed` (read only: it runs inside the app's
    /// sync, which holds the model).
    pub(super) fn update_progress(&self) {
        let pairs = self.hash_pairs();
        let refs: Vec<(&str, u64)> = pairs.iter().map(|(p, h)| (p.as_str(), *h)).collect();
        let mut marks = self.review_state().unwrap_or_default();
        let (done, total) = marks.progress(&refs);
        self.head.progress(done, total);
    }

    /// Each changed file's path and content hash (every change, whatever
    /// the scope).
    pub(super) fn hash_pairs(&self) -> Vec<(String, u64)> {
        let st = self.state.borrow();
        let Some(c) = st.all.as_ref().or(st.changes.as_ref()) else {
            return vec![];
        };
        c.files
            .iter()
            .filter_map(|f| Some((f.path.clone(), *st.hashes.get(&f.path)?)))
            .collect()
    }

    /// Drop the Viewed marks of files no longer changed (after a refresh,
    /// outside the app's sync).
    pub(super) fn prune_viewed(&self) {
        let (Some((p, w, _)), Some(app)) = (self.place(), try_app()) else {
            return;
        };
        let pairs = self.hash_pairs();
        let refs: Vec<(&str, u64)> = pairs.iter().map(|(p, h)| (p.as_str(), *h)).collect();
        let stale = self
            .review_state()
            .is_some_and(|r| r.viewed.keys().any(|k| !refs.iter().any(|(p, _)| p == k)));
        if stale {
            app.touch(|m| m.prune_viewed(&p, &w, &refs));
        }
    }

    /// Mark the file of row `pos` (a file header) or `path` viewed or not.
    /// Viewing folds it and moves to the next unread file (R118.3).
    pub(super) fn set_viewed(&self, path: &str, on: bool) {
        let (Some((p, w, _)), Some(app)) = (self.place(), try_app()) else {
            return;
        };
        let Some(hash) = self.state.borrow().hashes.get(path).copied() else {
            return;
        };
        let _ = app.update(|m| m.set_viewed(&p, &w, path, hash, on));
        {
            let mut st = self.state.borrow_mut();
            if on {
                st.folded_files.insert(path.to_string());
                st.auto_folded.insert(path.to_string());
            } else {
                st.folded_files.remove(path);
                st.auto_folded.remove(path);
            }
        }
        self.render_files();
        self.render_diff();
        if on {
            let next = {
                let viewed = self.viewed_flags();
                let st = self.state.borrow();
                let c = st.changes.as_ref();
                let i = c.and_then(|c| c.files.iter().position(|f| f.path == path));
                c.zip(i).and_then(|(c, i)| {
                    (i + 1..c.files.len())
                        .chain(0..i)
                        .find(|&j| !viewed.get(j).copied().unwrap_or(false))
                        .map(|j| c.files[j].path.clone())
                })
            };
            if let Some(n) = next {
                self.select(&n);
            }
        }
    }

    /// `v`: the file of the cursor row (or the selected file) flips.
    pub(super) fn toggle_viewed_here(&self) {
        let path = {
            let st = self.state.borrow();
            let fi = st
                .cursor
                .and_then(|i| st.rows.get(i))
                .and_then(|r| match *r {
                    Row::File(f) | Row::Hunk(f, _) | Row::Line(f, _, _) | Row::Binary(f) => Some(f),
                    _ => None,
                });
            fi.and_then(|f| self.file_path_in(&st, f))
                .or(st.selected.clone())
        };
        let Some(path) = path else { return };
        let i = self
            .state
            .borrow()
            .changes
            .as_ref()
            .and_then(|c| c.files.iter().position(|f| f.path == path));
        let on = !i.is_some_and(|i| self.viewed_flags().get(i).copied().unwrap_or(false));
        self.set_viewed(&path, on);
    }

    /// The rail as `debug.review` reports it.
    pub(super) fn rail_info(&self) -> Vec<serde_json::Value> {
        let viewed = self.viewed_flags();
        let st = self.state.borrow();
        let counts = self.comment_counts();
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(row) = self.rail.list.row_at_index(i) {
            let name = row.widget_name().to_string();
            let v = match name.split_once(':') {
                Some(("file", path)) => {
                    let idx = st
                        .changes
                        .as_ref()
                        .and_then(|c| c.files.iter().position(|f| f.path == path));
                    serde_json::json!({
                        "kind": "file", "path": path,
                        "status": idx.and_then(|j| st.changes.as_ref().map(|c| c.files[j].status.to_string())),
                        "viewed": idx.is_some_and(|j| viewed.get(j).copied().unwrap_or(false)),
                        "comments": counts.get(path).copied().unwrap_or(0),
                        "indent": row.child().map(|c| c.margin_start()),
                    })
                }
                Some(("folder", path)) => serde_json::json!({"kind": "folder", "path": path,
                    "folded": st.folded_rail.contains(path)}),
                _ => serde_json::json!({"kind": "generated",
                    "folded": st.folded_rail.contains(GENERATED)}),
            };
            out.push(v);
            i += 1;
        }
        out
    }
}
