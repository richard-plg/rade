//! Reading what was appended to the CLIs' logs since last time (R109.2):
//! each file keeps an offset and inode, only complete lines are read, and a
//! file that shrank or was replaced is read again from the start. Never
//! re-reads a whole file otherwise.

use super::claude::{self, Seen};
use super::codex::{self, RolloutState};
use super::{add_tokens, Limit, ProjectOf, TokenRow};
use crate::account::AgentKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One log file's read position and the parser state at that position.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileTail {
    pub offset: u64,
    #[serde(default)]
    pub inode: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub codex: RolloutState,
    #[serde(default, skip_serializing_if = "is_default")]
    pub claude: Seen,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// Complete lines read by [`read_new`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chunk {
    pub text: String,
    /// The file shrank or was replaced: read from offset 0 (parser state
    /// reset).
    pub restarted: bool,
    /// The size cap was reached; call again for the rest.
    pub more: bool,
}

/// At most this many bytes per [`read_new`] (a line is never split).
pub const CHUNK_MAX: usize = 8 << 20;

/// The complete lines appended to `path` since `tail.offset`; advances the
/// offset past them. A trailing line without its newline is left for the
/// next read. Invalid UTF-8 is replaced (the parser then skips the line).
pub fn read_new(path: &Path, tail: &mut FileTail) -> std::io::Result<Chunk> {
    let f = std::fs::File::open(path)?;
    let meta = f.metadata()?;
    let mut out = Chunk::default();
    if (tail.inode != 0 && meta.ino() != tail.inode) || meta.len() < tail.offset {
        *tail = FileTail::default();
        out.restarted = true;
    }
    tail.inode = meta.ino();
    let mut r = BufReader::new(f);
    r.seek(SeekFrom::Start(tail.offset))?;
    let mut buf = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = r.read_until(b'\n', &mut line)?;
        if n == 0 || line.last() != Some(&b'\n') {
            break;
        }
        buf.extend_from_slice(&line);
        tail.offset += n as u64;
        if buf.len() >= CHUNK_MAX {
            out.more = true;
            break;
        }
    }
    out.text = String::from_utf8_lossy(&buf).into_owned();
    Ok(out)
}

/// The log files of an account: Codex `sessions/**/rollout-*.jsonl`, Claude
/// `projects/*/*.jsonl`; files not modified since `since` are skipped.
pub fn log_files(agent: AgentKind, config_dir: &Path, since: Option<SystemTime>) -> Vec<PathBuf> {
    let fresh = |p: &Path| match since {
        None => true,
        Some(s) => std::fs::metadata(p)
            .and_then(|m| m.modified())
            .is_ok_and(|m| m >= s),
    };
    let mut out = Vec::new();
    match agent {
        AgentKind::Codex => walk(&config_dir.join("sessions"), &mut out, 0),
        AgentKind::Claude => {
            for d in read_dir(&config_dir.join("projects")) {
                if d.is_dir() {
                    out.extend(
                        read_dir(&d)
                            .into_iter()
                            .filter(|f| f.is_file() && f.extension().is_some_and(|e| e == "jsonl")),
                    );
                }
            }
        }
    }
    out.retain(|p| fresh(p));
    out.sort();
    out
}

fn read_dir(d: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect()
}

fn walk(d: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 6 {
        return;
    }
    for p in read_dir(d) {
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        if p.is_dir() {
            walk(&p, out, depth + 1);
        } else if name.starts_with("rollout-") && name.ends_with(".jsonl") {
            out.push(p);
        }
    }
}

/// Every file's tail of one account, saved beside its Usage store.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TailBook {
    #[serde(default)]
    pub files: BTreeMap<String, FileTail>,
}

impl TailBook {
    pub fn load(path: &Path) -> TailBook {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Written atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }
}

/// What one scan of an account's logs found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scan {
    /// The newest sample of each limit seen in this scan.
    pub limits: Vec<Limit>,
    pub tokens: Vec<TokenRow>,
    pub read_bytes: u64,
    pub lines: usize,
    pub samples: usize,
    pub broken: usize,
    /// Lines handed to the JSON parser (R111-e).
    pub parsed: usize,
}

/// Read every log file's new lines (`since` skips untouched old files on a
/// first scan) and parse them. Files that vanished leave the book.
pub fn scan(
    agent: AgentKind,
    config_dir: &Path,
    book: &mut TailBook,
    project_of: ProjectOf<'_>,
    utc_offset: i64,
    since: Option<SystemTime>,
) -> Scan {
    let files = log_files(agent, config_dir, since);
    let names: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let listed: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
    book.files
        .retain(|k, _| listed.contains(k.as_str()) || Path::new(k).exists());
    let mut out = Scan::default();
    for (path, name) in files.iter().zip(names) {
        let tail = book.files.entry(name).or_default();
        while let Ok(chunk) = read_new(path, tail) {
            out.read_bytes += chunk.text.len() as u64;
            match agent {
                AgentKind::Codex => {
                    let r =
                        codex::parse_rollout(&chunk.text, &mut tail.codex, project_of, utc_offset);
                    out.lines += r.lines;
                    out.samples += r.samples;
                    out.broken += r.broken;
                    out.parsed += r.parsed;
                    for l in r.limits {
                        match out.limits.iter_mut().find(|x| x.key() == l.key()) {
                            Some(x) if l.t >= x.t => *x = l,
                            Some(_) => {}
                            None => out.limits.push(l),
                        }
                    }
                    r.tokens
                        .into_iter()
                        .for_each(|t| add_tokens(&mut out.tokens, t));
                }
                AgentKind::Claude => {
                    let r = claude::transcript_tokens(
                        &chunk.text,
                        &mut tail.claude,
                        project_of,
                        utc_offset,
                    );
                    out.lines += r.lines;
                    out.samples += r.samples;
                    out.parsed += r.parsed;
                    r.tokens
                        .into_iter()
                        .for_each(|t| add_tokens(&mut out.tokens, t));
                }
            }
            if !chunk.more {
                break;
            }
        }
    }
    // Credits last, as in one chunk's answer (a file read in several
    // chunks would otherwise interleave them).
    let (mut limits, credits): (Vec<Limit>, Vec<Limit>) =
        out.limits.drain(..).partition(|l| l.limit_id != "credits");
    limits.extend(credits);
    out.limits = limits;
    out
}

/// [`scan`] of one login at `now`: logs untouched for the store's retention
/// period are never read (on the first scan or any later one) unless they
/// are written to again.
pub fn scan_login(
    agent: AgentKind,
    config_dir: &Path,
    book: &mut TailBook,
    project_of: ProjectOf<'_>,
    utc_offset: i64,
    now: SystemTime,
) -> Scan {
    let keep = std::time::Duration::from_secs(super::store::RETENTION_DAYS as u64 * 86_400);
    let since = now.checked_sub(keep);
    scan(agent, config_dir, book, project_of, utc_offset, since)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rade-tail-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn line(i: usize) -> String {
        format!("{{\"n\":{i},\"pad\":\"{}\"}}\n", "x".repeat(i % 7))
    }

    #[test]
    fn r109_d_tail_reads_appended_bytes_truncation_and_half_lines() {
        let d = tmp("d");
        let p = d.join("rollout-1.jsonl");
        let first: String = (0..1000).map(line).collect();
        std::fs::write(&p, &first).unwrap();
        let mut t = FileTail::default();
        let c = read_new(&p, &mut t).unwrap();
        assert_eq!(c.text.len(), first.len());
        assert_eq!(t.offset, first.len() as u64);

        // 3 appended lines: exactly their bytes.
        let three: String = (1000..1003).map(line).collect();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(three.as_bytes())
            .unwrap();
        let c = read_new(&p, &mut t).unwrap();
        assert_eq!(c.text, three);
        assert!(!c.restarted);
        assert_eq!(read_new(&p, &mut t).unwrap().text, "");

        // Truncated to 10 lines (same inode): read again from offset 0.
        let ten: String = (0..10).map(line).collect();
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_len(0).unwrap();
        drop(f);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(ten.as_bytes())
            .unwrap();
        let c = read_new(&p, &mut t).unwrap();
        assert!(c.restarted);
        assert_eq!(c.text, ten);

        // A half-written line waits for its newline.
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"{\"n\":\"half").unwrap();
        assert_eq!(read_new(&p, &mut t).unwrap().text, "");
        assert_eq!(t.offset, ten.len() as u64);
        f.write_all(b"\"}\n").unwrap();
        assert_eq!(read_new(&p, &mut t).unwrap().text, "{\"n\":\"half\"}\n");

        // A replaced file (new inode) is read from the start too.
        std::fs::remove_file(&p).unwrap();
        std::fs::write(&p, line(1)).unwrap();
        let c = read_new(&p, &mut t).unwrap();
        assert!(c.restarted);
        assert_eq!(c.text, line(1));
    }

    #[test]
    fn logs_untouched_for_the_retention_stay_unread_on_every_scan() {
        let d = tmp("old");
        let day = d.join("sessions/2026/07/01");
        std::fs::create_dir_all(&day).unwrap();
        let fixture = include_str!("../../tests/fixtures/usage/rollout-0.163.0.jsonl");
        let (old, fresh) = (day.join("rollout-old.jsonl"), day.join("rollout-new.jsonl"));
        std::fs::write(&old, fixture).unwrap();
        std::fs::write(&fresh, fixture).unwrap();
        let now = SystemTime::now();
        let ago = now - std::time::Duration::from_secs(100 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(ago)
            .unwrap();
        let none = |_: &str| None;
        let mut book = TailBook::default();
        let s = scan_login(AgentKind::Codex, &d, &mut book, &none, 0, now);
        assert_eq!(s.read_bytes, fixture.len() as u64);
        // The second scan (the book is no longer empty) skips it too.
        let s = scan_login(AgentKind::Codex, &d, &mut book, &none, 0, now);
        assert_eq!(s.read_bytes, 0);
        assert_eq!(book.files.len(), 1);
        // Written to again, it is a live log.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&old)
            .unwrap()
            .write_all(b"{}\n")
            .unwrap();
        let s = scan_login(AgentKind::Codex, &d, &mut book, &none, 0, SystemTime::now());
        assert!(s.read_bytes > 0);
    }

    /// A big conversation line (the bulk of a real rollout's bytes).
    fn chatter(i: usize) -> String {
        format!(
            "{{\"timestamp\":\"2026-10-23T09:00:02.000Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{} token_count usage {i}\"}}]}}}}\n",
            "lorem ipsum ".repeat(400)
        )
    }

    #[test]
    fn r111_e_scan_reads_only_sample_lines_and_appended_bytes() {
        let d = tmp("r111e");
        let day = d.join("sessions/2026/10/23");
        std::fs::create_dir_all(&day).unwrap();
        let fixture = include_str!("../../tests/fixtures/usage/rollout-0.163.0.jsonl");
        // 7 fixture lines (6 parsed: session_meta, turn_context, 4 token_count)
        // around 2 000 conversation lines.
        let mut text = String::new();
        for (i, l) in fixture.lines().enumerate() {
            text.push_str(l);
            text.push('\n');
            for j in 0..300 {
                text.push_str(&chatter(i * 1000 + j));
            }
        }
        let p = day.join("rollout-big.jsonl");
        std::fs::write(&p, &text).unwrap();
        let none = |_: &str| None;
        let mut book = TailBook::default();
        let s = scan(AgentKind::Codex, &d, &mut book, &none, 0, None);
        assert_eq!(s.read_bytes, text.len() as u64);
        assert_eq!(s.lines, 7 + 7 * 300);
        assert_eq!(s.parsed, 6, "only sample / context lines are JSON-parsed");
        assert_eq!(s.broken, 0);
        // The same limits as the fixture alone.
        let alone = scan(
            AgentKind::Codex,
            &d,
            &mut TailBook::default(),
            &none,
            0,
            None,
        );
        assert_eq!(s.limits, alone.limits);
        let mut st = codex::RolloutState::default();
        let r = codex::parse_rollout(fixture, &mut st, &none, 0);
        assert_eq!(s.limits, r.limits);
        assert_eq!(s.tokens, r.tokens);

        // 10 appended lines: the next tail reads exactly their bytes.
        let ten: String = (0..10).map(chatter).collect();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(ten.as_bytes())
            .unwrap();
        let s = scan(AgentKind::Codex, &d, &mut book, &none, 0, None);
        assert_eq!(s.read_bytes, ten.len() as u64);
        assert_eq!((s.lines, s.parsed), (10, 0));

        // Claude transcripts: only lines that carry a usage are parsed.
        let c = d.join("claude");
        std::fs::create_dir_all(c.join("projects/-p")).unwrap();
        let tr = include_str!("../../tests/fixtures/usage/transcript-2.1.309.jsonl");
        let user = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n";
        std::fs::write(c.join("projects/-p/s.jsonl"), format!("{user}{tr}{user}")).unwrap();
        let with_usage = tr.lines().filter(|l| l.contains("\"usage\"")).count();
        let s = scan(
            AgentKind::Claude,
            &c,
            &mut TailBook::default(),
            &none,
            0,
            None,
        );
        assert_eq!(s.parsed, with_usage);
        assert_eq!(s.tokens.iter().map(TokenRow::total).sum::<u64>(), 391);
    }

    #[test]
    fn scan_finds_account_logs_and_resumes_by_offset() {
        let d = tmp("scan");
        let day = d.join("sessions/2026/10/23");
        std::fs::create_dir_all(&day).unwrap();
        let fixture = include_str!("../../tests/fixtures/usage/rollout-0.163.0.jsonl");
        std::fs::write(day.join("rollout-a.jsonl"), fixture).unwrap();
        std::fs::write(day.join("notes.txt"), "x\n").unwrap();
        let none = |_: &str| None;
        let mut book = TailBook::default();
        let s = scan(AgentKind::Codex, &d, &mut book, &none, 0, None);
        assert_eq!(s.read_bytes, fixture.len() as u64);
        assert!(s.limits.iter().any(|l| l.label() == "Spark 5-hour"));
        // Saved and loaded, the next scan reads nothing new.
        let bp = d.join("book.json");
        book.save(&bp).unwrap();
        let mut book = TailBook::load(&bp);
        let s = scan(AgentKind::Codex, &d, &mut book, &none, 0, None);
        assert_eq!((s.read_bytes, s.samples), (0, 0));

        let c = d.join("claude");
        std::fs::create_dir_all(c.join("projects/-p")).unwrap();
        let tr = include_str!("../../tests/fixtures/usage/transcript-2.1.309.jsonl");
        std::fs::write(c.join("projects/-p/s.jsonl"), tr).unwrap();
        let s = scan(
            AgentKind::Claude,
            &c,
            &mut TailBook::default(),
            &none,
            0,
            None,
        );
        assert_eq!(s.tokens.iter().map(TokenRow::total).sum::<u64>(), 391);
        assert_eq!(
            log_files(
                AgentKind::Claude,
                &c,
                Some(SystemTime::now() + std::time::Duration::from_secs(60))
            )
            .len(),
            0
        );
    }
}
