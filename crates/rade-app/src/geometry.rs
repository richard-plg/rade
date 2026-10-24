//! `debug.ui {"op": "geometry", "widget", "nth"?, "all"?}` (PRD R102.5,
//! test mode only, ADR-0021): what a widget looks like, for the spec
//! sheets' measurements (`docs/internals/design/spec/README.md` "Measurement method").
//!
//! GTK 4 exposes a widget's allocation, CSS margin / padding / border
//! widths, color, font and classes. Its computed background, border colors,
//! radius and box-shadows are read from what GTK draws for the widget's own
//! CSS box: the render nodes of `gtk_snapshot_render_background` /
//! `render_frame` with the widget's style (every selector, libadwaita's CSS
//! and the loaded tokens apply), children left out. A box that draws
//! nothing has no visible radius: `radius` then falls back to `style.css`
//! (`radius_source`). `drawn` samples the window as rendered (what a
//! screenshot shows, overlays included): the widget's background where no
//! child covers it, and each edge just inside and just outside its border
//! box. A Terminal's background is read from its pixels.

use adw::prelude::*;
use gtk::glib::translate::IntoGlib;
use gtk4 as gtk;
use libadwaita as adw;
use serde_json::{json, Value};

/// Widgets matching `selector` under `root` in document order: a CSS name,
/// class or widget name, or a descendant chain (`window-controls tb-button`).
pub fn find(root: &gtk::Widget, selector: &str) -> Vec<gtk::Widget> {
    let mut scope = vec![root.clone()];
    for part in selector.split_whitespace() {
        let mut next: Vec<gtk::Widget> = Vec::new();
        for s in &scope {
            for w in descendants(s, s != root) {
                if matches(&w, part) && !next.contains(&w) {
                    next.push(w);
                }
            }
        }
        scope = next;
    }
    scope
}

fn matches(w: &gtk::Widget, name: &str) -> bool {
    let name = name.trim_start_matches('.');
    w.css_name() == name || w.has_css_class(name) || w.widget_name() == name
}

/// `w` (unless `skip_self`) and every widget under it, in document order.
fn descendants(w: &gtk::Widget, skip_self: bool) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    let mut stack = vec![w.clone()];
    while let Some(x) = stack.pop() {
        let mut kids = Vec::new();
        let mut c = x.first_child();
        while let Some(k) = c {
            c = k.next_sibling();
            kids.push(k);
        }
        stack.extend(kids.into_iter().rev());
        out.push(x);
    }
    if skip_self {
        out.remove(0);
    }
    out
}

/// A length as JSON: an integer when it is one (`26`, not `26.0`).
fn num(v: f32) -> Value {
    if (v - v.round()).abs() < 0.01 {
        json!(v.round() as i64)
    } else {
        json!((v * 100.0).round() / 100.0)
    }
}

/// `#rrggbbaa` of a GDK color.
pub fn hex(c: &gtk::gdk::RGBA) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        b(c.red()),
        b(c.green()),
        b(c.blue()),
        b(c.alpha())
    )
}

/// `transparent` as `#rrggbbaa`.
fn transparent() -> String {
    hex(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0))
}

/// One rule of `style.css` with a single compound selector.
struct Rule {
    /// CSS name the selector starts with, if any (`window`).
    element: Option<String>,
    classes: Vec<String>,
    decls: Vec<(String, String)>,
}

/// The rules and the `:root` variables of a stylesheet.
type Sheet = (Vec<Rule>, Vec<(String, String)>);

/// The single-compound rules of the stylesheet (and `:root` variables):
/// only for the radius of a box that draws nothing.
fn rules() -> &'static Sheet {
    static RULES: std::sync::OnceLock<Sheet> = std::sync::OnceLock::new();
    RULES.get_or_init(|| parse(crate::CSS))
}

fn parse(css: &str) -> Sheet {
    // Comments out.
    let mut text = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        text.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
    }
    text.push_str(rest);
    let mut rules = Vec::new();
    let mut vars = Vec::new();
    for block in text.split('}') {
        let Some((sel, body)) = block.split_once('{') else {
            continue;
        };
        let decls: Vec<(String, String)> = body
            .split(';')
            .filter_map(|d| {
                let (k, v) = d.split_once(':')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        for s in sel.split(',').map(str::trim) {
            if s == ":root" {
                vars.extend(decls.iter().filter(|(k, _)| k.starts_with("--")).cloned());
                continue;
            }
            if s.is_empty() || s.contains([' ', '>', ':', '[', '+', '~']) {
                continue;
            }
            let mut parts = s.split('.');
            let head = parts.next().unwrap_or("");
            rules.push(Rule {
                element: (!head.is_empty()).then(|| head.to_string()),
                classes: parts.map(String::from).collect(),
                decls: decls.clone(),
            });
        }
    }
    (rules, vars)
}

/// The value of property `prop` for `w` from the matching rules (last wins).
fn css_value(w: &gtk::Widget, props: &[&str]) -> Option<String> {
    let (rules, _) = rules();
    let name = w.css_name();
    let mut out = None;
    for r in rules {
        if r.element.as_deref().is_some_and(|e| e != name.as_str()) {
            continue;
        }
        if r.element.is_none() && r.classes.is_empty() {
            continue;
        }
        if !r.classes.iter().all(|c| w.has_css_class(c)) {
            continue;
        }
        for (k, v) in &r.decls {
            if props.contains(&k.as_str()) {
                out = Some(v.clone());
            }
        }
    }
    out
}

/// What GTK draws for a widget's own CSS box (children left out).
#[derive(Default)]
struct Own {
    /// The background color, if one is drawn.
    background: Option<gtk::gdk::RGBA>,
    /// Border colors (top, right, bottom, left) of a drawn border.
    border: Option<[gtk::gdk::RGBA; 4]>,
    /// Top-left corner radius of the first rounded shape drawn.
    radius: Option<f32>,
    /// Box-shadows in CSS order: (inset, dx, dy, blur, spread, color).
    shadows: Vec<(bool, f32, f32, f32, f32, gtk::gdk::RGBA)>,
}

impl Own {
    fn draws(&self) -> bool {
        self.background.is_some() || self.border.is_some() || !self.shadows.is_empty()
    }

    /// The shadows as CSS text with `#rrggbbaa` colors, or None.
    fn shadow_css(&self) -> Option<String> {
        if self.shadows.is_empty() {
            return None;
        }
        let n = |v: f32| num(v).to_string();
        let len = |v: f32| {
            if v == 0.0 {
                "0".to_string()
            } else {
                format!("{}px", n(v))
            }
        };
        Some(
            self.shadows
                .iter()
                .map(|(inset, dx, dy, blur, spread, c)| {
                    let mut t = format!("{} {} {}", len(*dx), len(*dy), len(*blur));
                    if *spread != 0.0 {
                        t = format!("{t} {}", len(*spread));
                    }
                    format!("{}{t} {}", if *inset { "inset " } else { "" }, hex(c))
                })
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

/// [`Own`] of `w`: its computed style rendered alone, read from the nodes.
#[allow(deprecated)] // gtk_snapshot_render_* draw one widget's CSS box.
fn own(w: &gtk::Widget) -> Own {
    let (wd, ht) = (w.width() as f64, w.height() as f64);
    let mut o = Own::default();
    if wd <= 0.0 || ht <= 0.0 {
        return o;
    }
    let sc = w.style_context();
    let snap = gtk::Snapshot::new();
    snap.render_background(&sc, 0.0, 0.0, wd, ht);
    snap.render_frame(&sc, 0.0, 0.0, wd, ht);
    if let Some(n) = snap.to_node() {
        walk(&n, &mut o);
    }
    o
}

fn walk(n: &gtk::gsk::RenderNode, o: &mut Own) {
    use gtk::gsk;
    let corner = |r: &gsk::RoundedRect| r.corner()[0].width();
    if let Some(c) = n.downcast_ref::<gsk::ContainerNode>() {
        for i in 0..c.n_children() {
            walk(&c.child(i), o);
        }
    } else if let Some(c) = n.downcast_ref::<gsk::RoundedClipNode>() {
        o.radius.get_or_insert(corner(&c.clip()));
        walk(&c.child(), o);
    } else if let Some(c) = n.downcast_ref::<gsk::ColorNode>() {
        o.background.get_or_insert(c.color());
    } else if let Some(c) = n.downcast_ref::<gsk::BorderNode>() {
        o.radius.get_or_insert(corner(&c.outline()));
        o.border = Some(*c.colors());
    } else if let Some(c) = n.downcast_ref::<gsk::OutsetShadowNode>() {
        o.radius.get_or_insert(corner(&c.outline()));
        o.shadows.push((
            false,
            c.dx(),
            c.dy(),
            c.blur_radius(),
            c.spread(),
            c.color(),
        ));
    } else if let Some(c) = n.downcast_ref::<gsk::InsetShadowNode>() {
        o.shadows
            .push((true, c.dx(), c.dy(), c.blur_radius(), c.spread(), c.color()));
    } else if let Some(c) = n.downcast_ref::<gsk::DebugNode>() {
        walk(&c.child(), o);
    } else if let Some(c) = n.downcast_ref::<gsk::ClipNode>() {
        walk(&c.child(), o);
    } else if let Some(c) = n.downcast_ref::<gsk::TransformNode>() {
        walk(&c.child(), o);
    } else if let Some(c) = n.downcast_ref::<gsk::OpacityNode>() {
        walk(&c.child(), o);
    }
}

/// The window as rendered (the screenshot's pixels), for `drawn`.
pub struct Frame {
    bytes: gtk::glib::Bytes,
    stride: usize,
    scale: f64,
    width: f64,
    height: f64,
}

impl Frame {
    /// Render every child of the window (its content and overlays).
    pub fn of(window: &gtk::Widget) -> Option<Frame> {
        let (wd, ht) = (window.width(), window.height());
        if wd <= 0 || ht <= 0 {
            return None;
        }
        let snapshot = gtk::Snapshot::new();
        let mut c = window.first_child();
        while let Some(k) = c {
            c = k.next_sibling();
            window.snapshot_child(&k, &snapshot);
        }
        let node = snapshot.to_node()?;
        let renderer = window.native()?.renderer()?;
        let bounds = gtk::graphene::Rect::new(0.0, 0.0, wd as f32, ht as f32);
        let texture = renderer.render_texture(&node, Some(&bounds));
        let mut d = gtk::gdk::TextureDownloader::new(&texture);
        d.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = d.download_bytes();
        Some(Frame {
            bytes,
            stride,
            scale: texture.width() as f64 / wd as f64,
            width: wd as f64,
            height: ht as f64,
        })
    }

    /// The pixel at window point (x, y), or None outside the window.
    fn at(&self, x: f64, y: f64) -> Option<String> {
        if x < 0.0 || y < 0.0 || x >= self.width || y >= self.height {
            return None;
        }
        let i = (y * self.scale) as usize * self.stride + (x * self.scale) as usize * 4;
        let p = self.bytes.get(i..i + 4)?;
        Some(format!("#{:02x}{:02x}{:02x}{:02x}", p[0], p[1], p[2], p[3]))
    }
}

/// Where a widget's own background shows in the window: the points that
/// no drawing descendant covers (one with a box of its own, or a leaf such
/// as a label or an image) among the middle of each padding band and a
/// 12 × 12 grid over the padding box `inner` (`pad` = t, r, b, l), inside
/// the rounded corners (`radius`).
fn free_points(
    window: &gtk::Widget,
    w: &gtk::Widget,
    inner: &gtk::graphene::Rect,
    pad: [f32; 4],
    radius: f32,
) -> Vec<(f64, f64)> {
    let covers: Vec<gtk::graphene::Rect> = descendants(w, true)
        .into_iter()
        .filter(|d| d.is_mapped() && d.opacity() > 0.0)
        .filter(|d| d.first_child().is_none() || own(d).draws())
        .filter_map(|d| d.compute_bounds(window))
        .collect();
    let shape = gtk::gsk::RoundedRect::from_rect(*inner, radius);
    let (x, y, wd, ht) = (inner.x(), inner.y(), inner.width(), inner.height());
    let mut points = Vec::new();
    for (i, p) in pad.iter().enumerate().filter(|(_, p)| **p > 0.0) {
        points.push(match i {
            0 => (x + wd / 2.0, y + p / 2.0),
            1 => (x + wd - p / 2.0, y + ht / 2.0),
            2 => (x + wd / 2.0, y + ht - p / 2.0),
            _ => (x + p / 2.0, y + ht / 2.0),
        });
    }
    const N: usize = 12;
    for j in 0..N {
        for i in 0..N {
            points.push((
                x + wd * (i as f32 + 0.5) / N as f32,
                y + ht * (j as f32 + 0.5) / N as f32,
            ));
        }
    }
    points
        .into_iter()
        .filter(|(px, py)| {
            let pt = gtk::graphene::Point::new(*px, *py);
            shape.contains_point(pt) && !covers.iter().any(|r| r.contains_point(&pt))
        })
        .map(|(px, py)| (px as f64, py as f64))
        .collect()
}

/// The most frequent color (the first one on a tie), or None.
fn mode(colors: Vec<String>) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for c in colors {
        match counts.iter_mut().find(|(k, _)| *k == c) {
            Some((_, n)) => *n += 1,
            None => counts.push((c, 1)),
        }
    }
    let best = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == best).map(|(c, _)| c)
}

fn px(v: &str) -> Option<f64> {
    v.split_whitespace()
        .next()?
        .trim_end_matches("px")
        .parse()
        .ok()
}

fn sides(top: i16, right: i16, bottom: i16, left: i16) -> Value {
    json!({"top": top, "right": right, "bottom": bottom, "left": left})
}

/// The text a widget shows: its own label, a button's or an entry's
/// placeholder, else the first label under it.
fn label_of(w: &gtk::Widget) -> Option<gtk::Label> {
    descendants(w, false)
        .into_iter()
        .find_map(|x| x.downcast::<gtk::Label>().ok())
}

/// Font facts of a label: the face Pango really loads (bundled or not),
/// its size in CSS px, weight, letter spacing (px) and OpenType features.
fn font(l: &gtk::Label) -> Value {
    let ctx = l.pango_context();
    let Some(desc) = ctx.font_description() else {
        return Value::Null;
    };
    let family = ctx
        .load_font(&desc)
        .and_then(|f| f.describe().family().map(|s| s.to_string()))
        .or_else(|| desc.family().map(|s| s.to_string()));
    let pt = desc.size() as f64 / pango::SCALE as f64;
    let size = if desc.is_size_absolute() {
        pt
    } else {
        pt * 96.0 / 72.0
    };
    let mut letter_spacing = 0.0;
    let mut features = Vec::<String>::new();
    if let Some(attrs) = l.layout().attributes() {
        for a in attrs.attributes() {
            match a.type_() {
                pango::AttrType::LetterSpacing => {
                    if let Some(i) = a.downcast_ref::<pango::AttrInt>() {
                        letter_spacing = i.value() as f64 / pango::SCALE as f64;
                    }
                }
                pango::AttrType::FontFeatures => {
                    if let Some(f) = a.downcast_ref::<pango::AttrFontFeatures>() {
                        features.extend(
                            f.features()
                                .split(',')
                                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                                .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                                .filter(|s| !s.is_empty()),
                        );
                    }
                }
                _ => {}
            }
        }
    }
    json!({
        "family": family,
        "size": (size * 100.0).round() / 100.0,
        "weight": desc.weight().into_glib(),
        "letter_spacing": (letter_spacing * 100.0).round() / 100.0,
        "features": features,
    })
}

/// The color of the pixel at (x, y) of `w` as drawn (a Terminal's GL area).
fn pixel(w: &gtk::Widget, x: f64, y: f64) -> Option<String> {
    let (wd, ht) = (w.width(), w.height());
    if wd <= 0 || ht <= 0 {
        return None;
    }
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(w)).snapshot(&snapshot, wd as f64, ht as f64);
    let node = snapshot.to_node()?;
    let renderer = w.native()?.renderer()?;
    let bounds = gtk::graphene::Rect::new(0.0, 0.0, wd as f32, ht as f32);
    let texture = renderer.render_texture(&node, Some(&bounds));
    let mut d = gtk::gdk::TextureDownloader::new(&texture);
    d.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = d.download_bytes();
    let (px, py) = (x.clamp(0.0, wd as f64 - 1.0), y.clamp(0.0, ht as f64 - 1.0));
    let scale = texture.width() as f64 / wd as f64;
    let i = (py * scale) as usize * stride + (px * scale) as usize * 4;
    let p = bytes.get(i..i + 4)?;
    Some(format!("#{:02x}{:02x}{:02x}{:02x}", p[0], p[1], p[2], p[3]))
}

/// The geometry of one widget (see the module doc); `frame` = the window
/// as rendered, for `drawn`.
#[allow(deprecated)] // StyleContext is the only reader of CSS box widths.
pub fn describe(window: &gtk::Widget, w: &gtk::Widget, frame: Option<&Frame>) -> Value {
    let b = w.compute_bounds(window);
    let sc = w.style_context();
    let (m, p, bd) = (sc.margin(), sc.padding(), sc.border());
    let label = label_of(w);
    let text = if let Some(l) = w.downcast_ref::<gtk::Label>() {
        Some(l.text().to_string())
    } else if let Some(bt) = w.downcast_ref::<gtk::Button>() {
        bt.label()
            .map(|s| s.to_string())
            .or_else(|| label.as_ref().map(|l| l.text().to_string()))
    } else {
        label.as_ref().map(|l| l.text().to_string())
    };
    let o = own(w);
    let background = if w.has_css_class("terminal") {
        pixel(w, 2.0, 2.0)
    } else {
        Some(o.background.as_ref().map_or_else(transparent, hex))
    };
    let border_color = |i: usize| o.border.as_ref().map_or_else(transparent, |c| hex(&c[i]));
    let (radius, radius_source) = match o.radius {
        Some(r) => (r as f64, "drawn"),
        None if o.draws() => (0.0, "drawn"),
        None => (
            css_value(w, &["border-radius"])
                .and_then(|v| px(&v))
                .unwrap_or(0.0),
            "style.css",
        ),
    };
    let drawn = match (frame, b.as_ref()) {
        (Some(f), Some(r)) if w.is_mapped() => {
            let (x, y, wd, ht) = (
                r.x() as f64,
                r.y() as f64,
                r.width() as f64,
                r.height() as f64,
            );
            let (cx, cy) = (x + wd / 2.0, y + ht / 2.0);
            let inner = gtk::graphene::Rect::new(
                r.x() + bd.left() as f32,
                r.y() + bd.top() as f32,
                (r.width() - (bd.left() + bd.right()) as f32).max(1.0),
                (r.height() - (bd.top() + bd.bottom()) as f32).max(1.0),
            );
            let pad = [p.top(), p.right(), p.bottom(), p.left()].map(f32::from);
            // A neighbour's glow may tint a few points: the most frequent wins.
            let bg = mode(
                free_points(window, w, &inner, pad, radius as f32)
                    .into_iter()
                    .filter_map(|(px, py)| f.at(px, py))
                    .collect(),
            );
            json!({
                "background": bg,
                "edge": {"top": f.at(cx, y + 0.5), "right": f.at(x + wd - 0.5, cy),
                    "bottom": f.at(cx, y + ht - 0.5), "left": f.at(x + 0.5, cy)},
                "outside": {"top": f.at(cx, y - 0.5), "right": f.at(x + wd + 0.5, cy),
                    "bottom": f.at(cx, y + ht + 0.5), "left": f.at(x - 0.5, cy)},
            })
        }
        _ => Value::Null,
    };
    let font_label = label.or_else(|| w.clone().downcast::<gtk::Label>().ok());
    json!({
        "x": b.map(|b| num(b.x())), "y": b.map(|b| num(b.y())),
        "width": b.map(|b| num(b.width())), "height": b.map(|b| num(b.height())),
        "margin": sides(
            m.top() + w.margin_top() as i16, m.right() + w.margin_end() as i16,
            m.bottom() + w.margin_bottom() as i16, m.left() + w.margin_start() as i16),
        "padding": sides(p.top(), p.right(), p.bottom(), p.left()),
        "border": sides(bd.top(), bd.right(), bd.bottom(), bd.left()),
        "border_color": {"top": border_color(0), "right": border_color(1),
            "bottom": border_color(2), "left": border_color(3)},
        "radius": radius,
        "radius_source": radius_source,
        "shadow": o.shadow_css(),
        "font": font_label.as_ref().map(font).unwrap_or(Value::Null),
        "color": hex(&w.color()),
        "background": background,
        "drawn": drawn,
        "opacity": w.opacity(),
        "classes": w.css_classes().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "css_name": w.css_name().as_str(),
        "visible": w.is_mapped(),
        "label": text,
        "tooltip": w.tooltip_text().map(|s| s.to_string()),
        "icon": w.downcast_ref::<gtk::Image>().and_then(|i| i.icon_name()).map(|s| s.to_string()),
        // Natural size: an icon's rendered size inside a larger allocation.
        "natural": {"width": w.measure(gtk::Orientation::Horizontal, -1).1,
            "height": w.measure(gtk::Orientation::Vertical, -1).1},
        "text_extent": text_extent(w),
    })
}

/// The height the text under `w` takes: from the top of the highest shown
/// label's text to the bottom of the lowest (each label's natural height
/// for its width, placed by its `yalign`); null without a shown label. A
/// row's height less this is its padding around the text (R111.2).
fn text_extent(w: &gtk::Widget) -> Value {
    let mut span: Option<(f64, f64)> = None;
    for k in descendants(w, true) {
        let Some(l) = k.downcast_ref::<gtk::Label>() else {
            continue;
        };
        if !k.is_mapped() || l.text().is_empty() {
            continue;
        }
        let Some(b) = k.compute_bounds(w) else {
            continue;
        };
        // The content box only: a label's own padding is not text.
        #[allow(deprecated)]
        let (m, p, bd) = {
            let sc = k.style_context();
            (sc.margin(), sc.padding(), sc.border())
        };
        let ex_w = i32::from(m.left() + m.right() + p.left() + p.right() + bd.left() + bd.right())
            + k.margin_start()
            + k.margin_end();
        let ex_h = i32::from(m.top() + m.bottom() + p.top() + p.bottom() + bd.top() + bd.bottom())
            + k.margin_top()
            + k.margin_bottom();
        let inner = f64::from(p.top() + bd.top());
        let nat = f64::from(k.measure(gtk::Orientation::Vertical, k.width() + ex_w).1 - ex_h);
        let h = f64::from(k.height());
        let top = f64::from(b.y()) + inner + (h - nat).max(0.0) * f64::from(l.yalign());
        let bottom = top + nat.min(h);
        span = Some(span.map_or((top, bottom), |(t, b)| (t.min(top), b.max(bottom))));
    }
    span.map_or(Value::Null, |(t, b)| json!(num((b - t) as f32)))
}

/// `debug.ui {"op": "clipped"}` (R111.2, test mode): every shown label
/// whose text does not fit its allocation and that does not ellipsize — too
/// wide (a wrapping label: its narrowest line), too tall for its width, or
/// laid out past the window's left / right edge.
pub fn clipped(window: &gtk::Widget) -> Value {
    let (ww, native) = (window.width(), window.native());
    let mut out = Vec::new();
    for w in descendants(window, true) {
        let Some(l) = w.downcast_ref::<gtk::Label>() else {
            continue;
        };
        if !w.is_mapped() || l.text().is_empty() || w.opacity() == 0.0 {
            continue;
        }
        // `measure` counts margin, border and padding; `width()` is the
        // content box: compare like with like.
        #[allow(deprecated)]
        let (m, p, bd) = {
            let sc = w.style_context();
            (sc.margin(), sc.padding(), sc.border())
        };
        let ex_w = i32::from(m.left() + m.right() + p.left() + p.right() + bd.left() + bd.right())
            + w.margin_start()
            + w.margin_end();
        let ex_h = i32::from(m.top() + m.bottom() + p.top() + p.bottom() + bd.top() + bd.bottom())
            + w.margin_top()
            + w.margin_bottom();
        let (aw, ah) = (w.width(), w.height());
        let (min_w, nat_w, _, _) = w.measure(gtk::Orientation::Horizontal, -1);
        let (min_h, _, _, _) = w.measure(gtk::Orientation::Vertical, aw + ex_w);
        let (min_w, nat_w, min_h) = (min_w - ex_w, nat_w - ex_w, min_h - ex_h);
        let ellipsizes = l.ellipsize() != gtk::pango::EllipsizeMode::None;
        let need_w = if l.wraps() { min_w } else { nat_w };
        let mut why = Vec::new();
        if !ellipsizes && need_w > aw + 1 {
            why.push("width");
        }
        if min_h > ah + 1 {
            why.push("height");
        }
        if w.native() == native {
            if let Some(b) = w.compute_bounds(window) {
                if b.x() < -1.0 || f64::from(b.x() + b.width()) > f64::from(ww) + 1.0 {
                    why.push("outside");
                }
            }
        }
        if !why.is_empty() {
            out.push(json!({
                "text": l.text().chars().take(60).collect::<String>(),
                "classes": w.css_classes().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                "parent": w.parent().map(|p| p.css_classes().iter().map(|c| c.to_string()).collect::<Vec<_>>()),
                "allocation": [aw, ah],
                "natural": [nat_w, min_h],
                "why": why,
            }));
        }
    }
    json!({"count": out.len(), "labels": out.into_iter().take(60).collect::<Vec<_>>()})
}

/// `debug.ui {"op": "hscroll"}` (R111, test mode): every shown scrolled
/// window that can scroll sideways (horizontal upper > page size), and the
/// window's own width (a minimum wider than asked shows as a wider window).
pub fn hscroll(window: &gtk::Widget) -> Value {
    let mut out = Vec::new();
    for w in descendants(window, true) {
        let Some(sw) = w.downcast_ref::<gtk::ScrolledWindow>() else {
            continue;
        };
        if !w.is_mapped() {
            continue;
        }
        let a = sw.hadjustment();
        if a.upper() > a.page_size() + 1.0 {
            out.push(json!({
                "classes": w.css_classes().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                "upper": a.upper(),
                "page_size": a.page_size(),
            }));
        }
    }
    json!({
        "count": out.len(),
        "scrolled": out,
        "window": {"width": window.width(), "height": window.height(),
            "min_width": window.measure(gtk::Orientation::Horizontal, -1).0},
    })
}

/// The op: the `nth` (default 0) match, or with `all` every shown match.
/// Matches that are shown come first; hidden ones only when none is.
pub fn op(window: &gtk::Widget, p: &Value) -> rade_core::rpc::Result<Value> {
    let sel = p
        .get("widget")
        .and_then(Value::as_str)
        .ok_or_else(|| rade_core::rpc::Error::usage("geometry needs \"widget\""))?;
    let found = find(window, sel);
    let shown: Vec<gtk::Widget> = found.iter().filter(|w| w.is_mapped()).cloned().collect();
    let frame = if shown.is_empty() {
        None
    } else {
        Frame::of(window)
    };
    if p.get("all").and_then(Value::as_bool) == Some(true) {
        return Ok(Value::Array(
            shown
                .iter()
                .map(|w| describe(window, w, frame.as_ref()))
                .collect(),
        ));
    }
    let list = if shown.is_empty() { found } else { shown };
    let n = p.get("nth").and_then(Value::as_u64).unwrap_or(0) as usize;
    let Some(w) = list.get(n) else {
        return Ok(json!({"widget": sel, "found": false}));
    };
    let mut d = describe(window, w, frame.as_ref());
    d["widget"] = json!(sel);
    d["found"] = json!(true);
    Ok(d)
}

#[cfg(test)]
mod tests {
    #[test]
    fn geometry_drawn_background_is_the_most_frequent_color() {
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(super::mode(c(&["#a", "#b", "#b"])).as_deref(), Some("#b"));
        assert_eq!(super::mode(c(&["#a", "#b"])).as_deref(), Some("#a"));
        assert_eq!(super::mode(c(&[])), None);
    }

    #[test]
    fn geometry_parses_single_compound_rules_and_root_vars() {
        let (rules, vars) = super::parse(
            ":root { --x: @bg1; }\n/* c { } */\n.a.b { background: @bg0; border-radius: 6px; }\n\
             .a .c, .d:hover { color: red; }\nwindow { background-color: var(--x); }",
        );
        assert_eq!(vars, [("--x".to_string(), "@bg1".to_string())]);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].classes, ["a", "b"]);
        assert_eq!(rules[1].element.as_deref(), Some("window"));
        assert_eq!(super::px("6px"), Some(6.0));
    }
}
