//! The Settings page's model (PRD R112, ADR-0037): the app-wide settings of
//! `config.toml` it shows, search, which are modified, and edits that keep
//! the file's comments and other keys (`toml_edit`). GTK-free.
use crate::config::Config;
use toml_edit::{DocumentMut, Item, Table};

/// How a setting is edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A monospace font family (the app lists them).
    Font,
    /// A style of the chosen family (the app lists them).
    FontStyle,
    /// One of these strings.
    Choice(&'static [&'static str]),
    Number {
        min: f64,
        max: f64,
        step: f64,
    },
    Integer {
        min: i64,
        max: i64,
    },
    Bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Setting {
    /// Dotted `config.toml` key.
    pub key: &'static str,
    pub category: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub kind: Kind,
}

/// Category order of the page.
pub const CATEGORIES: &[&str] = &[
    "Terminal",
    "Code",
    "Theme",
    "Notifications",
    "Agents",
    "Source Control",
    "Voice",
    "Statusline",
    "Updates",
];

pub const SETTINGS: &[Setting] = &[
    Setting {
        key: "terminal.font",
        category: "Terminal",
        label: "Font family",
        description:
            "Monospace font of every Terminal. Maple Mono and JetBrains Mono ship with RADE.",
        kind: Kind::Font,
    },
    Setting {
        key: "terminal.font_style",
        category: "Terminal",
        label: "Font style",
        description: "Weight of normal text; bold text stays bold.",
        kind: Kind::FontStyle,
    },
    Setting {
        key: "terminal.font_size",
        category: "Terminal",
        label: "Font size",
        description: "In pixels at 100 % display scale.",
        kind: Kind::Number {
            min: 8.0,
            max: 32.0,
            step: 1.0,
        },
    },
    Setting {
        key: "terminal.line_height",
        category: "Terminal",
        label: "Line height",
        description: "Multiplies the font's own line height.",
        kind: Kind::Number {
            min: 1.0,
            max: 2.0,
            step: 0.05,
        },
    },
    Setting {
        key: "terminal.ligatures",
        category: "Terminal",
        label: "Ligatures",
        description: "Draw -> != => as one glyph where the font has them.",
        kind: Kind::Bool,
    },
    Setting {
        key: "terminal.theme",
        category: "Terminal",
        label: "Look",
        description:
            "rade: RADE's colors, font and padding. ghostty: your Ghostty config's own look.",
        kind: Kind::Choice(&["rade", "ghostty"]),
    },
    Setting {
        key: "code_font.family",
        category: "Code",
        label: "Code font family",
        description: "Font of the Viewer, Review diffs and Markdown code. Empty: the Terminal's.",
        kind: Kind::Font,
    },
    Setting {
        key: "code_font.style",
        category: "Code",
        label: "Code font style",
        description: "Empty: the Terminal's style when the family is the Terminal's, else Regular.",
        kind: Kind::FontStyle,
    },
    Setting {
        key: "code_font.size",
        category: "Code",
        label: "Code font size",
        description: "In pixels at 100 % display scale.",
        kind: Kind::Number {
            min: 8.0,
            max: 32.0,
            step: 1.0,
        },
    },
    Setting {
        key: "code_font.ligatures",
        category: "Code",
        label: "Code ligatures",
        description: "Draw -> != => as one glyph where the font has them.",
        kind: Kind::Bool,
    },
    Setting {
        key: "appearance.color_scheme",
        category: "Theme",
        label: "Color scheme",
        description: "system: dark unless the system asks for light.",
        kind: Kind::Choice(&["system", "dark", "light"]),
    },
    Setting {
        key: "desktop_notify",
        category: "Notifications",
        label: "Desktop notifications",
        description: "smart: only when RADE is not focused or the Agent is not shown.",
        kind: Kind::Choice(&["smart", "always", "off"]),
    },
    Setting {
        key: "idle_seconds",
        category: "Notifications",
        label: "Idle after",
        description: "Seconds without output before `rade wait --until idle` returns.",
        kind: Kind::Integer { min: 1, max: 3600 },
    },
    Setting {
        key: "limits.max_depth",
        category: "Agents",
        label: "Max spawn depth",
        description: "How deep Agents may spawn Agents.",
        kind: Kind::Integer { min: 1, max: 10 },
    },
    Setting {
        key: "limits.max_children",
        category: "Agents",
        label: "Max children",
        description: "Agents one Agent may spawn.",
        kind: Kind::Integer { min: 1, max: 64 },
    },
    Setting {
        key: "scm.view",
        category: "Source Control",
        label: "Changes view",
        description: "List or tree of the changed files.",
        kind: Kind::Choice(&["list", "tree"]),
    },
    Setting {
        key: "scm.sort",
        category: "Source Control",
        label: "Sort changes by",
        description: "Path, file name or status (list view).",
        kind: Kind::Choice(&["path", "name", "status"]),
    },
    Setting {
        key: "scm.autofetch",
        category: "Source Control",
        label: "Auto-fetch",
        description: "Fetch every Changes card's remote in the background. Never pulls or pushes.",
        kind: Kind::Bool,
    },
    Setting {
        key: "scm.autofetch_period",
        category: "Source Control",
        label: "Auto-fetch every (seconds)",
        description: "60 to 3600.",
        kind: Kind::Integer { min: 60, max: 3600 },
    },
    Setting {
        key: "scm.diff_layout",
        category: "Source Control",
        label: "Diff Pane layout",
        description: "Side by side from 900 px wide (auto), inline or side by side.",
        kind: Kind::Choice(&["auto", "inline", "side_by_side"]),
    },
    Setting {
        key: "scm.quick_diff",
        category: "Source Control",
        label: "Quick Diff in the Viewer",
        description: "Mark lines changed against the index in the Viewer's gutter.",
        kind: Kind::Bool,
    },
    Setting {
        key: "voice.agent",
        category: "Voice",
        label: "Voice agent",
        description: "Who turns your words into a prompt. Claude Code is faster (one process per draft).",
        kind: Kind::Choice(&["claude", "codex"]),
    },
    Setting {
        key: "voice.tools",
        category: "Voice",
        label: "Voice agent reads files",
        description: "Read, search and list files of the checkout to understand you. Never writes.",
        kind: Kind::Choice(&["read", "none"]),
    },
    Setting {
        key: "voice.max_turns",
        category: "Voice",
        label: "Max steps",
        description: "Claude Code turns per draft for reading and searching files (1 to 20). Codex has no limit.",
        kind: Kind::Integer { min: 1, max: 20 },
    },
    Setting {
        key: "voice.timeout_secs",
        category: "Voice",
        label: "Draft timeout (seconds)",
        description: "Stop a draft that takes longer (10 to 300).",
        kind: Kind::Integer { min: 10, max: 300 },
    },
    Setting {
        key: "voice.max_recording_secs",
        category: "Voice",
        label: "Max recording (seconds)",
        description: "A take stops by itself after this long (10 to 600).",
        kind: Kind::Integer { min: 10, max: 600 },
    },
    Setting {
        key: "voice.live_preview",
        category: "Voice",
        label: "Live words",
        description: "Show what you say while you speak (one speech-to-text request per second).",
        kind: Kind::Bool,
    },
    Setting {
        key: "voice.draft_language",
        category: "Voice",
        label: "Draft language",
        description: "The language you spoke, or always English.",
        kind: Kind::Choice(&["same", "en"]),
    },
    Setting {
        key: "voice.stt_language",
        category: "Voice",
        label: "Speech language",
        description: "The language you speak, for speech-to-text. Auto detects it per take; set it when you always speak one language (more reliable on short takes). Other ISO 639-1 codes (ja, zh, fr, …) in config.toml.",
        kind: Kind::Choice(&["auto", "vi", "en"]),
    },
    Setting {
        key: "status_bar",
        category: "Statusline",
        label: "Show the Statusline",
        description: "The bar at the bottom of the window.",
        kind: Kind::Bool,
    },
    Setting {
        key: "system_monitor",
        category: "Statusline",
        label: "System monitor",
        description:
            "CPU, memory, GPU and network in the Statusline; a click lists the busiest Panes.",
        kind: Kind::Bool,
    },
    Setting {
        key: "update.check",
        category: "Updates",
        label: "Check for updates",
        description: "Once a day, ask GitHub whether a newer RADE is out. Off: only Check now asks.",
        kind: Kind::Bool,
    },
    Setting {
        key: "update.channel",
        category: "Updates",
        label: "Update channel",
        description: "Stable releases, or release candidates too (rc).",
        kind: Kind::Choice(&["stable", "rc"]),
    },
];

pub fn find(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// A dotted key's value in a table.
fn get<'a>(t: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let (head, rest) = match key.split_once('.') {
        Some((h, r)) => (h, Some(r)),
        None => (key, None),
    };
    match (t.get(head)?, rest) {
        (v, None) => Some(v),
        (toml::Value::Table(sub), Some(r)) => get(sub, r),
        _ => None,
    }
}

/// The value the app uses: the effective config's, which is the default
/// where the file says nothing.
pub fn value(config: &Config, key: &str) -> toml::Value {
    let table = toml::Table::try_from(config).unwrap_or_default();
    get(&table, key)
        .cloned()
        .unwrap_or(toml::Value::String(String::new()))
}

pub fn default_value(key: &str) -> toml::Value {
    value(&Config::default(), key)
}

/// Whether the effective value differs from the default.
pub fn modified(config: &Config, key: &str) -> bool {
    value(config, key) != default_value(key)
}

/// Search: every word of `query` in the label, description, category or key.
pub fn matches(s: &Setting, query: &str) -> bool {
    let hay = format!("{} {} {} {}", s.label, s.description, s.category, s.key).to_lowercase();
    query
        .split_whitespace()
        .all(|w| hay.contains(&w.to_lowercase()))
}

/// Categories with at least one match, in page order.
pub fn categories(query: &str) -> Vec<&'static str> {
    CATEGORIES
        .iter()
        .copied()
        .filter(|c| {
            SETTINGS
                .iter()
                .any(|s| s.category == *c && matches(s, query))
        })
        .collect()
}

/// `config.toml` with `key` set: comments and other keys kept; the result
/// must still parse, else the file's error and nothing written.
pub fn set(text: &str, key: &str, v: toml::Value) -> Result<String, String> {
    let s = find(key).ok_or_else(|| format!("unknown setting '{key}'"))?;
    check(s, &v)?;
    let mut doc = document(text)?;
    let (tables, leaf) = split(key);
    let mut t: &mut Table = doc.as_table_mut();
    for name in tables {
        if !t.contains_key(name) {
            let mut new = Table::new();
            new.set_implicit(false);
            t.insert(name, Item::Table(new));
        }
        t = t[name]
            .as_table_mut()
            .ok_or_else(|| format!("'{name}' in config.toml is not a table"))?;
    }
    let item = toml_edit::value(edit_value(&v)?);
    match t.get_mut(leaf) {
        Some(slot) => {
            // Keep the comment written before / after the old value.
            let decor = slot.as_value().map(|old| old.decor().clone());
            *slot = item;
            if let (Some(d), Some(new)) = (decor, slot.as_value_mut()) {
                *new.decor_mut() = d;
            }
        }
        None => t[leaf] = item,
    }
    checked(doc.to_string())
}

/// `config.toml` without `key` (back to the default); an emptied table goes.
pub fn reset(text: &str, key: &str) -> Result<String, String> {
    find(key).ok_or_else(|| format!("unknown setting '{key}'"))?;
    let mut doc = document(text)?;
    let (tables, leaf) = split(key);
    match tables.as_slice() {
        [] => {
            doc.remove(leaf);
        }
        [name] => {
            if let Some(t) = doc.get_mut(name).and_then(Item::as_table_mut) {
                t.remove(leaf);
                if t.is_empty() {
                    doc.remove(name);
                }
            }
        }
        _ => return Err(format!("'{key}' is nested too deep")),
    }
    checked(doc.to_string())
}

fn split(key: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let leaf = parts.pop().unwrap_or(key);
    (parts, leaf)
}

fn document(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>()
        .map_err(|e| format!("config.toml: {}", e.to_string().trim()))
}

fn checked(text: String) -> Result<String, String> {
    Config::parse(&text).map_err(|e| format!("config.toml: {e}"))?;
    Ok(text)
}

fn check(s: &Setting, v: &toml::Value) -> Result<(), String> {
    let bad = || format!("{}: {v} is not allowed", s.key);
    match (s.kind, v) {
        (Kind::Font | Kind::FontStyle, toml::Value::String(t)) if !t.contains('\n') => Ok(()),
        (Kind::Choice(list), toml::Value::String(t)) if list.contains(&t.as_str()) => Ok(()),
        (Kind::Number { min, max, .. }, toml::Value::Float(f)) if (min..=max).contains(f) => Ok(()),
        (Kind::Number { min, max, .. }, toml::Value::Integer(i))
            if (min..=max).contains(&(*i as f64)) =>
        {
            Ok(())
        }
        (Kind::Integer { min, max }, toml::Value::Integer(i)) if (min..=max).contains(i) => Ok(()),
        (Kind::Bool, toml::Value::Boolean(_)) => Ok(()),
        _ => Err(bad()),
    }
}

fn edit_value(v: &toml::Value) -> Result<toml_edit::Value, String> {
    Ok(match v {
        toml::Value::String(s) => s.as_str().into(),
        toml::Value::Integer(i) => (*i).into(),
        // Whole numbers stay whole (`font_size = 15`, not `15.0`).
        toml::Value::Float(f) if f.fract() == 0.0 && f.abs() < 1e9 => (*f as i64).into(),
        toml::Value::Float(f) => ((f * 1000.0).round() / 1000.0).into(),
        toml::Value::Boolean(b) => (*b).into(),
        other => return Err(format!("unsupported value {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r120_f_scm_settings_rows_are_found() {
        for k in [
            "scm.view",
            "scm.sort",
            "scm.autofetch",
            "scm.autofetch_period",
            "scm.diff_layout",
            "scm.quick_diff",
        ] {
            let s = find(k).unwrap_or_else(|| panic!("{k}"));
            assert_eq!(s.category, "Source Control");
        }
        assert!(set("", "scm.view", toml::Value::String("tree".into())).is_ok());
        assert!(set("", "scm.view", toml::Value::String("grid".into())).is_err());
        assert!(set("", "scm.autofetch_period", toml::Value::Integer(30)).is_err());
    }

    #[test]
    fn r112_a_every_setting_has_a_category_a_default_and_a_valid_kind() {
        let d = Config::default();
        for s in SETTINGS {
            assert!(CATEGORIES.contains(&s.category), "{}", s.key);
            let v = value(&d, s.key);
            assert!(check(s, &v).is_ok(), "{} default {v} fits its kind", s.key);
            assert!(!modified(&d, s.key));
        }
        assert_eq!(
            value(&d, "terminal.font"),
            toml::Value::String("Maple Mono".into())
        );
    }

    #[test]
    fn r112_b_set_keeps_comments_and_other_keys_and_reset_removes() {
        let text =
            "# my config\nidle_seconds = 9 # mine\n\n[terminal]\n# size in px\nfont_size = 14\n";
        let out = set(text, "terminal.font_size", toml::Value::Float(16.0)).unwrap();
        assert_eq!(
            out,
            "# my config\nidle_seconds = 9 # mine\n\n[terminal]\n# size in px\nfont_size = 16\n"
        );
        let out = set(&out, "idle_seconds", toml::Value::Integer(12)).unwrap();
        assert!(out.contains("idle_seconds = 12 # mine\n"));
        let out = set(
            &out,
            "appearance.color_scheme",
            toml::Value::String("light".into()),
        )
        .unwrap();
        assert!(out.ends_with("[appearance]\ncolor_scheme = \"light\"\n"));
        let c = Config::parse(&out).unwrap();
        assert!(modified(&c, "appearance.color_scheme"));
        assert!(modified(&c, "terminal.font_size"));
        let out = reset(&out, "appearance.color_scheme").unwrap();
        assert!(!out.contains("appearance"));
        let out = reset(&out, "terminal.font_size").unwrap();
        assert!(!out.contains("[terminal]"), "an emptied table goes: {out}");
        assert!(!Config::parse(&out)
            .map(|c| modified(&c, "terminal.font_size"))
            .unwrap());
        assert_eq!(
            set("", "status_bar", toml::Value::Boolean(false)).unwrap(),
            "status_bar = false\n"
        );
    }

    #[test]
    fn r112_b_bad_values_and_a_broken_file_write_nothing() {
        let t = toml::Value::String("catppuccin".into());
        assert!(set("", "terminal.theme", t)
            .unwrap_err()
            .contains("not allowed"));
        assert!(set("", "terminal.font_size", toml::Value::Float(99.0)).is_err());
        assert!(set("", "nope", toml::Value::Boolean(true)).is_err());
        let err = set("[terminal\n", "status_bar", toml::Value::Boolean(true)).unwrap_err();
        assert!(err.starts_with("config.toml:"), "{err}");
        let err = set(
            "terminal = 3\n",
            "terminal.font_size",
            toml::Value::Float(15.0),
        )
        .unwrap_err();
        assert!(err.contains("not a table"), "{err}");
        assert_eq!(
            set("", "terminal.line_height", toml::Value::Float(1.2000001)).unwrap(),
            "[terminal]\nline_height = 1.2\n"
        );
    }

    #[test]
    fn r112_d_search_by_words_and_categories_that_match() {
        let hits: Vec<&str> = SETTINGS
            .iter()
            .filter(|s| matches(s, "font SIZE"))
            .map(|s| s.key)
            .collect();
        assert_eq!(hits, ["terminal.font_size", "code_font.size"]);
        assert_eq!(categories("font"), ["Terminal", "Code"]);
        assert_eq!(categories(""), CATEGORIES);
        assert_eq!(CATEGORIES.last(), Some(&"Updates"));
        assert_eq!(categories("channel"), ["Updates"]);
        assert_eq!(categories("light"), ["Theme"]);
        assert!(categories("zzz").is_empty());
    }
}
