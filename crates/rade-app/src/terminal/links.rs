//! RADE (R7.1): Ctrl+click on links. Ghostty matches URLs and paths with a
//! slash itself and emits `OPEN_URL` (callbacks.rs); names without one
//! (`main.rs:10`, `README.md`) fall back to the word under the pointer,
//! read from the Terminal's screen here.

use super::*;

thread_local! {
    /// Set when Ghostty emitted `OPEN_URL` during the current release.
    static OPENED: Cell<bool> = const { Cell::new(false) };
    /// Shift was held on the current Ctrl+click: Open to the Side (R113.2).
    static SIDE: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn set_side(side: bool) {
    SIDE.with(|s| s.set(side));
}

/// Whether the link being opened came from a Ctrl+Shift+click (read once).
pub(super) fn take_side() -> bool {
    SIDE.with(|s| s.replace(false))
}

pub(super) fn mark_opened() {
    OPENED.with(|o| o.set(true));
}

/// Padding and cell size in device pixels. The padding comes from the IME
/// point, which Ghostty computes with the real padding.
fn geometry(surface: ghostty_surface_t, scale: f64) -> Option<(f64, f64, f64, f64)> {
    let size = unsafe { ghostty_surface_size(surface) };
    let (cw, ch) = (size.cell_width_px as f64, size.cell_height_px as f64);
    if cw == 0.0 || ch == 0.0 {
        return None;
    }
    let (mut x, mut y, mut w, mut h) = (0.0, 0.0, 0.0, 0.0);
    unsafe { ghostty_surface_ime_point(surface, &mut x, &mut y, &mut w, &mut h) };
    Some((
        (x * scale - cw / 2.0).rem_euclid(cw),
        (y * scale).rem_euclid(ch),
        cw,
        ch,
    ))
}

fn row_text(surface: ghostty_surface_t, row: u32, cols: u16) -> String {
    let point = |x: u32| ghostty_point_s {
        tag: GHOSTTY_POINT_VIEWPORT,
        coord: GHOSTTY_POINT_COORD_EXACT,
        x,
        y: row,
    };
    let selection = ghostty_selection_s {
        top_left: point(0),
        bottom_right: point(cols.saturating_sub(1) as u32),
        rectangle: false,
    };
    let mut text = empty_ghostty_text();
    if !unsafe { ghostty_surface_read_text(surface, selection, &mut text) } || text.text.is_null() {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(text.text.cast::<u8>(), text.text_len) };
    let out = String::from_utf8_lossy(bytes).into_owned();
    unsafe { ghostty_surface_free_text(surface, &mut text) };
    out
}

/// Call around a left-button release: `before` clears the flag, `after`
/// (with Ctrl held) opens the word under (x, y) if Ghostty opened nothing.
pub(super) fn before_release() {
    OPENED.with(|o| o.set(false));
}

pub(super) fn after_release(surface: ghostty_surface_t, gl_area: &gtk::GLArea, x: f64, y: f64) {
    if OPENED.with(|o| o.get()) || unsafe { ghostty_surface_has_selection(surface) } {
        return;
    }
    let Some(word) = word_at_point(surface, gl_area, x, y) else {
        return;
    };
    let key = surface as usize;
    glib::idle_add_local_once(move || {
        SURFACE_MAP.with(|map| {
            if let Some(cb) = map.borrow().get(&key).and_then(|e| e.on_open_url.as_ref()) {
                cb(&word);
            }
        });
    });
}

/// The whitespace-delimited word at widget point (x, y), if any.
pub(super) fn word_at_point(
    surface: ghostty_surface_t,
    gl_area: &gtk::GLArea,
    x: f64,
    y: f64,
) -> Option<String> {
    let scale = gl_area.scale_factor().max(1) as f64;
    let (px, py, cw, ch) = geometry(surface, scale)?;
    let (col, row) = (
        ((x * scale - px) / cw).floor(),
        ((y * scale - py) / ch).floor(),
    );
    if col < 0.0 || row < 0.0 {
        return None;
    }
    let cols = unsafe { ghostty_surface_size(surface) }.columns;
    let line = row_text(surface, row as u32, cols);
    rade_core::link::word_at(&line, col as usize)
}

/// Centre of the viewport cell (row, col) in the GL area's coordinates.
pub(super) fn cell_point(
    surface: ghostty_surface_t,
    gl_area: &gtk::GLArea,
    row: u32,
    col: u32,
) -> Option<(f64, f64)> {
    let scale = gl_area.scale_factor().max(1) as f64;
    let (px, py, cw, ch) = geometry(surface, scale)?;
    Some((
        (px + (col as f64 + 0.5) * cw) / scale,
        (py + (row as f64 + 0.5) * ch) / scale,
    ))
}

/// Ctrl+click the viewport cell (row, col), as a user would (test driver).
pub(super) fn ctrl_click_cell(
    surface: ghostty_surface_t,
    gl_area: &gtk::GLArea,
    row: u32,
    col: u32,
) -> bool {
    let Some((x, y)) = cell_point(surface, gl_area, row, col) else {
        return false;
    };
    let mods = GHOSTTY_MODS_CTRL;
    unsafe {
        ghostty_surface_mouse_pos(surface, x, y, mods);
        ghostty_surface_mouse_button(surface, GHOSTTY_MOUSE_PRESS, GHOSTTY_MOUSE_LEFT, mods);
    }
    before_release();
    unsafe {
        ghostty_surface_mouse_button(surface, GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_LEFT, mods)
    };
    after_release(surface, gl_area, x, y);
    true
}
