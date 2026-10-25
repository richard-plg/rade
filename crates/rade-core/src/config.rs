//! `config.toml` schema (R17.1) with defaults. Keybindings use GTK
//! accelerator syntax (`<Ctrl><Shift>t`); the app parses them.

use crate::preset::{self, Preset, PresetOverride};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Default keybindings (R17.3): in-app actions use Ctrl+Shift, Source
/// switching uses Alt+<n>. Never plain Ctrl+<key>, never Shift+Enter (R16.5),
/// never Ctrl+Alt+<arrow> (GNOME workspace switching). Split/focus/zoom mirror
/// the owner's Ghostty keybinds (vim-style focus).
pub const DEFAULT_KEYBINDINGS: &[(&str, &str)] = &[
    ("new_tab", "<Ctrl><Shift>t"),
    ("close_pane", "<Ctrl><Shift>w"),
    ("split_right", "<Ctrl><Shift>Return"),
    ("split_down", "<Ctrl><Shift>d"),
    ("focus_left", "<Ctrl><Shift>h"),
    ("focus_right", "<Ctrl><Shift>l"),
    ("focus_up", "<Ctrl><Shift>k"),
    ("focus_down", "<Ctrl><Shift>j"),
    ("zoom", "<Ctrl><Shift>z"),
    ("next_tab", "<Ctrl><Shift>Page_Down"),
    ("prev_tab", "<Ctrl><Shift>Page_Up"),
    ("add_source", "<Ctrl><Shift>n"),
    // Newest unread Notification (R3.3), like cmux's Cmd+Shift+U.
    // Not Ctrl+Shift+U (IBus Unicode entry) nor A (Ghostty select_all).
    ("jump_unread", "<Ctrl><Shift>g"),
    // File tree (R8.1). Ghostty's default here is new_split:down, which RADE
    // replaces with its own split keys.
    ("toggle_file_tree", "<Ctrl><Shift>e"),
    // New Worktree dialog for the Source on screen (R12.1; B = branch).
    ("new_worktree", "<Ctrl><Shift>b"),
    // Review Pane of the Worktree on screen (R13.1).
    ("open_review", "<Ctrl><Shift>r"),
    // Command palette (R14.1). Ghostty's own default here is its command
    // palette (toggle_command_palette, 1.2+); RADE's replaces it on purpose:
    // it lists Ghostty-style actions plus Sources, Panes and files.
    ("command_palette", "<Ctrl><Shift>p"),
    // Settings (R112), like VS Code / GNOME apps. Ghostty's default here
    // opens its config file; RADE's page covers the Terminal font.
    ("open_settings", "<Ctrl>comma"),
    // Browser Pane beside the focused Pane (R18; O = Open). Ghostty's
    // default here is new_split:right, which RADE replaces with its own
    // split keys (Ctrl+Shift+Enter).
    ("open_browser", "<Ctrl><Shift>o"),
    // Right Panel pages (R21.9, R22.5): M = modified files (Changes),
    // S = Search. Neither is a Ghostty or IBus default.
    // Agent Overview (R117): every Agent of every Workspace. Space is free
    // in Ghostty's, GNOME's (Super+Space, Alt+Space) and IBus's (Super+Space;
    // legacy Ctrl+Space) defaults; in a Terminal it would only send NUL.
    ("agent_overview", "<Ctrl><Shift>space"),
    ("show_changes", "<Ctrl><Shift>m"),
    ("search_worktree", "<Ctrl><Shift>s"),
    ("source_1", "<Alt>1"),
    ("source_2", "<Alt>2"),
    ("source_3", "<Alt>3"),
    ("source_4", "<Alt>4"),
    ("source_5", "<Alt>5"),
    ("source_6", "<Alt>6"),
    ("source_7", "<Alt>7"),
    ("source_8", "<Alt>8"),
    ("source_9", "<Alt>9"),
    // Workspaces (R23.5): Ctrl+Alt+<n> is free in GNOME (its Ctrl+Alt keys
    // are arrows, Tab, Escape, Delete, T, D, F1–F12), Ghostty (Alt+<n> =
    // its tabs) and IBus.
    ("workspace_1", "<Ctrl><Alt>1"),
    ("workspace_2", "<Ctrl><Alt>2"),
    ("workspace_3", "<Ctrl><Alt>3"),
    ("workspace_4", "<Ctrl><Alt>4"),
    ("workspace_5", "<Ctrl><Alt>5"),
    ("workspace_6", "<Ctrl><Alt>6"),
    ("workspace_7", "<Ctrl><Alt>7"),
    ("workspace_8", "<Ctrl><Alt>8"),
    ("workspace_9", "<Ctrl><Alt>9"),
    // Voice prompt (R124): hold to talk, tap to toggle. F8 is free in
    // Ghostty, GNOME, IBus / fcitx Vietnamese input and the Claude Code /
    // Codex TUIs.
    ("voice", "F8"),
];

/// Keys a Workspace, Project or Source file may set (R29.1, ADR-0017).
pub const LAYERED: &[&str] = &["agents", "scm", "fanout", "viewer", "sources", "code"];
/// Keys of the engines removed in 1.0.0 (R101.4, ADR-0033): any layer that
/// sets them is reported `ignored (removed in 1.0.0)`, never an error.
pub const REMOVED: &[&str] = &["task", "profiles", "batch", "report", "initiative"];
/// Keys only the user's `config.toml` sets: elsewhere `ignored (app-wide)`.
pub const APP_WIDE: &[&str] = &[
    "keybindings",
    "limits",
    "desktop_notify",
    "status_bar",
    "system_monitor",
    "idle_seconds",
    "browser",
    "terminal",
    "appearance",
    "code_font",
    // User config only (D19): it runs commands and sends audio and code out.
    "voice",
];

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_depth: u32,
    pub max_children: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_children: 8,
        }
    }
}

/// `[viewer]` (R7.2).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Viewer {
    /// The Viewer's editor button runs `<editor> <worktree> --goto <file>:<line>`
    /// (VS Code's CLI); not on PATH → `$EDITOR` / the default app.
    pub editor: String,
    /// A local HTML file shown rendered runs its JavaScript (R114.3).
    pub html_scripts: bool,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            editor: "code".into(),
            html_scripts: true,
        }
    }
}

/// `[fanout]` (R15.3).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FanoutConfig {
    /// Agent Preset of the Judge. It runs read-only whatever the Preset's
    /// own permissions (claude: Read/Grep/Glob tools only; codex: read-only
    /// sandbox), so an override with `--dangerously-skip-permissions` is safe.
    pub judge: String,
}

impl Default for FanoutConfig {
    fn default() -> Self {
        Self {
            judge: "claude".into(),
        }
    }
}

/// `[scm]` (R21.7).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScmConfig {
    /// Generate's Draft command, run with `sh -c` in the Worktree, the
    /// prompt on stdin, the Draft on stdout (ADR-0013).
    pub draft_command: String,
}

impl Default for ScmConfig {
    fn default() -> Self {
        Self {
            draft_command: crate::scm::DEFAULT_DRAFT_COMMAND.into(),
        }
    }
}

/// `[sources.<id or name>]` (R12.2).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceConfig {
    /// Shell command run in a new Worktree's first Terminal, before its
    /// Agent or shell (e.g. `npm ci`, `cp ../.env .`).
    pub setup: Option<String>,
    /// `false` = no language server for this repository (R83.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DesktopNotify {
    #[default]
    Smart,
    Always,
    Off,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// User overrides; `""` unbinds. See [`Config::keybindings`].
    keybindings: BTreeMap<String, String>,
    pub limits: Limits,
    /// Desktop notifications: smart (default), always, off (R3.5).
    pub desktop_notify: DesktopNotify,
    /// `rade wait --until idle`: seconds without output (R10.4).
    pub idle_seconds: u64,
    /// Show the Status Bar (R20.9); the palette toggles it for the session.
    pub status_bar: bool,
    /// The Statusline's system monitor cell (R116): CPU, memory, GPU, network.
    pub system_monitor: bool,
    /// Agent Preset overrides and additions (R16.2); see [`Config::presets`].
    agents: BTreeMap<String, PresetOverride>,
    pub viewer: Viewer,
    /// Per-Source settings, keyed by Source id or name.
    pub sources: BTreeMap<String, SourceConfig>,
    pub fanout: FanoutConfig,
    pub scm: ScmConfig,
    pub browser: BrowserConfig,
    pub code: CodeConfig,
    pub terminal: TerminalConfig,
    pub appearance: AppearanceConfig,
    pub code_font: CodeFontConfig,
    pub voice: VoiceConfig,
}

/// `[voice]` (R124, ADR-0046): voice prompts. User config only.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct VoiceConfig {
    pub agent: VoiceAgent,
    /// `""` = `haiku` (claude) / Codex's own default.
    pub model: String,
    /// Non-empty replaces the built command (`sh -c`); the protocol is still
    /// `agent`'s.
    pub agent_command: String,
    pub tools: VoiceTools,
    /// Claude Code turns per draft; read through [`VoiceConfig::max_turns`].
    pub max_turns: u32,
    /// Seconds per turn; read through [`VoiceConfig::timeout`].
    pub timeout_secs: u32,
    /// Seconds; read through [`VoiceConfig::max_recording`].
    pub max_recording_secs: u32,
    pub live_preview: bool,
    pub draft_language: DraftLanguage,
    /// OpenAI-compatible transcription endpoint.
    pub stt_url: String,
    /// Ignored by whisper.cpp, needed by OpenAI.
    pub stt_model: String,
    /// `auto` or a language code; read through [`VoiceConfig::stt_language`].
    pub stt_language: String,
    /// Prints the API key on stdout; empty = no key.
    pub stt_key_command: String,
    /// Raw s16le mono 16 kHz on stdout.
    pub record_command: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            agent: VoiceAgent::default(),
            model: String::new(),
            agent_command: String::new(),
            tools: VoiceTools::default(),
            max_turns: 6,
            timeout_secs: 60,
            max_recording_secs: 120,
            live_preview: true,
            draft_language: DraftLanguage::default(),
            stt_url: "http://127.0.0.1:8178/v1/audio/transcriptions".into(),
            stt_model: "whisper-1".into(),
            stt_language: "auto".into(),
            stt_key_command: String::new(),
            record_command: "pw-record --rate 16000 --channels 1 --format s16 -".into(),
        }
    }
}

impl VoiceConfig {
    /// The configured code when it is 2 or 3 lowercase ASCII letters
    /// (ISO 639), else `auto`.
    pub fn stt_language(&self) -> &str {
        let l = self.stt_language.trim();
        if (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase()) {
            l
        } else {
            "auto"
        }
    }

    /// 1 to 20.
    pub fn max_turns(&self) -> u32 {
        self.max_turns.clamp(1, 20)
    }

    /// 10 to 300 seconds.
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.timeout_secs.clamp(10, 300)))
    }

    /// 10 to 600 seconds.
    pub fn max_recording(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.max_recording_secs.clamp(10, 600)))
    }
}

/// Who writes the Voice Draft.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceAgent {
    #[default]
    Claude,
    Codex,
}

/// What the voice agent may use: `read` = Read, Grep, Glob.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTools {
    #[default]
    Read,
    None,
}

/// `same` = the language spoken, `en` = always English.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftLanguage {
    #[default]
    Same,
    En,
}

/// `[code]` (R83, ADR-0029): code navigation through language servers.
/// `idle_minutes` and `max_servers` are read from `config.toml` only (one
/// pool of servers for the app); `enabled` and `server.<lang>` per layer.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CodeConfig {
    pub enabled: bool,
    pub idle_minutes: f64,
    pub max_servers: usize,
    /// `rust`, `typescript`: overrides of the built-in servers.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub server: BTreeMap<String, ServerConfig>,
}

impl Default for CodeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_minutes: 15.0,
            max_servers: 4,
            server: BTreeMap::new(),
        }
    }
}

/// `[code.server.<lang>]`: the command (resolved in the repository's
/// `node_modules/.bin`, then on PATH), extra `settings` (answers to
/// `workspace/configuration`, merged over the built-in ones), extra
/// `init` (initializationOptions) and, for TypeScript, a TypeScript ≥ 5
/// `lib` folder used when the repository's own is older.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<toml::Table>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub init: Option<toml::Table>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tsserver_path: Option<String>,
}

/// `[terminal]` (R102.6, R112, ADR-0037): who styles the Terminals and,
/// with `theme = "rade"`, their font.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub theme: TerminalTheme,
    /// A fontconfig family (installed, or one RADE bundles).
    pub font: String,
    /// The family's style for normal text, e.g. `Medium`.
    pub font_style: String,
    /// Pixels at 100 % scale.
    pub font_size: f32,
    /// × the font's own line height, like VS Code's terminal setting.
    pub line_height: f32,
    /// `->` `!=` drawn as one glyph where the font has them.
    pub ligatures: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            theme: TerminalTheme::Rade,
            font: "Maple Mono".into(),
            font_style: "Medium".into(),
            font_size: 14.0,
            line_height: 1.15,
            ligatures: true,
        }
    }
}

/// `[code_font]` (R112.4): the font of code outside Terminals — the Viewer,
/// Review diffs and Markdown code. Empty family / style = the Terminal's.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CodeFontConfig {
    pub family: String,
    pub style: String,
    /// Pixels at 100 % scale.
    pub size: f32,
    pub ligatures: bool,
}

impl Default for CodeFontConfig {
    fn default() -> Self {
        Self {
            family: String::new(),
            style: String::new(),
            size: 13.0,
            ligatures: true,
        }
    }
}

/// The code font in use: (family, style, px, ligatures).
#[derive(Clone, Debug, PartialEq)]
pub struct CodeFont {
    pub family: String,
    pub style: String,
    pub size: f32,
    pub ligatures: bool,
}

impl Config {
    /// `[code_font]` with its empty family / style taken from `[terminal]`.
    pub fn code_font(&self) -> CodeFont {
        let c = &self.code_font;
        let or = |own: &str, terminal: &str| {
            if own.trim().is_empty() {
                terminal.to_string()
            } else {
                own.trim().to_string()
            }
        };
        CodeFont {
            family: or(&c.family, &self.terminal.font),
            style: if c.family.trim().is_empty() || !c.style.trim().is_empty() {
                or(&c.style, &self.terminal.font_style)
            } else {
                // Another family than the Terminal's: its own style or Regular.
                "Regular".into()
            },
            size: c.size,
            ligatures: c.ligatures,
        }
    }
}

/// `[appearance]` (R112): light / dark.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceConfig {
    pub color_scheme: ColorScheme,
}

/// `system` (default): dark unless the system asks for light (R102.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    #[default]
    System,
    Dark,
    Light,
}

/// `rade` (default): RADE's palette, font and padding over the user's
/// Ghostty config; `ghostty`: the Ghostty config's own look (0.7 behavior).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalTheme {
    #[default]
    Rade,
    Ghostty,
}

/// `[browser]` (R72.1): the Chromium `rade browser --engine chromium`
/// drives; none = found on this machine.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chromium: Option<std::path::PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keybindings: BTreeMap::new(),
            limits: Limits::default(),
            desktop_notify: DesktopNotify::default(),
            idle_seconds: 5,
            status_bar: true,
            system_monitor: true,
            agents: BTreeMap::new(),
            viewer: Viewer::default(),
            sources: BTreeMap::new(),
            fanout: FanoutConfig::default(),
            scm: ScmConfig::default(),
            browser: BrowserConfig::default(),
            code: CodeConfig::default(),
            terminal: TerminalConfig::default(),
            appearance: AppearanceConfig::default(),
            code_font: CodeFontConfig::default(),
            voice: VoiceConfig::default(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let table: toml::Table =
            toml::from_str(text).map_err(|e| crate::project::toml_error(text, &e))?;
        Self::from_table(table)
    }

    /// From an already merged table (R29 layers); same checks as [`Self::parse`].
    pub fn from_table(mut table: toml::Table) -> Result<Self, String> {
        // The M15 names are gone (R31.2): say what replaced them.
        if table.contains_key("projects") {
            return Err("[projects.*] is now [sources.*] (renamed in M16)".into());
        }
        // Removed in 1.0.0 (R101.4): old files keep loading.
        table.retain(|k, _| !REMOVED.contains(&k));
        let config = Config::deserialize(toml::Value::Table(table))
            .map_err(|e| e.message().trim().to_string())?;
        let unknown: Vec<String> = config
            .keybindings
            .keys()
            .filter(|k| !DEFAULT_KEYBINDINGS.iter().any(|(a, _)| a == k))
            .map(|k| {
                let new = k.replace("project", "source");
                if DEFAULT_KEYBINDINGS.iter().any(|(a, _)| *a == new) {
                    format!("{k} (now {new})")
                } else {
                    k.clone()
                }
            })
            .collect();
        if !unknown.is_empty() {
            return Err(format!(
                "unknown keybinding action(s): {}",
                unknown.join(", ")
            ));
        }
        config.presets()?;
        Ok(config)
    }

    /// A Source's `setup` command (`[sources.<id>]`, else by name).
    pub fn setup_for(&self, id: &str, name: &str) -> Option<String> {
        self.sources
            .get(id)
            .or_else(|| self.sources.get(name))
            .and_then(|p| p.setup.clone())
            .filter(|s| !s.trim().is_empty())
    }

    /// Whether code navigation may start a server for this Source (R83.4).
    pub fn code_enabled(&self, id: &str, name: &str) -> bool {
        self.code.enabled
            && self
                .sources
                .get(id)
                .or_else(|| self.sources.get(name))
                .and_then(|s| s.code)
                != Some(false)
    }

    /// Built-in Agent Presets merged with `[agents.*]`.
    pub fn presets(&self) -> Result<BTreeMap<String, Preset>, String> {
        preset::registry(&self.agents)
    }

    /// Effective (action, accelerator) pairs: defaults merged with overrides,
    /// unbound actions left out.
    pub fn keybindings(&self) -> Vec<(String, String)> {
        DEFAULT_KEYBINDINGS
            .iter()
            .map(|(a, d)| {
                (
                    a.to_string(),
                    self.keybindings
                        .get(*a)
                        .cloned()
                        .unwrap_or_else(|| d.to_string()),
                )
            })
            .filter(|(_, accel)| !accel.is_empty())
            .collect()
    }

    /// The key shown at the right of a context-menu item (R19.8): RADE's
    /// binding of the action it runs, as configured (none when unbound),
    /// or the Terminal's own key for Copy / Paste / Select All / Search.
    pub fn menu_accel(&self, item: &str) -> Option<String> {
        let action = match item {
            "pane-split-right" => "split_right",
            "pane-split-down" => "split_down",
            "pane-close" => "close_pane",
            "pane-zoom" => "zoom",
            "new-worktree" => "new_worktree",
            "review" => "open_review",
            // Ghostty's defaults (RADE leaves them to the Terminal).
            "pane-copy" => return Some("<Ctrl><Shift>c".into()),
            "pane-paste" => return Some("<Ctrl><Shift>v".into()),
            "pane-select-all" => return Some("<Ctrl><Shift>a".into()),
            "pane-search" => return Some("<Ctrl><Shift>f".into()),
            _ => return None,
        };
        self.keybindings()
            .into_iter()
            .find(|(a, _)| a == action)
            .map(|(_, k)| k)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn r19_8_menu_items_show_their_configured_key() {
        let mut c = super::Config::default();
        assert_eq!(
            c.menu_accel("pane-split-right").as_deref(),
            Some("<Ctrl><Shift>Return")
        );
        assert_eq!(c.menu_accel("pane-zoom").as_deref(), Some("<Ctrl><Shift>z"));
        assert_eq!(c.menu_accel("pane-copy").as_deref(), Some("<Ctrl><Shift>c"));
        assert_eq!(c.menu_accel("tab-rename"), None);
        // Rebound: the new key; unbound: none.
        c.keybindings
            .insert("split_right".into(), "<Alt>Return".into());
        c.keybindings.insert("zoom".into(), String::new());
        assert_eq!(
            c.menu_accel("pane-split-right").as_deref(),
            Some("<Alt>Return")
        );
        assert_eq!(c.menu_accel("pane-zoom"), None);
    }

    #[test]
    fn r112_h_code_font_follows_the_terminal_unless_set() {
        use super::{CodeFont, Config};
        let font = |t: &str| Config::parse(t).unwrap().code_font();
        let cf = |f: &str, s: &str, px: f32, l: bool| CodeFont {
            family: f.into(),
            style: s.into(),
            size: px,
            ligatures: l,
        };
        assert_eq!(font(""), cf("Maple Mono", "Medium", 13.0, true));
        assert_eq!(
            font("[terminal]\nfont = \"Geist Mono\"\nfont_style = \"Bold\"\n"),
            cf("Geist Mono", "Bold", 13.0, true)
        );
        assert_eq!(
            font("[code_font]\nfamily = \"Iosevka\"\nsize = 15\nligatures = false\n"),
            cf("Iosevka", "Regular", 15.0, false),
            "another family: Regular, not the Terminal's Medium"
        );
        assert_eq!(
            font("[code_font]\nfamily = \"Iosevka\"\nstyle = \"Light\"\n"),
            cf("Iosevka", "Light", 13.0, true)
        );
    }

    #[test]
    fn r102_i_terminal_theme_defaults_to_rade_and_takes_ghostty() {
        use super::{ColorScheme, Config, TerminalTheme};
        let d = Config::default();
        assert_eq!(d.terminal.theme, TerminalTheme::Rade);
        assert_eq!(
            (d.terminal.font.as_str(), d.terminal.font_style.as_str()),
            ("Maple Mono", "Medium")
        );
        assert_eq!((d.terminal.font_size, d.terminal.line_height), (14.0, 1.15));
        assert_eq!(d.appearance.color_scheme, ColorScheme::System);
        let c = Config::parse("[terminal]\ntheme = \"ghostty\"\nfont_size = 16\n").unwrap();
        assert_eq!(
            (c.terminal.theme, c.terminal.font_size),
            (TerminalTheme::Ghostty, 16.0)
        );
        assert_eq!(
            c.terminal.font, "Maple Mono",
            "unset keys keep their default"
        );
        assert!(Config::parse("[terminal]\ntheme = \"catppuccin\"\n").is_err());
        let c = Config::parse("[appearance]\ncolor_scheme = \"light\"\n").unwrap();
        assert_eq!(c.appearance.color_scheme, ColorScheme::Light);
    }

    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        let c = Config::parse("").unwrap();
        assert_eq!(
            c.limits,
            Limits {
                max_depth: 3,
                max_children: 8
            }
        );
        assert_eq!(c.desktop_notify, DesktopNotify::Smart);
        assert_eq!(c.viewer.editor, "code");
        assert_eq!(c.fanout.judge, "claude");
        assert!(c.scm.draft_command.starts_with("claude -p"));
        let c2 = Config::parse("[scm]\ndraft_command = \"cat\"\n").unwrap();
        assert_eq!(c2.scm.draft_command, "cat");
        assert_eq!(c.keybindings().len(), DEFAULT_KEYBINDINGS.len());
    }

    #[test]
    fn r117_l_agent_overview_is_ctrl_shift_space() {
        let c = Config::default();
        assert!(c
            .keybindings()
            .contains(&("agent_overview".into(), "<Ctrl><Shift>space".into())));
    }

    #[test]
    fn r17_3_defaults_never_plain_ctrl_or_shift_enter() {
        for (action, accel) in DEFAULT_KEYBINDINGS {
            let ctrl_shift =
                accel.contains("<Ctrl>") && (accel.contains("<Shift>") || accel.contains("<Alt>"));
            let alt_only = accel.starts_with("<Alt>") && !accel.contains("<Ctrl>");
            // R112.1: Ctrl+, (Settings, as in VS Code / GNOME apps) is the one
            // plain-Ctrl default: a legacy terminal has no code for it, so no
            // program can receive it.
            let settings = *action == "open_settings" && *accel == "<Ctrl>comma";
            // R124: F8 (voice) is a bare function key: held to talk, so it
            // must not need a chord.
            let voice = *action == "voice" && *accel == "F8";
            assert!(
                ctrl_shift || alt_only || settings || voice,
                "{action} = {accel}"
            );
            assert_ne!(*accel, "<Shift>Return");
            // R5.3: Ctrl+V reaches the program (Agents paste images with it).
            assert!(!accel.eq_ignore_ascii_case("<Ctrl>v"), "{action} = {accel}");
        }
    }

    #[test]
    fn overrides_merge_and_empty_unbinds() {
        let c = Config::parse(
            "desktop_notify = \"off\"\n[keybindings]\nnew_tab = \"<Ctrl><Shift>y\"\nzoom = \"\"\n[limits]\nmax_depth = 5\n",
        )
        .unwrap();
        let kb = c.keybindings();
        assert!(kb.contains(&("new_tab".into(), "<Ctrl><Shift>y".into())));
        assert!(!kb.iter().any(|(a, _)| a == "zoom"));
        assert_eq!(c.limits.max_depth, 5);
        assert_eq!(c.limits.max_children, 8);
        assert_eq!(c.desktop_notify, DesktopNotify::Off);
        let c = Config::parse("[viewer]\neditor = \"codium\"\n").unwrap();
        assert_eq!(c.viewer.editor, "codium");
        assert!(Config::parse("[viewer]\nbogus = 1\n").is_err());
    }

    #[test]
    fn rejects_unknown_actions_and_keys() {
        assert!(Config::parse("[keybindings]\nfly = \"<Alt>f\"")
            .unwrap_err()
            .contains("fly"));
        assert!(Config::parse("bogus = 1").is_err());
        assert!(
            Config::parse("[agents.x]\nprompt = \"arg\"").is_err(),
            "new preset needs command"
        );
    }

    #[test]
    fn r12_2_source_setup_by_id_or_name() {
        let c = Config::parse(
            "[sources.my-app]\nsetup = \"npm ci\"\n[sources.\"Other One\"]\nsetup = \"make\"\n[sources.blank]\nsetup = \"\"\n",
        )
        .unwrap();
        assert_eq!(c.setup_for("my-app", "My App").as_deref(), Some("npm ci"));
        assert_eq!(
            c.setup_for("other-one", "Other One").as_deref(),
            Some("make")
        );
        assert_eq!(c.setup_for("blank", "blank"), None);
        assert_eq!(c.setup_for("x", "x"), None);
        assert!(Config::parse("[sources.x]\nbogus = 1\n").is_err());
    }

    #[test]
    fn r31_d_old_project_names_are_errors_naming_the_new_ones() {
        let e = Config::parse("[projects.my-app]\nsetup = \"npm ci\"\n").unwrap_err();
        assert!(e.contains("[sources.*]"), "{e}");
        let e = Config::parse("[keybindings]\nproject_1 = \"<Alt>F1\"\n").unwrap_err();
        assert!(e.contains("project_1 (now source_1)"), "{e}");
    }

    #[test]
    fn r101_h_removed_tables_load_and_leave_the_layered_list() {
        // Even nonsense inside them is not an error: they are not read.
        let c = Config::parse(
            "[profiles.deep]\nagent = \"claude\"\nbogus = 1\n[task]\nverify = \"make test\"\n[report]\nx = 1\n[batch]\nparallel = 2\n[initiative]\nagents = 0\n",
        )
        .unwrap();
        assert_eq!(c, Config::default());
        for k in REMOVED {
            assert!(!LAYERED.contains(k) && !APP_WIDE.contains(k), "{k}");
        }
    }

    #[test]
    fn r83_code_config_and_per_repo_switch() {
        let c = Config::default();
        assert!(c.code.enabled && c.code_enabled("p1", "api"));
        assert_eq!((c.code.idle_minutes, c.code.max_servers), (15.0, 4));
        let c = Config::parse(
            "[code]\nmax_servers = 2\n[code.server.typescript]\ntsserver_path = \"~/ts5/lib\"\n[sources.api]\ncode = false\n",
        )
        .unwrap();
        assert!(!c.code_enabled("p1", "api") && c.code_enabled("p2", "web"));
        assert_eq!(
            c.code.server["typescript"].tsserver_path.as_deref(),
            Some("~/ts5/lib")
        );
        assert!(!Config::parse("[code]\nenabled = false\n")
            .unwrap()
            .code_enabled("p", "x"));
        assert!(Config::parse("[code.server.rust]\nbogus = 1\n").is_err());
        assert!(LAYERED.contains(&"code"));
    }

    #[test]
    fn r72_browser_chromium_path() {
        let c = Config::parse("[browser]\nchromium = \"/opt/c/chrome\"\n").unwrap();
        assert_eq!(
            c.browser.chromium.as_deref(),
            Some(std::path::Path::new("/opt/c/chrome"))
        );
        assert_eq!(Config::default().browser.chromium, None);
        assert!(Config::parse("[browser]\nengine = \"x\"\n").is_err());
        assert!(
            !LAYERED.contains(&"browser") && APP_WIDE.contains(&"browser"),
            "the user's file only"
        );
    }

    #[test]
    fn agents_and_idle_seconds() {
        let c = Config::parse("idle_seconds = 2\n[agents.fake]\ncommand = \"fake\"\n").unwrap();
        assert_eq!(c.idle_seconds, 2);
        assert_eq!(Config::default().idle_seconds, 5);
        assert!(Config::default().status_bar);
        assert!(!Config::parse("status_bar = false\n").unwrap().status_bar);
        let p = c.presets().unwrap();
        assert_eq!(p["fake"].command, "fake");
        assert!(p.contains_key("claude"));
    }

    /// Keys owned by Ghostty's Linux defaults (`ghostty +list-keybinds
    /// --default`, 1.3) or IBus (`unicode-hotkey`). A RADE default on one of
    /// these silently steals it (RADE's shortcuts run first). RADE's own
    /// actions that deliberately replace Ghostty's same action are allowed.
    #[test]
    fn r23_e_defaults_do_not_steal_gnome_ghostty_or_ibus_keys() {
        const TAKEN: &[&str] = &[
            "<Ctrl><Shift>a", // Ghostty select_all
            "<Ctrl><Shift>c", // copy
            "<Ctrl><Shift>v", // paste
            "<Ctrl><Shift>f", // search
            "<Ctrl><Shift>i", // inspector
            "<Ctrl><Shift>q", // quit
            "<Ctrl><Shift>u", // IBus Unicode entry
            "<Ctrl>space",    // IBus legacy trigger
            "<Super>space",   // GNOME / IBus input source switching
            "<Shift><Super>space",
            "<Alt>space", // GNOME window menu
            // GNOME defaults (gsettings wm / media-keys / mutter, R17.3,
            // R23-e): workspace switching, panels, show desktop, terminal,
            // logout, VT switching.
            "<Ctrl><Alt>Left",
            "<Ctrl><Alt>Right",
            "<Ctrl><Alt>Up",
            "<Ctrl><Alt>Down",
            "<Ctrl><Alt>Tab",
            "<Ctrl><Alt>Escape",
            "<Ctrl><Alt>Delete",
            "<Ctrl><Alt>t",
            "<Ctrl><Alt>d",
            "<Ctrl><Alt>F1",
            "<Ctrl><Alt>F2",
        ];
        // Ghostty 1.3's other Ctrl+Shift defaults (`ghostty +list-keybinds
        // --default`, Linux): a RADE default on one of them must say why it
        // takes it over (RADE's keys run before the Terminal sees them).
        const GHOSTTY: &[&str] = &[
            "<Ctrl><Shift>j",         // write_screen_file:paste
            "<Ctrl><Shift>n",         // new_window
            "<Ctrl><Shift>w",         // close_tab
            "<Ctrl><Shift>t",         // new_tab
            "<Ctrl><Shift>o",         // new_split:right
            "<Ctrl><Shift>e",         // new_split:down
            "<Ctrl><Shift>Page_Up",   // jump_to_prompt:-1
            "<Ctrl><Shift>Page_Down", // jump_to_prompt:1
            "<Ctrl><Shift>Return",    // toggle_split_zoom
            "<Ctrl><Shift>p",         // toggle_command_palette
            "<Ctrl><Shift>comma",     // reload_config
            "<Ctrl><Shift>Tab",       // previous_tab
            "<Alt>1",                 // goto_tab:1 … 8, last_tab (9)
            "<Alt>2",
            "<Alt>3",
            "<Alt>4",
            "<Alt>5",
            "<Alt>6",
            "<Alt>7",
            "<Alt>8",
            "<Alt>9",
            "<Ctrl><Alt>Left", // goto_split
            "<Ctrl><Alt>Right",
            "<Ctrl><Alt>Up",
            "<Ctrl><Alt>Down",
        ];
        const REPLACED: &[(&str, &str)] = &[
            ("new_tab", "<Ctrl><Shift>t"),          // same action, RADE's Tabs
            ("close_pane", "<Ctrl><Shift>w"),       // RADE closes the Pane
            ("add_source", "<Ctrl><Shift>n"),       // one window; N = new Source
            ("focus_down", "<Ctrl><Shift>j"),       // owner's vim-style focus keys
            ("split_right", "<Ctrl><Shift>Return"), // owner's Ghostty split key
            ("next_tab", "<Ctrl><Shift>Page_Down"), // RADE's Tabs
            ("prev_tab", "<Ctrl><Shift>Page_Up"),
            ("toggle_file_tree", "<Ctrl><Shift>e"), // RADE splits with its own keys
            ("command_palette", "<Ctrl><Shift>p"), // RADE's palette lists Ghostty-style actions too
            ("open_browser", "<Ctrl><Shift>o"),    // RADE splits with its own keys
            // R17.3: Alt+<n> switches Sources; RADE's Tabs have no numbers.
            ("source_1", "<Alt>1"),
            ("source_2", "<Alt>2"),
            ("source_3", "<Alt>3"),
            ("source_4", "<Alt>4"),
            ("source_5", "<Alt>5"),
            ("source_6", "<Alt>6"),
            ("source_7", "<Alt>7"),
            ("source_8", "<Alt>8"),
            ("source_9", "<Alt>9"),
        ];
        for (action, accel) in DEFAULT_KEYBINDINGS {
            assert!(
                !GHOSTTY.contains(accel) || REPLACED.contains(&(*action, *accel)),
                "{action} = {accel} takes a Ghostty key without saying why"
            );
            assert!(
                !TAKEN.contains(accel),
                "{action} = {accel} steals a Ghostty/IBus key"
            );
        }
    }
}
