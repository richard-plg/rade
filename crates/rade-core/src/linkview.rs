//! A Source link as the UI shows it (PRD R100.3, R104, R105, R107): one
//! resolved checkout ([`crate::checkout::Resolved`]) plus what git says
//! about it ([`crate::checkout::Facts`]), with Project names instead of
//! ids. The Navigator, Project home, Changes and the palette all read this
//! one shape, so a link reads the same everywhere. Pure.

use crate::checkout::{Facts, Kind, Resolved};
use crate::drift::{self, Drift};
use std::path::{Path, PathBuf};

/// The Workspace's Projects in their shown order: `(id, name)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names(pub Vec<(String, String)>);

impl Names {
    /// A Project's name; its id when it is not listed.
    pub fn name(&self, id: &str) -> String {
        self.0
            .iter()
            .find(|(i, _)| i == id)
            .map_or_else(|| id.to_string(), |(_, n)| n.clone())
    }

    /// Where a Project sits in the shown order (unlisted ones last).
    pub fn rank(&self, id: &str) -> usize {
        self.0
            .iter()
            .position(|(i, _)| i == id)
            .unwrap_or(usize::MAX)
    }

    /// `(project id, base)` pairs → `(project name, base)` in shown order.
    pub fn named(&self, pairs: &[(String, String)]) -> Vec<(String, String)> {
        let mut v: Vec<&(String, String)> = pairs.iter().collect();
        v.sort_by(|a, b| self.rank(&a.0).cmp(&self.rank(&b.0)).then(a.0.cmp(&b.0)));
        v.into_iter()
            .map(|(p, b)| (self.name(p), b.clone()))
            .collect()
    }
}

/// One link with its checkout and git facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkView {
    /// Project id and name.
    pub project: String,
    pub project_name: String,
    /// Source id.
    pub source: String,
    pub base: String,
    pub kind: Option<Kind>,
    /// The checkout folder (canonical); `None` when refused.
    pub path: Option<PathBuf>,
    /// The Project checkout is planned, not made yet.
    pub create: bool,
    pub refused: Option<String>,
    pub head: Option<String>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
    pub dirty: usize,
    /// Base drift (R100); `None` also while the user keeps it ([`KeepSet`]).
    pub drift: Option<Drift>,
    /// Other Projects linking the Source, `(name, base)`, shown order.
    pub also_in: Vec<(String, String)>,
    /// Other Projects using this same checkout, `(name, base)`.
    pub shared_with: Vec<(String, String)>,
}

/// A state chip's color (spec B.27–B.29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Info,
}

impl Tone {
    /// The CSS class (`chip-ok`, `chip-warn`, `chip-info`).
    pub fn class(self) -> &'static str {
        match self {
            Tone::Ok => "chip-ok",
            Tone::Warn => "chip-warn",
            Tone::Info => "chip-info",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub tone: Tone,
    pub text: String,
}

/// `n changes` / `1 change`.
pub fn changes_short(n: usize) -> String {
    if n == 1 {
        "1 change".into()
    } else {
        format!("{n} changes")
    }
}

impl LinkView {
    pub fn new(r: &Resolved, f: &Facts, names: &Names) -> Self {
        let drift = f
            .head
            .as_ref()
            .filter(|_| f.exists && r.refused.is_none())
            .and_then(|h| drift::drift(&r.base, h, f.dirty));
        LinkView {
            project: r.project.clone(),
            project_name: names.name(&r.project),
            source: r.source.clone(),
            base: r.base.clone(),
            kind: r.kind,
            path: r.path.clone(),
            create: r.create,
            refused: r.refused.clone(),
            head: f.head.as_ref().map(|h| h.name().to_string()),
            ahead: f.ahead,
            behind: f.behind,
            dirty: f.dirty,
            drift,
            also_in: names.named(&r.also_in),
            shared_with: names.named(&r.shared_with),
        }
    }

    /// The also-in marker's tooltip (R104.2): `Also in Billing @
    /// release/2.4`, one `<Project> @ <base>` per entry.
    pub fn also_in_tooltip(&self) -> Option<String> {
        if self.also_in.is_empty() {
            return None;
        }
        let parts: Vec<String> = self
            .also_in
            .iter()
            .map(|(p, b)| format!("{p} @ {b}"))
            .collect();
        Some(format!("Also in {}", parts.join(", ")))
    }

    /// The link table's `VS BASE` cell (R105.3): `=` when level, `↓2`
    /// when only behind, else `↑4 ↓0`; `—` when unknown.
    pub fn vs_base(&self) -> String {
        match (self.ahead, self.behind) {
            (Some(0), Some(0)) => "=".into(),
            (Some(0), Some(b)) => format!("↓{b}"),
            (Some(a), Some(b)) => format!("↑{a} ↓{b}"),
            _ => "—".into(),
        }
    }

    /// The state chip (R105.3): off base wins over also in, also in over
    /// uncommitted changes, those over clean; a refused link says so.
    pub fn chip(&self) -> Chip {
        if let Some(why) = &self.refused {
            return Chip {
                tone: Tone::Warn,
                text: format!("refused · {why}"),
            };
        }
        if let Some(d) = &self.drift {
            let text = if d.dirty == 0 {
                "off base".to_string()
            } else {
                format!("off base · {}", changes_short(d.dirty))
            };
            return Chip {
                tone: Tone::Warn,
                text,
            };
        }
        if let Some((p, b)) = self.also_in.first() {
            let more = match self.also_in.len() {
                1 => String::new(),
                n => format!(" +{}", n - 1),
            };
            return Chip {
                tone: Tone::Info,
                text: format!("also in · {p} @ {b}{more}"),
            };
        }
        if self.dirty > 0 {
            return Chip {
                tone: Tone::Info,
                text: changes_short(self.dirty),
            };
        }
        Chip {
            tone: Tone::Ok,
            text: "clean".into(),
        }
    }

    /// The link table's `CHECKOUT` cell: the kind, and the folder under
    /// `root` for anything but the main checkout (R105-b).
    pub fn checkout_cell(&self, root: &Path) -> (String, Option<String>) {
        let rel = self.path.as_deref().map(|p| {
            p.strip_prefix(root)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| p.display().to_string())
        });
        match (self.kind, self.create) {
            _ if self.refused.is_some() => ("refused".into(), None),
            (Some(Kind::Main), _) => ("main checkout".into(), None),
            (Some(Kind::Project), true) => ("new project checkout".into(), rel),
            (Some(Kind::Project), false) => ("project checkout".into(), rel),
            _ => ("checkout".into(), rel),
        }
    }
}

/// Drifts the user chose to Keep (R100.2): hidden until the checkout's
/// HEAD changes again. In memory: a restart shows them again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeepSet {
    /// (project, source, HEAD when kept).
    kept: Vec<(String, String, String)>,
}

impl KeepSet {
    pub fn keep(&mut self, project: &str, source: &str, head: &str) {
        self.kept.retain(|(p, s, _)| !(p == project && s == source));
        self.kept.push((project.into(), source.into(), head.into()));
    }

    /// Hide the kept drifts of `links`; forget a keep whose link moved to
    /// another HEAD or back on its base.
    pub fn apply(&mut self, links: &mut [LinkView]) {
        for l in links.iter_mut() {
            let Some(i) = self
                .kept
                .iter()
                .position(|(p, s, _)| *p == l.project && *s == l.source)
            else {
                continue;
            };
            match &l.drift {
                Some(d) if d.head == self.kept[i].2 => l.drift = None,
                _ => {
                    self.kept.remove(i);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::Head;

    fn names() -> Names {
        Names(vec![
            ("shop".into(), "Shop".into()),
            ("billing".into(), "Billing".into()),
            ("docs".into(), "Docs".into()),
        ])
    }

    fn resolved(source: &str, kind: Kind, also: &[(&str, &str)]) -> Resolved {
        Resolved {
            project: "shop".into(),
            source: source.into(),
            base: "develop".into(),
            kind: Some(kind),
            path: Some(PathBuf::from(format!("/r/{source}"))),
            create: false,
            shared_with: vec![],
            also_in: also
                .iter()
                .map(|(p, b)| (p.to_string(), b.to_string()))
                .collect(),
            refused: None,
            commands: vec![],
            record: None,
        }
    }

    fn facts(head: &str, dirty: usize, ahead: usize, behind: usize) -> Facts {
        Facts {
            exists: true,
            head: Some(Head::Branch(head.into())),
            dirty,
            ahead: Some(ahead),
            behind: Some(behind),
        }
    }

    #[test]
    fn home_link_rows_vs_base_and_chip_precedence() {
        let web = LinkView::new(
            &resolved("shop-web", Kind::Main, &[]),
            &facts("develop", 0, 0, 2),
            &names(),
        );
        assert_eq!(web.vs_base(), "↓2");
        assert_eq!(web.chip().text, "clean");
        assert_eq!(web.chip().tone, Tone::Ok);
        let api = LinkView::new(
            &resolved("shop-api", Kind::Main, &[]),
            &facts("feat/checkout-v2", 3, 4, 0),
            &names(),
        );
        assert_eq!(api.vs_base(), "↑4 ↓0");
        assert_eq!(api.chip().text, "off base · 3 changes");
        assert_eq!(api.chip().tone, Tone::Warn);
        let ui = LinkView::new(
            &resolved("shared-ui", Kind::Project, &[("billing", "release/2.4")]),
            &facts("develop", 0, 0, 0),
            &names(),
        );
        assert_eq!(ui.vs_base(), "=");
        assert_eq!(ui.chip().text, "also in · Billing @ release/2.4");
        assert_eq!(ui.chip().tone, Tone::Info);
        assert_eq!(
            ui.checkout_cell(Path::new("/r")),
            ("project checkout".into(), Some("shared-ui".into()))
        );
        assert_eq!(
            web.checkout_cell(Path::new("/r")),
            ("main checkout".into(), None)
        );
        // Off base wins over also in; also in wins over dirty; dirty → n changes.
        let both = LinkView::new(
            &resolved(
                "shared-ui",
                Kind::Project,
                &[("billing", "x"), ("docs", "y")],
            ),
            &facts("other", 1, 0, 0),
            &names(),
        );
        assert_eq!(both.chip().text, "off base · 1 change");
        let dirty_also = LinkView::new(
            &resolved(
                "shared-ui",
                Kind::Project,
                &[("docs", "y"), ("billing", "x")],
            ),
            &facts("develop", 2, 0, 0),
            &names(),
        );
        assert_eq!(dirty_also.chip().text, "also in · Billing @ x +1");
        let dirty = LinkView::new(
            &resolved("shop-web", Kind::Main, &[]),
            &facts("develop", 2, 0, 0),
            &names(),
        );
        assert_eq!(dirty.chip().text, "2 changes");
        let clean_drift = LinkView::new(
            &resolved("shop-api", Kind::Main, &[]),
            &facts("feat/x", 0, 1, 0),
            &names(),
        );
        assert_eq!(clean_drift.chip().text, "off base");
    }

    #[test]
    fn also_in_tooltip_names_each_project_at_its_base_in_shown_order() {
        let ui = LinkView::new(
            &resolved(
                "shared-ui",
                Kind::Project,
                &[("docs", "main"), ("billing", "release/2.4")],
            ),
            &facts("develop", 0, 0, 0),
            &names(),
        );
        assert_eq!(
            ui.also_in_tooltip().unwrap(),
            "Also in Billing @ release/2.4, Docs @ main"
        );
        let web = LinkView::new(
            &resolved("shop-web", Kind::Main, &[]),
            &facts("develop", 0, 0, 0),
            &names(),
        );
        assert_eq!(web.also_in_tooltip(), None);
    }

    #[test]
    fn keep_hides_until_head_changes() {
        let mk = |head: &str| {
            vec![LinkView::new(
                &resolved("shop-api", Kind::Main, &[]),
                &facts(head, 3, 0, 0),
                &names(),
            )]
        };
        let mut keep = KeepSet::default();
        let mut v = mk("feat/checkout-v2");
        keep.apply(&mut v);
        assert!(v[0].drift.is_some(), "nothing kept yet");
        keep.keep("shop", "shop-api", "feat/checkout-v2");
        let mut v = mk("feat/checkout-v2");
        keep.apply(&mut v);
        assert!(v[0].drift.is_none(), "kept at the same HEAD");
        let mut v = mk("other");
        keep.apply(&mut v);
        assert!(v[0].drift.is_some(), "HEAD moved: shown again");
        let mut v = mk("feat/checkout-v2");
        keep.apply(&mut v);
        assert!(v[0].drift.is_some(), "the keep was forgotten");
    }
}
