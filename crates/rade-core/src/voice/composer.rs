//! The Voice Composer's state machine (R124, D2, D11–D14, D17). The app feeds
//! it [`Event`]s (keys, recorder, speech-to-text and agent output, a clock
//! tick) and carries out the [`Action`]s it returns; the widget draws
//! [`Composer::view`] and nothing else. No clocks, threads or processes here:
//! times are milliseconds passed in by the caller.

use super::agent::{self, AgentEvent, Turn};
use super::stt;
use super::words;
use crate::config::{VoiceAgent, VoiceConfig};
use std::ops::Range;

/// A key held at least this long is push-to-talk; a shorter press toggles.
pub const HOLD_MS: u64 = 250;
/// After the first Esc on an edited draft, a second one within this discards.
pub const ESC_WINDOW_MS: u64 = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Listening,
    Transcribing,
    Drafting,
    Ready,
    Error,
    Empty,
}

impl State {
    /// The `debug.voice info` name.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Listening => "listening",
            State::Transcribing => "transcribing",
            State::Drafting => "drafting",
            State::Ready => "ready",
            State::Error => "error",
            State::Empty => "empty",
        }
    }
}

/// How the running take ends: on release, or on the next tap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hold,
    Toggle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The `voice` key went down; `repeat` = auto-repeat.
    KeyDown {
        repeat: bool,
        at_ms: u64,
    },
    KeyUp {
        at_ms: u64,
    },
    /// The mic button or the palette action.
    Tap,
    FocusLost,
    PaneClosed,
    /// The close button: discard without asking.
    Discard,
    Escape {
        at_ms: u64,
    },
    /// New meter levels: only redraws.
    Levels,
    /// The newest live words (the whole take so far).
    Partial(String),
    /// The final words of the take (empty = nothing heard).
    Final(String),
    SttError(String),
    /// The record command failed; its first stderr line.
    RecordError(String),
    AgentEvent(AgentEvent),
    AgentExited {
        ok: bool,
        stderr: String,
    },
    /// The voice agent process could not be started.
    AgentStartFailed(String),
    /// Every ~100 ms: timeouts, the recording cap, the Esc window, the clock.
    Tick {
        at_ms: u64,
    },
    /// The user changed the draft box; its whole text.
    DraftEdited(String),
    /// Enter / the Send button.
    Send,
    /// Alt+Enter / the Paste only button.
    Paste,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    StartRecorder,
    /// Stop recording; `keep: false` throws the audio away.
    StopRecorder {
        keep: bool,
    },
    /// Post the take so far (live words).
    SttPartial,
    /// Post the whole take (final words).
    SttFinal,
    /// Start the voice agent process.
    SpawnAgent,
    /// A user message for the voice agent: a stream-json line's text for
    /// Claude, the whole stdin for Codex (which gets a `SpawnAgent` first).
    SendTurn(String),
    KillAgent,
    SendToPane {
        enter: bool,
        text: String,
    },
    /// Close the composer (the controller returns focus to the Terminal).
    Close,
    FocusDraft,
    Render,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityIcon {
    None,
    Read,
    Search,
    List,
    Info,
    Warning,
}

/// Everything the widget shows (PLAN §5.1).
#[derive(Clone, Debug, PartialEq)]
pub struct ComposerView {
    pub open: bool,
    pub state: Option<State>,
    pub mode: Option<Mode>,
    /// Header label: `Listening`, `Revising`, `Transcribing`, `Drafting`,
    /// `Ready`, the error's label or `Didn't catch that`.
    pub label: String,
    /// Dot class: `rec`, `run`, `ready`, `err`, `idle`.
    pub dot: &'static str,
    /// Card class `listening`, the meter and the REC cell.
    pub listening: bool,
    /// Header time: `0:06`, `0:11 · 3.2 s`, `revision 2 · 1.9 s`.
    pub time: String,
    /// Live words that will not change / may still change.
    pub heard: String,
    pub tentative: String,
    /// The heard line starts folded (the draft is ready); the user may toggle.
    pub heard_folded: bool,
    pub draft: String,
    /// Bumped whenever the composer (not the user) changes `draft`.
    pub draft_version: u64,
    /// Byte ranges of `draft` marked `changed`.
    pub changed: Vec<Range<usize>>,
    pub draft_editable: bool,
    /// The draft dims: a new take is replacing it.
    pub stale: bool,
    /// A caret after the text while it streams.
    pub caret: bool,
    pub placeholder: Option<String>,
    pub activity: String,
    pub activity_icon: ActivityIcon,
    pub activity_error: bool,
    /// The `Set up voice…` link follows the error.
    pub setup_link: bool,
    /// `2 / 6`.
    pub step: Option<String>,
    /// Footer hints, e.g. `Hold F8 to revise`, `Esc discard`.
    pub hints: Vec<String>,
    /// The second hint is the warning `Esc again to discard`.
    pub hint_warn: bool,
    pub send_enabled: bool,
    pub paste_enabled: bool,
    pub revision: u32,
    pub error: Option<String>,
    /// The mic button's `recording` / `open` classes.
    pub recording: bool,
}

#[derive(Clone, Debug)]
pub struct Composer {
    agent: VoiceAgent,
    max_turns: u32,
    timeout_ms: u64,
    max_recording_ms: u64,
    key: String,
    target: String,
    system: String,

    open: bool,
    state: State,
    mode: Mode,
    now: u64,
    key_down_at: Option<u64>,
    take_start: u64,
    take_ms: u64,
    heard: String,
    tentative: String,
    draft: String,
    draft_version: u64,
    edited: bool,
    changed: Vec<Range<usize>>,
    revision: u32,
    /// The current turn revises a draft.
    revising: bool,
    stream: Option<String>,
    turn_start: u64,
    draft_ms: u64,
    steps: u32,
    retried: bool,
    agent_alive: bool,
    /// The alive process has been sent a turn.
    agent_used: bool,
    context: Option<String>,
    /// Words waiting for the context.
    pending: Option<String>,
    error: Option<String>,
    error_label: &'static str,
    activity: String,
    esc_armed_until: Option<u64>,
}

fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn first_line(s: &str) -> &str {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

impl Composer {
    /// A closed composer. `key` is the bound key's label (`F8`).
    pub fn new(cfg: &VoiceConfig, key: &str) -> Self {
        Self {
            agent: cfg.agent,
            max_turns: cfg.max_turns(),
            timeout_ms: cfg.timeout().as_millis() as u64,
            max_recording_ms: cfg.max_recording().as_millis() as u64,
            key: key.to_string(),
            target: String::new(),
            system: agent::system_prompt(cfg),
            open: false,
            state: State::Listening,
            mode: Mode::Hold,
            now: 0,
            key_down_at: None,
            take_start: 0,
            take_ms: 0,
            heard: String::new(),
            tentative: String::new(),
            draft: String::new(),
            draft_version: 0,
            edited: false,
            changed: Vec::new(),
            revision: 0,
            revising: false,
            stream: None,
            turn_start: 0,
            draft_ms: 0,
            steps: 0,
            retried: false,
            agent_alive: false,
            agent_used: false,
            context: None,
            pending: None,
            error: None,
            error_label: "",
            activity: String::new(),
            esc_armed_until: None,
        }
    }

    /// The worktree or link named in the listening note.
    pub fn set_target(&mut self, target: &str) {
        self.target = target.to_string();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn state(&self) -> Option<State> {
        self.open.then_some(self.state)
    }

    /// The draft as the composer knows it (the box's text after edits).
    pub fn draft(&self) -> &str {
        self.stream.as_deref().unwrap_or(&self.draft)
    }

    /// The rendered context (`voice::context::render`) is ready. If words
    /// were waiting for it, their turn goes out now.
    pub fn set_context(&mut self, context: String) -> Vec<Action> {
        self.context = Some(context);
        let mut a = Vec::new();
        if self.open && self.state == State::Drafting {
            if let Some(words) = self.pending.take() {
                self.send_turn(&words, &mut a);
            }
        }
        a
    }

    /// Closed and empty again; the settings stay.
    fn reset(&mut self) {
        let blank = Composer::new(&VoiceConfig::default(), "");
        *self = Composer {
            agent: self.agent,
            max_turns: self.max_turns,
            timeout_ms: self.timeout_ms,
            max_recording_ms: self.max_recording_ms,
            key: std::mem::take(&mut self.key),
            target: std::mem::take(&mut self.target),
            system: std::mem::take(&mut self.system),
            now: self.now,
            draft_version: self.draft_version + 1,
            ..blank
        };
    }

    fn has_draft(&self) -> bool {
        !self.draft.trim().is_empty()
    }

    fn end_take(&mut self, a: &mut Vec<Action>) {
        self.take_ms = self.now.saturating_sub(self.take_start);
        self.key_down_at = None;
        self.state = State::Transcribing;
        a.push(Action::StopRecorder { keep: true });
        a.push(Action::SttFinal);
    }

    fn begin_take(&mut self, mode: Mode, a: &mut Vec<Action>) {
        self.open = true;
        self.state = State::Listening;
        self.mode = mode;
        self.take_start = self.now;
        self.take_ms = 0;
        self.heard.clear();
        self.tentative.clear();
        self.error = None;
        self.error_label = "";
        self.activity.clear();
        self.esc_armed_until = None;
        self.stream = None;
        self.retried = false;
        a.push(Action::StartRecorder);
        // Claude warms up while the owner speaks; Codex starts per turn.
        if self.agent == VoiceAgent::Claude && !self.agent_alive {
            a.push(Action::SpawnAgent);
            self.agent_alive = true;
            self.agent_used = false;
        }
    }

    /// Back to the draft we had before a revise take began.
    fn back_to_ready(&mut self) {
        self.state = State::Ready;
        self.error = None;
        self.error_label = "";
        self.tentative.clear();
    }

    fn close(&mut self, a: &mut Vec<Action>) {
        a.push(Action::KillAgent);
        a.push(Action::Close);
        self.reset();
    }

    /// Send the words as the next turn (a new process when none is alive).
    fn send_turn(&mut self, words: &str, a: &mut Vec<Action>) {
        let fresh = !self.agent_alive || !self.agent_used || self.agent == VoiceAgent::Codex;
        let draft = self.has_draft().then(|| self.draft.clone());
        let context = self.context.clone();
        if fresh && context.is_none() {
            // The first message carries the context: wait for it.
            if self.agent == VoiceAgent::Claude && !self.agent_alive {
                a.push(Action::SpawnAgent);
                self.agent_alive = true;
            }
            self.pending = Some(words.to_string());
            return;
        }
        if self.agent == VoiceAgent::Codex || !self.agent_alive {
            a.push(Action::SpawnAgent);
            self.agent_alive = true;
        }
        let text = match (self.agent, fresh) {
            (VoiceAgent::Codex, _) => agent::codex_prompt(
                &self.system,
                context.as_deref().unwrap_or(""),
                draft.as_deref(),
                words,
            ),
            (_, true) => agent::turn_message(
                Turn::First {
                    context: context.as_deref().unwrap_or(""),
                    draft: draft.as_deref(),
                },
                words,
            ),
            (_, false) => agent::turn_message(
                Turn::Revise {
                    draft: draft.as_deref().unwrap_or(""),
                },
                words,
            ),
        };
        self.revising = draft.is_some();
        self.agent_used = true;
        a.push(Action::SendTurn(text));
    }

    fn fail(&mut self, label: &'static str, text: String) {
        self.commit_stream();
        self.state = State::Error;
        self.error_label = label;
        self.error = Some(text);
        self.key_down_at = None;
    }

    /// Streamed text becomes the draft (stopped, timed out or failed turn).
    fn commit_stream(&mut self) {
        if let Some(s) = self.stream.take() {
            self.draft = s;
            self.changed.clear();
            self.draft_version += 1;
        }
    }

    pub fn on(&mut self, ev: Event) -> Vec<Action> {
        let mut a = Vec::new();
        let before = self.view();
        let enter = matches!(ev, Event::Send);
        let levels = matches!(ev, Event::Levels);
        match ev {
            Event::KeyDown { repeat: true, .. } => return a,
            Event::KeyDown { at_ms, .. } => {
                self.now = at_ms;
                match (self.open, self.state) {
                    (true, State::Listening) => self.end_take(&mut a),
                    (true, State::Transcribing | State::Drafting) => {}
                    _ => {
                        self.begin_take(Mode::Hold, &mut a);
                        self.key_down_at = Some(at_ms);
                    }
                }
            }
            Event::KeyUp { at_ms } => {
                self.now = at_ms;
                if let (true, State::Listening, Some(down)) =
                    (self.open, self.state, self.key_down_at.take())
                {
                    if at_ms.saturating_sub(down) >= HOLD_MS {
                        self.end_take(&mut a);
                    } else {
                        self.mode = Mode::Toggle;
                    }
                }
            }
            Event::Tap => match (self.open, self.state) {
                (true, State::Listening) => self.end_take(&mut a),
                (true, State::Transcribing | State::Drafting) => {}
                _ => self.begin_take(Mode::Toggle, &mut a),
            },
            Event::FocusLost => {
                if self.open && self.state == State::Listening {
                    self.end_take(&mut a);
                }
            }
            Event::PaneClosed => {
                if self.open {
                    a.push(Action::StopRecorder { keep: false });
                    self.close(&mut a);
                }
                return a;
            }
            Event::Discard => {
                if self.open {
                    if self.state == State::Listening {
                        a.push(Action::StopRecorder { keep: false });
                    }
                    self.close(&mut a);
                }
                return a;
            }
            Event::Escape { at_ms } => {
                self.now = at_ms;
                self.escape(&mut a);
                if !self.open {
                    return a;
                }
            }
            Event::Levels => {
                if !(self.open && self.state == State::Listening) {
                    return a;
                }
            }
            Event::Partial(t) => {
                if self.open && self.state == State::Listening {
                    (self.heard, self.tentative) = stt::split_tentative(&t);
                }
            }
            Event::Final(t) => {
                if self.open && self.state == State::Transcribing {
                    let words = t.trim().to_string();
                    self.heard = words.clone();
                    self.tentative.clear();
                    if words.is_empty() {
                        self.state = State::Empty;
                    } else {
                        self.state = State::Drafting;
                        self.turn_start = self.now;
                        self.steps = 0;
                        self.stream = None;
                        self.send_turn(&words, &mut a);
                    }
                }
            }
            Event::SttError(msg) => {
                if self.open && matches!(self.state, State::Listening | State::Transcribing) {
                    self.fail("Can't transcribe", msg);
                }
            }
            Event::RecordError(msg) => {
                if self.open && self.state == State::Listening {
                    a.push(Action::StopRecorder { keep: false });
                    self.fail(
                        "Can't record",
                        format!("Can't record: {}", first_line(&msg)),
                    );
                }
            }
            Event::AgentEvent(e) => {
                if !(self.open && self.state == State::Drafting) {
                    return a;
                }
                self.agent_event(e, &mut a);
            }
            Event::AgentExited { ok, stderr } => {
                self.agent_alive = false;
                self.agent_used = false;
                if self.open && self.state == State::Drafting {
                    let why = if ok || first_line(&stderr).is_empty() {
                        "exited without an answer"
                    } else {
                        first_line(&stderr)
                    };
                    self.pending = None;
                    self.fail("Voice agent failed", format!("Voice agent failed: {why}"));
                }
            }
            Event::AgentStartFailed(err) => {
                self.agent_alive = false;
                self.agent_used = false;
                if self.open && matches!(self.state, State::Drafting | State::Listening) {
                    let name = match self.agent {
                        VoiceAgent::Claude => "claude",
                        VoiceAgent::Codex => "codex",
                    };
                    self.pending = None;
                    self.fail(
                        "Voice agent failed",
                        format!("Can't start {name}: {}", first_line(&err)),
                    );
                    if self.state == State::Error {
                        a.push(Action::StopRecorder { keep: false });
                    }
                }
            }
            Event::Tick { at_ms } => {
                self.now = at_ms;
                if !self.open {
                    return a;
                }
                match self.state {
                    State::Listening
                        if at_ms.saturating_sub(self.take_start) >= self.max_recording_ms =>
                    {
                        self.end_take(&mut a)
                    }
                    State::Drafting if at_ms.saturating_sub(self.turn_start) >= self.timeout_ms => {
                        a.push(Action::KillAgent);
                        self.agent_alive = false;
                        self.agent_used = false;
                        self.pending = None;
                        self.fail(
                            "Timed out",
                            format!("Took longer than {} s", self.timeout_ms / 1000),
                        );
                    }
                    _ => {}
                }
                if self.esc_armed_until.is_some_and(|t| at_ms > t) {
                    self.esc_armed_until = None;
                }
            }
            Event::DraftEdited(text) => {
                if self.open {
                    self.draft = text;
                    self.edited = true;
                    self.changed.clear();
                    self.esc_armed_until = None;
                }
                return a;
            }
            Event::Send | Event::Paste => {
                if self.open && self.state == State::Ready && self.has_draft() {
                    a.push(Action::SendToPane {
                        enter,
                        text: self.draft.clone(),
                    });
                    self.close(&mut a);
                    return a;
                }
            }
        }
        if levels || self.view() != before {
            a.push(Action::Render);
        }
        a
    }

    fn escape(&mut self, a: &mut Vec<Action>) {
        match self.state {
            State::Listening | State::Transcribing if self.open => {
                if self.state == State::Listening {
                    a.push(Action::StopRecorder { keep: false });
                }
                self.key_down_at = None;
                if self.has_draft() {
                    self.back_to_ready();
                } else {
                    self.close(a);
                }
            }
            State::Drafting if self.open => {
                // Stop the turn: the process goes, the streamed text stays.
                a.push(Action::KillAgent);
                self.agent_alive = false;
                self.agent_used = false;
                self.pending = None;
                self.commit_stream();
                self.state = if self.has_draft() {
                    State::Ready
                } else {
                    State::Error
                };
                if self.state == State::Error {
                    self.error_label = "Voice agent failed";
                    self.error = Some("Voice agent failed: stopped".into());
                }
            }
            State::Ready if self.open => {
                let changed = self.edited || self.revision > 0;
                if changed && self.esc_armed_until.is_none_or(|t| self.now > t) {
                    self.esc_armed_until = Some(self.now + ESC_WINDOW_MS);
                } else {
                    self.close(a);
                }
            }
            State::Error | State::Empty if self.open => self.close(a),
            _ => {}
        }
    }

    fn agent_event(&mut self, e: AgentEvent, a: &mut Vec<Action>) {
        match e {
            AgentEvent::Delta(t) => {
                self.stream.get_or_insert_with(String::new).push_str(&t);
                self.draft_version += 1;
            }
            AgentEvent::Tool { label } => {
                self.steps += 1;
                self.activity = label;
            }
            AgentEvent::Final { text } => {
                let text = if text.trim().is_empty() {
                    self.stream.clone().unwrap_or_default()
                } else {
                    text
                };
                if text.trim().is_empty() {
                    a.push(Action::KillAgent);
                    self.agent_alive = false;
                    self.fail("Voice agent failed", "Voice agent failed: no answer".into());
                    return;
                }
                self.changed = if self.revising {
                    words::changed_spans(&self.draft, &text)
                } else {
                    Vec::new()
                };
                if self.revising {
                    self.revision += 1;
                }
                self.draft = text;
                self.stream = None;
                self.draft_version += 1;
                self.edited = false;
                self.draft_ms = self.now.saturating_sub(self.turn_start);
                self.activity.clear();
                self.state = State::Ready;
                if self.agent == VoiceAgent::Codex {
                    self.agent_alive = false;
                    self.agent_used = false;
                }
                a.push(Action::FocusDraft);
            }
            AgentEvent::MaxTurns => {
                if self.retried || self.agent != VoiceAgent::Claude {
                    a.push(Action::KillAgent);
                    self.agent_alive = false;
                    self.agent_used = false;
                    let n = self.max_turns;
                    self.fail(
                        "Voice agent failed",
                        format!("Voice agent failed: used all {n} steps"),
                    );
                } else {
                    self.retried = true;
                    self.steps = 0;
                    self.activity.clear();
                    a.push(Action::SendTurn(agent::WRITE_NOW.to_string()));
                }
            }
            AgentEvent::Error { text } => {
                a.push(Action::KillAgent);
                self.agent_alive = false;
                self.agent_used = false;
                self.fail(
                    "Voice agent failed",
                    format!("Voice agent failed: {}", first_line(&text)),
                );
            }
        }
    }

    fn time_text(&self) -> String {
        match self.state {
            State::Listening => clock(self.now.saturating_sub(self.take_start)),
            State::Transcribing | State::Drafting => clock(self.take_ms),
            State::Ready => {
                let secs = format!("{:.1} s", self.draft_ms as f64 / 1000.0);
                if self.revision > 0 {
                    format!("revision {} · {secs}", self.revision)
                } else {
                    format!("{} · {secs}", clock(self.take_ms))
                }
            }
            State::Error | State::Empty => String::new(),
        }
    }

    fn hints(&self) -> (Vec<String>, bool) {
        let k = &self.key;
        let h = |a: &str, b: &str| (vec![a.to_string(), b.to_string()], false);
        match self.state {
            State::Listening if self.has_draft() => match self.mode {
                Mode::Hold => h(&format!("Release {k} to apply"), "Esc keep the draft"),
                Mode::Toggle => h(&format!("Press {k} to apply"), "Esc keep the draft"),
            },
            State::Listening => match self.mode {
                Mode::Hold => h(&format!("Release {k} to draft"), "Esc cancel"),
                Mode::Toggle => h(&format!("Press {k} to draft"), "Esc cancel"),
            },
            State::Transcribing | State::Drafting => (vec!["Esc stop".to_string()], false),
            State::Ready => {
                if self.esc_armed_until.is_some() {
                    (
                        vec![format!("Hold {k} to revise"), "Esc again to discard".into()],
                        true,
                    )
                } else {
                    h(&format!("Hold {k} to revise"), "Esc discard")
                }
            }
            State::Error => h(&format!("Hold {k} to try again"), "Esc close"),
            State::Empty => h(&format!("Hold {k} and speak"), "Esc close"),
        }
    }

    pub fn view(&self) -> ComposerView {
        if !self.open {
            return ComposerView {
                open: false,
                state: None,
                mode: None,
                label: String::new(),
                dot: "idle",
                listening: false,
                time: String::new(),
                heard: String::new(),
                tentative: String::new(),
                heard_folded: false,
                draft: String::new(),
                draft_version: self.draft_version,
                changed: Vec::new(),
                draft_editable: false,
                stale: false,
                caret: false,
                placeholder: None,
                activity: String::new(),
                activity_icon: ActivityIcon::None,
                activity_error: false,
                setup_link: false,
                step: None,
                hints: Vec::new(),
                hint_warn: false,
                send_enabled: false,
                paste_enabled: false,
                revision: 0,
                error: None,
                recording: false,
            };
        }
        let st = self.state;
        let has_draft = self.has_draft();
        let label = match st {
            State::Listening if has_draft => "Revising".to_string(),
            State::Listening => "Listening".into(),
            State::Transcribing => "Transcribing".into(),
            State::Drafting => "Drafting".into(),
            State::Ready => "Ready".into(),
            State::Error => self.error_label.to_string(),
            State::Empty => "Didn't catch that".into(),
        };
        let dot = match st {
            State::Listening => "rec",
            State::Transcribing | State::Drafting => "run",
            State::Ready => "ready",
            State::Error => "err",
            State::Empty => "idle",
        };
        let (activity, icon, act_err) = match (&self.error, st) {
            (Some(e), State::Error) => (e.clone(), ActivityIcon::Warning, true),
            _ if !self.activity.is_empty() && st == State::Drafting => {
                let icon = match self.activity.split(' ').next().unwrap_or("") {
                    "Read" => ActivityIcon::Read,
                    "Searched" => ActivityIcon::Search,
                    "Listed" => ActivityIcon::List,
                    _ => ActivityIcon::Info,
                };
                (self.activity.clone(), icon, false)
            }
            _ if st == State::Listening && !has_draft && !self.target.is_empty() => (
                format!("Reading the session and {} while you speak", self.target),
                ActivityIcon::Info,
                false,
            ),
            _ => (String::new(), ActivityIcon::None, false),
        };
        let streaming = st == State::Drafting && self.stream.is_some();
        let (hints, hint_warn) = self.hints();
        let ready = st == State::Ready && has_draft;
        ComposerView {
            open: true,
            state: Some(st),
            mode: Some(self.mode),
            label,
            dot,
            listening: st == State::Listening,
            time: self.time_text(),
            heard: self.heard.clone(),
            tentative: self.tentative.clone(),
            heard_folded: st == State::Ready,
            draft: self.draft().to_string(),
            draft_version: self.draft_version,
            changed: self.changed.clone(),
            draft_editable: st == State::Ready,
            stale: has_draft
                && (matches!(st, State::Listening | State::Transcribing)
                    || (st == State::Drafting && !streaming)),
            caret: streaming,
            placeholder: match st {
                State::Listening if !has_draft => Some(format!(
                    "The prompt appears here when you let go of {}.",
                    self.key
                )),
                State::Error | State::Empty if !has_draft => Some("Nothing heard yet.".into()),
                _ => None,
            },
            activity,
            activity_icon: icon,
            activity_error: act_err,
            setup_link: st == State::Error
                && self
                    .error
                    .as_deref()
                    .is_some_and(|e| e.starts_with("No speech-to-text server")),
            step: (st == State::Drafting
                && self.agent == VoiceAgent::Claude
                && !self.activity.is_empty())
            .then(|| format!("{} / {}", self.steps, self.max_turns)),
            hints,
            hint_warn,
            send_enabled: ready,
            paste_enabled: ready,
            revision: self.revision,
            error: if st == State::Error {
                self.error.clone()
            } else {
                None
            },
            recording: st == State::Listening,
        }
    }
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod tests;
