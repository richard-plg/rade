//! Markdown → a complete HTML page for the Viewer's WebView (R7.3): GFM
//! (tables, task lists, strikethrough, autolinks, footnotes, alerts),
//! code highlighted at render time with syntect CSS classes, Mermaid
//! fences left for the bundled mermaid.js, `file:` links pointed at the
//! Viewer's `rade-file:` scheme.
//!
//! Security: the page carries a Content-Security-Policy that allows no
//! script at all and loads nothing but `rade-file:` and `data:` images, so
//! remote requests are blocked, and raw HTML (kept: READMEs use it) can't
//! run anything or embed frames. The Viewer also disables script in markup
//! and injects only its bundled renderer.

use comrak::adapters::CodefenceRendererAdapter;
use comrak::options::Plugins;
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::Options;
use std::collections::HashMap;
use std::fmt::{self, Write};
use std::sync::{Arc, OnceLock};
use syntect::highlighting::ThemeSet;
use syntect::html::{css_for_theme_with_class_style, ClassStyle};

const CSS: &str = include_str!("markdown.css");
const CSP: &str =
    "default-src 'none'; img-src rade-file: data:; style-src 'unsafe-inline'; font-src data:";
const LIGHT_THEME: &str = "InspiredGitHub";
const DARK_THEME: &str = "base16-ocean.dark";

fn adapter() -> &'static SyntectAdapter {
    static A: OnceLock<SyntectAdapter> = OnceLock::new();
    A.get_or_init(|| SyntectAdapterBuilder::new().css().build())
}

/// Token colors for code blocks, one theme per appearance.
fn code_css(dark: bool) -> &'static str {
    static CSS: OnceLock<[String; 2]> = OnceLock::new();
    let both = CSS.get_or_init(|| {
        let themes = ThemeSet::load_defaults();
        [LIGHT_THEME, DARK_THEME].map(|name| {
            css_for_theme_with_class_style(&themes.themes[name], ClassStyle::Spaced)
                .unwrap_or_default()
        })
    });
    &both[dark as usize]
}

/// ```` ```mermaid ```` → `<pre class="mermaid">` for mermaid.js.
struct Mermaid;

impl CodefenceRendererAdapter for Mermaid {
    fn write(
        &self,
        out: &mut dyn Write,
        _lang: &str,
        _meta: &str,
        code: &str,
        _sourcepos: Option<comrak::nodes::Sourcepos>,
    ) -> fmt::Result {
        out.write_str("<pre class=\"mermaid\">")?;
        comrak::html::escape(out, code)?;
        out.write_str("</pre>\n")
    }
}

/// `file:` URLs load through the Viewer's own scheme; everything else is
/// kept (relative paths resolve against the document's `rade-file:` URI).
pub fn rewrite_url(url: &str) -> String {
    match url.get(..5) {
        Some(s) if s.eq_ignore_ascii_case("file:") => {
            let rest = url[5..].trim_start_matches('/');
            format!("rade-file:///{rest}")
        }
        _ => url.to_string(),
    }
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    let e = &mut o.extension;
    e.strikethrough = true;
    e.table = true;
    e.autolink = true;
    e.tasklist = true;
    e.footnotes = true;
    e.alerts = true;
    e.header_id_prefix = Some(String::new());
    e.front_matter_delimiter = Some("---".into());
    e.link_url_rewriter = Some(Arc::new(rewrite_url));
    e.image_url_rewriter = Some(Arc::new(rewrite_url));
    o.render.r#unsafe = true; // raw HTML (READMEs use it); the CSP makes it inert
    o.render.tasklist_classes = true;
    o.render.github_pre_lang = false;
    o
}

/// The rendered body only (tests, and the page below).
pub fn to_html_body(md: &str) -> String {
    let mermaid = Mermaid;
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(adapter());
    plugins.render.codefence_renderers = HashMap::from([(
        "mermaid".to_string(),
        &mermaid as &dyn CodefenceRendererAdapter,
    )]);
    comrak::markdown_to_html_with_plugins(md, &options(), &plugins)
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    let _ = comrak::html::escape(&mut out, s);
    out
}

/// A complete page: CSP, styles for `dark` or light, the rendered body.
pub fn to_html_page(md: &str, title: &str, dark: bool) -> String {
    to_html_page_styled(md, title, dark, code_css(dark))
}

/// [`to_html_page`] with the caller's CSS (design tokens, code colors and
/// font, R112.5) in place of the built-in code theme.
pub fn to_html_page_styled(md: &str, title: &str, dark: bool, code: &str) -> String {
    format!(
        "<!doctype html>\n<html class=\"{scheme}\"><head><meta charset=\"utf-8\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">\
<meta name=\"color-scheme\" content=\"{scheme}\">\
<title>{title}</title><style>{CSS}\n{code}</style></head>\n\
<body><article class=\"markdown-body\">\n{body}</article></body></html>\n",
        scheme = if dark { "dark" } else { "light" },
        title = escape(title),
        body = to_html_body(md),
    )
}

pub use pango::to_pango;

// ---- Markdown → Pango markup -----------------------------------------------

mod pango {
    use comrak::nodes::{AstNode, ListType, NodeValue};
    use comrak::{parse_document, Arena, Options};

    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    /// Markdown as Pango markup for a wrapping GtkLabel (hover docs, R90):
    /// headings bold, lists with `•` / `1.` / `☐` / `☑`, `code` monospace,
    /// links clickable. Raw HTML is shown as text.
    pub fn to_pango(md: &str) -> String {
        let mut o = Options::default();
        o.extension.strikethrough = true;
        o.extension.autolink = true;
        o.extension.tasklist = true;
        o.extension.table = true;
        let arena = Arena::new();
        let root = parse_document(&arena, md, &o);
        join_blocks(root, 0)
    }

    fn blocks<'a>(n: &'a AstNode<'a>, depth: usize) -> Vec<String> {
        n.children().filter_map(|c| block(c, depth)).collect()
    }

    /// Blocks joined by a blank line, but a heading sits right on its content.
    fn join_blocks<'a>(n: &'a AstNode<'a>, depth: usize) -> String {
        let mut out = String::new();
        let mut after_heading = false;
        for c in n.children() {
            let Some(b) = block(c, depth) else { continue };
            if !out.is_empty() {
                out.push_str(if after_heading { "\n" } else { "\n\n" });
            }
            out.push_str(&b);
            after_heading = matches!(c.data.borrow().value, NodeValue::Heading(_));
        }
        out
    }

    fn block<'a>(n: &'a AstNode<'a>, depth: usize) -> Option<String> {
        let value = n.data.borrow().value.clone();
        Some(match value {
            // Headings stay small: the label sits in a popover.
            NodeValue::Heading(h) if h.level <= 2 => {
                format!(
                    "<span size=\"medium\" weight=\"bold\">{}</span>",
                    inlines(n)
                )
            }
            NodeValue::Heading(_) => format!("<b>{}</b>", inlines(n)),
            NodeValue::Paragraph => inlines(n),
            NodeValue::List(l) => {
                let indent = "    ".repeat(depth);
                let mut lines = Vec::new();
                for (i, item) in n.children().enumerate() {
                    let mut marker = match l.list_type {
                        ListType::Ordered => format!("{}. ", l.start + i),
                        ListType::Bullet => "• ".to_string(),
                    };
                    if let NodeValue::TaskItem(t) = &item.data.borrow().value {
                        marker = if t.symbol.is_some() { "☑ " } else { "☐ " }.into();
                    }
                    let mut first = true;
                    for c in item.children() {
                        if matches!(c.data.borrow().value, NodeValue::List(_)) {
                            lines.extend(block(c, depth + 1));
                            continue;
                        }
                        let text = block(c, depth + 1).unwrap_or_default();
                        if first {
                            lines.push(format!("{indent}{marker}{text}"));
                            first = false;
                        } else {
                            lines.push(format!("{indent}    {text}"));
                        }
                    }
                    if first {
                        lines.push(format!("{indent}{}", marker.trim_end()));
                    }
                }
                lines.join("\n")
            }
            NodeValue::CodeBlock(c) => {
                format!("<tt>{}</tt>", esc(c.literal.trim_end_matches('\n')))
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) | NodeValue::Alert(_) => {
                format!("<i>{}</i>", blocks(n, depth).join("\n"))
            }
            NodeValue::ThematicBreak => "───".into(),
            // Template hints (`<!-- … -->`) are not content.
            NodeValue::HtmlBlock(h) if h.literal.trim_start().starts_with("<!--") => return None,
            NodeValue::HtmlBlock(h) => esc(h.literal.trim_end()),
            NodeValue::Table(_) => n
                .children()
                .map(|row| {
                    let cells: Vec<String> = row.children().map(inlines).collect();
                    cells.join(" · ")
                })
                .collect::<Vec<_>>()
                .join("\n"),
            NodeValue::FrontMatter(_) => return None,
            _ => {
                let b = blocks(n, depth);
                if b.is_empty() {
                    return None;
                }
                b.join("\n\n")
            }
        })
    }

    fn inlines<'a>(n: &'a AstNode<'a>) -> String {
        n.children().map(inline).collect()
    }

    fn inline<'a>(n: &'a AstNode<'a>) -> String {
        let value = n.data.borrow().value.clone();
        match value {
            NodeValue::Text(t) => esc(&t),
            NodeValue::SoftBreak => " ".into(),
            NodeValue::LineBreak => "\n".into(),
            NodeValue::Code(c) => format!("<tt>{}</tt>", esc(&c.literal)),
            NodeValue::Emph => format!("<i>{}</i>", inlines(n)),
            NodeValue::Strong => format!("<b>{}</b>", inlines(n)),
            NodeValue::Strikethrough => format!("<s>{}</s>", inlines(n)),
            NodeValue::Link(l) => {
                let text = inlines(n);
                // Only web and mail links are clickable (a label opens them).
                if ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|p| l.url.starts_with(p))
                {
                    format!("<a href=\"{}\">{text}</a>", esc(&l.url))
                } else {
                    text
                }
            }
            NodeValue::Image(l) => esc(&l.url),
            NodeValue::HtmlInline(h) if h.starts_with("<!--") => String::new(),
            NodeValue::HtmlInline(h) => esc(&h),
            _ => inlines(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r7_3_gfm_extensions() {
        let h = to_html_body(
            "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n~~old~~ \
             see https://example.com and a note[^1]\n\n[^1]: note\n\n> [!NOTE]\n> hi\n",
        );
        assert!(
            h.contains("<h1 id=\"title\">Title<a href=\"#title\""),
            "{h}"
        );
        assert!(h.contains("<table>") && h.contains("<td>1</td>"), "{h}");
        assert!(
            h.contains("type=\"checkbox\"") && h.contains("checked"),
            "{h}"
        );
        assert!(h.contains("class=\"task-list-item"), "{h}");
        assert!(h.contains("<del>old</del>"), "{h}");
        assert!(h.contains("<a href=\"https://example.com\">"), "{h}");
        assert!(h.contains("footnote"), "{h}");
        assert!(h.contains("markdown-alert-note"), "{h}");
    }

    #[test]
    fn r7_3_code_is_highlighted_with_classes_and_mermaid_left_for_mermaid() {
        let h = to_html_body("```rust\nfn main() {}\n```\n\n```mermaid\ngraph TD\n  A-->B\n```\n");
        assert!(h.contains("<span class=\"source rust\">"), "{h}");
        assert!(h.contains("storage type function"), "{h}");
        assert!(
            h.contains("<pre class=\"mermaid\">graph TD\n  A--&gt;B\n</pre>"),
            "{h}"
        );
    }

    #[test]
    fn r7_3_links_and_images_keep_relative_paths_file_urls_rewritten() {
        let h = to_html_body(
            "[o](other.md) [s](#sec) ![i](img/a.png) [f](file:///etc/x.md) ![g](FILE:/w/a.png) [w](https://x.org)",
        );
        assert!(h.contains("href=\"other.md\""), "{h}");
        assert!(h.contains("href=\"#sec\""), "{h}");
        assert!(h.contains("src=\"img/a.png\""), "{h}");
        assert!(h.contains("href=\"rade-file:///etc/x.md\""), "{h}");
        assert!(h.contains("src=\"rade-file:///w/a.png\""), "{h}");
        assert!(h.contains("href=\"https://x.org\""), "{h}");
    }

    #[test]
    fn r7_3_page_blocks_scripts_and_remote_loads() {
        let p = to_html_page("<b>ok</b>", "a<b>.md", true);
        assert!(
            p.contains("default-src 'none'; img-src rade-file: data:"),
            "{p}"
        );
        assert!(!p.contains("script-src"), "no script source is allowed");
        assert!(p.contains("<b>ok</b>"));
        assert!(p.contains("<html class=\"dark\">"));
        assert!(p.contains("<title>a&lt;b&gt;.md</title>"));
        assert!(to_html_page("x", "t", false).contains("<html class=\"light\">"));
        assert_ne!(code_css(true), code_css(false));
    }

    #[test]
    fn r55_3_pango_markup() {
        let m = to_pango(
            "## Goal\nLog in *fast* with `otp` & <b>x</b>.\n\n## Done when\n- [ ] a\n- [x] b\n1. one\n\nSee https://e.com/a?b=1&c=2 and [doc](file:///etc/passwd).",
        );
        assert!(m.contains("<span size=\"medium\" weight=\"bold\">Goal</span>\nLog in"));
        assert!(m.contains("Log in <i>fast</i> with <tt>otp</tt> &amp; &lt;b&gt;x&lt;/b&gt;."));
        assert!(m.contains("☐ a\n☑ b"));
        assert!(m.contains("1. one"));
        assert!(m.contains("<a href=\"https://e.com/a?b=1&amp;c=2\">"));
        assert!(!m.contains("file:///"), "{m}");
        // Comment hints (`<!-- … -->`) don't show.
        let m = to_pango("## Verify\n<!-- how to check it -->\n\nRun it.\n");
        assert!(!m.contains("&lt;!--") && m.contains("Verify"), "{m}");
        assert_eq!(to_pango(""), "");
    }
}
