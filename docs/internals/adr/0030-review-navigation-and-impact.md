# Code navigation in the Review and a diff's Impact

Status: accepted 2026-10-06 (M29, PRD R87–R88). Owner decisions 4 and 5 in docs/internals/research/code-navigation.md ("Owner decisions (2026-10-05)"); the spike's "Keeping results fresh" and "Impact list" recommendations (docs/internals/research/code-nav-spike.md). Builds on ADR-0029 (app-owned language servers, text matches when they can't answer) and ADR-0020 (deterministic facts in, judgment out).

Amended by ADR-0033 (1.0.0): Impact no longer goes into Batch Reviewer or Initiative acceptance prompts; the Fan-out Judge keeps it.

Reviewing an Agent's diff needs two things the Viewer alone does not give: tracing names on the diff's lines, and "who else calls what this diff changed?" — for the owner and for the Agents that review (Batch Reviewer, Fan-out Judge, Initiative acceptance).

## Decision

1. **Diff lines navigate like the Viewer.** The Review Pane has its own F12 / Ctrl+click, Shift+F12, Alt+Shift+H, Ctrl+F12. A diff row is a label, not a text view, so a click sets a **point** (row + character from Pango's `xy_to_index`) and the footer names the word there; keys ask about the point. Added and context lines ask the Worktree's server at their new position.
2. **Removed lines: the current tree, else the base, labelled** (owner decision 4). The name is looked up as a declaration in the current tree (text search); found → the query is asked from there (the hint says so). Not found → `git grep -w` at the base commit, state `base`, "text matches at the base". No second checkout or server at the merge-base (it would double memory, spike).
3. **Jumps from the Review open the Tab's Viewer** (a new one beside the Review when there is none) as a code jump: the Viewer's place goes on Back and Forward is cleared, so Alt+Left walks back through the jumps made from the Review. The Review itself stays where it was.
4. **Impact is computed in `rade-core` (`code/impact.rs`) from the diff and the server.** Changed symbols = the innermost `documentSymbol` range holding an added line or both lines around a removal (locals of functions skipped; `added` when the line with its name is new and no removed line declared the name); callers = incoming calls for functions (each call site), references otherwise, minus the diff's added lines and the symbol itself; plus whole-word matches in test files the server missed (mocks typed `any`, spike) and removed symbols still used (text). Before asking, RADE sends `workspace/didChangeWatchedFiles` for the diff's files (spike: cheap freshness for client-watch servers). Without a server the same list comes from declarations at or above the changed lines and text matches — every such entry labelled.
5. **Never block for long.** Impact runs with a time budget (UI 30 s, prompts 20 s, CLI `--wait` 60 s): it waits for an indexing server only up to `wait` (the UI asks first without waiting and retries by itself, like the Code page), stops asking after the deadline, and the server part runs on a thread the caller abandons after three quarters of the budget (a server hanging even in `initialize` cannot hold it) — then a text-only answer gets the rest, saying why. The text path stops looking up callers at its deadline and lists at most 60 symbols in all.
6. **Impact in the Review Pane is a section under the header, behind an Impact toggle**, not on the Code page: the Code page groups results by file for one name; Impact groups callers by changed symbol and belongs next to the diff it explains. The toggle is the explicit action that starts a server (ADR-0029 "first navigation"); while shown it follows diff changes.
7. **Agents get it automatically** (owner decision 5): the Batch Reviewer (per Source), the Fan-out Judge (per candidate, in the candidates' shuffled order) and the Initiative acceptance Agent (the integration branch) get an `## Impact` section of at most 60 lines appended to their rubric; nothing when no symbol changed. Review Comments sent to an Agent don't (the owner chose what to say). `rade code impact` gives the full list.
8. **Low items from M28's acceptance.** Requests queued behind one a hung server never answers each wait at most their own timeout (the document lock is taken with a deadline, and the wait counts toward the request); a start that failed while a request waited for it is not retried by that request at once. The app's file descriptors (GPU and others libraries open without `O_CLOEXEC`) are marked close-on-exec before Ghostty starts a Terminal's process, and language servers, verify runs and shell probes close them on exec (`close_range`).

## Considered Options

- **A second server on a merge-base checkout for removed lines**: exact, about 2× memory and a slow start per review. Rejected (owner decision 4).
- **Impact on the Code page**: one place for all results, but its rows are grouped by file for one query; a symbol → callers tree would need a second layout there, and the reviewer would leave the diff. Rejected.
- **Compute Impact as soon as a Review opens**: starts a 1.5–2 GB server for every Review shown. Rejected; behind the toggle (and automatic for Agent prompts, which owner decision 5 asks for).
- **Ask Agents to run `rade code impact` themselves** (owner option 5b): every Reviewer would have to remember to; a fixed section is deterministic for any Agent (Claude Code, Codex, OpenCode). Rejected as the only path; the CLI exists too.

## Consequences

- A Batch Reviewer, Judge or acceptance Agent may start up to 20 s later when a server is slow; usually well under a second once warm.
- Reviews of Task Worktrees start a server per Worktree under review (`max_servers` still caps them; idle stop reclaims them).
- Text matches in prompts can name unrelated code with the same name; they are labelled so the Reviewer can discount them.
