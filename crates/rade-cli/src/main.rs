//! `rade` — control CLI for the running RADE app (docs/API.md).

mod account;
mod browser;
mod chromium;
mod clone;
mod code;
mod fanout;
mod local;
mod project;
mod version;
mod workspace;

use clap::{Parser, Subcommand, ValueEnum};
use rade_core::paths;
use rade_core::rpc::{Error, ErrorKind, Request, Response};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "rade", version, about = "Control the running RADE app")]
struct Cli {
    /// Print the raw JSON result.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum TabArg {
    New,
    Current,
}

#[derive(Clone, Copy, ValueEnum)]
enum SplitArg {
    Right,
    Down,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open a Terminal: an Agent (`--agent`), a command (`--cmd`), or a shell.
    Spawn {
        /// Agent Preset (claude, codex, agy, opencode, or from config.toml).
        #[arg(long)]
        agent: Option<String>,
        /// Command to run instead of the user's shell (with --agent: instead
        /// of the Preset's command, keeping its account and config).
        #[arg(long)]
        cmd: Option<String>,
        /// Agent Account of this Workspace (default: your Pane's, else the
        /// Workspace default for the Preset's agent).
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_enum)]
        tab: Option<TabArg>,
        #[arg(long, value_enum)]
        split: Option<SplitArg>,
        /// Worktree name, or `new` to create one (named --name, else wt-<agent>-<n>).
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        source: Option<String>,
        /// Prompt for the Agent.
        prompt: Option<String>,
    },
    /// Type text into a Pane.
    Send {
        pane: String,
        text: String,
        /// Press Enter after the text.
        #[arg(long)]
        enter: bool,
        /// Send as a bracketed paste.
        #[arg(long)]
        paste: bool,
    },
    /// Print a Pane's screen text.
    Read {
        pane: String,
        /// Last N lines, scrollback included.
        #[arg(long)]
        lines: Option<usize>,
        /// Everything, scrollback included.
        #[arg(long)]
        scrollback: bool,
    },
    /// Block until an Agent Status, idle output, or /regex/ on screen.
    Wait {
        pane: String,
        /// done | waiting | exited | error | running | idle | /regex/
        #[arg(long)]
        until: String,
        /// Seconds; exit 6 when exceeded.
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// List Panes.
    List {
        #[arg(long)]
        source: Option<String>,
        /// Panes of this Workspace (default: yours, else the shown one).
        #[arg(long, conflicts_with = "all")]
        workspace: Option<String>,
        /// Panes of every Workspace.
        #[arg(long)]
        all: bool,
        /// Indent Subagents under their Parent.
        #[arg(long)]
        tree: bool,
    },
    /// Close a Pane (its Subagents stay).
    Close {
        pane: String,
        #[arg(long)]
        force: bool,
    },
    /// Show and focus a Pane.
    Focus { pane: String },
    /// Name a Pane (shown in its caption; addresses it in commands). Empty: no name.
    Rename { pane: String, name: String },
    /// Tabs: list, rename, close (others / to the right).
    #[command(subcommand)]
    Tab(TabCmd),
    /// Set Agent Status / session id (the Agent hook entry point).
    Status {
        /// running | waiting | done | error | exited (omit to only record ids)
        state: Option<String>,
        /// Default: $RADE_PANE_ID.
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        /// Mark the Pane as this Agent Preset.
        #[arg(long)]
        agent: Option<String>,
        /// Hook mode: session id from stdin JSON, silent, always exit 0.
        #[arg(long)]
        hook: bool,
        /// Hook payload passed as an argument (Codex `notify`).
        #[arg(hide = true)]
        payload: Option<String>,
        /// Claude Code statusline wrapper (set by `rade setup`): record the
        /// usage limits on stdin, then run `--then`; prints only its output.
        #[arg(long, conflicts_with_all = ["state", "hook"])]
        statusline: bool,
        /// With --statusline: the previous statusline command (`sh -c`).
        #[arg(long, requires = "statusline")]
        then: Option<String>,
    },
    /// Raise a Notification on a Pane.
    Notify {
        text: String,
        #[arg(long)]
        title: Option<String>,
        /// Default: $RADE_PANE_ID.
        #[arg(long)]
        pane: Option<String>,
        /// Also set Agent Status.
        #[arg(long)]
        status: Option<String>,
    },
    /// Show a file to the user in a Viewer: `path`, `path:line`, `path:line:col`.
    Open {
        /// Relative to the current directory; `~/` is expanded.
        target: String,
        /// Open beside this Terminal (default: $RADE_PANE_ID, else the Pane on screen).
        #[arg(long)]
        pane: Option<String>,
        /// In a new Viewer beside it, not the Tab's Viewer (Open to the Side).
        #[arg(long)]
        side: bool,
    },
    /// Manage Sources (repositories or folders).
    #[command(subcommand)]
    Source(SourceCmd),
    /// Agent Accounts: a login per Agent (claude, codex) declared per Workspace.
    #[command(subcommand)]
    Account(account::AccountCmd),
    /// Projects: Source links with one base each (`<root>/.rade/projects/<id>/project.toml`).
    #[command(subcommand)]
    Project(project::ProjectCmd),
    /// Layered config: which file sets a value; trust a Source's `.rade.toml`.
    #[command(subcommand)]
    Config(project::ConfigCmd),
    /// Code navigation through language servers: def, refs, callers, callees, impl, symbols at path:line:col; impact of a Worktree's diff.
    #[command(subcommand)]
    Code(code::CodeCmd),
    /// Workspaces: identity roots (`<root>/.rade/workspace.toml`); one is shown at a time.
    #[command(subcommand)]
    Workspace(WorkspaceCmd),
    /// Manage a Source's Worktrees (git worktrees).
    #[command(subcommand)]
    Worktree(WorktreeCmd),
    /// Review Comments on a Worktree's diff, sent to its Agent as one prompt.
    #[command(subcommand)]
    Review(ReviewCmd),
    /// One prompt to several Agents, each in its own Worktree; judge, choose a Winner.
    Fanout(fanout::FanoutArgs),
    /// A Browser Pane beside you: open a URL, snapshot it with refs, click / fill / press, eval, screenshot, console.
    Browser(browser::BrowserArgs),
    /// The `--engine chromium` daemon (started by `rade browser --engine chromium open`).
    #[command(name = "__chromium", hide = true)]
    Chromium {
        socket: PathBuf,
        #[arg(long)]
        headed: bool,
    },
    /// Install Agent hooks (and the Claude Code skill). Local, no socket.
    Setup {
        /// Only this Agent (repeatable). Default: those on PATH.
        #[arg(long)]
        agent: Vec<String>,
        /// Show the diff, write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the CLI's, the running app's and the update's versions (local).
    Version {
        /// Ask GitHub once now (not with RADE_UPDATE_CHECK=0 or a Devel build).
        #[arg(long)]
        check: bool,
    },
    #[command(subcommand, hide = true)]
    Debug(DebugCmd),
}

#[derive(Subcommand)]
enum SourceCmd {
    /// Register a folder as a Source, or clone `owner/repo` / `repo` / a URL
    /// with `gh` under the Workspace root first. Already there: printed, exit 0.
    Add {
        /// A folder, or what `gh repo clone` takes (`repo` = default_owner/repo).
        spec: String,
        #[arg(long)]
        name: Option<String>,
        /// Default: the Workspace whose root holds the folder, else yours / the shown one.
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Sources of your Workspace (default), one Workspace, or all.
    List {
        #[arg(long, conflicts_with = "all")]
        workspace: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Unregister a Source (files are never touched).
    Remove { name: String },
    /// Change a Source's display name (its id stays).
    Rename { source: String, name: String },
    /// Stop every process in a Source; its Tabs and Panes stay until resumed.
    Pause {
        source: String,
        /// Also when one of its Fan-outs is at work (its Agents come back interrupted).
        #[arg(long)]
        force: bool,
    },
    /// Start a paused Source's Panes again (Agents resume their sessions).
    Resume { source: String },
}

#[derive(Subcommand)]
enum WorkspaceCmd {
    /// Register a folder as a Workspace; Sources under it move in.
    Add {
        root: PathBuf,
        /// Default: `name` in its workspace.toml, else the folder name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Every Workspace: key, Sources, Unread, waiting Agents.
    List,
    /// Show a Workspace (its last Source, Tab and focus).
    Switch { workspace: String },
    /// Unregister an empty Workspace (files are never touched).
    Remove { workspace: String },
    #[command(flatten)]
    Local(workspace::LocalCmd),
}

#[derive(Subcommand)]
enum TabCmd {
    /// List Tabs (id, label, source/worktree, panes).
    List {
        #[arg(long)]
        source: Option<String>,
    },
    /// Rename a Tab (id or name); empty: follow the terminal title again.
    Rename { tab: String, name: String },
    /// Close a Tab (id or name), or the others / those to its right.
    /// Fan-out Tabs are never closed this way (`rade fanout discard`).
    Close {
        tab: String,
        /// Every other Tab of its Worktree.
        #[arg(long, conflicts_with = "right")]
        others: bool,
        /// The Tabs to its right.
        #[arg(long)]
        right: bool,
        /// SIGKILL at once instead of SIGHUP.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum WorktreeCmd {
    /// Create a Worktree on a new branch <name> (an existing branch is checked out).
    Add {
        source: String,
        name: String,
        /// Start the new branch at this ref (default: the Source link's base,
        /// else the Source's main branch).
        #[arg(long)]
        base: Option<String>,
        /// The Project this Worktree is for (default: the only one linking the Source).
        #[arg(long)]
        project: Option<String>,
    },
    /// Remove a Worktree (refuses uncommitted changes unless --force).
    Remove {
        source: String,
        name: String,
        #[arg(long)]
        force: bool,
        /// Also delete its branch (git branch -d; -D with --force).
        #[arg(long)]
        delete_branch: bool,
    },
    /// List a Source's Worktrees (default: the caller's / the one on screen).
    List { source: Option<String> },
}

/// Worktree selection shared by `rade review …` (default: the caller's
/// Worktree, else the one on screen).
#[derive(clap::Args)]
struct WtArgs {
    #[arg(long)]
    source: Option<String>,
    #[arg(long)]
    worktree: Option<String>,
}

#[derive(Subcommand)]
enum ReviewCmd {
    /// List the pending Review Comments and the prompt they compose.
    List {
        #[command(flatten)]
        at: WtArgs,
    },
    /// Comment on new-file lines: `path:line` or `path:line-end`.
    Add {
        target: String,
        text: String,
        #[command(flatten)]
        at: WtArgs,
    },
    /// Send all comments as one prompt to the Worktree's Agent Pane, then clear them.
    Send {
        /// Agent Pane (needed when the Worktree has several).
        #[arg(long)]
        pane: Option<String>,
        /// No Agent Pane: spawn this Preset with the prompt.
        #[arg(long)]
        agent: Option<String>,
        #[command(flatten)]
        at: WtArgs,
    },
    /// Show the Worktree's Review Pane.
    Open {
        #[command(flatten)]
        at: WtArgs,
    },
    /// The Review's Rounds, open comments and Viewed progress (R119.6).
    Status {
        #[command(flatten)]
        at: WtArgs,
    },
    /// Say the work is ready for review; the summary shows in the Review header.
    Request {
        #[arg(long)]
        summary: Option<String>,
        #[command(flatten)]
        at: WtArgs,
    },
}

#[derive(Subcommand)]
enum DebugCmd {
    /// Dump the model as JSON.
    State,
    /// Render the window to a PNG.
    Screenshot { file: PathBuf },
    /// Save state and quit the app.
    Quit,
    /// Run a keybinding action by name (e.g. zoom, split_right).
    Action { name: String },
    /// Notification panel: activate <id> | toggle | mark_all_read | clear | counts.
    Panel { op: String, id: Option<u64> },
    /// Desktop notifications sent / skipped / withdrawn, with reasons.
    Desktop,
    /// Ctrl+click the first occurrence of TEXT on a Terminal's screen.
    Click { pane: String, text: String },
    /// Viewer: info | escape | back | open | js <script>.
    Viewer {
        pane: String,
        op: String,
        arg: Option<String>,
    },
    /// Review Pane: info | refresh | select <path> | toggle <text> | comment FIRST|LAST|TEXT | send [<pane>] | locate <text>.
    Review {
        pane: String,
        op: String,
        arg: Option<String>,
    },
    /// File tree: rows | expand <path> | collapse <path> | activate <path> | toggle_ignored.
    Tree { op: String, path: Option<PathBuf> },
    /// Put data on RADE's clipboard: MIME=FILE pairs (e.g. image/png=shot.png).
    Clipboard {
        #[arg(required = true)]
        items: Vec<String>,
    },
    /// Window: active, size, pointer; --pane bounds (+ --text centre); --widget bounds.
    Window {
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        text: Option<String>,
        /// A widget by CSS class or tooltip (+ --nth).
        #[arg(long)]
        widget: Option<String>,
        #[arg(long, default_value_t = 0)]
        nth: usize,
    },
    /// Command palette: ranked results for QUERY (`>` actions, `/` sources,
    /// `@` panes, `#` files); with --ui, drive the overlay: info | type <text>
    /// | key <up|down|page_up|page_down|enter|escape|tab|shift+tab> | field <key>=<value>.
    Palette {
        #[arg(long)]
        ui: bool,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Fan-out bar of a Fan-out's Tab: info | click judge|choose|discard.
    Fanout {
        fanout: String,
        #[arg(num_args = 1.., default_value = "info")]
        op: Vec<String>,
    },
    /// Context menu of a Tab / Source / Terminal: items | activate <label> |
    /// open | shot <file.png> | close (`--text`: a Terminal's menu at that text).
    Menu {
        menu: String,
        target: String,
        #[arg(default_value = "items")]
        op: String,
        arg: Option<String>,
        #[arg(long)]
        text: Option<String>,
    },
    /// The confirmation dialog on screen: info | respond <id>.
    Dialog {
        #[arg(default_value = "info")]
        op: String,
        response: Option<String>,
    },
    /// Type into the open rename entry and press Enter.
    Rename { text: String },
    /// The window's chrome: an op of `debug.ui` with its params as JSON
    /// (`geometry '{"widget":"statusline"}'`, `chrome`).
    Ui {
        #[arg(default_value = "chrome")]
        op: String,
        params: Option<String>,
    },
    /// A Terminal's search bar: info | open | type <text> | next | prev | close.
    Search {
        pane: String,
        #[arg(default_value = "info")]
        op: String,
        text: Option<String>,
    },
    /// Drop files (or --image data) onto a Terminal, as a drag-and-drop does.
    Drop {
        pane: String,
        #[arg(long)]
        image: Option<PathBuf>,
        paths: Vec<PathBuf>,
    },
}

pub(crate) fn abs(path: PathBuf) -> Result<PathBuf, Error> {
    // A quoted `~` the shell did not expand (R24.2 treats `~…` as a path).
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let path = match path.strip_prefix("~") {
        Ok(rest) => home().join(rest), // `~` or `~/…`, never `~user`
        Err(_) => path,
    };
    let p = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|e| Error::internal(e.to_string()))?
            .join(path)
    };
    Ok(p.canonicalize().unwrap_or(p))
}

/// `rade open` target → absolute path and line (the same parsing as a
/// Ctrl+click, R7.1), resolved against the current directory.
fn open_target(target: &str) -> Result<(PathBuf, Option<u32>), Error> {
    use rade_core::link::{parse, resolve, Link};
    let cwd = std::env::current_dir().map_err(|e| Error::internal(e.to_string()))?;
    let home = paths::home_dir(&paths::process_env);
    let Some(Link::File(t)) = parse(target) else {
        return Err(Error::usage(format!("not a file path: {target}")));
    };
    let exists = |p: &Path| p.exists();
    let t = resolve(&t, Some(target), &[&cwd], &home, &exists)
        .ok_or_else(|| Error::not_found(format!("no such file: {target}")))?;
    Ok((t.path, t.line))
}

impl WtArgs {
    fn json(&self) -> Value {
        json!({"source": self.source, "worktree": self.worktree})
    }
}

/// `path:line[-end]` → absolute path (relative to the current directory), lines.
fn review_target(target: &str) -> Result<(PathBuf, u32, Option<u32>), Error> {
    let bad = || Error::usage(format!("expected path:line or path:line-end, got {target}"));
    let (path, lines) = target.rsplit_once(':').ok_or_else(bad)?;
    let (a, b) = match lines.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (lines, None),
    };
    let a: u32 = a.parse().map_err(|_| bad())?;
    let b = b.map(|b| b.parse::<u32>().map_err(|_| bad())).transpose()?;
    Ok((abs(path.into())?, a, b))
}

fn request(cmd: &Cmd) -> Result<(&'static str, Value), Error> {
    Ok(match cmd {
        Cmd::Source(SourceCmd::Add {
            spec,
            name,
            workspace,
        }) => (
            "source.add",
            json!({"path": abs(spec.into())?, "name": name, "workspace": workspace}),
        ),
        Cmd::Source(SourceCmd::List { workspace, all }) => {
            ("source.list", json!({"workspace": workspace, "all": all}))
        }
        Cmd::Workspace(WorkspaceCmd::Add { root, name }) => (
            "workspace.add",
            json!({"root": abs(root.clone())?, "name": name}),
        ),
        Cmd::Workspace(WorkspaceCmd::List) => ("workspace.list", json!({})),
        Cmd::Workspace(WorkspaceCmd::Switch { workspace }) => {
            ("workspace.switch", json!({"workspace": workspace}))
        }
        Cmd::Workspace(WorkspaceCmd::Remove { workspace }) => {
            ("workspace.remove", json!({"workspace": workspace}))
        }
        Cmd::Open { target, pane, side } => {
            let (path, line) = open_target(target)?;
            (
                "viewer.open",
                json!({"path": path, "line": line, "pane": pane, "side": side}),
            )
        }
        Cmd::Source(SourceCmd::Remove { name }) => ("source.remove", json!({"source": name})),
        Cmd::Source(SourceCmd::Rename { source, name }) => {
            ("source.rename", json!({"source": source, "name": name}))
        }
        Cmd::Source(SourceCmd::Pause { source, force }) => {
            ("source.pause", json!({"source": source, "force": force}))
        }
        Cmd::Source(SourceCmd::Resume { source }) => ("source.resume", json!({"source": source})),
        Cmd::Rename { pane, name } => ("pane.rename", json!({"pane": pane, "name": name})),
        Cmd::Tab(TabCmd::List { source }) => ("tab.list", json!({"source": source})),
        Cmd::Tab(TabCmd::Rename { tab, name }) => ("tab.rename", json!({"tab": tab, "name": name})),
        Cmd::Tab(TabCmd::Close {
            tab,
            others,
            right,
            force,
        }) => (
            "tab.close",
            json!({
                "tab": tab, "force": force,
                "mode": if *others { "others" } else if *right { "right" } else { "only" },
            }),
        ),
        Cmd::Debug(DebugCmd::Menu {
            menu,
            target,
            op,
            arg,
            text,
        }) => {
            let mut p = json!({"menu": menu, "target": target, "op": op, "text": text});
            match op.as_str() {
                "activate" | "rightclick" => {
                    p["item"] = json!(arg);
                    p["arg"] = json!(arg);
                }
                "shot" => {
                    let f = arg
                        .clone()
                        .ok_or_else(|| Error::usage("shot needs a file"))?;
                    p["path"] = json!(abs(f.into())?);
                }
                _ => {}
            }
            ("debug.menu", p)
        }
        Cmd::Debug(DebugCmd::Dialog { op, response }) => {
            ("debug.dialog", json!({"op": op, "response": response}))
        }
        Cmd::Debug(DebugCmd::Rename { text }) => ("debug.rename", json!({"text": text})),
        Cmd::Debug(DebugCmd::Ui { op, params }) => {
            let mut p = match params.as_deref().map(serde_json::from_str::<Value>) {
                Some(Ok(v @ Value::Object(_))) => v,
                Some(_) => {
                    eprintln!("rade: debug ui: params must be a JSON object");
                    std::process::exit(2);
                }
                None => json!({}),
            };
            p["op"] = json!(op);
            ("debug.ui", p)
        }
        Cmd::Debug(DebugCmd::Search { pane, op, text }) => (
            "debug.search",
            json!({"pane": pane, "op": op, "text": text}),
        ),
        Cmd::Worktree(WorktreeCmd::Add {
            source,
            name,
            base,
            project,
        }) => (
            "worktree.add",
            json!({"source": source, "name": name, "base": base, "project": project}),
        ),
        Cmd::Worktree(WorktreeCmd::Remove {
            source,
            name,
            force,
            delete_branch,
        }) => (
            "worktree.remove",
            json!({"source": source, "name": name, "force": force, "delete_branch": delete_branch}),
        ),
        Cmd::Worktree(WorktreeCmd::List { source }) => ("worktree.list", json!({"source": source})),
        Cmd::Review(ReviewCmd::List { at }) => ("review.list", at.json()),
        Cmd::Review(ReviewCmd::Open { at }) => ("review.open", at.json()),
        Cmd::Review(ReviewCmd::Status { at }) => ("review.status", at.json()),
        Cmd::Review(ReviewCmd::Request { summary, at }) => {
            let mut p = at.json();
            p["summary"] = json!(summary);
            ("review.request", p)
        }
        Cmd::Review(ReviewCmd::Send { pane, agent, at }) => {
            let mut p = at.json();
            p["pane"] = json!(pane);
            p["agent"] = json!(agent);
            ("review.send", p)
        }
        Cmd::Review(ReviewCmd::Add { target, text, at }) => {
            let (path, line, end) = review_target(target)?;
            let mut p = at.json();
            p["path"] = json!(path);
            p["line"] = json!(line);
            p["end_line"] = json!(end);
            p["text"] = json!(text);
            ("review.add", p)
        }
        Cmd::List {
            source,
            workspace,
            all,
            ..
        } => (
            "pane.list",
            json!({"source": source, "workspace": workspace, "all": all}),
        ),
        Cmd::Fanout(a) => fanout::request(a),
        Cmd::Project(c) => project::request(c)?,
        Cmd::Account(c) => account::request(c).map_err(Error::usage)?,
        Cmd::Config(c) => project::config_request(c),
        Cmd::Code(c) => code::request(c)?,
        Cmd::Browser(a) => browser::request(a)?,
        Cmd::Spawn {
            agent,
            cmd,
            account,
            name,
            tab,
            split,
            worktree,
            source,
            prompt,
        } => (
            "pane.spawn",
            json!({
                "agent": agent, "cmd": cmd, "name": name, "worktree": worktree,
                "source": source, "prompt": prompt, "account": account,
                "tab": tab.map(|t| match t { TabArg::New => "new", TabArg::Current => "current" }),
                "split": split.map(|s| match s { SplitArg::Right => "right", SplitArg::Down => "down" }),
            }),
        ),
        Cmd::Send {
            pane,
            text,
            enter,
            paste,
        } => (
            "pane.send",
            json!({"pane": pane, "text": text, "enter": enter, "paste": paste}),
        ),
        Cmd::Read {
            pane,
            lines,
            scrollback,
        } => (
            "pane.read",
            json!({"pane": pane, "lines": lines, "scrollback": scrollback}),
        ),
        Cmd::Wait {
            pane,
            until,
            timeout,
        } => (
            "pane.wait",
            json!({"pane": pane, "until": until, "timeout": timeout}),
        ),
        Cmd::Focus { pane } => ("pane.focus", json!({"pane": pane})),
        Cmd::Close { pane, force } => ("pane.close", json!({"pane": pane, "force": force})),
        Cmd::Status {
            state,
            pane,
            session_id,
            agent,
            ..
        } => (
            "pane.set_status",
            json!({"status": state, "pane": pane, "session_id": session_id, "agent": agent}),
        ),
        Cmd::Notify {
            text,
            title,
            pane,
            status,
        } => (
            "notify.post",
            json!({"text": text, "title": title, "pane": pane, "status": status}),
        ),
        Cmd::Debug(DebugCmd::State) => ("debug.state", json!({})),
        Cmd::Debug(DebugCmd::Screenshot { file }) => {
            ("debug.screenshot", json!({"path": abs(file.clone())?}))
        }
        Cmd::Debug(DebugCmd::Quit) => ("debug.quit", json!({})),
        Cmd::Debug(DebugCmd::Action { name }) => ("debug.action", json!({"action": name})),
        Cmd::Debug(DebugCmd::Panel { op, id }) => ("debug.panel", json!({"op": op, "id": id})),
        Cmd::Debug(DebugCmd::Desktop) => ("debug.desktop", json!({})),
        Cmd::Debug(DebugCmd::Window {
            pane,
            text,
            widget,
            nth,
        }) => (
            "debug.window",
            json!({"pane": pane, "text": text, "widget": widget, "nth": nth}),
        ),
        Cmd::Debug(DebugCmd::Click { pane, text }) => {
            ("debug.click", json!({"pane": pane, "text": text}))
        }
        Cmd::Debug(DebugCmd::Viewer { pane, op, arg }) => {
            ("debug.viewer", json!({"pane": pane, "op": op, "arg": arg}))
        }
        Cmd::Debug(DebugCmd::Review { pane, op, arg }) => {
            ("debug.review", json!({"pane": pane, "op": op, "arg": arg}))
        }
        Cmd::Debug(DebugCmd::Fanout { fanout, op }) => (
            "debug.fanout",
            json!({"fanout": fanout, "op": op.join(" ")}),
        ),
        Cmd::Debug(DebugCmd::Tree { op, path }) => (
            "debug.tree",
            json!({"op": op, "path": path.clone().map(abs).transpose()?}),
        ),
        Cmd::Debug(DebugCmd::Palette {
            ui: false,
            limit,
            args,
        }) => (
            "debug.palette",
            json!({"query": args.join(" "), "limit": limit}),
        ),
        Cmd::Debug(DebugCmd::Palette { args, .. }) => (
            "debug.palette",
            json!({"op": args.first(), "arg": (args.len() > 1).then(|| args[1..].join(" "))}),
        ),
        Cmd::Debug(DebugCmd::Clipboard { items }) => {
            let mut pairs = Vec::new();
            for item in items {
                let (mime, file) = item
                    .split_once('=')
                    .ok_or_else(|| Error::usage(format!("expected MIME=FILE, got {item}")))?;
                pairs.push(json!([mime, abs(file.into())?]));
            }
            ("debug.clipboard", json!({ "items": pairs }))
        }
        Cmd::Debug(DebugCmd::Drop { pane, image, paths }) => (
            "debug.drop",
            json!({
                "pane": pane,
                "image": image.clone().map(abs).transpose()?,
                "paths": paths.iter().cloned().map(abs).collect::<Result<Vec<_>, _>>()?,
            }),
        ),
        Cmd::Setup { .. }
        | Cmd::Version { .. }
        | Cmd::Workspace(WorkspaceCmd::Local(_))
        | Cmd::Chromium { .. } => {
            unreachable!("local command")
        }
    })
}

/// One JSON-RPC call. `timeout` bounds socket I/O (hooks must stay fast).
fn call_at(
    socket: &Path,
    method: &str,
    mut params: Value,
    timeout: Option<Duration>,
) -> Result<Value, Error> {
    let not_running = |e: std::io::Error| {
        Error::new(
            ErrorKind::NotRunning,
            format!("RADE is not running ({}: {e})", socket.display()),
        )
    };
    let mut stream = UnixStream::connect(socket).map_err(not_running)?;
    let io = |e: std::io::Error| Error::internal(format!("socket: {e}"));
    stream.set_read_timeout(timeout).map_err(io)?;
    stream.set_write_timeout(timeout).map_err(io)?;
    if let Some(caller) = paths::process_env("RADE_PANE_ID") {
        params["caller"] = caller.into();
    }
    let mut line = serde_json::to_string(&Request::new(1, method, params)).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).map_err(io)?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).map_err(io)?;
    if reply.is_empty() {
        return Err(Error::internal("connection closed by RADE"));
    }
    let resp: Response =
        serde_json::from_str(&reply).map_err(|e| Error::internal(format!("bad reply: {e}")))?;
    resp.into_result()
}

fn call(method: &str, params: Value) -> Result<Value, Error> {
    call_at(
        &paths::socket_path(&paths::process_env),
        method,
        params,
        None,
    )
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("-")
}

fn pane_line(p: &Value) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}/{}\t{}\t{}",
        s(&p["pane"]),
        s(&p["name"]),
        s(&p["status"]),
        s(&p["agent"]),
        s(&p["source"]),
        s(&p["worktree"]),
        s(&p["tab"]),
        s(&p["cwd"])
    )
}

/// Parents first, each Subagent indented under its Parent.
fn print_tree(panes: &[Value]) {
    fn walk(panes: &[Value], p: &Value, depth: usize) {
        let unread = p["unread"].as_u64().unwrap_or(0);
        println!(
            "{}{} {} [{} {}]{}",
            "  ".repeat(depth),
            s(&p["pane"]),
            s(&p["name"]),
            s(&p["agent"]),
            s(&p["status"]),
            if unread > 0 {
                format!(" ({unread} unread)")
            } else {
                String::new()
            }
        );
        for c in panes.iter().filter(|c| c["parent"] == p["pane"]) {
            walk(panes, c, depth + 1);
        }
    }
    let ids: Vec<&Value> = panes.iter().map(|p| &p["pane"]).collect();
    for p in panes.iter().filter(|p| !ids.contains(&&p["parent"])) {
        walk(panes, p, 0);
    }
}

fn print_human(cmd: &Cmd, r: &Value) {
    match cmd {
        Cmd::Source(SourceCmd::Add { .. }) => {
            if r["existing"] == true {
                eprintln!("rade: already source '{}'", s(&r["source"]));
            }
            println!("{}", s(&r["source"]))
        }
        Cmd::Workspace(WorkspaceCmd::Add { .. }) => println!("{}", s(&r["workspace"])),
        Cmd::Workspace(WorkspaceCmd::Switch { .. }) => println!("{}", s(&r["source"])),
        Cmd::Workspace(WorkspaceCmd::List) => {
            for w in r["workspaces"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}\t{}\t{} source(s)\t{} unread\t{} waiting{}",
                    s(&w["workspace"]),
                    s(&w["name"]),
                    s(&w["root"]),
                    s(&w["key"]),
                    w["sources"],
                    w["unread"],
                    w["waiting"],
                    if w["active"] == true { "\tshown" } else { "" }
                );
            }
        }
        Cmd::Source(SourceCmd::List { .. }) => {
            for p in r["sources"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}\t{}{}",
                    s(&p["source"]),
                    s(&p["name"]),
                    s(&p["path"]),
                    s(&p["branch"]),
                    if p["paused"] == true { "\tpaused" } else { "" }
                );
            }
        }
        Cmd::Tab(TabCmd::List { .. }) => {
            for t in r["tabs"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}/{}\t{} pane(s){}{}{}",
                    s(&t["tab"]),
                    s(&t["label"]),
                    s(&t["source"]),
                    s(&t["worktree"]),
                    t["panes"].as_array().map_or(0, |a| a.len()),
                    if t["active"] == true { "\tactive" } else { "" },
                    t["fanout"]
                        .as_str()
                        .map_or(String::new(), |f| format!("\tfan-out {f}")),
                    t["task"]
                        .as_str()
                        .map_or(String::new(), |f| format!("\ttask {f}"))
                );
            }
        }
        Cmd::Tab(TabCmd::Close { .. }) => {
            for k in r["kept"].as_array().into_iter().flatten() {
                eprintln!(
                    "rade: kept {} (fan-out {}: `rade fanout discard {}`)",
                    s(&k["tab"]),
                    s(&k["fanout"]),
                    s(&k["fanout"])
                );
            }
        }
        Cmd::List { tree, .. } => {
            let panes = r["panes"].as_array().cloned().unwrap_or_default();
            if *tree {
                print_tree(&panes);
            } else {
                panes.iter().for_each(|p| println!("{}", pane_line(p)));
            }
        }
        Cmd::Spawn { .. } | Cmd::Open { .. } | Cmd::Review(ReviewCmd::Open { .. }) => {
            println!("{}", s(&r["pane"]))
        }
        Cmd::Worktree(WorktreeCmd::Add { .. }) => println!("{}", s(&r["path"])),
        Cmd::Project(c) => project::print(c, r),
        Cmd::Account(c) => account::print(c, r),
        Cmd::Config(c) => project::print_config(c, r),
        Cmd::Code(c) => code::print(c, r),
        Cmd::Worktree(WorktreeCmd::Remove { .. }) => {
            if let Some(e) = r["branch_error"].as_str() {
                eprintln!("rade: branch {} kept: {e}", s(&r["branch"]));
            }
        }
        Cmd::Worktree(WorktreeCmd::List { .. }) => {
            for w in r["worktrees"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}{}",
                    s(&w["worktree"]),
                    s(&w["branch"]),
                    s(&w["path"]),
                    if w["missing"] == true {
                        "\t(missing)"
                    } else {
                        ""
                    }
                );
            }
        }
        Cmd::Review(ReviewCmd::List { .. }) => {
            if let Some(p) = r["prompt"].as_str() {
                println!("{p}");
            }
        }
        Cmd::Review(ReviewCmd::Add { .. }) => println!("{}", s(&r["location"])),
        Cmd::Review(ReviewCmd::Send { .. }) => {
            println!(
                "sent {} comment(s) to {} (round {})",
                r["comments"],
                s(&r["pane"]),
                r["round"]
            )
        }
        Cmd::Review(ReviewCmd::Status { .. }) => {
            let round = r["rounds"].as_array().and_then(|a| a.last());
            match round {
                Some(x) => print!("Round {} · {}", x["n"], s(&x["state"]).replace('_', " ")),
                None => print!("No round yet"),
            }
            println!(
                " · {} open · {}/{} viewed",
                r["open"], r["viewed"]["done"], r["viewed"]["total"]
            );
            for c in r["comments"].as_array().into_iter().flatten() {
                if c["round"].is_null() || c["resolved"] == true {
                    continue;
                }
                let loc = match c["end_line"].as_u64() {
                    Some(e) => format!("{}:{}-{e}", s(&c["path"]), c["line"]),
                    None => format!("{}:{}", s(&c["path"]), c["line"]),
                };
                let first = s(&c["text"]).lines().next().unwrap_or("").to_string();
                println!("{loc}  {}  {first}", s(&c["state"]).replace('_', " "));
            }
        }
        Cmd::Review(ReviewCmd::Request { .. }) => {
            println!("ready for review: {}", s(&r["worktree"]))
        }
        Cmd::Read { .. } => print!("{}", s(&r["text"])),
        Cmd::Fanout(a) => fanout::print(a, r),
        Cmd::Browser(a) => browser::print(a, r),
        Cmd::Wait { .. } => match (r["status"].as_str(), r["matched"].as_str()) {
            (Some(st), Some(m)) if st != m => println!("{st}\t{m}"),
            (Some(st), _) => println!("{st}"),
            (None, m) => println!("{}", m.unwrap_or("-")),
        },
        Cmd::Debug(
            DebugCmd::State
            | DebugCmd::Desktop
            | DebugCmd::Window { .. }
            | DebugCmd::Viewer { .. }
            | DebugCmd::Review { .. }
            | DebugCmd::Palette { .. }
            | DebugCmd::Fanout { .. }
            | DebugCmd::Menu { .. }
            | DebugCmd::Dialog { .. }
            | DebugCmd::Rename { .. }
            | DebugCmd::Ui { .. }
            | DebugCmd::Search { .. }
            | DebugCmd::Tree { .. },
        ) => {
            println!("{}", serde_json::to_string_pretty(r).unwrap())
        }
        _ => {}
    }
}

/// Commands removed in 1.0.0 (ADR-0033, R101.2).
const REMOVED_COMMANDS: [&str; 7] = [
    "task",
    "batch",
    "initiative",
    "digest",
    "report",
    "flow",
    "lessons",
];

/// `rade task …` and the other removed commands: a `USAGE` error naming the
/// command, checked before clap so the user reads why (R101.2).
fn removed_command(args: &[String]) -> Option<Error> {
    let cmd = args.iter().skip(1).find(|a| !a.starts_with('-'))?;
    REMOVED_COMMANDS
        .contains(&cmd.as_str())
        .then(|| Error::usage(format!("'{cmd}' was removed in 1.0.0 (ADR-0033)")))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(e) = removed_command(&args) {
        eprintln!("rade: {e}");
        std::process::exit(e.kind.exit_code());
    }
    let cli = Cli::parse();
    let result = match &cli.cmd {
        Cmd::Status {
            statusline: true,
            then,
            ..
        } => local::statusline(then),
        Cmd::Status {
            hook: true,
            state,
            session_id,
            agent,
            payload,
            ..
        } => local::hook(state, session_id, agent, payload),
        Cmd::Setup { agent, dry_run } => local::setup(agent, *dry_run),
        Cmd::Version { check } => version::run(*check, cli.json),
        Cmd::Workspace(WorkspaceCmd::Local(c)) => workspace::run(c, cli.json),
        Cmd::Source(SourceCmd::Add {
            spec,
            name,
            workspace,
        }) if is_clone_spec(spec) => clone::add(spec, name.as_deref(), workspace.as_deref()),
        Cmd::Chromium { socket, headed } => std::process::exit(chromium::daemon(socket, *headed)),
        Cmd::Browser(a) if a.engine == browser::Engine::Chromium => {
            request(&cli.cmd).and_then(|(method, params)| match method {
                "browser.ports" => call(method, params),
                _ => chromium::call(method, params, a.headed),
            })
        }
        cmd => request(cmd).and_then(|(method, params)| call(method, params)),
    };
    match result {
        Ok(_)
            if matches!(
                cli.cmd,
                Cmd::Setup { .. }
                    | Cmd::Version { .. }
                    | Cmd::Status { hook: true, .. }
                    | Cmd::Status {
                        statusline: true,
                        ..
                    }
                    | Cmd::Workspace(WorkspaceCmd::Local(_))
            ) => {}
        Ok(v) => {
            if cli.json {
                println!("{v}");
            } else {
                print_human(&cli.cmd, &v);
            }
            if let Cmd::Project(c) = &cli.cmd {
                if project::refused(c, &v) {
                    std::process::exit(2);
                }
            }
        }
        Err(e) => {
            eprintln!("rade: {e}");
            if e.message.contains("unknown method '") {
                if let Some(line) = version::app_hint() {
                    eprintln!("{line}");
                }
            }
            std::process::exit(e.kind.exit_code());
        }
    }
}

/// `rade source add <spec>`: a clone unless it is a folder or path-like
/// (R24.2).
fn is_clone_spec(spec: &str) -> bool {
    !(spec.starts_with(['/', '.', '~']) || Path::new(spec).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r31_b_project_flag_is_gone_and_clone_specs() {
        for args in [
            &["rade", "spawn", "--project", "x"][..],
            &["rade", "list", "--project", "x"],
            &["rade", "tab", "list", "--project", "x"],
            &["rade", "review", "list", "--project", "x"],
            &["rade", "fanout", "--agents", "a,b", "--project", "x", "p"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
        assert!(is_clone_spec("org/x") && is_clone_spec("x") && is_clone_spec("https://h/o/x.git"));
        assert!(!is_clone_spec("./x") && !is_clone_spec("/tmp") && !is_clone_spec("~/x"));
        assert_eq!(
            clone::repo_name("https://github.com/o/x.git"),
            Some("x".into())
        );
        assert_eq!(clone::repo_name("git@github.com:o/x.git"), Some("x".into()));
        assert_eq!(clone::repo_name("o/x"), Some("x".into()));
        assert_eq!(clone::full_spec("x", Some("org")).unwrap(), "org/x");
        assert_eq!(clone::full_spec("o/x", Some("org")).unwrap(), "o/x");
        assert!(clone::full_spec("x", None).is_err());
    }

    #[test]
    fn r108_spawn_account_and_account_commands_parse() {
        // `--agent` with `--cmd` (the Preset's account, `--cmd` runs) and `--account`.
        let cli = Cli::try_parse_from([
            "rade",
            "spawn",
            "--agent",
            "claude",
            "--account",
            "acme-team",
            "--cmd",
            "env",
        ])
        .unwrap();
        let (m, p) = request(&cli.cmd).unwrap();
        assert_eq!(m, "pane.spawn");
        assert_eq!(p["account"], "acme-team");
        assert_eq!(p["agent"], "claude");
        assert_eq!(p["cmd"], "env");
        let parsed = |args: &[&str]| {
            let cli = Cli::try_parse_from([&["rade", "account"], args].concat()).unwrap();
            request(&cli.cmd).unwrap()
        };
        assert_eq!(parsed(&["list"]).0, "account.list");
        let (m, p) = parsed(&["list", "--workspace", "acme"]);
        assert_eq!((m, p["workspace"].as_str()), ("account.list", Some("acme")));
        let (m, p) = parsed(&[
            "add",
            "--agent",
            "claude",
            "--name",
            "acme-team",
            "--config-dir",
            "~/.claude-team",
            "--default",
        ]);
        assert_eq!(m, "account.add");
        assert_eq!(p["config_dir"], "~/.claude-team");
        assert_eq!(p["default"], true);
        let (m, p) = parsed(&["default", "personal", "--agent", "codex"]);
        assert_eq!(m, "account.set_default");
        assert_eq!(
            (p["name"].as_str(), p["agent"].as_str()),
            (Some("personal"), Some("codex"))
        );
        assert_eq!(parsed(&["usage", "personal"]).0, "usage.get");
        assert_eq!(parsed(&["usage"]).0, "usage.list");
        // `--json` works after the subcommand too (global).
        assert!(
            Cli::try_parse_from(["rade", "account", "list", "--json"])
                .unwrap()
                .json
        );
        assert!(Cli::try_parse_from(["rade", "account", "add", "--name", "x"]).is_err());
    }

    #[test]
    fn r24_2_a_tilde_path_is_the_home_folder() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(abs("~/no-such-x".into()).unwrap(), home.join("no-such-x"));
        assert_eq!(abs("~".into()).unwrap(), home.canonicalize().unwrap());
        assert_eq!(abs("/a~b".into()).unwrap(), PathBuf::from("/a~b"));
    }

    #[test]
    fn r24_2_clone_env_is_the_target_workspaces_only() {
        let ws = |id: &str, env: Value| json!({"workspace": id, "env": env});
        let all = [
            ws("default", json!({})),
            ws(
                "acme",
                json!({"GH_CONFIG_DIR": "/a", "CLAUDE_CONFIG_DIR": "/c", "PATH": "/x"}),
            ),
            ws("bravo", json!({"GH_CONFIG_DIR": "/b"})),
        ];
        // Into bravo: its own value; acme's other variables dropped (the
        // caller may be an acme Pane); PATH never dropped.
        let (set, unset) = clone::clone_env(&all[2], &all);
        assert_eq!(set, [("GH_CONFIG_DIR".to_string(), "/b".to_string())]);
        assert_eq!(unset, ["CLAUDE_CONFIG_DIR"]);
        // Into default: every Workspace variable dropped.
        let (set, mut unset) = clone::clone_env(&all[0], &all);
        unset.sort();
        assert!(set.is_empty());
        assert_eq!(unset, ["CLAUDE_CONFIG_DIR", "GH_CONFIG_DIR"]);
    }

    #[test]
    fn r15_fanout_parses_start_and_subcommands() {
        let req = |args: &[&str]| {
            let cli = Cli::try_parse_from([&["rade", "fanout"], args].concat()).unwrap();
            request(&cli.cmd).unwrap()
        };
        let (m, p) = req(&[
            "--agents",
            "claude,codex,codex",
            "--base",
            "dev",
            "add a line",
        ]);
        assert_eq!(m, "fanout.start");
        assert_eq!(p["agents"], json!(["claude", "codex", "codex"]));
        assert_eq!(
            (p["base"].as_str(), p["prompt"].as_str()),
            (Some("dev"), Some("add a line"))
        );
        let (m, p) = req(&[
            "choose",
            "fanout-1",
            "codex-2",
            "--strategy",
            "keep",
            "--cleanup",
        ]);
        assert_eq!(m, "fanout.choose");
        assert_eq!(
            (p["winner"].as_str(), p["cleanup"].as_bool()),
            (Some("codex-2"), Some(true))
        );
        let (m, p) = req(&["wait", "fanout-1", "--until", "judged"]);
        assert_eq!((m, p["until"].as_str()), ("fanout.wait", Some("judged")));
        assert_eq!(req(&["list"]).0, "fanout.list");
        assert!(Cli::try_parse_from(["rade", "fanout", "no agents"]).is_err());
    }

    #[test]
    fn r101_a_removed_commands_exit_2_naming_the_command() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for (argv, cmd) in [
            (&["rade", "task", "list"][..], "task"),
            (&["rade", "batch", "status"], "batch"),
            (&["rade", "initiative", "list"], "initiative"),
            (&["rade", "digest"], "digest"),
            (&["rade", "report", "list"], "report"),
            (&["rade", "flow"], "flow"),
            (&["rade", "lessons"], "lessons"),
            (&["rade", "--json", "task", "add", "x"], "task"),
        ] {
            let e = removed_command(&args(argv)).unwrap_or_else(|| panic!("{argv:?}"));
            assert_eq!(e.kind.exit_code(), 2, "{argv:?}");
            assert_eq!(
                format!("rade: {e}"),
                format!("rade: '{cmd}' was removed in 1.0.0 (ADR-0033)")
            );
            // Clap no longer knows them either.
            assert!(Cli::try_parse_from(argv).is_err(), "{argv:?}");
        }
        for kept in [
            &["rade", "list"][..],
            &["rade", "spawn", "--cmd", "task"],
            &["rade", "send", "p", "report"],
            &["rade"],
        ] {
            assert!(removed_command(&args(kept)).is_none(), "{kept:?}");
        }
        // `--profile` is gone too (R101).
        assert!(Cli::try_parse_from(["rade", "spawn", "--profile", "deep"]).is_err());
    }

    #[test]
    fn r13_review_target_parses_line_and_range() {
        let (p, a, b) = review_target("/w/src/a.rs:12-14").unwrap();
        assert_eq!((p, a, b), (PathBuf::from("/w/src/a.rs"), 12, Some(14)));
        let (p, a, b) = review_target("/w/a b.py:3").unwrap();
        assert_eq!((p, a, b), (PathBuf::from("/w/a b.py"), 3, None));
        for bad in ["/w/a.rs", "/w/a.rs:x", "/w/a.rs:1-y"] {
            assert!(review_target(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn r19_tab_rename_source_commands_parse() {
        let req = |args: &[&str]| {
            let cli = Cli::try_parse_from([&["rade"], args].concat()).unwrap();
            request(&cli.cmd).unwrap()
        };
        let (m, p) = req(&["tab", "close", "t_3", "--others", "--force"]);
        assert_eq!(m, "tab.close");
        assert_eq!(
            (p["mode"].as_str(), p["force"].as_bool()),
            (Some("others"), Some(true))
        );
        assert_eq!(req(&["tab", "close", "t_3", "--right"]).1["mode"], "right");
        assert_eq!(req(&["tab", "close", "build"]).1["mode"], "only");
        assert!(Cli::try_parse_from(["rade", "tab", "close", "t", "--others", "--right"]).is_err());
        assert_eq!(req(&["tab", "rename", "t_3", ""]).0, "tab.rename");
        assert_eq!(req(&["tab", "list"]).0, "tab.list");
        let (m, p) = req(&["rename", "p_4", "server"]);
        assert_eq!(
            (m, p["pane"].as_str(), p["name"].as_str()),
            ("pane.rename", Some("p_4"), Some("server"))
        );
        assert_eq!(req(&["source", "rename", "x", "Shop"]).0, "source.rename");
        let (m, p) = req(&["source", "pause", "x", "--force"]);
        assert_eq!((m, p["force"].as_bool()), ("source.pause", Some(true)));
        assert_eq!(req(&["source", "resume", "x"]).0, "source.resume");
    }
}
