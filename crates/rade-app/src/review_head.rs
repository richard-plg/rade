//! The Review Pane's header (PRD R118.1): line 1 says whose work it is —
//! the branch and its base, the Worktree's Agent (glyph, name, `+n`), its
//! task, `n commits ▾` — and line 2 holds the view: the scope, the Viewed
//! progress, Diff options (Hide whitespace), Impact, Refresh and Close.

use super::{button, ReviewView};
use crate::agent_glyph::Glyph;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::design::AgentMark;
use rade_core::review_loop::{Commit, Head};

pub(super) struct HeadUi {
    pub root: gtk::Box,
    branch: gtk::Label,
    base: gtk::Label,
    agent: gtk::Box,
    glyph: Glyph,
    agent_name: gtk::Label,
    more: gtk::Label,
    task: gtk::Label,
    pub commits: gtk::MenuButton,
    commit_list: gtk::Box,
    /// `All changes n` (the scope control; Rounds add `Since round n`).
    pub scope_all: gtk::ToggleButton,
    /// `Since round n  j` (R119.2); hidden before the first Round.
    pub scope_since: gtk::ToggleButton,
    /// `Round n · state` (R119.4).
    round: gtk::Label,
    /// The Agent's summary when it asked for a review (R119.6).
    summary: gtk::Label,
    pub viewed: gtk::Label,
    pub meter: gtk::LevelBar,
    pub whitespace: gtk::CheckButton,
    pub refresh: gtk::Button,
    pub close: gtk::Button,
    /// The commits the popover lists (rebuilt only when they change).
    shown: std::cell::RefCell<Vec<Commit>>,
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l
}

impl HeadUi {
    pub fn new(impact: &gtk::ToggleButton) -> Self {
        let line1 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line1.add_css_class("review-head1");
        let branch_icon = gtk::Image::from_icon_name("ph-git-branch-symbolic");
        branch_icon.add_css_class("review-branch-icon");
        let branch = label("", "review-branch");
        let base = label("", "review-base");
        let agent = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        agent.add_css_class("review-agent");
        let glyph = Glyph::new(AgentMark::Other, None);
        let agent_name = label("", "review-agent-name");
        let more = label("", "review-agent-more");
        agent.append(&label("·", "review-sep"));
        agent.append(&glyph.root);
        agent.append(&agent_name);
        agent.append(&more);
        let task = label("", "review-task");
        task.set_xalign(0.0);
        task.set_hexpand(true);
        task.set_ellipsize(gtk::pango::EllipsizeMode::End);
        task.set_single_line_mode(true);
        let commit_list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        commit_list.add_css_class("review-commit-list");
        let pop = gtk::Popover::new();
        pop.set_child(Some(&commit_list));
        let commits = gtk::MenuButton::new();
        commits.add_css_class("flat");
        commits.add_css_class("review-commits");
        commits.set_popover(Some(&pop));
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        line1.append(&branch_icon);
        line1.append(&branch);
        line1.append(&base);
        line1.append(&agent);
        line1.append(&task);
        let round = label("", "review-round");
        round.set_visible(false);
        line1.append(&round);
        line1.append(&commits);

        let line2 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line2.add_css_class("review-head2");
        let scope = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        scope.add_css_class("review-scope");
        scope.add_css_class("linked");
        scope.set_valign(gtk::Align::Center);
        let scope_all = gtk::ToggleButton::with_label("All changes");
        scope_all.set_active(true);
        scope_all.set_can_focus(false);
        scope.append(&scope_all);
        let scope_since = gtk::ToggleButton::with_label("Since round");
        scope_since.set_group(Some(&scope_all));
        scope_since.set_can_focus(false);
        scope_since.set_visible(false);
        scope.append(&scope_since);
        let viewed = label("", "review-viewed");
        let meter = gtk::LevelBar::new();
        meter.add_css_class("review-progress");
        meter.set_mode(gtk::LevelBarMode::Continuous);
        meter.set_valign(gtk::Align::Center);
        // No "low / high / full" colour steps: one accent fill.
        for name in [
            gtk::LEVEL_BAR_OFFSET_LOW,
            gtk::LEVEL_BAR_OFFSET_HIGH,
            gtk::LEVEL_BAR_OFFSET_FULL,
        ] {
            meter.remove_offset_value(Some(name));
        }
        let whitespace = gtk::CheckButton::with_label("Hide whitespace changes  (w)");
        whitespace.add_css_class("review-ws");
        let opts = gtk::Box::new(gtk::Orientation::Vertical, 4);
        opts.add_css_class("review-options");
        opts.append(&whitespace);
        let opts_pop = gtk::Popover::new();
        opts_pop.set_child(Some(&opts));
        let options = gtk::MenuButton::new();
        options.set_icon_name("ph-gear-six-symbolic");
        options.set_tooltip_text(Some("Diff options"));
        options.add_css_class("flat");
        options.add_css_class("review-options-button");
        options.set_popover(Some(&opts_pop));
        let refresh = button("ph-arrow-counter-clockwise-symbolic", "Refresh");
        let close = button("ph-x-symbolic", "Close Review");
        line2.append(&scope);
        line2.append(&spacer);
        line2.append(&viewed);
        line2.append(&meter);
        line2.append(&options);
        line2.append(impact);
        line2.append(&refresh);
        line2.append(&close);

        let summary = label("", "review-summary");
        summary.set_xalign(0.0);
        summary.set_wrap(true);
        summary.set_lines(3);
        summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        summary.set_visible(false);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("review-head");
        root.append(&line1);
        root.append(&summary);
        root.append(&line2);
        Self {
            root,
            branch,
            base,
            agent,
            glyph,
            agent_name,
            more,
            task,
            commits,
            commit_list,
            scope_all,
            scope_since,
            round,
            summary,
            viewed,
            meter,
            whitespace,
            refresh,
            close,
            shown: Default::default(),
        }
    }

    /// Line 1 from the diff's branch / base, the Worktree's head and its
    /// commits.
    pub fn show(&self, branch: &str, base: Option<&str>, head: &Head, commits: &[Commit]) {
        self.branch.set_label(branch);
        self.base
            .set_label(&base.map(|b| format!("← {b}")).unwrap_or_default());
        self.agent.set_visible(head.agent.is_some());
        if let Some(a) = &head.agent {
            self.glyph.set(a.mark, None, a.status);
            self.agent_name.set_label(&a.name);
            self.more.set_label(&if a.more > 0 {
                format!("+{}", a.more)
            } else {
                String::new()
            });
            self.more.set_visible(a.more > 0);
        }
        match &head.task {
            Some(t) => {
                self.task.set_label(&format!("“{t}”"));
                self.task.set_tooltip_text(Some(t));
            }
            None => {
                self.task.set_label("");
                self.task.set_tooltip_text(None);
            }
        }
        self.commits.set_visible(!commits.is_empty());
        self.commits.set_label(&match commits.len() {
            1 => "1 commit".to_string(),
            n if n >= rade_core::review_loop::COMMITS_MAX => format!("{n}+ commits"),
            n => format!("{n} commits"),
        });
        if *self.shown.borrow() == commits {
            return;
        }
        *self.shown.borrow_mut() = commits.to_vec();
        while let Some(c) = self.commit_list.first_child() {
            self.commit_list.remove(&c);
        }
        let now = crate::panes::now();
        for c in commits {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.add_css_class("review-commit");
            row.append(&label(&c.sha, "review-commit-sha"));
            let s = label(&c.subject, "review-commit-subject");
            s.set_xalign(0.0);
            s.set_hexpand(true);
            s.set_max_width_chars(60);
            s.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.append(&s);
            row.append(&label(
                &rade_core::activity::ago(now.saturating_sub(c.at)),
                "review-commit-age",
            ));
            self.commit_list.append(&row);
        }
    }

    /// `k/n viewed` and the meter; the scope's file count.
    pub fn progress(&self, done: usize, total: usize) {
        self.viewed.set_label(&format!("{done}/{total} viewed"));
        self.meter.set_max_value(total.max(1) as f64);
        self.meter.set_value(done as f64);
        self.scope_all.set_label(&format!("All changes  {total}"));
    }

    /// The Round chip, the scope's second choice and the summary (R119).
    pub fn rounds(
        &self,
        last: Option<(u32, rade_core::review_loop::RoundState)>,
        since: Option<usize>,
        summary: Option<&str>,
    ) {
        use rade_core::review_loop::RoundState as S;
        match last {
            Some((n, st)) => {
                self.round.set_label(&format!("Round {n} · {}", st.words()));
                for (c, on) in [
                    ("state-requested", st == S::ChangesRequested),
                    ("state-approved", st == S::Approved),
                    ("state-changed", st == S::ChangedAfterApproval),
                ] {
                    if on {
                        self.round.add_css_class(c);
                    } else {
                        self.round.remove_css_class(c);
                    }
                }
                self.round.set_visible(true);
                self.scope_since
                    .set_label(&format!("Since round {n}  {}", since.unwrap_or(0)));
                self.scope_since.set_visible(true);
            }
            None => {
                self.round.set_visible(false);
                self.scope_since.set_visible(false);
            }
        }
        match summary {
            Some(t) => {
                // Markdown marks are noise on one line.
                let plain: String = t
                    .chars()
                    .filter(|c| !matches!(c, '*' | '`' | '#' | '_'))
                    .collect();
                self.summary.set_label(&plain);
                self.summary.set_visible(true);
            }
            None => self.summary.set_visible(false),
        }
    }

    pub fn info(&self, head: &Head) -> serde_json::Value {
        serde_json::json!({
            "branch": self.branch.label().to_string(),
            "base": self.base.label().trim_start_matches("← ").to_string(),
            "agent": head.agent.as_ref().map(|a| &a.name),
            "more": head.agent.as_ref().map(|a| a.more),
            "glyph": self.agent.is_visible().then(|| self.glyph.debug()),
            "task": head.task,
            "task_shown": self.task.label().to_string(),
            "commits": self.commits.is_visible().then(|| self.commits.label().map(|l| l.to_string())).flatten(),
            "viewed": self.viewed.label().to_string(),
            "scope": self.scope_all.label().map(|l| l.to_string()),
            "scope_since": self.scope_since.is_visible().then(|| self.scope_since.label().map(|l| l.to_string())).flatten(),
            "since_active": self.scope_since.is_active(),
            "round": self.round.is_visible().then(|| self.round.label().to_string()),
            "summary": self.summary.is_visible().then(|| self.summary.label().to_string()),
            "whitespace": self.whitespace.is_active(),
        })
    }
}

impl ReviewView {
    /// The commits popover's rows (`debug.review` `commit_list`).
    pub(super) fn commit_rows(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut c = self.head.commit_list.first_child();
        while let Some(row) = c {
            let mut parts = Vec::new();
            let mut l = row.first_child();
            while let Some(w) = l {
                if let Some(t) = w.downcast_ref::<gtk::Label>() {
                    parts.push(t.label().to_string());
                }
                l = w.next_sibling();
            }
            out.push(parts.join(" "));
            c = row.next_sibling();
        }
        out
    }
}
