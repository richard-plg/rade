//! What a Changes card's buttons, keys and menus do (PRD R120, R121): stage,
//! unstage, discard to the trash, the commit variants and Undo, Push /
//! Publish / Sync / Fetch / Pull, Create PR, Draft, copy path, Add to
//! .gitignore, open the diff or the file. git and `gh` run off the GTK
//! thread and only on a click (or the palette action standing for it);
//! Push, Publish, Sync, Discard and Create PR ask first (ADR-0013). The git
//! work itself is `rade_core::scm`.

use crate::app::try_app;
use crate::changes_card::{plural, text_of, Card};
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::rpc::{Error, Result};
use rade_core::scm::{self, Change, CommitKind, DraftKind, PushPlan, Section};
use rade_core::scm_view::{self, Group, Item};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The open Create PR dialog's fields (`debug.scm`).
pub(crate) struct PrForm {
    pub title: gtk::Entry,
    pub base: gtk::Entry,
    pub body: gtk::TextView,
    pub generate: gtk::Button,
    pub note: gtk::Label,
}

pub(crate) fn toast(markup: &str) {
    if let Some(app) = try_app() {
        app.chrome.toast(markup, None);
    }
}

fn esc(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

/// Ask before a destructive or publishing action (ADR-0013).
pub(crate) fn confirm(
    heading: &str,
    body: &str,
    id: &'static str,
    label: &str,
    yes: impl Fn() + 'static,
) {
    let d = adw::AlertDialog::new(Some(heading), Some(body));
    d.add_response("cancel", "Cancel");
    d.add_response(id, label);
    d.set_response_appearance(
        id,
        if matches!(id, "discard" | "delete") {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == id {
            yes();
        }
    });
    crate::menus::present(d, &["cancel", id]);
}

/// The paths a set of rows touches (a rename's source too), once each.
fn paths_of(items: &[(Group, Item)]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (_, it) in items {
        for p in std::iter::once(&it.path).chain(it.orig.as_ref()) {
            if !v.contains(p) {
                v.push(p.clone());
            }
        }
    }
    v
}

impl Card {
    /// The Workspace env of this card's Source for push / pull / gh / Draft
    /// (R23.3).
    pub(crate) fn env(&self) -> Vec<(String, String)> {
        let Some(app) = try_app() else {
            return Vec::new();
        };
        let m = app.model.borrow();
        // A Worktree's folder first; a Project checkout is no Worktree.
        let env = crate::app::workspace_env_at(&m, &self.path);
        if env.is_empty() {
            crate::app::workspace_env(&m, &self.source)
        } else {
            env
        }
    }

    /// Run git work off the GTK thread, then `ok` with its result; an error
    /// shows in the card, a success clears it (R120). The page refreshes
    /// either way.
    pub(crate) fn run<T: Send + 'static>(
        self: &Rc<Self>,
        what: &'static str,
        work: impl FnOnce(PathBuf) -> Result<T> + Send + 'static,
        ok: impl FnOnce(&Rc<Self>, T) + 'static,
    ) {
        self.ops.set(self.ops.get() + 1);
        self.update_commit();
        let root = self.path.clone();
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || work(root),
            move |r| {
                let Some(c) = w.upgrade() else { return };
                c.ops.set(c.ops.get() - 1);
                match r {
                    Ok(v) => {
                        c.show_error(None);
                        ok(&c, v);
                    }
                    Err(e) => c.show_error(Some(format!("{what}: {}", e.message))),
                }
                c.update_commit();
                crate::changes::refresh_soon();
            },
        );
    }

    // ---- stage, unstage, discard ---------------------------------------------------

    fn group_items(&self, group: Group) -> Vec<(Group, Item)> {
        self.snap()
            .map(|s| {
                scm_view::groups(&s.status)
                    .into_iter()
                    .filter(|(g, _)| *g == group)
                    .flat_map(|(g, items)| items.into_iter().map(move |i| (g, i)))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn stage_items(self: &Rc<Self>, items: &[(Group, Item)]) {
        let paths = paths_of(items);
        if paths.is_empty() {
            return;
        }
        self.run("Stage", move |r| scm::stage(&r, &paths), |_, _| ());
    }

    pub(crate) fn unstage_items(self: &Rc<Self>, items: &[(Group, Item)]) {
        let paths = paths_of(items);
        if paths.is_empty() {
            return;
        }
        let initial = self.snap().is_some_and(|s| s.status.initial);
        self.run(
            "Unstage",
            move |r| scm::unstage(&r, &paths, initial),
            |_, _| (),
        );
    }

    pub(crate) fn stage_group(self: &Rc<Self>, group: Group) {
        self.stage_items(&self.group_items(group));
    }

    pub(crate) fn unstage_group(self: &Rc<Self>) {
        self.unstage_items(&self.group_items(Group::Staged));
    }

    pub(crate) fn discard_group(self: &Rc<Self>) {
        self.discard_items(&self.group_items(Group::Changes));
    }

    /// Stage All Changes of the `…` menu: the Merge and Changes groups.
    pub(crate) fn stage_all(self: &Rc<Self>) {
        let mut items = self.group_items(Group::Merge);
        items.extend(self.group_items(Group::Changes));
        self.stage_items(&items);
    }

    pub(crate) fn unstage_all(self: &Rc<Self>) {
        self.unstage_group();
    }

    pub(crate) fn discard_all(self: &Rc<Self>) {
        self.discard_group();
    }

    fn folder_items(&self, group: Group, prefix: &str) -> Vec<(Group, Item)> {
        self.group_items(group)
            .into_iter()
            .filter(|(_, i)| i.path.starts_with(prefix))
            .collect()
    }

    pub(crate) fn stage_folder(self: &Rc<Self>, group: Group, prefix: &str) {
        self.stage_items(&self.folder_items(group, prefix));
    }

    pub(crate) fn unstage_folder(self: &Rc<Self>, group: Group, prefix: &str) {
        self.unstage_items(&self.folder_items(group, prefix));
    }

    pub(crate) fn discard_folder(self: &Rc<Self>, group: Group, prefix: &str) {
        self.discard_items(&self.folder_items(group, prefix));
    }

    /// Discard (R120.4): the unstaged changes of tracked files, untracked
    /// files to the trash. One dialog names the count; only rows of the
    /// Changes group are discarded.
    pub(crate) fn discard_items(self: &Rc<Self>, items: &[(Group, Item)]) {
        let Some(snap) = self.snap() else { return };
        let mut tracked: Vec<Change> = Vec::new();
        let mut untracked: Vec<String> = Vec::new();
        for (g, it) in items {
            if *g != Group::Changes {
                continue;
            }
            if it.section == Some(Section::Untracked) {
                untracked.push(it.path.clone());
            } else if let Some(c) = snap
                .status
                .files
                .iter()
                .find(|c| c.path == it.path && !c.untracked())
            {
                tracked.push(c.clone());
            }
        }
        let n = tracked.len() + untracked.len();
        if n == 0 {
            return;
        }
        let names: Vec<&str> = tracked
            .iter()
            .map(|c| c.path.rsplit('/').next().unwrap_or(&c.path))
            .chain(untracked.iter().map(|p| p.rsplit('/').next().unwrap_or(p)))
            .collect();
        let (heading, body, button) =
            scm_view::discard_dialog(&names, tracked.len(), untracked.len());
        let w = Rc::downgrade(self);
        confirm(&heading, &body, "discard", &button, move || {
            let Some(me) = w.upgrade() else { return };
            let (tracked, untracked) = (tracked.clone(), untracked.clone());
            me.run(
                "Discard",
                move |r| {
                    let trash = |p: &Path| {
                        gio::File::for_path(p)
                            .trash(gio::Cancellable::NONE)
                            .map_err(|e| std::io::Error::other(e.to_string()))
                    };
                    scm::discard_many(&r, &tracked, &untracked, &trash)
                },
                |c, failed| {
                    if !failed.is_empty() {
                        c.trash_failed(failed);
                    }
                },
            );
        });
    }

    /// D7: the trash failed; offer to delete the files permanently.
    fn trash_failed(self: &Rc<Self>, failed: Vec<(String, String)>) {
        let n = failed.len();
        let first = failed[0].1.clone();
        let paths: Vec<String> = failed.into_iter().map(|(p, _)| p).collect();
        let w = Rc::downgrade(self);
        confirm(
            &format!("Can't move {} to the trash", plural(n, "file", "files")),
            &format!("{first}. Delete permanently?"),
            "delete",
            "Delete Permanently",
            move || {
                if let Some(me) = w.upgrade() {
                    let paths = paths.clone();
                    me.run(
                        "Delete",
                        move |r| scm::delete_untracked(&r, &paths),
                        |_, _| (),
                    );
                }
            },
        );
    }

    // ---- open, copy, ignore -----------------------------------------------------------

    /// Open the diff in the Diff Pane (R122.1); the keyboard stays where it
    /// was unless `focus`.
    pub(crate) fn open_diff(self: &Rc<Self>, path: &str, section: Section, focus: bool) {
        let Some(app) = try_app() else { return };
        let (s, w) = {
            let m = app.model.borrow();
            let (s, w) = m.active();
            (s.id.clone(), w.id.clone())
        };
        let prev = gtk::prelude::GtkWindowExt::focus(&app.window);
        let root = self.path.clone();
        let r = app.update(|m| m.open_diff(&s, &w, &root, path, section, focus));
        match r {
            Ok(_) => {
                // The sync that shows the Pane would hand the keyboard to
                // the focused Pane: it stays in the list.
                if !focus {
                    if let Some(p) = prev {
                        p.grab_focus();
                    }
                }
            }
            Err(e) => self.show_error(Some(format!("Open Changes: {}", e.message))),
        }
    }

    pub(crate) fn open_file(self: &Rc<Self>, path: &str, side: bool) {
        let p = self.path.join(path);
        if side {
            crate::files::open_side(&p);
        } else {
            crate::files::open_in_viewer(&p);
        }
    }

    pub(crate) fn copy_paths(self: &Rc<Self>, items: &[(Group, Item)], relative: bool) {
        let paths: Vec<String> = items
            .iter()
            .map(|(_, i)| {
                if relative {
                    i.path.clone()
                } else {
                    self.path.join(&i.path).display().to_string()
                }
            })
            .collect();
        let Some(first) = paths.first().cloned() else {
            return;
        };
        if let Some(d) = gtk::gdk::Display::default() {
            d.clipboard().set_text(&paths.join("\n"));
        }
        toast(&if paths.len() == 1 {
            format!("Copied {}", esc(&first))
        } else {
            format!("Copied {}", plural(paths.len(), "path", "paths"))
        });
    }

    /// Add to .gitignore (R120.5): `/<path>` appended for each untracked row.
    pub(crate) fn gitignore_items(self: &Rc<Self>, items: &[(Group, Item)]) {
        let paths: Vec<String> = items
            .iter()
            .filter(|(_, i)| i.section == Some(Section::Untracked))
            .map(|(_, i)| i.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        self.run(
            "Add to .gitignore",
            {
                let paths = paths.clone();
                move |r| paths.iter().try_for_each(|p| scm::gitignore_add(&r, p))
            },
            move |_, _| {
                for p in &paths {
                    toast(&format!("Added /{} to .gitignore", esc(p)));
                }
            },
        );
    }

    // ---- commit -----------------------------------------------------------------------

    /// The Commit button / Ctrl+Enter (R121.2).
    pub(crate) fn commit_clicked(self: &Rc<Self>) {
        let s = self.commit_state();
        if !scm_view::commit_enabled(&s) || self.busy() {
            if !s.message {
                self.show_error(Some(
                    "Commit: write a message first (or Generate one)".into(),
                ));
            } else if !self.busy() {
                self.show_error(Some("Commit: nothing is staged".into()));
            }
            return;
        }
        self.commit_flow(CommitKind::Normal, false);
    }

    /// A Commit ▾ item: `amend`, `push` (Commit & Push) or `all`.
    pub(crate) fn commit_variant(self: &Rc<Self>, variant: &str) {
        match variant {
            "amend" => self.commit_flow(CommitKind::Amend, false),
            "all" => self.commit_flow(CommitKind::All, false),
            "push" => self.commit_flow(CommitKind::Normal, true),
            _ => self.commit_flow(CommitKind::Normal, false),
        }
    }

    fn commit_flow(self: &Rc<Self>, kind: CommitKind, push: bool) {
        let s = self.commit_state();
        if kind == CommitKind::Normal && scm_view::smart_prompt(&s) {
            let w = Rc::downgrade(self);
            confirm(
                &format!("Stage all {} changes and commit?", s.changes),
                &format!(
                    "Nothing is staged. Every change in {} will be committed.",
                    self.name
                ),
                "commit",
                "Stage All & Commit",
                move || {
                    if let Some(me) = w.upgrade() {
                        me.do_commit(CommitKind::All, push);
                    }
                },
            );
            return;
        }
        self.do_commit(kind, push);
    }

    fn do_commit(self: &Rc<Self>, kind: CommitKind, push: bool) {
        let msg = text_of(&self.message);
        self.run(
            "Commit",
            move |r| scm::commit_kind(&r, &msg, kind),
            move |c, done| {
                c.set_message("");
                c.st.borrow_mut().history = None;
                let (short, subject) = done.split_once(' ').unwrap_or((&done, ""));
                crate::activity_view::committed(&c.path, &c.name, short, subject);
                toast(&format!("Committed <b>{}</b>", esc(&done)));
                if push {
                    c.push_flow();
                }
            },
        );
    }

    /// Undo Last Commit (R121.2): the undone message fills the box.
    pub(crate) fn undo(self: &Rc<Self>) {
        let head = self.snap().and_then(|s| s.head);
        self.run(
            "Undo",
            |r| scm::undo_last_commit(&r),
            move |c, msg| {
                c.set_message(&msg);
                c.st.borrow_mut().history = None;
                let done = head.map_or(String::new(), |(h, s)| format!("{h} {s}"));
                toast(&format!("Undid <b>{}</b>", esc(&done)));
            },
        );
    }

    // ---- remotes ----------------------------------------------------------------------

    pub(crate) fn pull(self: &Rc<Self>) {
        let env = self.env();
        self.run("Pull", move |r| scm::pull(&r, &env), |_, _| toast("Pulled"));
    }

    /// Fetch (R121.4): the own upstream's remote, else the only / `origin`.
    pub(crate) fn fetch(self: &Rc<Self>, manual: bool) {
        let env = self.env();
        self.run(
            "Fetch",
            move |r| {
                let st = scm::status(&r)?;
                let remote = scm::fetch_remote(&r, &st)?;
                scm::fetch(&r, &remote, &env)?;
                Ok(remote)
            },
            move |_, remote| {
                if manual {
                    toast(&format!("Fetched {}", esc(&remote)));
                }
            },
        );
    }

    pub(crate) fn push_flow(self: &Rc<Self>) {
        self.run(
            "Push",
            |r| scm::push_plan(&r, &scm::status(&r)?),
            |c, plan| c.confirm_push(plan, false),
        );
    }

    /// Publish Branch (D13): the Push confirmation with `-u`.
    pub(crate) fn publish_flow(self: &Rc<Self>) {
        self.run(
            "Publish",
            |r| scm::push_plan(&r, &scm::status(&r)?),
            |c, plan| c.confirm_push(plan, true),
        );
    }

    fn confirm_push(self: &Rc<Self>, plan: PushPlan, publish: bool) {
        let n = plan.commits;
        let mut body = format!(
            "{n} commit{} on {} → {}/{}.",
            if n == 1 { "" } else { "s" },
            plan.branch,
            plan.remote,
            plan.branch
        );
        if plan.set_upstream {
            body.push_str(&format!(
                "\nSets {}/{} as the branch's upstream.",
                plan.remote, plan.branch
            ));
        }
        let (heading, button) = if publish {
            (
                format!("Publish {} to {}?", plan.branch, plan.remote),
                "Publish",
            )
        } else {
            (format!("Push to {}?", plan.remote), "Push")
        };
        let w = Rc::downgrade(self);
        confirm(&heading, &body, "push", button, move || {
            let Some(me) = w.upgrade() else { return };
            let plan = plan.clone();
            let env = me.env();
            me.run(
                if publish { "Publish" } else { "Push" },
                move |r| scm::push(&r, &plan, &env).map(|_| plan),
                move |_, p| {
                    toast(&if publish {
                        format!("Published <b>{}</b> to {}", esc(&p.branch), esc(&p.remote))
                    } else {
                        format!("Pushed <b>{}</b> to {}", esc(&p.branch), esc(&p.remote))
                    })
                },
            );
        });
    }

    /// Sync Changes (D13): one confirmation, then pull `--ff-only` and push.
    pub(crate) fn sync_flow(self: &Rc<Self>) {
        self.run(
            "Sync",
            |r| scm::status(&r),
            |c, st| {
                let Some(branch) = st.branch.clone() else {
                    c.show_error(Some("Sync: HEAD is detached: check out a branch".into()));
                    return;
                };
                let Some((remote, _)) = st.own_upstream() else {
                    // No upstream of its own: Publish sets one.
                    c.publish_flow();
                    return;
                };
                let remote = remote.to_string();
                let body = format!(
                    "Pulls {} from {remote}/{branch}, then pushes {}.",
                    plural(st.behind as usize, "commit", "commits"),
                    plural(st.ahead as usize, "commit", "commits"),
                );
                let w = Rc::downgrade(c);
                confirm(
                    &format!("Sync {branch} with {remote}?"),
                    &body,
                    "sync",
                    "Sync",
                    move || {
                        let Some(me) = w.upgrade() else { return };
                        let env = me.env();
                        let (branch, remote) = (branch.clone(), remote.clone());
                        me.run(
                            "Sync",
                            move |r| scm::sync(&r, &env).map(|_| ()),
                            move |_, _| {
                                toast(&format!(
                                    "Synced <b>{}</b> with {}",
                                    esc(&branch),
                                    esc(&remote)
                                ))
                            },
                        );
                    },
                );
            },
        );
    }

    // ---- Create PR ---------------------------------------------------------------------

    pub(crate) fn pr_clicked(self: &Rc<Self>) {
        // A Worktree's recorded base (R28.3) is its PR's base.
        let wt_base = try_app().and_then(|a| a.model.borrow().comparison_base(&self.path));
        self.run(
            "Create PR",
            move |r| {
                let st = scm::status(&r)?;
                let branch = st
                    .branch
                    .clone()
                    .ok_or_else(|| Error::usage("HEAD is detached: check out a branch"))?;
                let base = scm::pr_base(
                    wt_base
                        // A Fan-out Worktree's base is a commit: not a PR base.
                        .filter(|b| !(b.len() >= 7 && b.bytes().all(|c| c.is_ascii_hexdigit())))
                        .or_else(|| rade_core::git::default_branch(&r))
                        .as_deref(),
                );
                let plan = if st.own_upstream().is_none() || st.ahead > 0 {
                    Some(scm::push_plan(&r, &st)?)
                } else {
                    None
                };
                Ok((branch, base, plan))
            },
            |c, (branch, base, plan)| c.pr_dialog(branch, base, plan),
        );
    }

    /// Create PR (R21.6): the form is the confirmation.
    fn pr_dialog(self: &Rc<Self>, branch: String, base: String, plan: Option<PushPlan>) {
        let title = gtk::Entry::new();
        title.set_placeholder_text(Some("Title"));
        title.set_activates_default(true);
        let base_e = gtk::Entry::new();
        base_e.set_text(&base);
        let base_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let bl = gtk::Label::new(Some(&format!("{branch} →")));
        bl.add_css_class("rade-dim");
        base_e.set_hexpand(true);
        base_e.set_tooltip_text(Some("Base branch"));
        base_row.append(&bl);
        base_row.append(&base_e);
        let body = gtk::TextView::new();
        body.set_wrap_mode(gtk::WrapMode::WordChar);
        body.set_top_margin(6);
        body.set_left_margin(6);
        body.set_right_margin(6);
        body.add_css_class("rade-commit-msg");
        let bs = gtk::ScrolledWindow::new();
        bs.set_child(Some(&body));
        bs.set_min_content_height(160);
        bs.set_min_content_width(420);
        bs.add_css_class("rade-commit-box");
        let generate = gtk::Button::new();
        generate.set_child(Some(
            &adw::ButtonContent::builder()
                .icon_name("document-edit-symbolic")
                .label("Generate")
                .build(),
        ));
        generate.set_halign(gtk::Align::Start);
        generate.set_tooltip_text(Some(
            "Ask an Agent to draft the title and description from the branch's diff",
        ));
        let note = gtk::Label::new(Some(&match &plan {
            Some(p) if p.set_upstream => format!(
                "Pushes {branch} to {} first ({} commit{}, sets its upstream).",
                p.remote,
                p.commits,
                if p.commits == 1 { "" } else { "s" }
            ),
            Some(p) => format!(
                "Pushes {} commit{} to {} first.",
                p.commits,
                if p.commits == 1 { "" } else { "s" },
                p.remote
            ),
            None => "The branch is pushed.".into(),
        }));
        note.add_css_class("rade-meta");
        note.set_wrap(true);
        note.set_xalign(0.0);
        let label = |t: &str| {
            let l = gtk::Label::new(Some(t));
            l.add_css_class("rade-meta");
            l.set_xalign(0.0);
            l
        };
        let form = gtk::Box::new(gtk::Orientation::Vertical, 6);
        form.append(&label("Title"));
        form.append(&title);
        form.append(&label("Base branch"));
        form.append(&base_row);
        form.append(&label("Description"));
        form.append(&bs);
        form.append(&generate);
        form.append(&note);
        let w = Rc::downgrade(self);
        let (t2, b2) = (title.clone(), body.clone());
        generate.connect_clicked(move |_| {
            let Some(c) = w.upgrade() else { return };
            let (t, b) = (t2.clone(), b2.clone());
            c.run_draft(DraftKind::Pr, move |title, body| {
                t.set_text(&title);
                b.buffer().set_text(&body);
            });
        });
        let d = adw::AlertDialog::new(Some("Create Pull Request"), None);
        d.set_extra_child(Some(&form));
        d.add_response("cancel", "Cancel");
        d.add_response("create", "Create");
        d.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        d.set_default_response(Some("create"));
        d.set_close_response("cancel");
        let w = Rc::downgrade(self);
        let (t3, b3, base3) = (title.clone(), body.clone(), base_e.clone());
        d.connect_response(None, move |_, r| {
            let Some(c) = w.upgrade() else { return };
            c.pr_form.borrow_mut().take();
            if r != "create" {
                return;
            }
            let (title, body, base) = (
                t3.text().to_string(),
                text_of(&b3),
                base3.text().to_string(),
            );
            let (branch, plan) = (branch.clone(), plan.clone());
            let env = c.env();
            c.run(
                "Create PR",
                move |r| {
                    if let Some(p) = &plan {
                        scm::push(&r, p, &env)?;
                    }
                    scm::create_pr(&r, "gh", &branch, base.trim(), &title, &body, &env)
                },
                move |c, url| {
                    crate::activity_view::pr_created(&c.path, &url);
                    let u = url.clone();
                    crate::app::app().chrome.toast(
                        &format!("Pull request created: {}", esc(&url)),
                        Some(("Open", Box::new(move || crate::viewer::launch_uri(&u)))),
                    );
                },
            );
        });
        *self.pr_form.borrow_mut() = Some(PrForm {
            title: title.clone(),
            base: base_e,
            body,
            generate,
            note,
        });
        crate::menus::present(d, &["cancel", "create"]);
        title.grab_focus();
    }

    // ---- Draft -------------------------------------------------------------------------

    /// Generate (R21.7): the Draft command's reply fills `done`; a second
    /// click cancels it.
    pub(crate) fn run_draft(
        self: &Rc<Self>,
        kind: DraftKind,
        done: impl FnOnce(String, String) + 'static,
    ) {
        if let Some(flag) = self.drafting.borrow_mut().take() {
            flag.store(true, Ordering::Relaxed);
            return;
        }
        let Some(app) = try_app() else { return };
        let root = self.path.clone();
        // The Worktree's layered `[scm] draft_command` (R29.2).
        let at = app
            .model
            .borrow()
            .worktrees()
            .find(|(_, w)| w.path == root)
            .map(|(s, w)| (s.id.clone(), w.id.clone()));
        let command = match &at {
            Some((s, w)) => app.config_for(s, Some(w)).scm.draft_command,
            None => app.config.borrow().scm.draft_command.clone(),
        };
        let wt_base = app.model.borrow().comparison_base(&root);
        let env = self.env();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.drafting.borrow_mut() = Some(cancel.clone());
        self.show_error(None);
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                let base = wt_base.or_else(|| rade_core::git::default_branch(&root));
                let input = scm::gather_draft(&root, kind, base.as_deref())?;
                let prompt = scm::draft_prompt(kind, &input);
                scm::run_draft(&command, &root, &prompt, scm::DRAFT_TIMEOUT, &cancel, &env)
                    .map(|out| scm::parse_draft(kind, &out))
            },
            move |r: Result<(String, String)>| {
                let Some(c) = w.upgrade() else { return };
                c.drafting.borrow_mut().take();
                match r {
                    Ok((a, b)) => done(a, b),
                    Err(e) => c.show_error(Some(format!("Generate: {}", e.message))),
                }
            },
        );
    }

    /// The sparkle button: Draft the commit message from the staged diff.
    pub(crate) fn generate(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        self.run_draft(DraftKind::Commit, move |subject, body| {
            if let Some(c) = w.upgrade() {
                let text = if body.is_empty() {
                    subject
                } else {
                    format!("{subject}\n\n{body}")
                };
                c.set_message(&text);
                c.message.grab_focus();
            }
        });
    }
}
