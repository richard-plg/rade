//! The Fan-out Judge (PRD R15.3): its read-only launch command, its prompt
//! (bias mitigations: neutral names, random order, rubric, fixed ending)
//! and reading its verdict. Part of [`crate::fanout`].

use super::Fanout;
use crate::diff::FileDiff;
use crate::preset::Preset;
use std::path::{Path, PathBuf};

/// Diffs up to this size (all candidates together) go into the Judge's
/// prompt itself; bigger ones are only named as files. The prompt is one
/// command-line argument, which Linux caps at 128 KiB.
pub const INLINE_DIFF_BYTES: usize = 48 * 1024;

/// The Judge's launch command (R15.3), without prompt or session id:
/// the Preset's command made read-only where the Agent allows it —
/// Claude Code gets only the Read/Grep/Glob tools and no MCP servers (so
/// even a `--dangerously-skip-permissions` override can't edit) plus read
/// access to `dir` and the candidates' Worktrees (`reads`); Codex runs in its read-only sandbox (bypass/sandbox/approval
/// flags of the override dropped). Other Presets are only told (prompt).
/// Returns (command, read_only).
pub fn judge_command(preset: &Preset, dir: &Path, reads: &[PathBuf]) -> (String, bool) {
    let q = |p: &Path| crate::preset::shell_quote(&p.display().to_string());
    match preset.binary() {
        // `--add-dir` and `--tools` take several values: each is ended by
        // the next flag; `--strict-mcp-config` drops MCP servers (their
        // tools could write).
        "claude" => {
            let dirs: Vec<String> = std::iter::once(dir)
                .chain(reads.iter().map(PathBuf::as_path))
                .map(q)
                .collect();
            (
                format!(
                    "{} --add-dir {} --tools Read,Grep,Glob --strict-mcp-config",
                    preset.command,
                    dirs.join(" ")
                ),
                true,
            )
        }
        "codex" => {
            let mut words: Vec<&str> = Vec::new();
            let mut it = crate::preset::shell_words(&preset.command).into_iter();
            while let Some(w) = it.next() {
                match w {
                    "--dangerously-bypass-approvals-and-sandbox" | "--yolo" | "--full-auto" => {}
                    "-s" | "--sandbox" | "-a" | "--ask-for-approval" => {
                        it.next();
                    }
                    w if w.starts_with("--sandbox=") || w.starts_with("--ask-for-approval=") => {}
                    w => words.push(w),
                }
            }
            (
                format!(
                    "{} --sandbox read-only --ask-for-approval never",
                    words.join(" ")
                ),
                true,
            )
        }
        _ => (preset.command.clone(), false),
    }
}

/// One candidate as the Judge sees it: by Worktree name (not the Agent's,
/// so it can't favour its own model), with its size and diff.
pub struct Candidate {
    pub name: String,
    pub files: usize,
    pub added: u32,
    pub removed: u32,
    pub diff: String,
    pub diff_file: PathBuf,
}

/// The Judge's prompt: the task, the candidates in the given (shuffled)
/// order with their diffs inline when small, `rubric` (each candidate's
/// Impact, R88.4; correctness first, length is no merit), and a fixed
/// ending RADE parses ([`parse_verdict`]).
/// Its example lines hold no real name, so an Agent echoing the prompt
/// can't be misread as a verdict.
pub fn judge_prompt(task: &str, cands: &[Candidate], rubric: &str) -> String {
    let mut s = format!(
        "You are the Judge of a RADE Fan-out. The same task was given to {} coding agents, \
         each working alone in its own git worktree. Compare their results and recommend the best one. \
         This is a read-only review: do not edit, create or delete any file, and run nothing that changes anything.\n\n\
         The task:\n<<<\n{}\n>>>\n\n\
         The candidates, in random order (every diff is against the same base commit; one with no changes did nothing):\n",
        cands.len(),
        task.trim()
    );
    for c in cands {
        s += &format!(
            "- {}: {} file(s) changed, +{} -{}; diff: {}\n",
            c.name,
            c.files,
            c.added,
            c.removed,
            c.diff_file.display()
        );
    }
    let total: usize = cands.iter().map(|c| c.diff.len()).sum();
    if total <= INLINE_DIFF_BYTES {
        for c in cands {
            s += &format!("\n### {}\n```diff\n{}```\n", c.name, c.diff);
        }
    } else {
        s += "\nThe diffs are too large to include here: read the files listed above.\n";
    }
    s += rubric;
    s += "\nJudge correctness and completeness against the task first, then scope (no unrelated \
          changes), then code quality and tests. A longer diff is not better for being longer. \
          Think it through, then give each candidate a one-line reason, best first. End your answer \
          with exactly these two lines and nothing after them:\n\
          RANKING: <all candidate names, best first, comma-separated>\n\
          ";
    s + PROMPT_END + "\n"
}

/// A Judge's recommendation: Member labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub pick: String,
    pub ranking: Vec<String>,
}

/// Find the verdict in the Judge's screen text: the last `WINNER:` line
/// naming a Member (by Worktree name or label, any case, markdown and
/// punctuation ignored, anywhere on the line — TUIs prefix `⏺`/`•`), and
/// the last `RANKING:` line's Members. Placeholders and prose don't match.
/// The prompt's last line: only text after it (the Judge's answer, not an
/// echoed prompt whose task or diffs might hold a `WINNER:` line) is read.
pub const PROMPT_END: &str = "WINNER: <the best candidate's name>";

pub fn parse_verdict(text: &str, f: &Fanout) -> Option<Verdict> {
    let text = text
        .rfind(PROMPT_END)
        .map_or(text, |i| &text[i + PROMPT_END.len()..]);
    let names = |line: &str, key: &str| -> Option<Vec<String>> {
        let up = line.to_ascii_uppercase();
        let at = up.rfind(&format!("{key}:"))?;
        let rest = &line[at + key.len() + 1..];
        let mut out: Vec<String> = Vec::new();
        for w in rest.split(|c: char| c.is_whitespace() || ",;>|".contains(c)) {
            let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || "-_".contains(c)));
            if let Some(m) = f.member(w) {
                if !out.contains(&m.label) {
                    out.push(m.label.clone());
                }
            }
        }
        (!out.is_empty()).then_some(out)
    };
    let mut pick = None;
    let mut ranking = Vec::new();
    for line in text.lines() {
        if let Some(n) = names(line, "WINNER") {
            pick = Some(n[0].clone());
        }
        if let Some(n) = names(line, "RANKING") {
            ranking = n;
        }
    }
    let pick = pick?;
    if ranking.is_empty() {
        ranking.push(pick.clone());
    }
    Some(Verdict { pick, ranking })
}

/// A Worktree's changes as a plain unified diff (for the Judge's files).
pub fn render_patch(files: &[FileDiff]) -> String {
    let mut s = String::new();
    for f in files {
        let old = f.old_path.as_deref().unwrap_or(&f.path);
        s += &format!("--- a/{old}\n+++ b/{}\n", f.path);
        if f.binary {
            s += "Binary file changed\n";
        }
        for h in &f.hunks {
            s += &h.header;
            s.push('\n');
            for l in &h.lines {
                s.push(match l.kind {
                    crate::diff::LineKind::Added => '+',
                    crate::diff::LineKind::Removed => '-',
                    crate::diff::LineKind::Context => ' ',
                });
                s += &l.text;
                s.push('\n');
            }
        }
    }
    s
}

/// A permutation of `0..n` from `seed` (candidate order for the Judge:
/// position bias is real in LLM judges).
pub fn shuffle(n: usize, seed: u64) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    let mut x = seed | 1;
    for i in (1..n).rev() {
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
    v
}
