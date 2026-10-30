//! Links clicked in a Terminal (R7.1): what Ghostty hands over on Ctrl+click
//! (a matched path or URL, a path it already resolved, or an OSC 8 URI) →
//! a web link for the system browser or an existing file for the Viewer.

use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTarget {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// http(s): the system browser.
    Web(String),
    /// A file, possibly relative or `~/…` until [`resolve`]d.
    File(FileTarget),
    /// Any other scheme (mailto:, ssh:, …): the system's default handler.
    Other(String),
}

const QUOTES: &str = "\"'`";
const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];
const TRAILING: &str = ".,;:!?";

/// Strip what surrounds a path in prose: quotes, unbalanced brackets,
/// trailing punctuation (`(src/a.rs:4).` → `src/a.rs:4`, but
/// `wiki/Rust_(game)` keeps its parentheses).
fn trim(raw: &str) -> &str {
    let count = |s: &str, c: char| s.matches(c).count();
    let mut s = raw.trim();
    loop {
        let before = s;
        s = s.trim_matches(|c| QUOTES.contains(c));
        s = s.trim_end_matches(|c| TRAILING.contains(c));
        for (open, close) in PAIRS {
            if s.starts_with(open) && count(s, open) > count(s, close) {
                s = &s[1..];
            }
            if s.ends_with(close) && count(s, close) > count(s, open) {
                s = &s[..s.len() - 1];
            }
            if s.starts_with(open) && s.ends_with(close) && s.len() > 1 {
                s = &s[1..s.len() - 1];
            }
        }
        if s == before {
            return s;
        }
    }
}

pub(crate) fn percent_decode(s: &str) -> String {
    String::from_utf8_lossy(&percent_decode_bytes(s)).into_owned()
}

pub(crate) fn percent_decode_bytes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).and_then(|c| hex(*c)),
            b.get(i + 2).and_then(|c| hex(*c)),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Split `path:line[:col]` (a trailing `:` is already trimmed).
fn split_position(s: &str) -> (&str, Option<u32>, Option<u32>) {
    let num = |x: &str| {
        (!x.is_empty() && x.bytes().all(|c| c.is_ascii_digit()))
            .then(|| x.parse().ok())
            .flatten()
    };
    if let Some((rest, last)) = s.rsplit_once(':') {
        if let Some(n) = num(last) {
            if let Some((path, line)) = rest.rsplit_once(':') {
                if let Some(l) = num(line) {
                    return (path, Some(l), Some(n));
                }
            }
            return (rest, Some(n), None);
        }
    }
    (s, None, None)
}

fn scheme(s: &str) -> Option<&str> {
    let (scheme, _) = s.split_once(':')?;
    let ok = scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    ok.then_some(scheme)
}

/// Classify what Ghostty passed on a Ctrl+click.
pub fn parse(raw: &str) -> Option<Link> {
    let s = trim(raw);
    if s.is_empty() {
        return None;
    }
    match scheme(s).map(str::to_ascii_lowercase).as_deref() {
        Some("http" | "https") => return Some(Link::Web(s.to_string())),
        Some("file") => {
            // file:///p, file://host/p, file:/p; a `#L12` fragment is a line.
            let rest = &s[5..];
            let rest = match rest.strip_prefix("//") {
                Some(r) => &r[r.find('/')?..],
                None => rest,
            };
            let (rest, frag) = rest.split_once('#').unwrap_or((rest, ""));
            let rest = rest.split_once('?').map_or(rest, |(p, _)| p);
            let decoded = percent_decode(rest);
            let (path, mut line, col) = split_position(&decoded);
            if line.is_none() {
                line = frag.strip_prefix('L').and_then(|n| n.parse().ok());
            }
            return Some(Link::File(FileTarget {
                path: path.into(),
                line,
                col,
            }));
        }
        // `main.rs:42` looks like a scheme too: only `x://…` and the
        // schemes Ghostty matches without slashes count as URLs.
        Some("mailto" | "tel" | "magnet" | "news") => return Some(Link::Other(s.to_string())),
        Some(sch) if s[sch.len() + 1..].starts_with("//") => {
            return Some(Link::Other(s.to_string()))
        }
        _ => {}
    }
    let (path, line, col) = split_position(s);
    Some(Link::File(FileTarget {
        path: path.into(),
        line,
        col,
    }))
}

/// `a/./b/../c` → `a/c`, without touching the file system.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Make a file target absolute and check it exists: `~/…` → `home`, relative
/// → the first of `bases` (the Terminal's cwd, then its Worktree root) where
/// it exists. A whole string that exists as a name (a file really called
/// `a:3`) wins over reading `:3` as a line.
pub fn resolve(
    t: &FileTarget,
    raw_whole: Option<&str>,
    bases: &[&Path],
    home: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<FileTarget> {
    let candidates = |p: &Path| -> Vec<PathBuf> {
        if let Ok(rest) = p.strip_prefix("~") {
            vec![home.join(rest)]
        } else if p.is_absolute() {
            vec![p.to_path_buf()]
        } else {
            bases.iter().map(|b| b.join(p)).collect()
        }
    };
    if let Some(whole) = raw_whole.filter(|_| t.line.is_some()) {
        if let Some(p) = candidates(Path::new(trim(whole)))
            .into_iter()
            .map(|p| normalize(&p))
            .find(|p| exists(p))
        {
            return Some(FileTarget {
                path: p,
                line: None,
                col: None,
            });
        }
    }
    let path = candidates(&t.path)
        .into_iter()
        .map(|p| normalize(&p))
        .find(|p| exists(p))?;
    Some(FileTarget {
        path,
        line: t.line,
        col: t.col,
    })
}

/// The whitespace-delimited word covering character `col` of `line`
/// (Ctrl+click fallback for names Ghostty's link regex skips, e.g.
/// `main.rs:10` without a `/`).
pub fn word_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if chars.get(col).is_none_or(|c| c.is_whitespace()) {
        return None;
    }
    let start = chars[..col]
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |i| i + 1);
    let end = chars[col..]
        .iter()
        .position(|c| c.is_whitespace())
        .map_or(chars.len(), |i| col + i);
    Some(chars[start..end].iter().collect())
}

/// The word under (`row`, `col`) of a block of screen rows, continued
/// over rows a wrap split it across (R134): while the word ends its row
/// within 3 columns of the right edge (the terminal's soft wrap, or
/// Claude Code / Codex breaking a long token at the width), the next
/// row's first word is appended; the same backwards. A URL takes only
/// pieces that look like more of a URL. Of
/// those joins the longest that `resolves` (a URL, an existing path) wins,
/// else the clicked row's word alone; `true` = it spans several rows.
pub fn token_across(
    rows: &[String],
    row: usize,
    col: usize,
    cols: usize,
    resolves: &dyn Fn(&str) -> bool,
) -> Option<(String, bool)> {
    let chars: Vec<Vec<char>> = rows.iter().map(|r| r.chars().collect()).collect();
    let line = chars.get(row)?;
    if line.get(col).is_none_or(|c| c.is_whitespace()) {
        return None;
    }
    let last = |r: &[char]| r.iter().rposition(|c| !c.is_whitespace());
    let first = |r: &[char]| r.iter().position(|c| !c.is_whitespace());
    let edge = |i: usize| {
        chars
            .get(i)
            .and_then(|r| last(r))
            .is_some_and(|l| l + 4 >= cols)
    };
    let start = line[..col]
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |i| i + 1);
    let end = line[col..]
        .iter()
        .position(|c| c.is_whitespace())
        .map_or(line.len(), |i| col + i);
    let word: String = line[start..end].iter().collect();
    // Pieces after: the next row's first word, while the current one ends
    // the row at the edge.
    let mut after: Vec<String> = Vec::new();
    let (mut r, mut ends) = (row, last(line) == Some(end - 1) && edge(row));
    while ends && after.len() < 8 {
        let Some(next) = chars.get(r + 1) else { break };
        let Some(f) = first(next) else { break };
        let e = next[f..]
            .iter()
            .position(|c| c.is_whitespace())
            .map_or(next.len(), |i| f + i);
        after.push(next[f..e].iter().collect());
        r += 1;
        ends = last(next) == Some(e - 1) && edge(r);
    }
    // Pieces before: the row above's last word, while the current one
    // starts its row and the row above ends at the edge.
    let mut before: Vec<String> = Vec::new();
    let (mut r, mut starts) = (row, first(line) == Some(start));
    while starts && r > 0 && before.len() < 8 {
        let prev = &chars[r - 1];
        if !edge(r - 1) {
            break;
        }
        let Some(l) = last(prev) else { break };
        let s = prev[..=l]
            .iter()
            .rposition(|c| c.is_whitespace())
            .map_or(0, |i| i + 1);
        before.push(prev[s..=l].iter().collect());
        r -= 1;
        starts = first(prev) == Some(s);
    }
    before.reverse();
    // A URL goes on only with pieces that look like more of a URL.
    let urlish = |p: &str| p.contains(['/', '?', '=', '&', '%', '#', '.', '-', '_']);
    if before.iter().any(|b| b.contains("://")) && !urlish(&word) {
        before.clear();
    }
    if word.contains("://") || before.iter().any(|b| b.contains("://")) {
        if let Some(i) = after.iter().position(|p| !urlish(p)) {
            after.truncate(i);
        }
    }
    let (nb, na) = (before.len(), after.len());
    let mut best: Option<String> = None;
    for b in 0..=nb {
        for a in 0..=na {
            if a == 0 && b == 0 {
                continue;
            }
            let t: String = before[nb - b..]
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(word.as_str()))
                .chain(after[..a].iter().map(String::as_str))
                .collect();
            if best
                .as_ref()
                .is_none_or(|x| t.chars().count() > x.chars().count())
                && resolves(&t)
            {
                best = Some(t);
            }
        }
    }
    Some(match best {
        Some(t) => (t, true),
        None => (word, false),
    })
}

/// [`parse`] + [`resolve`]: `None` for a file that does not exist.
pub fn open_target(raw: &str, cwd: &Path, root: &Path, home: &Path) -> Option<Link> {
    match parse(raw)? {
        Link::File(t) => {
            let exists = |p: &Path| p.exists();
            resolve(&t, Some(raw), &[cwd, root], home, &exists).map(Link::File)
        }
        other => Some(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r134_a_token_across_wrapped_rows() {
        let rows = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let any = |_: &str| true;
        // A soft wrap: the first row is full (20 columns).
        let r = rows(&["see src/very/long/pa", "th/file.rs for it"]);
        assert_eq!(
            token_across(&r, 0, 6, 20, &|t| t == "src/very/long/path/file.rs"),
            Some(("src/very/long/path/file.rs".into(), true))
        );
        // …clicked on the second row.
        assert_eq!(
            token_across(&r, 1, 2, 20, &|t| t == "src/very/long/path/file.rs"),
            Some(("src/very/long/path/file.rs".into(), true))
        );
        // A TUI's hard wrap 2 columns short of the width, the next row indented.
        let r = rows(&["⏺ Open https://ex.am", "  ple.com/a/b/c then", "  check it"]);
        assert_eq!(
            token_across(&r, 0, 9, 22, &any),
            Some(("https://ex.ample.com/a/b/c".into(), true))
        );
        // Three rows.
        let r = rows(&["docs/aaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbbbbbb", "/c.md done"]);
        let want = "docs/aaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbb/c.md";
        assert_eq!(
            token_across(&r, 1, 3, 20, &|t| t == want),
            Some((want.to_string(), true))
        );
        // A join that resolves to nothing: the clicked row's word alone.
        let r = rows(&["word at the edge xxxx", "yyyy and more"]);
        assert_eq!(
            token_across(&r, 0, 18, 21, &|_| false),
            Some(("xxxx".into(), false))
        );
        // m4's case: a URL ending a short row above a plain word — no join.
        let r = rows(&["web https://example.com/rade?q=1", "osc8-guide"]);
        assert_eq!(
            token_across(&r, 1, 2, 80, &any),
            Some(("osc8-guide".into(), false))
        );
        let r = rows(&["see https://example.com/rad", "e?q=1"]);
        assert_eq!(
            token_across(&r, 1, 1, 27, &any),
            Some(("https://example.com/rade?q=1".into(), true))
        );
        assert_eq!(
            token_across(
                &rows(&["see https://example.com/rad", "word"]),
                1,
                1,
                27,
                &any
            ),
            Some(("word".into(), false))
        );
        // Not at the edge (the next row is wider): never joined.
        let r = rows(&["short path.rs", "next/row.rs and a much longer row of text"]);
        assert_eq!(
            token_across(&r, 0, 7, 40, &any),
            Some(("path.rs".into(), false))
        );
        assert_eq!(token_across(&r, 0, 5, 40, &any), None);
        // A URL ending a full row, then plain words: not part of it.
        let r = rows(&["see https://ex.am/a", "and more words here"]);
        assert_eq!(
            token_across(&r, 0, 6, 19, &any),
            Some(("https://ex.am/a".into(), false))
        );
    }

    fn file(p: &str, line: Option<u32>, col: Option<u32>) -> Option<Link> {
        Some(Link::File(FileTarget {
            path: p.into(),
            line,
            col,
        }))
    }

    #[test]
    fn r7_1_parses_path_line_col() {
        assert_eq!(parse("src/main.rs:42"), file("src/main.rs", Some(42), None));
        assert_eq!(
            parse("crates/rade-core/src/notify.rs:167:5"),
            file("crates/rade-core/src/notify.rs", Some(167), Some(5))
        );
        assert_eq!(parse("docs/PRD.md"), file("docs/PRD.md", None, None));
        assert_eq!(parse("/abs/x.rs:3"), file("/abs/x.rs", Some(3), None));
        assert_eq!(parse("~/notes.md"), file("~/notes.md", None, None));
        assert_eq!(parse("a.rs:x"), file("a.rs:x", None, None));
        assert_eq!(parse("main.rs:42"), file("main.rs", Some(42), None));
    }

    #[test]
    fn r7_1_strips_quotes_brackets_and_punctuation() {
        assert_eq!(
            parse("src/main.rs:42:"),
            file("src/main.rs", Some(42), None)
        );
        assert_eq!(parse("(src/a.rs:4)."), file("src/a.rs", Some(4), None));
        assert_eq!(parse("\"docs/PRD.md\","), file("docs/PRD.md", None, None));
        assert_eq!(parse("`src/lib.rs`"), file("src/lib.rs", None, None));
        assert_eq!(parse("[x/y.md]"), file("x/y.md", None, None));
        assert_eq!(parse("'a/b.rs:1:2';"), file("a/b.rs", Some(1), Some(2)));
        assert_eq!(parse("  "), None);
    }

    #[test]
    fn r7_1_urls_and_osc8_file_links() {
        assert_eq!(
            parse("https://example.com/a(b)"),
            Some(Link::Web("https://example.com/a(b)".into()))
        );
        assert_eq!(
            parse("HTTP://x.org"),
            Some(Link::Web("HTTP://x.org".into()))
        );
        assert_eq!(
            parse("mailto:a@b.c"),
            Some(Link::Other("mailto:a@b.c".into()))
        );
        assert_eq!(parse("ssh://h/x"), Some(Link::Other("ssh://h/x".into())));
        assert_eq!(
            parse("file:///home/u/My%20Doc.md"),
            file("/home/u/My Doc.md", None, None)
        );
        assert_eq!(
            parse("file://laptop/home/u/a.rs:12:3"),
            file("/home/u/a.rs", Some(12), Some(3))
        );
        assert_eq!(parse("file:///x/a.rs#L9"), file("/x/a.rs", Some(9), None));
    }

    #[test]
    fn r7_1_word_under_the_click() {
        let l = "  edited main.rs:10, and README.md";
        assert_eq!(word_at(l, 9).as_deref(), Some("main.rs:10,"));
        assert_eq!(word_at(l, 18).as_deref(), Some("main.rs:10,"));
        assert_eq!(word_at(l, 33).as_deref(), Some("README.md"));
        assert_eq!(word_at(l, 8), None, "a space");
        assert_eq!(word_at(l, 99), None, "past the end");
        assert_eq!(word_at("ä/b.md x", 0).as_deref(), Some("ä/b.md"));
    }

    #[test]
    fn r7_1_resolves_cwd_then_root_then_home() {
        let have = ["/wt/sub/a.rs", "/wt/docs/PRD.md", "/h/n.md", "/wt/odd:3"];
        let exists = |p: &Path| have.iter().any(|h| Path::new(h) == p);
        let r = |raw: &str| {
            let Some(Link::File(t)) = parse(raw) else {
                panic!("{raw}")
            };
            resolve(
                &t,
                Some(raw),
                &[Path::new("/wt/sub"), Path::new("/wt")],
                Path::new("/h"),
                &exists,
            )
        };
        let t = |p: &str, l| {
            Some(FileTarget {
                path: p.into(),
                line: l,
                col: None,
            })
        };
        assert_eq!(r("a.rs:7"), t("/wt/sub/a.rs", Some(7)), "cwd");
        assert_eq!(
            r("docs/PRD.md"),
            t("/wt/docs/PRD.md", None),
            "Worktree root"
        );
        assert_eq!(r("../docs/PRD.md"), t("/wt/docs/PRD.md", None), "..");
        assert_eq!(r("./a.rs"), t("/wt/sub/a.rs", None));
        assert_eq!(r("~/n.md"), t("/h/n.md", None), "home");
        assert_eq!(r("/wt/sub/a.rs:2"), t("/wt/sub/a.rs", Some(2)));
        assert_eq!(r("odd:3"), t("/wt/odd:3", None), "the whole name exists");
        assert_eq!(r("missing.rs:3"), None);
    }
}
