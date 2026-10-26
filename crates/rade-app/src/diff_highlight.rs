//! Syntax highlighting of diff lines into Pango markup, shared by the
//! Review Pane's rows and the Diff Pane (R122, D16): a hidden
//! GtkSourceBuffer highlights the lines of a file once.

use gtk::glib;
use gtk4 as gtk;
use sourceview5::prelude::*;

/// Files with more diff lines than this are shown without highlighting.
/// ponytail: highlighting runs on the UI thread on first show; move it off
/// thread if someone reviews 20k-line files often.
pub(crate) const HIGHLIGHT_MAX: usize = 20_000;

/// A line shown at most this long (a minified file's one line would make
/// a screen-tall row and a slow layout).
const MAX_LINE_CHARS: usize = 2000;

pub(crate) fn shorten(t: &str) -> std::borrow::Cow<'_, str> {
    match t.char_indices().nth(MAX_LINE_CHARS) {
        Some((i, _)) => format!("{}…", &t[..i]).into(),
        None => t.into(),
    }
}

/// A hunk header (`@@ -1,5 +1,5 @@ fn x`) as Pango markup. Maple Mono (the
/// default code font) draws `@` as a small swirl, so `@@` reads as one odd
/// glyph: its `cv01` alternate is the plain `@`.
pub(crate) fn hunk_header_markup(header: &str) -> String {
    glib::markup_escape_text(header).replace("@@", "<span font_features=\"cv01\">@@</span>")
}

/// The Pango markup of each of `texts` (the lines of file `path`), made in
/// `b`: highlighted when the language is known and the file is not larger
/// than [`HIGHLIGHT_MAX`], else escaped plain text.
pub(crate) fn highlight(
    path: &str,
    texts: &[std::borrow::Cow<str>],
    b: &sourceview5::Buffer,
) -> Vec<String> {
    let lang = sourceview5::LanguageManager::default().guess_language(Some(path), None);
    match lang {
        Some(lang) if texts.len() <= HIGHLIGHT_MAX => {
            b.set_language(Some(&lang));
            b.set_text(&texts.join("\n"));
            b.ensure_highlight(&b.start_iter(), &b.end_iter());
            let out = (0..texts.len() as i32).map(|i| line_markup(b, i)).collect();
            b.set_text("");
            out
        }
        _ => texts
            .iter()
            .map(|t| glib::markup_escape_text(t).to_string())
            .collect(),
    }
}

/// One line of the highlighter buffer as Pango markup: each run of text
/// with the colour, weight and style of its highest-priority tags.
fn line_markup(b: &sourceview5::Buffer, line: i32) -> String {
    let Some(mut it) = b.iter_at_line(line) else {
        return String::new();
    };
    let mut end = it;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    let mut out = String::new();
    while it < end {
        let mut next = it;
        next.forward_to_tag_toggle(None::<&gtk::TextTag>);
        if next > end || next <= it {
            next = end;
        }
        let text = glib::markup_escape_text(&b.text(&it, &next, true));
        let (mut fg, mut bold, mut italic) = (None, false, false);
        for t in it.tags() {
            if t.is_foreground_set() {
                fg = t.foreground_rgba();
            }
            if t.is_weight_set() {
                bold = t.weight() >= 600;
            }
            if t.is_style_set() {
                italic = t.style() == gtk::pango::Style::Italic;
            }
        }
        let mut attrs = String::new();
        if let Some(c) = fg {
            let h = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            attrs += &format!(
                " foreground=\"#{:02x}{:02x}{:02x}\"",
                h(c.red()),
                h(c.green()),
                h(c.blue())
            );
        }
        if bold {
            attrs += " weight=\"bold\"";
        }
        if italic {
            attrs += " style=\"italic\"";
        }
        if attrs.is_empty() {
            out += &text;
        } else {
            out += &format!("<span{attrs}>{text}</span>");
        }
        it = next;
    }
    out
}
