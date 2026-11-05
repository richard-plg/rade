//! Project views (PRD R105, R107.1, R100.3): what Project home and the
//! Inspector's Changes page show for one Project, from its links
//! ([`LinkView`]) and the window state. Pure: git counts come in from the
//! app (gathered off the UI thread).

use crate::agents::AgentStatus;
use crate::drift::{Action, Drift};
use crate::linkview::LinkView;
use crate::model::State;
use crate::project::Project;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One KPI card (spec B.14–B.17).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Kpi {
    pub label: String,
    pub value: String,
    pub detail: String,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `acme · 3 sources · .rade/projects/shop/project.toml` (spec B.8).
pub fn sub_line(workspace: &str, project: &Project, links: usize, root: &Path) -> String {
    let file = project
        .file
        .strip_prefix(root)
        .unwrap_or(&project.file)
        .display()
        .to_string();
    format!(
        "{workspace} · {} · {file}",
        plural(links, "source", "sources")
    )
}

/// The Project's Worktrees, in Source then Worktree order: (source id,
/// worktree id, folder).
pub fn worktrees(
    state: &State,
    project: &str,
    links: &[LinkView],
) -> Vec<(String, String, PathBuf)> {
    links
        .iter()
        .filter(|l| l.project == project)
        .filter_map(|l| state.source(&l.source))
        .flat_map(|s| {
            s.worktrees
                .iter()
                .skip(1)
                .filter(|w| w.project.as_deref() == Some(project))
                .map(move |w| (s.id.clone(), w.id.clone(), w.path.clone()))
        })
        .collect()
}

/// The three KPIs (R105.2): Agents (waiting / failed / running counts),
/// Worktrees (names), Uncommitted (files in n sources). `changed` = a
/// Worktree folder's changed files.
pub fn kpis(
    state: &State,
    project: &str,
    links: &[LinkView],
    changed: &dyn Fn(&Path) -> usize,
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Vec<Kpi> {
    let agents: Vec<_> = crate::navigator::project_panes(state, project, links, canon)
        .into_iter()
        .filter(|p| p.agent.is_some())
        .collect();
    let count = |s: AgentStatus| agents.iter().filter(|p| p.status == Some(s)).count();
    let active: Vec<String> = [
        (AgentStatus::Waiting, "waiting"),
        (AgentStatus::Error, "failed"),
        (AgentStatus::Running, "running"),
    ]
    .iter()
    .filter(|(s, _)| count(*s) > 0)
    .map(|(s, w)| format!("{} {w}", count(*s)))
    .collect();
    let agents_detail = match (agents.len(), active.is_empty()) {
        (0, _) => "none".to_string(),
        (_, true) => "idle".to_string(),
        _ => active.join(" · "),
    };
    let wts = worktrees(state, project, links);
    let names: Vec<String> = wts
        .iter()
        .filter_map(|(s, w, _)| state.worktree(s, w).map(|w| w.name.clone()))
        .collect();
    // Uncommitted files per Source: its links' checkouts (each folder once)
    // and its Worktrees.
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut per_source: Vec<(String, usize)> = Vec::new();
    let mut add = |source: &str, n: usize| match per_source.iter_mut().find(|(s, _)| s == source) {
        Some((_, k)) => *k += n,
        None => per_source.push((source.into(), n)),
    };
    for l in links.iter().filter(|l| l.project == project) {
        if let Some(p) = &l.path {
            if !seen.contains(p) {
                seen.push(p.clone());
                add(&l.source, l.dirty);
            }
        }
    }
    for (s, _, path) in &wts {
        add(s, changed(path));
    }
    let files: usize = per_source.iter().map(|(_, n)| n).sum();
    let dirty_sources = per_source.iter().filter(|(_, n)| *n > 0).count();
    vec![
        Kpi {
            label: "Agents".into(),
            value: agents.len().to_string(),
            detail: agents_detail,
        },
        Kpi {
            label: "Worktrees".into(),
            value: names.len().to_string(),
            detail: if names.is_empty() {
                "none".into()
            } else {
                names.join(", ")
            },
        },
        Kpi {
            label: "Uncommitted".into(),
            value: files.to_string(),
            detail: if files == 0 {
                "clean".into()
            } else {
                format!(
                    "{} in {}",
                    if files == 1 { "file" } else { "files" },
                    plural(dirty_sources, "source", "sources")
                )
            },
        },
    ]
}

/// One row of the Worktrees card (spec B.34).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorktreeRow {
    pub source: String,
    pub worktree: String,
    pub name: String,
    /// `shop-web ← develop`.
    pub sub: String,
    /// `↑3 · 4 changed · 2 agents`, or `clean`.
    pub meta: String,
    /// The worst status of its Agents.
    pub led: Option<AgentStatus>,
    /// A Branch set's Worktree (indented under its set row) or the set row
    /// itself: the set's name (R150.1).
    pub set: Option<String>,
    /// The set row `<name> · <n> sources` heading its Worktrees.
    pub group: bool,
    /// `PR #482 ✓` (R146.3) or a set row's `2 PRs · 1 failing` (R151.4),
    /// with its tone.
    pub pr_chip: Option<crate::pr::Chip>,
    /// The PR's URL (a Worktree's last read or recorded PR).
    #[serde(skip)]
    pub pr_url: Option<String>,
    /// The set row: each member's checks (`None` = no PR).
    #[serde(skip)]
    set_checks: Option<crate::branch_set::Checks>,
    /// (ahead, changed, agents): the set row sums them.
    #[serde(skip)]
    counts: (usize, usize, usize),
}

/// The last PR read of a Worktree (source id, worktree id), R146.
pub type PrRead<'a> = &'a dyn Fn(&str, &str) -> Option<crate::pr::PrStatus>;

/// The 4th KPI `Pull requests` (R146.3): open PRs of the Project's
/// Worktrees and the checks sub-line; `None` while no Worktree of it has a
/// PR (read or recorded), so a Project without PRs keeps three cards.
pub fn pr_kpi(state: &State, project: &str, links: &[LinkView], pr: PrRead) -> Option<Kpi> {
    let wts = worktrees(state, project, links);
    let reads: Vec<Option<crate::pr::PrStatus>> = wts.iter().map(|(s, w, _)| pr(s, w)).collect();
    let any = reads.iter().any(Option::is_some)
        || wts
            .iter()
            .any(|(s, w, _)| state.worktree(s, w).is_some_and(|w| w.pr.is_some()));
    if !any {
        return None;
    }
    let k = crate::pr::kpi(reads.iter().map(Option::as_ref));
    Some(Kpi {
        label: "Pull requests".into(),
        value: k.value,
        detail: if k.sub.is_empty() {
            "none open".into()
        } else {
            k.sub
        },
    })
}

/// The Worktrees card rows (R105.4): `facts` = (ahead, changed files) of
/// a Worktree folder; `pr` = a Worktree's last PR read.
pub fn worktree_rows(
    state: &State,
    project: &str,
    links: &[LinkView],
    facts: &dyn Fn(&Path) -> (usize, usize),
    pr: PrRead,
) -> Vec<WorktreeRow> {
    let rows: Vec<WorktreeRow> = worktrees(state, project, links)
        .into_iter()
        .filter_map(|(s, w, path)| {
            let src = state.source(&s)?;
            let wt = state.worktree(&s, &w)?;
            let link_base = links
                .iter()
                .find(|l| l.project == project && l.source == s)
                .map(|l| l.base.as_str());
            let base = crate::checkout::comparison_base(wt.base.as_deref(), link_base)
                .unwrap_or_else(|| "main".into());
            let (ahead, changed) = facts(&path);
            let agents: Vec<_> = wt
                .tabs
                .iter()
                .flat_map(|t| t.panes())
                .filter(|p| p.agent.is_some())
                .collect();
            let read = pr(&s, &w);
            let recorded = wt.pr.as_ref();
            let chip =
                crate::pr::worktree_chip(read.as_ref(), recorded.map(|p| p.number)).map(|c| {
                    crate::pr::Chip {
                        text: format!("PR {}", c.text),
                        tone: c.tone,
                    }
                });
            Some(WorktreeRow {
                source: s.clone(),
                worktree: w.clone(),
                name: wt.name.clone(),
                sub: format!("{} ← {base}", src.name),
                meta: meta_text(ahead, changed, agents.len()),
                led: crate::status::worst(agents.iter().map(|p| p.status)),
                set: wt.set.clone(),
                group: false,
                pr_chip: chip,
                pr_url: read
                    .as_ref()
                    .map(|r| r.url.clone())
                    .or_else(|| recorded.map(|p| p.url.clone()))
                    .filter(|u| !u.is_empty()),
                set_checks: crate::pr::set_checks(read.as_ref()),
                counts: (ahead, changed, agents.len()),
            })
        })
        .collect();
    group_sets(rows, links)
}

/// `↑3 · 4 changed · 2 agents`, or `clean`.
fn meta_text(ahead: usize, changed: usize, agents: usize) -> String {
    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(format!("↑{ahead}"));
    }
    if changed > 0 {
        parts.push(format!("{changed} changed"));
    }
    if agents > 0 {
        parts.push(plural(agents, "agent", "agents"));
    }
    if parts.is_empty() {
        "clean".into()
    } else {
        parts.join(" · ")
    }
}

/// R150.1: a Branch set's Worktrees under one set row (`<name> · <n>
/// sources`, the worst status of them, their summed meta) at the place of
/// its first Worktree; its Worktrees follow in link order.
fn group_sets(rows: Vec<WorktreeRow>, links: &[LinkView]) -> Vec<WorktreeRow> {
    let rank = |s: &str| {
        links
            .iter()
            .position(|l| l.source == s)
            .unwrap_or(usize::MAX)
    };
    let mut out: Vec<WorktreeRow> = Vec::new();
    let mut done: Vec<String> = Vec::new();
    for r in &rows {
        let Some(set) = &r.set else {
            out.push(r.clone());
            continue;
        };
        if done.contains(set) {
            continue;
        }
        done.push(set.clone());
        let mut members: Vec<WorktreeRow> = rows
            .iter()
            .filter(|x| x.set.as_ref() == Some(set))
            .cloned()
            .collect();
        members.sort_by_key(|m| rank(&m.source));
        let sum = members.iter().fold((0, 0, 0), |a, m| {
            (a.0 + m.counts.0, a.1 + m.counts.1, a.2 + m.counts.2)
        });
        let lead = &members[0];
        // R151.4: the worst of its PRs, `<k> PRs · <j> failing`.
        let checks: Vec<Option<crate::branch_set::Checks>> =
            members.iter().map(|m| m.set_checks).collect();
        let pr = crate::branch_set::pr_chip(&checks).map(|(text, worst)| crate::pr::Chip {
            text,
            tone: crate::pr::set_tone(worst),
        });
        out.push(WorktreeRow {
            pr_chip: pr,
            pr_url: None,
            set_checks: None,
            source: lead.source.clone(),
            worktree: lead.worktree.clone(),
            name: crate::branch_set::row_text(set, members.len()),
            sub: String::new(),
            meta: meta_text(sum.0, sum.1, sum.2),
            led: crate::branch_set::row_status(members.iter().map(|m| m.led)),
            set: Some(set.clone()),
            group: true,
            counts: sum,
        });
        out.extend(members);
    }
    out
}

/// What a Changes link group shows (R107.1): the focused Worktree when it
/// is one of this link's, else the link's own checkout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Group {
    pub project: String,
    pub source: String,
    pub path: PathBuf,
    /// `(source id, worktree id)` when the group is a Worktree.
    pub worktree: Option<(String, String)>,
    /// The header's left meta: the Worktree's name or the checkout's HEAD.
    pub label: String,
    /// The ref the group's files compare with.
    pub base: String,
}

/// The Changes groups of `project`, one per link with a checkout, in link
/// order; `focused` = the window's (source, worktree).
pub fn change_groups(
    state: &State,
    project: &str,
    links: &[LinkView],
    focused: Option<(&str, &str)>,
) -> Vec<Group> {
    // R150.4: a focused set Worktree → each link shows the set's Worktree.
    let set = focused
        .and_then(|(s, w)| state.worktree(s, w))
        .filter(|w| w.project.as_deref() == Some(project))
        .and_then(|w| w.set.clone());
    let set_wt = |source: &str| {
        let name = set.as_deref()?;
        state
            .source(source)?
            .worktrees
            .iter()
            .skip(1)
            .find(|w| w.set.as_deref() == Some(name) && w.project.as_deref() == Some(project))
    };
    let mut groups: Vec<Group> = links
        .iter()
        .filter(|l| l.project == project)
        .filter_map(|l| {
            let wt = focused
                .filter(|(s, _)| *s == l.source)
                .and_then(|(s, w)| state.worktree(s, w))
                .filter(|w| {
                    !state.is_main_worktree(&l.source, &w.id)
                        && w.project.as_deref() == Some(project)
                })
                .or_else(|| set_wt(&l.source));
            match wt {
                Some(w) => Some(Group {
                    project: project.into(),
                    source: l.source.clone(),
                    path: w.path.clone(),
                    worktree: Some((l.source.clone(), w.id.clone())),
                    label: w.name.clone(),
                    base: crate::checkout::comparison_base(w.base.as_deref(), Some(&l.base))
                        .unwrap_or_else(|| l.base.clone()),
                }),
                None => Some(Group {
                    project: project.into(),
                    source: l.source.clone(),
                    path: l.path.clone()?,
                    worktree: None,
                    label: l.head.clone().unwrap_or_else(|| l.base.clone()),
                    base: l.base.clone(),
                }),
            }
        })
        .collect();
    // The set's cards first, in link order (a stable sort).
    if let Some(name) = &set {
        groups.sort_by_key(|g| {
            let in_set = g
                .worktree
                .as_ref()
                .and_then(|(s, w)| state.worktree(s, w))
                .is_some_and(|w| w.set.as_ref() == Some(name));
            !in_set
        });
    }
    groups
}

/// A group header's right meta (spec A.60): `↑3 · 4`, `3`, or `clean`.
pub fn group_meta(ahead: usize, files: usize) -> String {
    match (ahead, files) {
        (_, 0) => "clean".into(),
        (0, f) => f.to_string(),
        (a, f) => format!("↑{a} · {f}"),
    }
}

/// The Changes header (R107.1): `PROJECT · SHOP`, `+212`, `−58`.
pub fn changes_header(
    project_name: &str,
    added: usize,
    removed: usize,
) -> (String, String, String) {
    (
        format!("PROJECT · {}", project_name.to_uppercase()),
        format!("+{added}"),
        format!("−{removed}"),
    )
}

/// The drift card / row menu of a drifted link (R100-g, spec A.67–A.68).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriftCard {
    pub text: String,
    pub sub: String,
    /// The buttons in order: (action, label).
    pub buttons: Vec<(Action, String)>,
}

impl DriftCard {
    /// The card as one text (R100-g).
    pub fn full(&self) -> String {
        format!("{} {}", self.text, self.sub)
    }
}

/// The card's texts and buttons: from [`crate::drift::drift`]'s actions,
/// so Back to base never shows on a dirty checkout.
pub fn drift_card(source: &str, project_name: &str, d: &Drift) -> DriftCard {
    let sub = if d.dirty > 0 {
        format!(
            "{}, so RADE won't switch it.",
            crate::drift::changes(d.dirty)
        )
    } else {
        format!("It is clean: Back to base switches it to {}.", d.base)
    };
    let buttons = [Action::BackToBase, Action::Keep, Action::MoveToWorktree]
        .into_iter()
        .filter(|a| d.actions.contains(a))
        .map(|a| {
            let label = match a {
                Action::BackToBase => "Back to base".to_string(),
                Action::Keep => format!("Keep, show in {project_name}"),
                Action::MoveToWorktree => "Move to a Worktree…".to_string(),
            };
            (a, label)
        })
        .collect();
    DriftCard {
        text: format!(
            "{source} is on {}, {project_name}'s base is {}.",
            d.head, d.base
        ),
        sub,
        buttons,
    }
}

/// One file row of a Changes group (spec A.61–A.63).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileRow {
    /// `M`, `A`, `D`, `R` (an untracked file is `A`).
    pub letter: char,
    pub path: String,
    /// `src/checkout/` (with the slash), or empty.
    pub dir: String,
    pub name: String,
    pub add: usize,
    pub del: usize,
    /// Staged as a whole (Commit takes it).
    pub staged: bool,
}

/// The letter of a `git status --porcelain=v2` entry: the worktree side
/// when it changed, else the index side; untracked = `A`.
pub fn file_letter(x: char, y: char) -> char {
    match (x, y) {
        ('?', _) => 'A',
        (x, '.') => x,
        (_, y) => y,
    }
}

/// `git diff --numstat` lines → path → (added, removed); binary files
/// (`-`) count 0.
pub fn numstat(text: &str) -> std::collections::HashMap<String, (usize, usize)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let a = it.next()?.parse().unwrap_or(0);
            let d = it.next()?.parse().unwrap_or(0);
            let path = it.next()?;
            // A rename: `old => new` or `dir/{old => new}/x`.
            let path = match path.split_once(" => ") {
                Some((l, r)) => match (l.rfind('{'), r.find('}')) {
                    (Some(i), Some(j)) => format!("{}{}{}", &l[..i], &r[..j], &r[j + 1..]),
                    _ => r.to_string(),
                },
                None => path.to_string(),
            };
            Some((path, (a, d)))
        })
        .collect()
}

/// The file rows of a checkout: its status entries (`(path, x, y)`) with
/// their counts (`numstat` against HEAD; an untracked file counts its
/// lines from `lines`), in path order.
pub fn file_rows(
    entries: &[(String, char, char)],
    counts: &std::collections::HashMap<String, (usize, usize)>,
    lines: &dyn Fn(&str) -> usize,
) -> Vec<FileRow> {
    let mut rows: Vec<FileRow> = entries
        .iter()
        .map(|(path, x, y)| {
            let (dir, name) = match path.rfind('/') {
                Some(i) => (path[..=i].to_string(), path[i + 1..].to_string()),
                None => (String::new(), path.clone()),
            };
            let (add, del) = if *x == '?' {
                (lines(path), 0)
            } else {
                counts.get(path).copied().unwrap_or((0, 0))
            };
            FileRow {
                letter: file_letter(*x, *y),
                path: path.clone(),
                dir,
                name,
                add,
                del,
                staged: *x != '?' && *x != '.' && *y == '.',
            }
        })
        .collect();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

/// The Changes header totals of every group's rows (R107.1).
pub fn totals<'a>(rows: impl IntoIterator<Item = &'a FileRow>) -> (usize, usize) {
    rows.into_iter()
        .fold((0, 0), |(a, d), r| (a + r.add, d + r.del))
}

/// The links Project home's Open terminal offers (R132.5): the one used
/// last first, then the others in table order; `(source, "name · base")`.
pub fn open_terminal_menu(
    links: &[(String, String, String)],
    last: Option<&str>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = links
        .iter()
        .map(|(s, name, base)| (s.clone(), format!("{name} · {base}")))
        .collect();
    if let Some(i) = last.and_then(|l| out.iter().position(|(s, _)| s == l)) {
        let hit = out.remove(i);
        out.insert(0, hit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::Head;
    use crate::navigator::tests::{scene_a, Scene, NOW};

    fn canon(p: &Path) -> PathBuf {
        p.to_path_buf()
    }

    /// Scene B: scene A plus Worktree `perf-img` (clean) and `dev-server`
    /// running (spec b-project-home.md "Content").
    fn scene_b() -> Scene {
        let mut sc = scene_a();
        let path = crate::navigator::tests::root().join(".worktrees-dev/shop-web/perf-img");
        let (s, id) = sc
            .state
            .add_worktree("shop-web", "perf-img", &path, None)
            .unwrap();
        sc.state
            .set_worktree_origin(&s, &id, Some("shop".into()), Some("develop".into()))
            .unwrap();
        sc.worktrees.insert("perf-img".into(), (s, id));
        let dev = sc.panes["dev-server"].clone();
        sc.state
            .set_status(&dev, Some(AgentStatus::Running), None, None, NOW - 60)
            .unwrap();
        sc
    }

    fn no_pr(_: &str, _: &str) -> Option<crate::pr::PrStatus> {
        None
    }

    fn read(number: u64, conclusion: &str) -> crate::pr::PrStatus {
        let checks = if conclusion.is_empty() {
            String::new()
        } else {
            format!(
                r#"{{"__typename":"CheckRun","name":"test","workflowName":"CI","status":"COMPLETED","conclusion":"{conclusion}","detailsUrl":"d"}}"#
            )
        };
        let json = format!(
            r#"[{{"number":{number},"title":"t","url":"https://github.com/o/r/pull/{number}","state":"OPEN","isDraft":false,"baseRefName":"develop","headRefOid":"h","reviewDecision":"","statusCheckRollup":[{checks}]}}]"#
        );
        crate::pr::status_from_json(&json, "h", None)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn r146_d_home_pr_kpi_worktree_meta_and_set_chip() {
        let mut sc = scene_set();
        let facts = |_: &Path| (0, 0);
        // No PR anywhere: three KPIs only, no chips.
        assert_eq!(pr_kpi(&sc.state, "shop", &sc.links, &no_pr), None);
        let rows = worktree_rows(&sc.state, "shop", &sc.links, &facts, &no_pr);
        assert!(rows.iter().all(|r| r.pr_chip.is_none()));
        // perf-img passing; the set's shop-web failing, shop-api pending.
        let perf = sc.worktrees["perf-img"].1.clone();
        let web = sc.worktrees["shop-web/feat-x"].1.clone();
        let api = sc.worktrees["shop-api/feat-x"].1.clone();
        let reads: std::collections::HashMap<String, crate::pr::PrStatus> = [
            (perf.clone(), read(482, "SUCCESS")),
            (web.clone(), read(7, "FAILURE")),
            (api.clone(), read(8, "")),
        ]
        .into_iter()
        .collect();
        let pr = |_: &str, w: &str| reads.get(w).cloned();
        let k = pr_kpi(&sc.state, "shop", &sc.links, &pr).unwrap();
        assert_eq!(
            (k.label.as_str(), k.value.as_str(), k.detail.as_str()),
            ("Pull requests", "3", "1 failing")
        );
        let rows = worktree_rows(&sc.state, "shop", &sc.links, &facts, &pr);
        let chip = |name: &str, group: bool| {
            rows.iter()
                .find(|r| r.name == name && r.group == group)
                .and_then(|r| r.pr_chip.clone())
                .map(|c| (c.text, c.tone))
        };
        use crate::pr::Tone;
        assert_eq!(
            chip("perf-img", false),
            Some(("PR #482 \u{2713}".into(), Tone::Ok))
        );
        assert_eq!(
            chip("feat-x · 3 sources", true),
            Some(("2 PRs · 1 failing".into(), Tone::Error))
        );
        assert_eq!(chip("checkout-v2", false), None);
        let perf_row = rows.iter().find(|r| r.name == "perf-img").unwrap();
        assert_eq!(
            perf_row.pr_url.as_deref(),
            Some("https://github.com/o/r/pull/482")
        );
        // Only a recorded PR (before the first read): the KPI shows, the
        // meta greys the number.
        let (s, cv2) = sc.worktrees["checkout-v2"].clone();
        sc.state.set_worktree_pr(
            &s,
            &cv2,
            crate::model::PrRef {
                number: 5,
                url: "https://github.com/o/r/pull/5".into(),
            },
        );
        let k = pr_kpi(&sc.state, "shop", &sc.links, &no_pr).unwrap();
        assert_eq!((k.value.as_str(), k.detail.as_str()), ("0", "none open"));
        let rows = worktree_rows(&sc.state, "shop", &sc.links, &facts, &no_pr);
        let r = rows.iter().find(|r| r.name == "checkout-v2").unwrap();
        assert_eq!(
            r.pr_chip.clone().map(|c| (c.text, c.tone)),
            Some(("PR #5".into(), Tone::Fg3))
        );
    }

    fn changed(p: &Path) -> usize {
        match p.file_name().and_then(|n| n.to_str()) {
            Some("checkout-v2") => 4,
            _ => 0,
        }
    }

    #[test]
    fn home_kpis_count_agents_worktrees_and_uncommitted_sources() {
        let sc = scene_b();
        let k = kpis(&sc.state, "shop", &sc.links, &changed, &canon);
        let got: Vec<(&str, &str, &str)> = k
            .iter()
            .map(|k| (k.label.as_str(), k.value.as_str(), k.detail.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Agents", "3", "1 waiting · 1 running"),
                ("Worktrees", "2", "checkout-v2, perf-img"),
                ("Uncommitted", "7", "files in 2 sources"),
            ]
        );
        let docs = kpis(&sc.state, "docs", &sc.links, &changed, &canon);
        let got: Vec<(&str, &str)> = docs
            .iter()
            .map(|k| (k.value.as_str(), k.detail.as_str()))
            .collect();
        assert_eq!(got, [("0", "none"), ("0", "none"), ("0", "clean")]);
    }

    #[test]
    fn home_worktree_meta_parts() {
        let sc = scene_b();
        let facts = |p: &Path| match p.file_name().and_then(|n| n.to_str()) {
            Some("checkout-v2") => (3, 4),
            _ => (0, 0),
        };
        let rows = worktree_rows(&sc.state, "shop", &sc.links, &facts, &no_pr);
        let got: Vec<(&str, &str, &str)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.sub.as_str(), r.meta.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "checkout-v2",
                    "shop-web ← develop",
                    "↑3 · 4 changed · 2 agents"
                ),
                ("perf-img", "shop-web ← develop", "clean"),
            ]
        );
        assert_eq!(rows[0].led, Some(AgentStatus::Waiting));
        let billing = worktree_rows(&sc.state, "billing", &sc.links, &facts, &no_pr);
        assert_eq!(billing[0].sub, "shared-ui ← release/2.4");
        assert_eq!(billing[0].meta, "1 agent");
    }

    /// Scene B with Shop's Branch set `feat-x`: a running Agent in its
    /// shop-web Worktree, a waiting one in its shop-api Worktree.
    fn scene_set() -> Scene {
        let mut sc = scene_b();
        crate::navigator::tests::add_set(&mut sc);
        for (src, st) in [
            ("shop-web", AgentStatus::Running),
            ("shop-api", AgentStatus::Waiting),
        ] {
            let (s, w) = sc.worktrees[&format!("{src}/feat-x")].clone();
            let (_, p) = sc.state.new_tab(&s, &w, None, None).unwrap();
            sc.state
                .set_status(&p, Some(st), None, Some("claude".into()), NOW)
                .unwrap();
        }
        sc
    }

    #[test]
    fn r150_b_home_groups_a_set_under_one_row_with_the_worst_status() {
        let sc = scene_set();
        let facts = |p: &Path| match p.to_str() {
            Some(x) if x.ends_with("shop-web/feat-x") => (2, 1),
            Some(x) if x.ends_with("shop-api/feat-x") => (1, 0),
            _ => (0, 0),
        };
        let rows = worktree_rows(&sc.state, "shop", &sc.links, &facts, &no_pr);
        let got: Vec<(&str, &str, bool, Option<&str>)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.sub.as_str(), r.group, r.set.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("checkout-v2", "shop-web ← develop", false, None),
                ("perf-img", "shop-web ← develop", false, None),
                ("feat-x · 3 sources", "", true, Some("feat-x")),
                ("feat-x", "shop-web ← develop", false, Some("feat-x")),
                ("feat-x", "shop-api ← develop", false, Some("feat-x")),
                ("feat-x", "shared-ui ← develop", false, Some("feat-x")),
            ]
        );
        let head = &rows[2];
        assert_eq!(
            head.led,
            Some(AgentStatus::Waiting),
            "worst of its Worktrees"
        );
        assert_eq!(head.meta, "↑3 · 1 changed · 2 agents");
        // Its click target is the lead (first link in Project order).
        let lead = &sc.worktrees["shop-web/feat-x"];
        assert_eq!((&head.source, &head.worktree), (&lead.0, &lead.1));
    }

    #[test]
    fn r150_changes_put_a_focused_set_worktrees_cards_first() {
        let sc = scene_set();
        let (s, w) = sc.worktrees["shop-api/feat-x"].clone();
        let g = change_groups(&sc.state, "shop", &sc.links, Some((&s, &w)));
        let got: Vec<(&str, &str, bool)> = g
            .iter()
            .map(|g| (g.source.as_str(), g.label.as_str(), g.worktree.is_some()))
            .collect();
        assert_eq!(
            got,
            [
                ("shop-web", "feat-x", true),
                ("shop-api", "feat-x", true),
                ("shared-ui", "feat-x", true),
            ]
        );
        // A set of two links: their cards before the third link's.
        let mut sc2 = scene_set();
        let (ws, ww) = sc2.worktrees["shop-web/feat-x"].clone();
        sc2.state.set_worktree_set(&ws, &ww, None).unwrap();
        let g = change_groups(&sc2.state, "shop", &sc2.links, Some((&s, &w)));
        let got: Vec<(&str, &str)> = g
            .iter()
            .map(|g| (g.source.as_str(), g.label.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("shop-api", "feat-x"),
                ("shared-ui", "feat-x"),
                ("shop-web", "develop")
            ]
        );
        // Without a set Worktree in focus: the links' own checkouts.
        let (s, w) = sc.worktrees["checkout-v2"].clone();
        let g = change_groups(&sc.state, "shop", &sc.links, Some((&s, &w)));
        let labels: Vec<&str> = g.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["checkout-v2", "feat/checkout-v2", "develop"]);
    }

    #[test]
    fn home_sub_line() {
        let sc = scene_a();
        let p = &sc.projects[0];
        assert_eq!(
            sub_line("acme", p, 3, &crate::navigator::tests::root()),
            "acme · 3 sources · .rade/projects/shop/project.toml"
        );
        assert_eq!(
            sub_line("acme", p, 1, &crate::navigator::tests::root()),
            "acme · 1 source · .rade/projects/shop/project.toml"
        );
    }

    #[test]
    fn changes_groups_pick_focused_worktree_or_link_checkout() {
        let sc = scene_a();
        let (s, w) = sc.worktrees["checkout-v2"].clone();
        let g = change_groups(&sc.state, "shop", &sc.links, Some((&s, &w)));
        let got: Vec<(&str, &str, bool)> = g
            .iter()
            .map(|g| (g.source.as_str(), g.label.as_str(), g.worktree.is_some()))
            .collect();
        assert_eq!(
            got,
            [
                ("shop-web", "checkout-v2", true),
                ("shop-api", "feat/checkout-v2", false),
                ("shared-ui", "develop", false),
            ]
        );
        assert_eq!(g[0].base, "develop");
        assert!(g[2].path.ends_with(".projects/shop/shared-ui"));
        // Focus on Billing's Worktree: Shop's groups are the link checkouts.
        let (s, w) = sc.worktrees["tokens"].clone();
        let g = change_groups(&sc.state, "shop", &sc.links, Some((&s, &w)));
        assert!(g.iter().all(|g| g.worktree.is_none()));
        // The label follows the checkout's current branch, else the base.
        let mut links = sc.links.clone();
        let i = links.iter().position(|l| l.source == "shop-api").unwrap();
        links[i].head = Some("hotfix/pay".into());
        let g = change_groups(&sc.state, "shop", &links, Some((&s, &w)));
        assert_eq!(g[1].label, "hotfix/pay");
        links[i].head = None;
        let g = change_groups(&sc.state, "shop", &links, Some((&s, &w)));
        assert_eq!(g[1].label, links[i].base);
        assert_eq!(group_meta(3, 4), "↑3 · 4");
        assert_eq!(group_meta(0, 3), "3");
        assert_eq!(group_meta(2, 0), "clean");
        assert_eq!(
            changes_header("Shop", 212, 58),
            ("PROJECT · SHOP".into(), "+212".into(), "−58".into())
        );
    }

    #[test]
    fn changes_file_rows_letters_counts_and_totals() {
        let counts = numstat(
            "84\t31\tsrc/checkout/Form.tsx\n12\t9\tsrc/api/quote.ts\n0\t18\tsrc/checkout/legacy.ts\n-\t-\tlogo.png\n3\t1\tsrc/{a.ts => b.ts}\n",
        );
        assert_eq!(counts["src/checkout/Form.tsx"], (84, 31));
        assert_eq!(counts["logo.png"], (0, 0));
        assert_eq!(counts["src/b.ts"], (3, 1));
        let entries = vec![
            ("src/checkout/Steps.tsx".to_string(), '?', '?'),
            ("src/checkout/Form.tsx".to_string(), '.', 'M'),
            ("src/api/quote.ts".to_string(), 'M', '.'),
            ("src/checkout/legacy.ts".to_string(), 'D', '.'),
        ];
        let rows = file_rows(&entries, &counts, &|p| {
            if p.ends_with("Steps.tsx") {
                66
            } else {
                0
            }
        });
        let got: Vec<(char, &str, &str, usize, usize, bool)> = rows
            .iter()
            .map(|r| {
                (
                    r.letter,
                    r.dir.as_str(),
                    r.name.as_str(),
                    r.add,
                    r.del,
                    r.staged,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ('M', "src/api/", "quote.ts", 12, 9, true),
                ('M', "src/checkout/", "Form.tsx", 84, 31, false),
                ('A', "src/checkout/", "Steps.tsx", 66, 0, false),
                ('D', "src/checkout/", "legacy.ts", 0, 18, true),
            ]
        );
        assert_eq!(totals(&rows), (162, 58));
        assert_eq!(file_letter('M', 'M'), 'M');
    }

    #[test]
    fn drift_card_texts_and_buttons_follow_the_actions() {
        let d =
            crate::drift::drift("develop", &Head::Branch("feat/checkout-v2".into()), 3).unwrap();
        let c = drift_card("shop-api", "Shop", &d);
        assert_eq!(
            c.full(),
            "shop-api is on feat/checkout-v2, Shop's base is develop. 3 uncommitted changes, so RADE won't switch it."
        );
        let labels: Vec<&str> = c.buttons.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(labels, ["Keep, show in Shop", "Move to a Worktree…"]);
        let d =
            crate::drift::drift("develop", &Head::Branch("feat/checkout-v2".into()), 0).unwrap();
        let c = drift_card("shop-api", "Shop", &d);
        let labels: Vec<&str> = c.buttons.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            labels,
            ["Back to base", "Keep, show in Shop", "Move to a Worktree…"]
        );
        assert_eq!(c.sub, "It is clean: Back to base switches it to develop.");
    }

    #[test]
    fn r132_e_open_terminal_menu_puts_the_last_link_first() {
        let l = |s: &str, b: &str| (s.to_string(), s.to_string(), b.to_string());
        let links = [
            l("shop-web", "develop"),
            l("shop-api", "develop"),
            l("shared-ui", "develop"),
        ];
        let names = |v: Vec<(String, String)>| v.into_iter().map(|(_, n)| n).collect::<Vec<_>>();
        assert_eq!(
            names(open_terminal_menu(&links, None)),
            [
                "shop-web · develop",
                "shop-api · develop",
                "shared-ui · develop"
            ]
        );
        assert_eq!(
            names(open_terminal_menu(&links, Some("shared-ui"))),
            [
                "shared-ui · develop",
                "shop-web · develop",
                "shop-api · develop"
            ]
        );
        assert_eq!(open_terminal_menu(&links, Some("gone")).len(), 3);
    }
}
