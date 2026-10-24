//! Search in a Worktree for the Right Panel's Search page (PRD R22): the
//! query options → a regex, the walk (the `ignore` crate's parallel walker:
//! `.gitignore`, `.ignore` and git's excludes applied, hidden files
//! searched, `.git` never), per-file matching with line previews, and the
//! rows the page shows. Cancellable and capped; the app runs it off the UI
//! thread.

use ignore::overrides::{Override, OverrideBuilder};
use ignore::{WalkBuilder, WalkState};
use regex::bytes::{Regex, RegexBuilder};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Stop after this many matches (R22.4).
pub const MAX_MATCHES: usize = 2000;
/// Files bigger than this are skipped (R22.2).
pub const MAX_FILE_BYTES: u64 = 4 << 20;
/// A preview longer than this many characters is cut around its first match.
const PREVIEW_CHARS: usize = 240;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Options {
    pub query: String,
    /// Match Case.
    pub case: bool,
    /// Whole Word.
    pub word: bool,
    /// Regular Expression.
    pub regex: bool,
    /// Comma-separated globs (`.gitignore` syntax).
    pub include: String,
    pub exclude: String,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The query as a regex; `Ok(None)` for an empty query, `Err` = why the
/// regular expression is invalid (R22.1).
pub fn matcher(o: &Options) -> Result<Option<Regex>, String> {
    if o.query.is_empty() {
        return Ok(None);
    }
    let mut pat = if o.regex {
        format!("(?:{})", o.query)
    } else {
        regex::escape(&o.query)
    };
    if o.word {
        // `\b` only beside word characters ("foo(" must still match "foo(x").
        let first = o.query.chars().next().is_some_and(is_word);
        let last = o.query.chars().last().is_some_and(is_word);
        if o.regex || first {
            pat = format!(r"\b{pat}");
        }
        if o.regex || last {
            pat = format!(r"{pat}\b");
        }
    }
    RegexBuilder::new(&pat)
        .case_insensitive(!o.case)
        .multi_line(true)
        .build()
        .map(Some)
        .map_err(|e| match e {
            regex::Error::Syntax(s) => s.lines().last().unwrap_or("invalid").trim().to_string(),
            e => e.to_string(),
        })
}

/// `a, *.rs ,b/**` → `["a", "*.rs", "b/**"]`.
pub fn globs(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(String::from)
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hit {
    /// 1-based.
    pub line: u32,
    /// The line, leading whitespace trimmed, cut around the first match.
    pub preview: String,
    /// Matches in `preview`, as char offsets `[start, end)`.
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileHits {
    /// Relative to the searched folder, `/`-separated.
    pub path: String,
    pub hits: Vec<Hit>,
    /// Matches in this file (several per line count separately).
    pub matches: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Results {
    /// Sorted by path.
    pub files: Vec<FileHits>,
    pub matches: usize,
    /// Stopped at [`MAX_MATCHES`].
    pub capped: bool,
    pub cancelled: bool,
}

impl Results {
    /// "N results in M files" (R22.3).
    pub fn summary(&self) -> String {
        let n =
            |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
        let mut s = format!(
            "{} in {}",
            n(self.matches, "result", "results"),
            n(self.files.len(), "file", "files")
        );
        if self.capped {
            s.push_str(&format!(" — showing the first {MAX_MATCHES}"));
        }
        s
    }
}

/// The preview of `line` with the byte ranges `m` of its matches (R22.3).
pub fn preview(line: &[u8], m: &[(usize, usize)]) -> (String, Vec<(usize, usize)>) {
    let chars = |b: &[u8]| String::from_utf8_lossy(b).chars().count();
    let text = String::from_utf8_lossy(line);
    let text = text.trim_end_matches(['\r', '\n']);
    let all: Vec<char> = text.chars().collect();
    let mut ranges: Vec<(usize, usize)> = m
        .iter()
        .map(|&(s, e)| (chars(&line[..s]), chars(&line[..e.min(line.len())])))
        .map(|(s, e)| (s.min(all.len()), e.min(all.len())))
        .collect();
    let indent = all.iter().take_while(|c| c.is_whitespace()).count();
    let first = ranges.first().map_or(0, |r| r.0);
    let mut start = indent.min(first);
    let mut prefix = "";
    if all.len() - start > PREVIEW_CHARS && first > start + 40 {
        start = first - 40;
        prefix = "…";
    }
    let end = (start + PREVIEW_CHARS).min(all.len());
    let mut s: String = prefix.to_string();
    s.extend(&all[start..end]);
    if end < all.len() {
        s.push('…');
    }
    let shift = prefix.chars().count();
    ranges = ranges
        .into_iter()
        .filter(|r| r.0 < end && r.1 > start)
        .map(|(a, b)| (a.max(start) - start + shift, b.min(end) - start + shift))
        .collect();
    (s, ranges)
}

/// A matching line being collected: its start, end and match ranges.
type LineAt = (usize, usize, Vec<(usize, usize)>);

/// Matches of `re` in `bytes`, grouped by line; `budget` = matches still
/// allowed. `None` for a binary file (a NUL in the first 8 KB).
pub fn search_bytes(re: &Regex, bytes: &[u8], budget: usize) -> Option<(Vec<Hit>, usize)> {
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return None;
    }
    let mut hits: Vec<Hit> = Vec::new();
    let mut count = 0;
    let (mut line_no, mut scanned) = (1u32, 0usize);
    let mut cur: Option<LineAt> = None; // line start, end, ranges
    let flush = |c: Option<LineAt>, n: u32, hits: &mut Vec<Hit>| {
        if let Some((s, e, r)) = c {
            let (preview, ranges) = preview(&bytes[s..e], &r);
            hits.push(Hit {
                line: n,
                preview,
                ranges,
            });
        }
    };
    let mut cur_no = 0;
    for m in re.find_iter(bytes) {
        if m.start() == m.end() {
            continue;
        }
        if count >= budget {
            break;
        }
        // Advance to the match's line.
        if cur.as_ref().is_none_or(|c| m.start() >= c.1) {
            flush(cur.take(), cur_no, &mut hits);
            line_no += bytes[scanned..m.start()]
                .iter()
                .filter(|b| **b == b'\n')
                .count() as u32;
            scanned = m.start();
            let s = bytes[..m.start()]
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |i| i + 1);
            let e = bytes[m.start()..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |i| m.start() + i);
            cur = Some((s, e, Vec::new()));
            cur_no = line_no;
        }
        let c = cur.as_mut().unwrap();
        c.2.push((m.start() - c.0, m.end().min(c.1) - c.0));
        count += 1;
    }
    flush(cur, cur_no, &mut hits);
    Some((hits, count))
}

fn override_of(root: &Path, globs: &[String], negate: bool) -> Result<Override, String> {
    let mut b = OverrideBuilder::new(root);
    for g in globs {
        let g = if negate { format!("!{g}") } else { g.clone() };
        b.add(&g).map_err(|e| e.to_string())?;
    }
    b.build().map_err(|e| e.to_string())
}

/// Search the files under `root` (R22.2). `cancelled` is polled per file.
pub fn search(
    root: &Path,
    o: &Options,
    max: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Results, String> {
    let Some(re) = matcher(o)? else {
        return Ok(Results::default());
    };
    let include = override_of(root, &globs(&o.include), false)?;
    let exclude = override_of(root, &globs(&o.exclude), true)?;
    let count = AtomicUsize::new(0);
    let found: Mutex<Vec<FileHits>> = Mutex::default();
    WalkBuilder::new(root)
        .hidden(false)
        .overrides(exclude)
        .filter_entry(|e| e.file_name() != ".git")
        .build_parallel()
        .run(|| {
            Box::new(|entry| {
                if cancelled() || count.load(Ordering::Relaxed) >= max {
                    return WalkState::Quit;
                }
                let Ok(e) = entry else {
                    return WalkState::Continue;
                };
                if !e.file_type().is_some_and(|t| t.is_file()) {
                    return WalkState::Continue;
                }
                let rel = e.path().strip_prefix(root).unwrap_or(e.path());
                if !include.is_empty() && !include.matched(rel, false).is_whitelist() {
                    return WalkState::Continue;
                }
                if e.metadata().map_or(true, |m| m.len() > MAX_FILE_BYTES) {
                    return WalkState::Continue;
                }
                let Ok(bytes) = std::fs::read(e.path()) else {
                    return WalkState::Continue;
                };
                // Claim at most what is left of the budget.
                let left = max.saturating_sub(count.load(Ordering::Relaxed));
                let Some((hits, n)) = search_bytes(&re, &bytes, left) else {
                    return WalkState::Continue;
                };
                if n == 0 {
                    return WalkState::Continue;
                }
                count.fetch_add(n, Ordering::Relaxed);
                found.lock().unwrap().push(FileHits {
                    path: rel.to_string_lossy().into_owned(),
                    hits,
                    matches: n,
                });
                WalkState::Continue
            })
        });
    let mut files = found.into_inner().unwrap();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    // Parallel workers may overshoot the cap by a file each: trim (by line;
    // a line's count is its highlighted ranges).
    let mut left = max;
    for f in files.iter_mut() {
        if f.matches > left {
            let mut keep = left;
            f.hits.retain(|h| {
                let k = keep > 0;
                keep = keep.saturating_sub(h.ranges.len().max(1));
                k
            });
            f.matches = left;
        }
        left -= f.matches;
    }
    files.retain(|f| !f.hits.is_empty());
    let total = files.iter().map(|f| f.matches).sum();
    Ok(Results {
        files,
        matches: total,
        capped: total >= max,
        cancelled: cancelled(),
    })
}

/// Where Search looks (R107.4): the focused link's checkout or Worktree,
/// or every link of its Project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Link,
    Project,
}

/// The folders a scope searches, each labelled by its Source:
/// `groups` = the Project's Changes groups ([`crate::home::change_groups`],
/// which already pick the focused Worktree for its link), `focused` = the
/// focused link's Source. `Link` → that group's folder (the link's
/// checkout or Worktree, which may differ from the Worktree the focused
/// Pane belongs to, R107.4); `Project` → all of them. With no group to
/// search, `root` (the focused checkout) alone, unlabelled.
pub fn scope_roots(
    scope: Scope,
    groups: &[crate::home::Group],
    focused: &str,
    root: &std::path::Path,
) -> Vec<(String, std::path::PathBuf)> {
    let r: Vec<_> = groups
        .iter()
        .filter(|g| scope == Scope::Project || g.source == focused)
        .map(|g| (g.source.clone(), g.path.clone()))
        .collect();
    if r.is_empty() {
        vec![(String::new(), root.to_path_buf())]
    } else {
        r
    }
}

/// [`search`] over several labelled folders with one budget, results
/// grouped per folder in the given order (folders with no match left out).
pub fn search_roots(
    roots: &[(String, std::path::PathBuf)],
    o: &Options,
    max: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<(String, Results)>, String> {
    let mut left = max;
    let mut out = Vec::new();
    for (label, root) in roots {
        if left == 0 {
            break;
        }
        let r = search(root, o, left, cancelled)?;
        left -= r.matches.min(left);
        if !r.files.is_empty() {
            out.push((label.clone(), r));
        }
    }
    Ok(out)
}

/// One row of the results list (R22.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    File(usize),
    Hit(usize, usize),
}

/// Each file's header, then its lines unless folded (by path).
pub fn rows(r: &Results, folded: &HashSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    for (fi, f) in r.files.iter().enumerate() {
        out.push(Row::File(fi));
        if !folded.contains(&f.path) {
            out.extend((0..f.hits.len()).map(|hi| Row::Hit(fi, hi)));
        }
    }
    out
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
