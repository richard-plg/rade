//! `debug.review` (E2E and live harness, ADR-0006): drive the Review Pane
//! like a user and report what it shows.

use super::{
    rows::{marks_shown, row_widget},
    ReviewView,
};
use crate::app::try_app;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::diff::Row;
use serde_json::{json, Value};
use sourceview5::prelude::*;
use std::rc::Rc;
use std::time::Duration;

impl ReviewView {
    /// A row's text as the list shows it (for `visible_text` and lookups).
    pub(super) fn row_text(&self, r: Row) -> String {
        let st = self.state.borrow();
        let Some(c) = st.changes.as_ref() else {
            return String::new();
        };
        match r {
            Row::File(f) => {
                let f = &c.files[f];
                format!("{} {} +{} −{}", f.status, f.path, f.added, f.removed)
            }
            Row::Hunk(f, h) => {
                let f = &c.files[f];
                let folded = st
                    .folded_hunks
                    .contains(&(f.path.clone(), f.hunks[h].header.clone()));
                format!("{} {}", if folded { "▸" } else { "▾" }, f.hunks[h].header)
            }
            Row::Line(f, h, l) => c.files[f].hunks[h].lines[l].text.clone(),
            Row::Binary(_) => "Binary file not shown".into(),
            Row::Comment(id) => st
                .all_comments
                .iter()
                .find(|c| c.id == id)
                .map_or(String::new(), |c| format!("[comment] {}", c.text)),
            Row::Composer => "[composer]".into(),
        }
    }

    /// The first row containing `text`, from the jumped-to file's header on
    /// (then from the top).
    pub(super) fn row_of(&self, text: &str) -> rade_core::rpc::Result<usize> {
        let rows = self.state.borrow().rows.clone();
        let sel = self.state.borrow().selected.clone();
        let start = rows
            .iter()
            .position(|r| matches!(r, Row::File(f) if self.file_path(*f) == sel))
            .unwrap_or(0);
        (start..rows.len())
            .chain(0..start)
            .find(|&i| self.row_text(rows[i]).contains(text))
            .ok_or_else(|| rade_core::rpc::Error::not_found(format!("'{text}' is not in the diff")))
    }

    /// `debug.review` (E2E, live): drive the view like a user.
    pub fn debug(self: &Rc<Self>, op: &str, arg: Option<&str>) -> rade_core::rpc::Result<Value> {
        use rade_core::rpc::Error;
        match (op, arg) {
            ("info", _) => {}
            ("refresh", _) => {
                self.state.borrow_mut().changes = None;
                self.refresh();
            }
            ("select", Some(p)) => self.select(p),
            // A click on the header of the first row containing the text
            // (a hunk header, or a file header: `fold <path>`).
            ("toggle", Some(t)) => self.toggle_row(self.row_of(t)?),
            ("fold", Some(p)) => {
                let i = self
                    .state
                    .borrow()
                    .rows
                    .iter()
                    .position(
                        |r| matches!(r, Row::File(f) if self.file_path(*f).as_deref() == Some(p)),
                    )
                    .ok_or_else(|| Error::not_found(format!("no file '{p}'")))?;
                self.toggle_row(i);
            }
            // `comment <first text>|<last text>|<comment>`: select those
            // rows, then the `c` key and Enter in the comment bar.
            ("comment", Some(a)) => {
                let parts: Vec<&str> = a.splitn(3, '|').collect();
                let [first, last, text] = parts[..] else {
                    return Err(Error::usage("comment wants FIRST|LAST|TEXT"));
                };
                let (i, j) = (self.row_of(first)?, self.row_of(last)?);
                self.selection
                    .select_range(i as u32, (j.max(i) - i + 1) as u32, true);
                self.start_comment();
                self.composer.set(text);
                self.add_pending();
            }
            // `Discard…` (asks), then its Discard response.
            ("discard", _) => self.discard.emit_clicked(),
            ("discard_confirm", _) => {
                let d = self.discard_dialog.borrow_mut().take();
                let d = d.ok_or_else(|| Error::usage("no Discard dialog is open"))?;
                use adw::prelude::*;
                d.emit_by_name::<()>("response", &[&"discard"]);
                d.force_close();
            }
            // R119: `scope since|all`, `resolve ID`, `reopen ID`,
            // `follow_up ID|TEXT`, `approve ITEM` (an Approve item's
            // snake_case name), `approve_menu` (fills the popover).
            ("scope", Some(a)) => self.set_scope(a == "since"),
            ("resolve", Some(id)) | ("reopen", Some(id)) => {
                let id = id
                    .parse()
                    .map_err(|_| Error::usage("resolve wants a comment id"))?;
                self.resolve(id, op == "resolve");
            }
            ("follow_up", Some(a)) => {
                let (id, text) = a
                    .split_once('|')
                    .ok_or_else(|| Error::usage("follow_up wants ID|TEXT"))?;
                let id: u64 = id.parse().map_err(|_| Error::usage("bad comment id"))?;
                let c = self
                    .comments_now()
                    .into_iter()
                    .find(|c| c.id == id)
                    .ok_or_else(|| Error::not_found(format!("no comment {id}")))?;
                self.follow_up(&c);
                self.composer.set(text);
                self.add_pending();
            }
            ("approve", Some(item)) => {
                let item = self
                    .approve_items()
                    .into_iter()
                    .find(|i| {
                        serde_json::to_value(i)
                            .ok()
                            .and_then(|v| v.as_str().map(String::from))
                            .as_deref()
                            == Some(item)
                    })
                    .ok_or_else(|| Error::usage(format!("{item} is not in the Approve menu")))?;
                super::comments::approve_now(&self.id, item);
            }
            ("send", pane) => crate::review::send_from_view(&self.id, pane.map(String::from)),
            // Window point of the row containing the text (scrolled into
            // view), for real clicks from the live harness.
            ("locate", Some(t)) => {
                let i = self.row_of(t)? as u32;
                self.scroll_to_top(i);
                let ctx = glib::MainContext::default();
                let mut row = None;
                for _ in 0..100 {
                    while ctx.iteration(false) {}
                    // Only once the jump has settled: the row must not move
                    // after its point is reported.
                    row = row_widget(&self.list, i).filter(|w| w.is_mapped());
                    if row.is_some() && self.scroll_req.get().is_none() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let row = row.ok_or_else(|| Error::internal("not on screen"))?;
                let win = &try_app().unwrap().window;
                let pt = row
                    .compute_point(
                        win,
                        &gtk::graphene::Point::new(90.0, row.height() as f32 / 2.0),
                    )
                    .ok_or_else(|| Error::internal("not on screen"))?;
                return Ok(json!({"row": i, "x": pt.x(), "y": pt.y()}));
            }
            // Code navigation (R87): `point ROW TEXT|WORD` (a click there),
            // `ctrl_click ROW TEXT|WORD` (through the click handler's
            // coordinates), `key F12|shift+F12|ctrl+F12|alt+shift+h`,
            // `locate_word ROW TEXT|WORD` (its window point, live clicks).
            ("point", Some(a)) | ("ctrl_click", Some(a)) => {
                let (i, row, x, y) = self.word_point(a)?;
                // A plain click selects the row (the list's own gesture);
                // Ctrl+click leaves the selection alone.
                if op == "point" {
                    self.selection.select_item(i as u32, true);
                }
                self.click_line(&row, i, x, y, op == "ctrl_click");
            }
            // Ctrl held with the pointer over `ROW TEXT|WORD` (0.7.1).
            ("link", Some(a)) => {
                let (_, row, x, y) = self.word_point(a)?;
                let p = row
                    .compute_point(&self.list, &gtk::graphene::Point::new(x as f32, y as f32))
                    .ok_or_else(|| Error::internal("not on screen"))?;
                self.set_link(Some((p.x() as f64, p.y() as f64)));
            }
            ("link_off", _) => self.set_link(None),
            ("locate_word", Some(a)) => {
                let (i, row, x, y) = self.word_point(a)?;
                let win = &try_app().unwrap().window;
                let pt = row
                    .compute_point(win, &gtk::graphene::Point::new(x as f32, y as f32))
                    .ok_or_else(|| Error::internal("not on screen"))?;
                return Ok(json!({"row": i, "x": pt.x(), "y": pt.y()}));
            }
            ("key", Some(k)) => {
                use gtk::gdk;
                let mut mods = gdk::ModifierType::empty();
                let mut name = k;
                while let Some((m, rest)) = name.split_once('+') {
                    mods |= match m {
                        "ctrl" => gdk::ModifierType::CONTROL_MASK,
                        "shift" => gdk::ModifierType::SHIFT_MASK,
                        "alt" => gdk::ModifierType::ALT_MASK,
                        _ => gdk::ModifierType::empty(),
                    };
                    name = rest;
                }
                let key = gdk::Key::from_name(name)
                    .ok_or_else(|| Error::usage(format!("unknown key {name}")))?;
                if !self.on_loop_key(key, mods) && !self.on_nav_key(key, mods) {
                    return Err(Error::usage(format!("{k} is not a Review key")));
                }
            }
            // The Impact section (R88.2): `impact` shows it (toggle on),
            // `impact_off`, `impact_retry`, `impact_activate TEXT` (the
            // first row containing it), `impact_locate TEXT` (window point).
            ("impact", _) => self.impact_ui.toggle.set_active(true),
            ("impact_off", _) => self.impact_ui.toggle.set_active(false),
            ("impact_retry", _) => self.compute_impact(Duration::from_secs(30)),
            ("impact_activate", Some(t)) | ("impact_locate", Some(t)) => {
                let i = self
                    .state
                    .borrow()
                    .impact_rows
                    .iter()
                    .position(|r| r.text.contains(t))
                    .ok_or_else(|| Error::not_found(format!("no Impact row with '{t}'")))?;
                if op == "impact_activate" {
                    self.activate_impact(i);
                } else {
                    return self.impact_locate(i);
                }
            }
            // Scroll through the whole list in 3 s and record frame times
            // (`info.perf` once done).
            ("perf", _) => self.start_perf(),
            (op, _) => return Err(Error::usage(format!("unknown review op '{op}'"))),
        }
        Ok(self.info())
    }

    fn start_perf(&self) {
        *self.perf.borrow_mut() = None;
        let adj = self.scroll.vadjustment();
        adj.set_value(0.0);
        let frames: Rc<std::cell::RefCell<Vec<i64>>> = Rc::default();
        let id = self.id.clone();
        self.list.add_tick_callback(move |_, clock| {
            let t = clock.frame_time();
            let mut f = frames.borrow_mut();
            f.push(t);
            let p = ((t - f[0]) as f64 / 3_000_000.0).min(1.0);
            adj.set_value(p * (adj.upper() - adj.page_size()));
            if p < 1.0 {
                return glib::ControlFlow::Continue;
            }
            let mut d: Vec<f64> = f
                .windows(2)
                .map(|w| (w[1] - w[0]) as f64 / 1000.0)
                .collect();
            d.sort_by(f64::total_cmp);
            let pct = |q: f64| d.get(((d.len() as f64 - 1.0) * q) as usize).copied();
            let result = json!({
                "frames": d.len(), "avg_ms": d.iter().sum::<f64>() / d.len().max(1) as f64,
                "p50_ms": pct(0.5), "p95_ms": pct(0.95), "max_ms": d.last(),
                "over_33ms": d.iter().filter(|&&x| x > 33.4).count(),
                "upper_px": adj.upper(),
            });
            if let Some(v) = try_app().and_then(|a| a.reviews.borrow().get(&id).cloned()) {
                *v.perf.borrow_mut() = Some(result);
            }
            glib::ControlFlow::Break
        });
    }

    /// The diff's summary, as the old single-line header said it.
    fn title_text(&self) -> String {
        let st = self.state.borrow();
        let Some(c) = st.changes.as_ref() else {
            return "Review".into();
        };
        let (add, rem) = c
            .files
            .iter()
            .fold((0, 0), |a, f| (a.0 + f.added, a.1 + f.removed));
        let base: String = c.base.chars().take(7).collect();
        format!(
            "{} file{} changed  +{add} −{rem}  ·  vs {} ({base}){}",
            c.files.len(),
            if c.files.len() == 1 { "" } else { "s" },
            c.branch.as_deref().unwrap_or("HEAD"),
            if c.truncated {
                "  ·  diff truncated"
            } else {
                ""
            }
        )
    }

    fn info(&self) -> Value {
        let st = self.state.borrow();
        let rows = st.rows.clone();
        let sel = st.selected.clone();
        let gutter: Vec<String> = {
            let c = st.changes.as_ref();
            rows.iter()
                .map(|r| match (*r, c) {
                    (Row::Line(f, h, l), Some(c)) => {
                        let f = &c.files[f];
                        let l = &f.hunks[h].lines[l];
                        let num = |x: Option<u32>| x.map_or(String::new(), |x| x.to_string());
                        let sign = super::rows::sign(l.kind, super::rows::marked(&st, &f.path, l));
                        format!("{:>5} {:>5}|{sign}", num(l.old), num(l.new))
                    }
                    _ => "|".into(),
                })
                .collect()
        };
        let lang = sel.as_deref().and_then(|p| {
            sourceview5::LanguageManager::default()
                .guess_language(Some(p), None)
                .map(|l| l.id().to_string())
        });
        let json = json!({
            "loading": self.loading.get(),
            "error": st.error,
            "title": self.title_text(),
            "head": self.head.info(&st.head),
            "commit_list": self.commit_rows(),
            "rail": self.rail_info(),
            "cursor": st.cursor,
            "composer": st.pending.as_ref().map(|p| json!({
                "path": p.path, "line": p.line, "end_line": p.end_line, "edit": p.edit,
                "row": st.rows.iter().position(|r| *r == Row::Composer),
                "text": self.composer.get(),
            })),
            "cards": st.rows.iter().enumerate().filter_map(|(i, r)| match r {
                Row::Comment(id) => Some(json!({"id": id, "row": i})),
                _ => None,
            }).collect::<Vec<_>>(),
            "drafts": self.drafts.label().to_string(),
            "focus": try_app().and_then(|a| gtk::prelude::GtkWindowExt::focus(&a.window)).map(|w| {
                let mut chain = Vec::new();
                let mut c = Some(w);
                while let Some(x) = c {
                    let cls: Vec<String> = x.css_classes().iter().map(|s| s.to_string()).collect();
                    chain.push(format!("{}{}", x.css_name(), if cls.is_empty() { String::new() } else { format!(".{}", cls.join(".")) }));
                    if chain.len() >= 6 { break; }
                    c = x.parent();
                }
                chain
            }),
            "composer_focus": self.composer.text.has_focus(),
            // The window's focus widget (true even in an inactive window).
            "composer_is_focus": self.composer.text.is_focus(),
            "primary": if self.send.is_visible() { self.send.label().map(|l| l.to_string()) } else { Some("Approve".into()) },
            "approve_items": self.approve_items().iter().map(|i| i.label()).collect::<Vec<_>>(),
            "card_states": st.all_comments.iter().map(|c| json!({
                "id": c.id, "round": c.round, "resolved": c.resolved, "reply_to": c.reply_to,
                "state": st.states.get(&c.id),
            })).collect::<Vec<_>>(),
            "discard_dialog": self.discard_dialog.borrow().as_ref().and_then(|d| {
                use adw::prelude::*;
                d.heading()
            }).map(|h| h.to_string()),
            "impact_rail": json!({"shown": self.split.shows_sidebar(), "collapsed": self.split.is_collapsed()}),
            "ignore_space": st.ignore_space,
            "branch": st.changes.as_ref().and_then(|c| c.branch.clone()),
            "files": st.changes.as_ref().map(|c| c.files.iter().map(|f| json!({
                "path": f.path, "status": f.status.to_string(), "added": f.added,
                "removed": f.removed, "binary": f.binary, "hunks": f.hunks.len(),
                "folded": st.folded_files.contains(&f.path),
            })).collect::<Vec<_>>()),
            "selected": sel,
            "rows": rows.len(),
            "gutter": gutter,
            "language": lang,
            "comment_bar": st.pending.is_some(),
            "comments": st.all_comments.len(),
            "outdated": st.comments_sig.iter().filter(|c| c.2).count(),
            "marks_shown": marks_shown(&self.list),
            "scroll_y": self.scroll.vadjustment().value(),
            "send": self.send.label().map(|s| s.to_string()),
            "note": self.note.label().to_string(),
            "render_ms": st.render_ms,
            "highlight_ms": self.highlight_ms.get(),
            "perf": *self.perf.borrow(),
            "point": st.point.map(|(r, c)| json!({"row": r, "col": c})),
            "link": self.link_word(),
            "pointer_cursor": self.list.cursor().and_then(|c| c.name()).map(|n| n.to_string()),
        });
        drop(st);
        let text: Vec<String> = rows.iter().map(|r| self.row_text(*r)).collect();
        let mut json = json;
        json["visible_text"] = text.join("\n").into();
        json["impact"] = self.impact_info();
        json
    }
}
