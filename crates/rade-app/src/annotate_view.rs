//! Browser Annotations in a Browser Pane (PRD R71, ADR-0026): the header's
//! annotate toggle (Ctrl+Shift+A), the page overlay (`annotate.js` in the
//! `rade` world, which posts what the owner picked to the `radeAnnotate`
//! handler of that world only), the cropped screenshot, and the panel
//! under the page that lists the pending Annotations (edit, delete, Send
//! to Agent, Clear) like the Review Pane lists Review Comments.

use crate::app::try_app;
use crate::browser_view::BrowserView;
use gtk::{gdk, glib};
use gtk4 as gtk;
use rade_core::annotate::{crop_box, Annotation, Pick, MAX_ERRORS};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use webkit6::prelude::*;

/// The list under the page.
pub struct Panel {
    pub revealer: gtk::Revealer,
    list: gtk::ListBox,
    hint: gtk::Label,
    pub(crate) note: gtk::Label,
    pub(crate) send: gtk::Button,
    pub(crate) clear: gtk::Button,
    /// (id, label) of the rows shown: typing in a comment must not
    /// rebuild them.
    sig: RefCell<Vec<(u64, String)>>,
    /// The Annotations as last shown (the overlay draws their marks).
    items: RefCell<Vec<Annotation>>,
    /// (id, comment entry, delete button) per row, for `debug.browser`.
    rows: RefCell<Vec<(u64, gtk::Entry, gtk::Button)>>,
    pushed: Cell<bool>,
}

impl Panel {
    pub fn new() -> Self {
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
        b.add_css_class("rade-browser-annotations");
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let title = gtk::Label::new(Some("Annotations"));
        title.add_css_class("heading");
        let hint = gtk::Label::new(None);
        hint.set_xalign(0.0);
        hint.set_hexpand(true);
        hint.set_wrap(true);
        hint.add_css_class("dim-label");
        hint.add_css_class("caption");
        let note = gtk::Label::new(None);
        note.add_css_class("caption");
        note.add_css_class("rade-annotation-note");
        let clear = gtk::Button::with_label("Clear");
        clear.set_tooltip_text(Some("Delete every pending annotation"));
        let send = gtk::Button::with_label("Send to Agent");
        send.add_css_class("rade-annotation-send");
        top.append(&title);
        top.append(&hint);
        top.append(&note);
        top.append(&clear);
        top.append(&send);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("rade-annotation-list");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(220);
        scroll.set_child(Some(&list));
        b.append(&top);
        b.append(&scroll);
        revealer.set_child(Some(&b));
        Panel {
            revealer,
            list,
            hint,
            note,
            send,
            clear,
            sig: RefCell::default(),
            items: RefCell::default(),
            rows: RefCell::default(),
            pushed: Cell::new(false),
        }
    }
}

/// `$XDG_CACHE_HOME/rade/annotations`: the crops (cleaned after 7 days
/// like pasted images, R5.4).
pub fn shots_dir() -> PathBuf {
    let paste = rade_core::paths::paste_dir(&rade_core::paths::process_env);
    paste.parent().unwrap_or(&paste).join("annotations")
}

/// Cut `pick`'s box (+8 px) out of a screenshot of the viewport and save
/// it as PNG.
fn save_crop(tex: &gdk::Texture, pick: &Pick, path: &Path) -> Result<(), String> {
    let (w, h) = (tex.width() as u32, tex.height() as u32);
    let (x, y, cw, ch) = crop_box(pick.rect, 8.0, pick.viewport.w, w, h)
        .ok_or_else(|| "the box is outside the page".to_string())?;
    let mut d = gdk::TextureDownloader::new(tex);
    d.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = d.download_bytes();
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for row in y..y + ch {
        let start = row as usize * stride + x as usize * 4;
        out.extend_from_slice(&bytes[start..start + cw as usize * 4]);
    }
    let t = gdk::MemoryTexture::new(
        cw as i32,
        ch as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(out),
        cw as usize * 4,
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    t.save_to_png(path).map_err(|e| e.to_string())
}

impl BrowserView {
    pub(crate) fn connect_annotate(self: &Rc<Self>, ucm: &webkit6::UserContentManager) {
        let this = Rc::downgrade(self);
        ucm.connect_script_message_received(Some("radeAnnotate"), move |_, msg| {
            if let Some(v) = this.upgrade() {
                v.on_message(&msg.to_str());
            }
        });
        let this = Rc::downgrade(self);
        self.annotate.connect_toggled(move |b| {
            let Some(v) = this.upgrade() else { return };
            v.refresh_panel();
            v.push_overlay();
            if b.is_active() {
                v.web.grab_focus();
            }
        });
        let id = self.id.clone();
        self.panel
            .send
            .connect_clicked(move |_| send_from_view(&id));
        let id = self.id.clone();
        self.panel.clear.connect_clicked(move |_| {
            if let Some(app) = try_app() {
                let _ = app.update(|m| m.take_annotations(&id).map(drop));
            }
        });
    }

    pub fn annotating(&self) -> bool {
        self.annotate.is_active()
    }

    /// The overlay's state in the page: mode and this page's marks.
    pub(crate) fn push_overlay(&self) {
        let on = self.annotating();
        // Off and never shown: nothing to remove from the page.
        if !on && !self.panel.pushed.get() {
            return;
        }
        self.panel.pushed.set(on);
        let items: Vec<Value> = self
            .panel
            .items
            .borrow()
            .iter()
            .map(|a| json!({"n": a.id, "url": a.url, "selector": a.selector, "kind": a.kind, "bbox": a.bbox}))
            .collect();
        self.automation("annotate", &[json!({"on": on, "items": items})], |_| {});
    }

    fn refresh_panel(&self) {
        let n = self.panel.items.borrow().len();
        let on = self.annotating();
        self.panel.revealer.set_reveal_child(on || n > 0);
        self.panel.hint.set_label(if on {
            "Click or drag to annotate · Ctrl+↓/↑ selects · Ctrl+Enter picks · Ctrl+Esc ends"
        } else {
            "Annotate is off (Ctrl+Shift+A)"
        });
        self.panel.send.set_label(&match n {
            0 => "Send to Agent".to_string(),
            n => format!("Send to Agent ({n})"),
        });
        self.panel.send.set_sensitive(n > 0);
        // A disabled primary button reads as an error (R20.1).
        if n > 0 {
            self.panel.send.add_css_class("suggested-action");
        } else {
            self.panel.send.remove_css_class("suggested-action");
        }
        self.panel.clear.set_sensitive(n > 0);
    }

    /// The model's Annotations of this Browser (every sync; cheap unless
    /// they changed).
    pub fn show_annotations(&self, list: &[Annotation]) {
        let sig: Vec<(u64, String)> = list.iter().map(|a| (a.id, a.label())).collect();
        if sig == *self.panel.sig.borrow() {
            return;
        }
        self.panel.sig.replace(sig);
        self.panel.items.replace(list.to_vec());
        while let Some(r) = self.panel.list.row_at_index(0) {
            self.panel.list.remove(&r);
        }
        let mut rows = Vec::new();
        for a in list {
            let (row, entry, del) = self.row(a);
            self.panel.list.append(&row);
            rows.push((a.id, entry, del));
        }
        self.panel.rows.replace(rows);
        self.refresh_panel();
        self.push_overlay();
    }

    fn row(&self, a: &Annotation) -> (gtk::Widget, gtk::Entry, gtk::Button) {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        b.add_css_class("rade-annotation-row");
        let n = gtk::Label::new(Some(&a.id.to_string()));
        n.add_css_class("rade-annotation-n");
        n.set_valign(gtk::Align::Center);
        n.set_halign(gtk::Align::Center);
        b.append(&n);
        if let Some(shot) = a.shot.as_ref().filter(|p| p.exists()) {
            // Scaled to fit 96×56, aspect kept (a wide crop stays readable).
            let pb = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(shot, 96, 56, true).ok();
            let pic = gtk::Picture::new();
            if let Some(pb) = pb {
                pic.set_paintable(Some(&gdk::Texture::for_pixbuf(&pb)));
            }
            pic.set_can_shrink(false);
            pic.set_valign(gtk::Align::Center);
            pic.add_css_class("rade-annotation-thumb");
            pic.set_tooltip_text(Some(&shot.display().to_string()));
            b.append(&pic);
        }
        let col = gtk::Box::new(gtk::Orientation::Vertical, 2);
        col.set_hexpand(true);
        let what = gtk::Label::new(Some(&format!("{} · {}", a.label(), short_url(&a.url))));
        what.set_xalign(0.0);
        what.set_ellipsize(gtk::pango::EllipsizeMode::End);
        what.add_css_class("caption-heading");
        what.set_tooltip_text(Some(&format!("{}\n{}", a.selector, a.url)));
        let entry = gtk::Entry::new();
        entry.set_text(&a.comment);
        entry.update_property(&[gtk::accessible::Property::Label(&format!(
            "Annotation {} comment",
            a.id
        ))]);
        entry.set_tooltip_text(Some(&format!("Annotation {} comment", a.id)));
        entry.add_css_class("rade-annotation-text");
        let (pane, id) = (self.id.clone(), a.id);
        entry.connect_changed(move |e| {
            let t = e.text();
            if let (false, Some(app)) = (t.trim().is_empty(), try_app()) {
                app.touch(|m| m.edit_annotation(&pane, id, &t).ok());
            }
        });
        col.append(&what);
        col.append(&entry);
        b.append(&col);
        let del = gtk::Button::from_icon_name("user-trash-symbolic");
        del.add_css_class("flat");
        del.set_valign(gtk::Align::Center);
        del.set_tooltip_text(Some("Delete annotation"));
        del.update_property(&[gtk::accessible::Property::Label(&format!(
            "Delete annotation {}",
            a.id
        ))]);
        let pane = self.id.clone();
        del.connect_clicked(move |_| {
            if let Some(app) = try_app() {
                let _ = app.update(|m| m.edit_annotation(&pane, id, ""));
            }
        });
        b.append(&del);
        (b.upcast(), entry, del)
    }

    fn on_message(self: &Rc<Self>, text: &str) {
        let Ok(m) = serde_json::from_str::<Value>(text) else {
            return;
        };
        match m["op"].as_str() {
            Some("toggle") => self.annotate.set_active(m["on"] == true),
            Some("add") => {
                if let Ok(p) = serde_json::from_value::<Pick>(m["pick"].clone()) {
                    self.add_pick(p);
                }
            }
            _ => {}
        }
    }

    /// Crop the screenshot, then keep the Annotation on this Browser.
    fn add_pick(self: &Rc<Self>, pick: Pick) {
        if pick.comment.trim().is_empty() || !self.annotating() {
            return;
        }
        let errors: Vec<String> = {
            let e = self.errors.borrow();
            e.iter()
                .skip(e.len().saturating_sub(MAX_ERRORS))
                .cloned()
                .collect()
        };
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = shots_dir().join(format!("{}-{millis}.png", self.id));
        let id = self.id.clone();
        self.web.snapshot(
            webkit6::SnapshotRegion::Visible,
            webkit6::SnapshotOptions::NONE,
            gtk::gio::Cancellable::NONE,
            move |r| {
                let shot = match r
                    .map_err(|e| e.to_string())
                    .and_then(|t| save_crop(&t, &pick, &path))
                {
                    Ok(()) => Some(path),
                    Err(e) => {
                        eprintln!("rade: annotation screenshot: {e}");
                        None
                    }
                };
                let a = Annotation::from_pick(0, pick, shot, errors);
                if let Some(app) = try_app() {
                    if let Err(e) = app.update(|m| m.add_annotation(&id, a)) {
                        eprintln!("rade: annotation: {}", e.message);
                    }
                }
            },
        );
    }
}

fn short_url(u: &str) -> String {
    let rest = u.split_once("://").map_or(u, |x| x.1);
    rest.chars().take(60).collect()
}

/// Send a Browser's Annotations to an Agent (R71.4): `pane`, else the
/// Agent in the Browser's Tab, else the Worktree's one; none → spawn
/// Preset `agent`. Then they are cleared.
pub fn send(
    browser: &str,
    pane: Option<String>,
    agent: Option<String>,
    caller: Option<&str>,
) -> rade_core::rpc::Result<Value> {
    let app = crate::app::app();
    let (loc, list) = {
        let m = app.model.borrow();
        let loc = m
            .locate(browser)
            .ok_or_else(|| rade_core::rpc::Error::not_found(format!("no pane {browser}")))?;
        (loc, m.annotations(browser)?.to_vec())
    };
    if list.is_empty() {
        return Err(rade_core::rpc::Error::usage(
            "no Browser Annotations to send",
        ));
    }
    let prompt = rade_core::annotate::compose(&list, false);
    let (target, spawned) = crate::review::deliver(
        &app,
        &loc.source,
        &loc.worktree,
        Some(&loc.tab),
        pane,
        agent,
        caller,
        &prompt,
    )?;
    app.update(|m| m.take_annotations(browser).map(drop))?;
    Ok(
        json!({"pane": target, "browser": browser, "annotations": list.len(), "prompt": prompt, "spawned": spawned}),
    )
}

/// The panel's Send to Agent: one Agent → send; several → choose; none →
/// offer to spawn one.
pub fn send_from_view(browser: &str) {
    let app = crate::app::app();
    let Some(loc) = app.model.borrow().locate(browser) else {
        return;
    };
    let b = browser.to_string();
    crate::review::choose_target(
        &app,
        &loc.source,
        &loc.worktree,
        Some(&loc.tab),
        "annotations",
        move |pane, preset| {
            let r = send(&b, pane, preset, None);
            let app = crate::app::app();
            let v = app.browsers.borrow().get(&b).cloned();
            if let Some(v) = v {
                v.panel.note.set_label(&match r {
                    Ok(x) => {
                        let pane = x["pane"].as_str().unwrap_or("?");
                        let name = app.model.borrow().pane(pane).and_then(|p| p.name.clone());
                        format!(
                            "Sent {} to {}",
                            x["annotations"],
                            name.map_or(pane.to_string(), |n| format!("{n} ({pane})"))
                        )
                    }
                    Err(e) => e.message,
                });
            }
        },
    );
}

impl BrowserView {
    /// `debug.browser` state of the panel and toggle.
    pub(crate) fn debug_state(&self) -> Value {
        let rows: Vec<Value> = self
            .panel
            .rows
            .borrow()
            .iter()
            .map(|(id, e, _)| json!({"id": id, "comment": e.text().to_string()}))
            .collect();
        json!({
            "pane": self.id, "engine": "webkit", "status": self.browser_status(), "url": self.preview_url(), "annotating": self.annotating(),
            "panel": self.panel.revealer.reveals_child(),
            "send": self.panel.send.label().map(|l| l.to_string()),
            "send_sensitive": self.panel.send.is_sensitive(),
            "note": self.panel.note.label().to_string(),
            "hint": self.panel.hint.label().to_string(),
            "rows": rows,
        })
    }

    /// `debug.browser edit|delete`: type in a row's comment / click its
    /// delete button.
    pub(crate) fn debug_row(&self, id: u64, text: Option<&str>) -> bool {
        let rows = self.panel.rows.borrow();
        let Some((_, e, d)) = rows.iter().find(|r| r.0 == id) else {
            return false;
        };
        let (e, d) = (e.clone(), d.clone());
        drop(rows);
        match text {
            Some(t) => e.set_text(t),
            None => d.emit_clicked(),
        }
        true
    }
}
