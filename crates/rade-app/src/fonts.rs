//! The bundled fonts (PRD R102.3, ADR-0036 §4, ADR-0037): Inter, JetBrains
//! Mono and Maple Mono ship in the `rade-fonts` GResource. Pango and Ghostty
//! load fonts only from files, so each is written once to
//! `$XDG_RUNTIME_DIR/rade/fonts/`. Ghostty builds its own fontconfig config
//! (`FcInitLoadConfigAndFonts`), which reads `FONTCONFIG_FILE`: [`prepare`]
//! points it at a `fonts.conf` that includes the system's (or the user's
//! `FONTCONFIG_FILE`) and adds that folder. Nothing is installed.
use gtk::{gio, glib, prelude::*};
use gtk4 as gtk;
use pango::prelude::FontMapExt;
use std::path::{Path, PathBuf};

/// The font files of the GResource (`/rade/fonts/<name>`).
pub const FILES: &[&str] = &[
    "Inter-Regular.ttf",
    "Inter-Medium.ttf",
    "Inter-SemiBold.ttf",
    "JetBrainsMono-Variable.ttf",
    "MapleMono-Regular.ttf",
    "MapleMono-Medium.ttf",
    "MapleMono-Bold.ttf",
    "MapleMono-Italic.ttf",
    "MapleMono-MediumItalic.ttf",
    "MapleMono-BoldItalic.ttf",
];

pub fn dir() -> PathBuf {
    glib::user_runtime_dir().join("rade").join("fonts")
}

/// Writes the font files and `fonts.conf`, then sets `FONTCONFIG_FILE` for
/// this process (and so its children, which see the same fonts plus RADE's).
/// Before Ghostty and GTK first touch fontconfig.
pub fn prepare() {
    if let Err(e) = gio::resources_register_include!("rade-fonts.gresource") {
        eprintln!("rade: fonts resource: {e}");
        return;
    }
    let dir = dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("rade: {}: {e}", dir.display());
        return;
    }
    for name in FILES {
        if let Err(e) = write_font(&dir, name) {
            eprintln!("rade: font {name}: {e}");
        }
    }
    let base = std::env::var("FONTCONFIG_FILE")
        .ok()
        .filter(|f| !f.is_empty() && !f.starts_with(&*dir.to_string_lossy()));
    let conf = dir.join("fonts.conf");
    if write_if_changed(&dir, "fonts.conf", &fonts_conf(base.as_deref(), &dir)).is_ok() {
        std::env::set_var("FONTCONFIG_FILE", &conf);
    }
}

/// A fontconfig file: the system's (or `base`) plus `dir`.
pub fn fonts_conf(base: Option<&str>, dir: &Path) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let include = base.unwrap_or("/etc/fonts/fonts.conf");
    format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n\
         <!-- Written by RADE (ADR-0037): the fonts it bundles, over the usual config. -->\n\
         <fontconfig>\n  <include ignore_missing=\"yes\">{}</include>\n  <dir>{}</dir>\n</fontconfig>\n",
        esc(include),
        esc(&dir.to_string_lossy())
    )
}

fn write_font(dir: &Path, name: &str) -> Result<(), String> {
    let bytes = gio::resources_lookup_data(
        &format!("/rade/fonts/{name}"),
        gio::ResourceLookupFlags::NONE,
    )
    .map_err(|e| e.to_string())?;
    let path = dir.join(name);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64) {
        return Ok(());
    }
    write_aside(dir, name, &bytes)
}

fn write_if_changed(dir: &Path, name: &str, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(dir.join(name)).ok().as_deref() == Some(text) {
        return Ok(());
    }
    write_aside(dir, name, text.as_bytes())
}

/// Another RADE (the other flavor) may write the same file: write aside,
/// then rename over.
fn write_aside(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let tmp = dir.join(format!(".{name}.{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())
}

/// Adds the written fonts to GTK's font map before the first window, in
/// case fontconfig was set up before [`prepare`] ran.
pub fn register() {
    let Some(map) = gtk::Label::new(None).pango_context().font_map() else {
        return;
    };
    let dir = dir();
    for name in FILES {
        let path = dir.join(name);
        if let Err(e) = map.add_font_file(&path) {
            eprintln!("rade: font {name}: {e}");
        }
    }
}

thread_local! {
    static CODE_CSS: gtk::CssProvider = gtk::CssProvider::new();
}

/// The code font (R112.4) on the Viewer, Review diffs and other `monospace`
/// text, above the app's stylesheet; called at start and on every config.
pub fn apply_code_font(font: &rade_core::config::CodeFont) {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    CODE_CSS.with(|p| {
        p.load_from_string(&code_css(font));
        gtk::style_context_add_provider_for_display(
            &display,
            p,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    });
}

/// CSS weight of a style name (`Medium` → 500); italics are left to the text.
pub fn weight_of(style: &str) -> u16 {
    let s = style.to_ascii_lowercase().replace([' ', '-'], "");
    [
        ("thin", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("light", 300),
        ("semilight", 350),
        ("medium", 500),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
    ]
    .iter()
    .find(|(k, _)| s.starts_with(k))
    .map_or(400, |(_, w)| *w)
}

/// The stylesheet of [`apply_code_font`].
pub fn code_css(font: &rade_core::config::CodeFont) -> String {
    let family = font.family.replace(['"', '\\', ';', '{', '}'], "");
    let features = if font.ligatures {
        "normal"
    } else {
        "\"calt\" 0, \"liga\" 0, \"dlig\" 0"
    };
    format!(
        "textview.monospace, .monospace {{ font-family: \"{family}\", monospace; \
         font-size: {}px; font-weight: {}; font-feature-settings: {features}; }}\n",
        font.size,
        weight_of(&font.style)
    )
}

/// The same font for a Markdown page's `code` / `pre` (sizes stay relative):
/// the variables `markdown.css` reads.
pub fn code_css_web(font: &rade_core::config::CodeFont) -> String {
    let family = font
        .family
        .replace(['"', '\\', ';', '{', '}', '<', '>'], "");
    format!(
        ":root, html.dark {{ --code-font: \"{family}\"; --code-weight: {}; }}{}",
        weight_of(&font.style),
        if font.ligatures {
            ""
        } else {
            "\ncode, pre, tt { font-variant-ligatures: none; }"
        }
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn r102_i_fonts_conf_includes_the_base_and_adds_the_folder() {
        let dir = std::path::Path::new("/run/user/1/rade/fonts");
        let conf = super::fonts_conf(None, dir);
        assert!(conf.contains("<include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>"));
        assert!(conf.contains("<dir>/run/user/1/rade/fonts</dir>"));
        let own = super::fonts_conf(Some("/home/a&b/fonts.conf"), dir);
        assert!(own.contains(">/home/a&amp;b/fonts.conf<"));
    }

    #[test]
    fn r112_h_code_css_sets_family_size_weight_and_ligatures() {
        let mut f = rade_core::config::Config::default().code_font();
        assert_eq!(
            super::code_css(&f),
            "textview.monospace, .monospace { font-family: \"Maple Mono\", monospace; \
             font-size: 13px; font-weight: 500; font-feature-settings: normal; }\n"
        );
        f.family = "Evil\"; } * {".into();
        f.ligatures = false;
        f.style = "SemiBold".into();
        let css = super::code_css(&f);
        assert!(
            css.contains("font-family: \"Evil  * \", monospace"),
            "{css}"
        );
        assert!(css.contains("font-weight: 600; font-feature-settings: \"calt\" 0"));
        assert!(super::code_css_web(&f).contains("font-variant-ligatures: none;"));
        assert_eq!(super::weight_of("ExtraBold"), 800);
        assert_eq!(super::weight_of("Regular"), 400);
    }
}
