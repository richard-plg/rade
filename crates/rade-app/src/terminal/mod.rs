// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use rade_core::config::TerminalTheme;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

use rade_ghostty_sys::*;

mod callbacks;
mod clipboard;
mod create;
mod ime;
mod input;
mod links;
mod runtime_env;
mod search;
use callbacks::*;
use clipboard::*;
use create::*;
pub use create::{create_terminal, TerminalCallbacks, TerminalOptions};
use input::*;
pub use runtime_env::prepare_ghostty_runtime;
pub use search::SearchBar;

// ---------------------------------------------------------------------------
// Global Ghostty app singleton
// ---------------------------------------------------------------------------

struct GhosttyState {
    app: ghostty_app_t,
}

// Safety: ghostty_app_t is thread-safe for the operations we perform
unsafe impl Send for GhosttyState {}
unsafe impl Sync for GhosttyState {}

static GHOSTTY: OnceLock<GhosttyState> = OnceLock::new();
static CURRENT_COLOR_SCHEME: AtomicI32 = AtomicI32::new(GHOSTTY_COLOR_SCHEME_LIGHT);
static CURRENT_SCROLLBAR_ENABLED: AtomicBool = AtomicBool::new(true);
static WAKEUP_IDLE_QUEUED: AtomicBool = AtomicBool::new(false);
static EMPTY_CLIPBOARD_TEXT: [u8; 1] = [0];

type TitleChangedCallback = dyn Fn(&str);
type PwdChangedCallback = dyn Fn(&str);
type DesktopNotificationCallback = dyn Fn(&str, &str, bool);
type BellCallback = dyn Fn(bool);
type VoidCallback = dyn Fn();
type ChildExitedCallback = dyn Fn(u32);

/// Agent Panes keep at least this much scrollback (PRD R2.7): Ghostty's
/// `scrollback-limit` counts page-memory bytes, ~8–10 bytes per cell, so
/// 10,000 lines of a 500-column Terminal fit (docs/internals/checks/M0.md row 6).
/// ponytail: fixed budget; wider Terminals keep fewer lines — size it from
/// the column count if that ever matters.
pub const AGENT_SCROLLBACK_BYTES: usize = 64 << 20;
/// The user's own `scrollback-limit`, kept for plain shells.
static USER_SCROLLBACK_BYTES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Per-surface state, stored in a global registry keyed by surface pointer.
struct SurfaceEntry {
    // Embedded Ghostty borrows config.command; keep its allocation alive until
    // after ghostty_surface_free, just like the callback userdata below.
    _startup_command: Option<CString>,
    identity: SurfaceIdentity,
    gl_area: gtk::GLArea,
    toast_overlay: gtk::Overlay,
    scrollbar: gtk::Scrollbar,
    scrollbar_adjustment: gtk::Adjustment,
    scrollbar_syncing: Rc<Cell<bool>>,
    on_title_changed: Option<Box<TitleChangedCallback>>,
    on_pwd_changed: Option<Box<PwdChangedCallback>>,
    on_desktop_notification: Option<Box<DesktopNotificationCallback>>,
    on_bell: Option<Box<BellCallback>>,
    on_close: Option<Box<VoidCallback>>,
    on_child_exited: Option<Box<ChildExitedCallback>>,
    on_open_url: Option<Box<PwdChangedCallback>>,
    on_command_finished: Option<Box<VoidCallback>>,
    /// RADE (R19.5): the search bar Ghostty's search actions drive.
    search: SearchBar,
    clipboard_context: *mut ClipboardContext,
    mouse_cursor: Cell<MouseCursorState>,
}

struct ClipboardContext {
    surface: Cell<ghostty_surface_t>,
    copy_selection_to_clipboard: Rc<dyn Fn() -> bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClipboardWritePolicy {
    write_clipboard: bool,
    write_primary: bool,
    show_toast: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MouseCursorState {
    shape: ghostty_action_mouse_shape_e,
    visible: bool,
}

impl Default for MouseCursorState {
    fn default() -> Self {
        Self {
            shape: GHOSTTY_MOUSE_SHAPE_TEXT,
            visible: true,
        }
    }
}

impl MouseCursorState {
    fn update_shape(self, shape: ghostty_action_mouse_shape_e) -> Self {
        Self { shape, ..self }
    }

    fn update_visibility(self, visibility: ghostty_action_mouse_visibility_e) -> Self {
        Self {
            visible: visibility != GHOSTTY_MOUSE_HIDDEN,
            ..self
        }
    }

    fn gtk_cursor_name(self) -> &'static str {
        if self.visible {
            gtk_cursor_name_for_ghostty_shape(self.shape)
        } else {
            "none"
        }
    }
}

#[derive(Clone, Copy)]
enum MouseCursorUpdate {
    Shape(ghostty_action_mouse_shape_e),
    Visibility(ghostty_action_mouse_visibility_e),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SurfaceIdentity {
    surface_key: usize,
    generation: u64,
}

#[derive(Default)]
struct SurfaceIdentityRegistry {
    next_generation: u64,
    active_generations: HashMap<usize, u64>,
}

impl SurfaceIdentityRegistry {
    fn register(&mut self, surface_key: usize) -> SurfaceIdentity {
        self.next_generation = self.next_generation.wrapping_add(1);
        let identity = SurfaceIdentity {
            surface_key,
            generation: self.next_generation,
        };
        self.active_generations
            .insert(surface_key, identity.generation);
        identity
    }

    fn current(&self, surface_key: usize) -> Option<SurfaceIdentity> {
        self.active_generations
            .get(&surface_key)
            .copied()
            .map(|generation| SurfaceIdentity {
                surface_key,
                generation,
            })
    }

    fn is_current(&self, identity: SurfaceIdentity) -> bool {
        self.current(identity.surface_key) == Some(identity)
    }

    fn unregister(&mut self, identity: SurfaceIdentity) {
        if self.is_current(identity) {
            self.active_generations.remove(&identity.surface_key);
        }
    }
}

thread_local! {
    static SURFACE_MAP: RefCell<HashMap<usize, SurfaceEntry>> = RefCell::new(HashMap::new());
}

static SURFACE_IDENTITIES: OnceLock<Mutex<SurfaceIdentityRegistry>> = OnceLock::new();

fn surface_identity_registry() -> &'static Mutex<SurfaceIdentityRegistry> {
    SURFACE_IDENTITIES.get_or_init(|| Mutex::new(SurfaceIdentityRegistry::default()))
}

fn register_surface_identity(surface_key: usize) -> SurfaceIdentity {
    surface_identity_registry()
        .lock()
        .expect("surface identity registry poisoned")
        .register(surface_key)
}

fn current_surface_identity(surface_key: usize) -> Option<SurfaceIdentity> {
    surface_identity_registry()
        .lock()
        .expect("surface identity registry poisoned")
        .current(surface_key)
}

fn unregister_surface_identity(identity: SurfaceIdentity) {
    surface_identity_registry()
        .lock()
        .expect("surface identity registry poisoned")
        .unregister(identity);
}

#[derive(Clone)]
pub struct TerminalHandle {
    surface_cell: Rc<RefCell<Option<ghostty_surface_t>>>,
    clipboard_context_cell: Rc<Cell<*mut ClipboardContext>>,
    shutting_down: Rc<Cell<bool>>,
    gl_area: gtk::GLArea,
    callbacks: Rc<RefCell<TerminalCallbacks>>,
    im_context: gtk::IMMulticontext,
    im_fallback: gtk::IMContextSimple,
    signal_handlers: Rc<RefCell<Vec<(glib::Object, glib::SignalHandlerId)>>>,
    controllers: Rc<RefCell<Vec<gtk::EventController>>>,
    /// RADE spike: number of `render` signals that drew this surface.
    draw_count: Rc<Cell<u64>>,
    /// RADE (R19.5): the search bar over this Terminal.
    search: SearchBar,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalHealth {
    pub realized: bool,
    pub process_exited: bool,
    pub columns: u16,
    pub rows: u16,
    pub width_px: u32,
    pub height_px: u32,
}

// Spike uses a subset; the rest is the API M1/M2 build on.
#[allow(dead_code)]
impl TerminalHandle {
    pub fn replace_callbacks(&self, callbacks: TerminalCallbacks) {
        if !self.shutting_down.get() {
            *self.callbacks.borrow_mut() = callbacks;
        }
    }

    pub fn shutdown(&self) {
        if self.shutting_down.replace(true) {
            return;
        }

        for (object, handler) in self.signal_handlers.borrow_mut().drain(..) {
            object.disconnect(handler);
        }
        for controller in self.controllers.borrow_mut().drain(..) {
            self.gl_area.remove_controller(&controller);
        }
        self.im_context.set_client_widget(gtk::Widget::NONE);
        self.im_fallback.set_client_widget(gtk::Widget::NONE);

        if let Some(surface) = self.surface_cell.borrow_mut().take() {
            free_terminal_surface(surface, &self.clipboard_context_cell, &self.gl_area);
        } else {
            let clipboard_context = self.clipboard_context_cell.replace(ptr::null_mut());
            if !clipboard_context.is_null() {
                unsafe {
                    drop(Box::from_raw(clipboard_context));
                }
            }
        }

        *self.callbacks.borrow_mut() = TerminalCallbacks::disconnected();
    }

    pub fn focus_surface(&self) -> bool {
        self.refresh_display();
        self.gl_area.grab_focus();
        true
    }

    pub fn refresh_display(&self) {
        let Some(surface) = *self.surface_cell.borrow() else {
            self.gl_area.queue_render();
            return;
        };

        refresh_surface_display(surface, &self.gl_area);
    }

    pub fn perform_binding_action(&self, action: &str) -> bool {
        let surface = *self.surface_cell.borrow();
        surface_action(surface, action);
        surface.is_some()
    }

    /// Paste what dropping `value` (a `FileList` or `Texture`) pastes (R5.2).
    pub fn drop_value(&self, value: &glib::Value) -> bool {
        dropped_text(value).is_some_and(|text| self.send_text(&text))
    }

    /// RADE (R19.5): the search bar over this Terminal.
    pub fn search(&self) -> &SearchBar {
        &self.search
    }

    /// RADE (R19.6) test driver: a right-click at widget point (x, y)
    /// through Ghostty's mouse path, as the gesture sends it; whether the
    /// program took it (mouse reporting), i.e. no menu. `None` = not started.
    pub fn right_click(&self, x: f64, y: f64, shift: bool) -> Option<bool> {
        let surface = (*self.surface_cell.borrow())?;
        let mods = if shift {
            GHOSTTY_MODS_SHIFT
        } else {
            GHOSTTY_MODS_NONE
        };
        unsafe {
            ghostty_surface_mouse_pos(surface, x, y, mods);
            let consumed = ghostty_surface_mouse_button(
                surface,
                GHOSTTY_MOUSE_PRESS,
                GHOSTTY_MOUSE_RIGHT,
                mods,
            );
            ghostty_surface_mouse_button(surface, GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_RIGHT, mods);
            Some(consumed)
        }
    }

    /// RADE (R19.6): whether the Terminal has a selection (the menu's Copy).
    pub fn has_selection(&self) -> bool {
        self.surface_cell
            .borrow()
            .is_some_and(|s| unsafe { ghostty_surface_has_selection(s) })
    }

    /// RADE (R19.6): the whitespace-delimited word at widget point (x, y)
    /// (the menu's "Open …" for a path or URL there).
    pub fn word_at(&self, x: f64, y: f64) -> Option<String> {
        let surface = (*self.surface_cell.borrow())?;
        links::word_at_point(surface, &self.gl_area, x, y)
    }

    pub fn copy_selection_to_clipboard(&self) -> bool {
        let Some(surface) = *self.surface_cell.borrow() else {
            return false;
        };
        if !unsafe { ghostty_surface_has_selection(surface) } {
            return false;
        }

        surface_action(Some(surface), "copy_to_clipboard");
        true
    }

    /// Inject text into the terminal surface for control-socket requests and
    /// drag/drop payloads. Ghostty treats this as pasted text, which matches
    /// the current control protocol semantics.
    pub fn send_text(&self, text: &str) -> bool {
        let Some(surface) = *self.surface_cell.borrow() else {
            return false;
        };

        unsafe {
            ghostty_surface_text(surface, text.as_ptr() as *const c_char, text.len());
        }
        true
    }

    /// Press and release `keyval` with `mods` on the surface, as a real key
    /// press does (`debug.ui key`, R103-c): Ghostty encodes it (Ctrl+K →
    /// `^K`), unlike [`TerminalHandle::send_text`]'s paste.
    pub fn press_key(&self, keyval: gtk::gdk::Key, mods: gtk::gdk::ModifierType) -> bool {
        let Some(surface) = *self.surface_cell.borrow() else {
            return false;
        };
        let keycode = gtk::gdk::Display::default()
            .and_then(|d| d.map_keyval(keyval))
            .and_then(|k| k.first().map(|k| k.keycode()))
            .unwrap_or(0);
        let text = if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            None
        } else {
            input::key_event_text(keyval)
        };
        for action in [GHOSTTY_ACTION_PRESS, GHOSTTY_ACTION_RELEASE] {
            let mut event = input::translate_key_event(action, None, None, keyval, keycode, mods);
            if action == GHOSTTY_ACTION_PRESS {
                if let Some(t) = &text {
                    event.text = t.as_ptr();
                }
            }
            unsafe { ghostty_surface_key(surface, event) };
        }
        true
    }

    pub fn health(&self) -> TerminalHealth {
        let Some(surface) = *self.surface_cell.borrow() else {
            return TerminalHealth {
                realized: false,
                process_exited: false,
                columns: 0,
                rows: 0,
                width_px: 0,
                height_px: 0,
            };
        };

        let size = unsafe { ghostty_surface_size(surface) };
        TerminalHealth {
            realized: true,
            process_exited: unsafe { ghostty_surface_process_exited(surface) },
            columns: size.columns,
            rows: size.rows,
            width_px: size.width_px,
            height_px: size.height_px,
        }
    }

    /// Cursor (row, col) in the viewport, from Ghostty's IME point (the
    /// bottom-centre of the cursor cell, in points).
    pub fn cursor(&self) -> Option<(u32, u32)> {
        let surface = (*self.surface_cell.borrow())?;
        let size = unsafe { ghostty_surface_size(surface) };
        if size.cell_width_px == 0 || size.cell_height_px == 0 {
            return None;
        }
        let (mut x, mut y, mut w, mut h) = (0.0, 0.0, 0.0, 0.0);
        unsafe { ghostty_surface_ime_point(surface, &mut x, &mut y, &mut w, &mut h) };
        let scale = self.gl_area.scale_factor().max(1) as f64;
        let row = ((y * scale) / size.cell_height_px as f64).floor() as i64 - 1;
        let col = ((x * scale) / size.cell_width_px as f64).floor() as i64;
        Some((row.max(0) as u32, col.max(0) as u32))
    }

    /// Ctrl+click the viewport cell (row, col) like a user (R7-a driver).
    pub fn ctrl_click_cell(&self, row: u32, col: u32) -> bool {
        let surface = *self.surface_cell.borrow();
        surface.is_some_and(|s| links::ctrl_click_cell(s, &self.gl_area, row, col))
    }

    /// Centre of the viewport cell (row, col) in the GL area's coordinates
    /// (live harness: where to put the real pointer).
    pub fn cell_point(&self, row: u32, col: u32) -> Option<(f64, f64)> {
        let surface = (*self.surface_cell.borrow())?;
        links::cell_point(surface, &self.gl_area, row, col)
    }

    /// Start the Terminal even if its widget was never shown: surfaces are
    /// created on GLArea realize, which GTK skips for hidden Tabs (M0 row 5,
    /// PRD R2.8). Realizing needs only a realized window, not a mapping.
    pub fn ensure_started(&self) {
        if self.surface_cell.borrow().is_some()
            || self.shutting_down.get()
            || self.gl_area.is_realized()
        {
            return;
        }
        if self.gl_area.root().is_some_and(|r| r.is_realized()) {
            self.gl_area.realize();
        }
    }

    pub fn draw_count(&self) -> u64 {
        self.draw_count.get()
    }

    pub fn gl_area(&self) -> &gtk::GLArea {
        &self.gl_area
    }

    /// Read everything between the top-left and bottom-right of `point_tag`
    /// (`GHOSTTY_POINT_VIEWPORT` = visible rows, `GHOSTTY_POINT_SCREEN` =
    /// scrollback + visible rows).
    pub fn read_text(&self, point_tag: c_int) -> Option<String> {
        let Some(surface) = *self.surface_cell.borrow() else {
            return Some(String::new());
        };

        let selection = ghostty_selection_s {
            top_left: ghostty_point_s {
                tag: point_tag,
                coord: GHOSTTY_POINT_COORD_TOP_LEFT,
                x: 0,
                y: 0,
            },
            bottom_right: ghostty_point_s {
                tag: point_tag,
                coord: GHOSTTY_POINT_COORD_BOTTOM_RIGHT,
                x: 0,
                y: 0,
            },
            rectangle: false,
        };
        let mut text = empty_ghostty_text();

        let has_text = unsafe { ghostty_surface_read_text(surface, selection, &mut text) };
        if !has_text || text.text.is_null() {
            return Some(String::new());
        }

        let bytes = unsafe { std::slice::from_raw_parts(text.text.cast::<u8>(), text.text_len) };
        let output = String::from_utf8_lossy(bytes).into_owned();
        unsafe { ghostty_surface_free_text(surface, &mut text) };
        Some(output)
    }
}

fn track_signal<T>(
    handlers: &RefCell<Vec<(glib::Object, glib::SignalHandlerId)>>,
    object: &T,
    handler: glib::SignalHandlerId,
) where
    T: IsA<glib::Object> + Clone,
{
    handlers
        .borrow_mut()
        .push((object.clone().upcast(), handler));
}

fn empty_ghostty_text() -> ghostty_text_s {
    ghostty_text_s {
        tl_px_x: 0.0,
        tl_px_y: 0.0,
        offset_start: 0,
        offset_len: 0,
        text: ptr::null(),
        text_len: 0,
    }
}

pub struct TerminalWidget {
    pub root: gtk::Widget,
    pub handle: TerminalHandle,
}

fn request_terminal_focus(gl_area: &gtk::GLArea, had_focus: &Cell<bool>) {
    had_focus.set(true);
    gl_area.grab_focus();
}

fn physical_size_for_allocation(
    logical_width: i32,
    logical_height: i32,
    scale_factor: i32,
) -> Option<(u32, u32, u32)> {
    let logical_width = u32::try_from(logical_width).ok()?;
    let logical_height = u32::try_from(logical_height).ok()?;
    let scale_factor = u32::try_from(scale_factor).ok()?;

    if logical_width == 0 || logical_height == 0 || scale_factor == 0 {
        return None;
    }

    let physical_width = logical_width.checked_mul(scale_factor)?;
    let physical_height = logical_height.checked_mul(scale_factor)?;
    Some((physical_width, physical_height, scale_factor))
}

fn refresh_surface_display(surface: ghostty_surface_t, gl_area: &gtk::GLArea) {
    let alloc = (gl_area.width(), gl_area.height());
    if let Some((width, height, scale_factor)) =
        physical_size_for_allocation(alloc.0, alloc.1, gl_area.scale_factor())
    {
        let scale = scale_factor as f64;
        unsafe {
            ghostty_surface_set_content_scale(surface, scale, scale);
            ghostty_surface_set_size(surface, width, height);
        }
    }
    unsafe { ghostty_surface_refresh(surface) };
    gl_area.queue_render();
}

/// Tracks whether Ghostty's renderer is realized for one surface.
///
/// `ghostty_surface_display_realized` is not idempotent: a second call with
/// no intervening unrealize replaces the renderer's swap chain without
/// freeing the old one, orphaning its render target and colour atlas
/// texture. The C export does not guard against that, so the host pairs the
/// two calls itself.
#[derive(Default)]
struct DisplayRealizeState {
    realized: Cell<bool>,
}

impl DisplayRealizeState {
    /// True when the caller must invoke `ghostty_surface_display_realized`.
    fn begin_realize(&self) -> bool {
        !self.realized.replace(true)
    }

    /// True when the caller must invoke `ghostty_surface_display_unrealized`.
    fn begin_unrealize(&self) -> bool {
        self.realized.replace(false)
    }

    /// Adopt the realized state a freshly created surface already carries,
    /// so its first unrealize is still forwarded.
    fn adopt_realized(&self) {
        self.realized.set(true);
    }
}

/// Realize Ghostty's renderer for a freshly realized `GLArea`, then resize.
///
/// Only the `realize` signal may call this. Every other path that needs the
/// surface to match the widget calls [`refresh_surface_display`], which
/// resizes without touching renderer lifetime.
fn refresh_realized_surface_display(
    surface: ghostty_surface_t,
    gl_area: &gtk::GLArea,
    display_realized: &DisplayRealizeState,
) {
    if gl_area.is_realized() {
        gl_area.make_current();
        if gl_area.error().is_none() && display_realized.begin_realize() {
            unsafe { ghostty_surface_display_realized(surface) };
        }
    }
    refresh_surface_display(surface, gl_area);
}

/// The user's Ghostty config, with `scrollback-limit` raised to the Agent
/// budget: Agent Panes inherit it, other Terminals are capped back to the
/// user's value at creation (`ghostty_surface_new_with_scrollback_limit`).
fn load_ghostty_config() -> ghostty_config_t {
    unsafe {
        let user_files = |config| {
            ghostty_config_load_default_files(config);
            ghostty_config_load_recursive_files(config);
        };
        let mut config = ghostty_config_new();
        user_files(config);
        // `ghostty_config_get` has no usize support: read the serialized form.
        let text = ghostty_config_serialize(config);
        let bytes = std::slice::from_raw_parts(text.ptr.cast::<u8>(), text.len);
        let serialized = String::from_utf8_lossy(bytes).into_owned();
        ghostty_string_free(text);
        let user = scrollback_limit_of(&serialized).unwrap_or(AGENT_SCROLLBACK_BYTES);
        let path = rade_core::paths::config_file(&rade_core::paths::process_env);
        let terminal = crate::keys::load_config(&path).terminal;
        if terminal.theme == TerminalTheme::Rade {
            // R102.6 (ADR-0037): RADE's look, loaded after the user's files so
            // it wins; their keybinds and scrollback stay.
            let look = rade_look(&terminal);
            ghostty_config_load_string(
                config,
                look.as_ptr().cast(),
                look.len(),
                c"/rade-look".as_ptr(),
            );
        } else if !theme_pinned(&serialized) {
            // R20.8: no theme or colors of the user's own → Terminals follow
            // the system light / dark (the user's files load after it, so win).
            ghostty_config_free(config);
            config = ghostty_config_new();
            let theme = default_theme();
            ghostty_config_load_string(
                config,
                theme.as_ptr().cast(),
                theme.len(),
                c"/rade-default-theme".as_ptr(),
            );
            user_files(config);
        }
        USER_SCROLLBACK_BYTES.store(user, Ordering::Relaxed);
        if user < AGENT_SCROLLBACK_BYTES {
            let line = format!("scrollback-limit = {AGENT_SCROLLBACK_BYTES}\n");
            ghostty_config_load_string(
                config,
                line.as_ptr().cast(),
                line.len(),
                c"/rade-agent-scrollback".as_ptr(),
            );
        }
        ghostty_config_finalize(config);
        config
    }
}

/// What a Terminal looks like when the user's config doesn't choose (R20.8,
/// R102.6): Ghostty's Adwaita / Adwaita Dark palettes with the background
/// = `bg0` and the foreground = `term-fg` of each theme (spec A.52), as two
/// theme files written to `$XDG_RUNTIME_DIR/rade/ghostty-themes/`.
fn default_theme() -> String {
    let dir = glib::user_runtime_dir().join("rade").join("ghostty-themes");
    let _ = std::fs::create_dir_all(&dir);
    let resources = std::env::var_os("GHOSTTY_RESOURCES_DIR").map(std::path::PathBuf::from);
    let mut paths = Vec::new();
    for (name, base, dark) in [
        ("rade-light", "Adwaita", false),
        ("rade-dark", "Adwaita Dark", true),
    ] {
        let path = dir.join(name);
        let text = theme_text(
            &resources
                .as_ref()
                .and_then(|r| std::fs::read_to_string(r.join("themes").join(base)).ok())
                .unwrap_or_default(),
            crate::tokens::rgb("bg0", dark),
            crate::tokens::rgb("term-fg", dark),
        );
        write_if_changed(&dir, name, &path, &text);
        paths.push(path.display().to_string());
    }
    format!("theme = light:{},dark:{}\n", paths[0], paths[1])
}

/// RADE's Terminal look (R102.6, spec A.51 / A.52, ADR-0037), loaded over
/// the user's Ghostty config: the token theme (light / dark live), opaque,
/// no Ghostty padding (the Pane's 10 × 12 px margin is the padding) and the
/// `[terminal]` font (Settings, R112).
fn rade_look(terminal: &rade_core::config::TerminalConfig) -> String {
    let dir = glib::user_runtime_dir().join("rade").join("ghostty-themes");
    let _ = std::fs::create_dir_all(&dir);
    let mut paths = Vec::new();
    for (name, dark) in [("rade-look-light", false), ("rade-look-dark", true)] {
        let path = dir.join(name);
        write_if_changed(&dir, name, &path, &look_theme(dark));
        paths.push(path.display().to_string());
    }
    look_text(terminal, &paths[0], &paths[1])
}

/// [`rade_look`] without the files: the config text for the two theme paths.
/// Every font key is reset first so the user's Ghostty fonts never mix in;
/// px → pt at Ghostty's 96 dpi; the line height scales the font's own.
fn look_text(terminal: &rade_core::config::TerminalConfig, light: &str, dark: &str) -> String {
    let pt = terminal.font_size * 0.75;
    let cell = ((terminal.line_height - 1.0) * 100.0).round();
    let mut out = format!(
        "theme = light:{light},dark:{dark}\nbackground-opacity = 1\n\
         window-padding-x = 0\nwindow-padding-y = 0\nwindow-padding-balance = false\n\
         window-padding-color = background\nadjust-cell-height = {cell}%\n\
         font-family = \"\"\nfont-family-bold = \"\"\nfont-family-italic = \"\"\n\
         font-family-bold-italic = \"\"\nfont-style = default\nfont-feature = \"\"\n\
         font-size = {pt}\n"
    );
    let font = terminal.font.trim();
    if !font.is_empty() {
        out += &format!("font-family = {font}\n");
    }
    let style = terminal.font_style.trim();
    if !style.is_empty() {
        out += &format!("font-style = {style}\n");
    }
    if !terminal.ligatures {
        out += "font-feature = -calt\nfont-feature = -liga\nfont-feature = -dlig\n";
    }
    out
}

/// The token theme of [`rade_look`]: `term-0`…`term-15`, `term-fg` on
/// `bg0`, an `acc` cursor and a `term-sel` selection.
fn look_theme(dark: bool) -> String {
    use crate::tokens::rgb;
    let mut out: String = (0..16)
        .map(|i| format!("palette = {i}={}\n", rgb(&format!("term-{i}"), dark)))
        .collect();
    out += &format!(
        "background = {}\nforeground = {}\ncursor-color = {}\ncursor-text = {}\n\
         selection-background = {}\nselection-foreground = {}\n",
        rgb("bg0", dark),
        rgb("term-fg", dark),
        rgb("acc", dark),
        rgb("bg0", dark),
        rgb("term-sel", dark),
        rgb("term-fg", dark),
    );
    out
}

/// Writes `text` to `path` unless it is there already; the other flavor may
/// write the same file, so write aside and rename over.
fn write_if_changed(dir: &std::path::Path, name: &str, path: &std::path::Path, text: &str) {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        let tmp = dir.join(format!(".{name}.{}", std::process::id()));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// A Ghostty theme file: `base`'s palette and other keys, with RADE's
/// background, foreground and cursor text.
fn theme_text(base: &str, bg: &str, fg: &str) -> String {
    let mut out: String = base
        .lines()
        .filter(|l| {
            let key = l.split('=').next().unwrap_or("").trim();
            !matches!(key, "background" | "foreground" | "cursor-text")
        })
        .map(|l| format!("{l}\n"))
        .collect();
    out += &format!("background = {bg}\nforeground = {fg}\ncursor-text = {bg}\n");
    out
}

/// Whether the user's Ghostty config chose the Terminal's colors: a
/// `theme`, or a `background` / `foreground` other than Ghostty's built-in.
fn theme_pinned(serialized: &str) -> bool {
    let value = |key: &str| {
        serialized
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(" = "))
            .map(str::trim)
    };
    value("theme").is_some_and(|v| !v.is_empty())
        || value("background").is_some_and(|v| parse_rgb(v) != Some(GHOSTTY_DEFAULT_BG))
        || value("foreground").is_some_and(|v| parse_rgb(v) != Some(GHOSTTY_DEFAULT_FG))
}

/// Ghostty's built-in colors, as its serialized config prints them when the
/// user set none (not RADE colors, so not design tokens).
const GHOSTTY_DEFAULT_BG: [u8; 3] = [0x28, 0x2c, 0x34];
const GHOSTTY_DEFAULT_FG: [u8; 3] = [0xff, 0xff, 0xff];

/// `#rrggbb` (any case) → its bytes.
fn parse_rgb(v: &str) -> Option<[u8; 3]> {
    let h = v.strip_prefix('#').filter(|h| h.len() == 6)?;
    let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

fn scrollback_limit_of(serialized: &str) -> Option<usize> {
    serialized
        .lines()
        .find_map(|l| l.strip_prefix("scrollback-limit = ")?.trim().parse().ok())
}

/// Scrollback cap for a new surface: 0 (inherit the raised budget) for
/// Agent Panes, the user's own limit for everything else.
fn surface_scrollback_cap(agent: bool) -> usize {
    if agent {
        0
    } else {
        USER_SCROLLBACK_BYTES.load(Ordering::Relaxed)
    }
}

/// Initialize the global Ghostty app. Must be called once before creating surfaces.
/// A link opened now came from a Ctrl+Shift+click (R113.2); read once.
pub fn take_side_click() -> bool {
    links::take_side()
}

pub fn init_ghostty() {
    GHOSTTY.get_or_init(|| {
        // Ghostty's event loops on epoll, not io_uring: rings are charged
        // to the user's shared locked-memory budget and ran it out
        // (`SystemResources` on new Terminals; ADR-0031).
        rade_core::process::without_locked_memory(|| unsafe {
            ghostty_init(0, ptr::null_mut());
        });

        let config = load_ghostty_config();
        CURRENT_SCROLLBAR_ENABLED.store(load_scrollbar_enabled(config), Ordering::Relaxed);

        let runtime_config = ghostty_runtime_config_s {
            userdata: ptr::null_mut(),
            supports_selection_clipboard: true,
            wakeup_cb: ghostty_wakeup_cb,
            action_cb: ghostty_action_cb,
            read_clipboard_cb: ghostty_read_clipboard_cb,
            confirm_read_clipboard_cb: ghostty_confirm_read_clipboard_cb,
            write_clipboard_cb: ghostty_write_clipboard_cb,
            close_surface_cb: ghostty_close_surface_cb,
            tmux_control_cb: None,
        };

        let app = unsafe { ghostty_app_new(&runtime_config, config) };

        // Ghostty's GTK apprt calls core_app.tick() on every GLib main
        // loop iteration to drain the app mailbox (which includes
        // redraw_surface messages from the renderer thread). The renderer
        // thread pushes these messages but doesn't wake the app.
        // We replicate this with a high-frequency timer (~8ms ≈ 120Hz).
        glib::timeout_add_local(std::time::Duration::from_millis(8), move || {
            unsafe { ghostty_app_tick(app) };
            glib::ControlFlow::Continue
        });

        GhosttyState { app }
    });
}

/// Reloads the Ghostty config into every Terminal (Settings, a
/// `config.toml` change; R112): Ghostty passes it on to each surface.
pub fn reload_config() {
    let Some(state) = GHOSTTY.get() else { return };
    let config = load_ghostty_config();
    CURRENT_SCROLLBAR_ENABLED.store(load_scrollbar_enabled(config), Ordering::Relaxed);
    unsafe {
        ghostty_app_update_config(state.app, config);
        ghostty_config_free(config);
    }
}

fn ghostty_app() -> ghostty_app_t {
    GHOSTTY.get().expect("ghostty not initialized").app
}

fn load_scrollbar_enabled(config: ghostty_config_t) -> bool {
    let mut value: *const c_char = ptr::null();
    let key = b"scrollbar";
    let loaded = unsafe {
        ghostty_config_get(
            config,
            (&mut value as *mut *const c_char).cast::<c_void>(),
            key.as_ptr().cast::<c_char>(),
            key.len(),
        )
    };

    !loaded || value.is_null() || unsafe { std::ffi::CStr::from_ptr(value) }.to_bytes() != b"never"
}

fn ghostty_color_scheme_for_dark_mode(dark: bool) -> c_int {
    if dark {
        GHOSTTY_COLOR_SCHEME_DARK
    } else {
        GHOSTTY_COLOR_SCHEME_LIGHT
    }
}

fn current_ghostty_color_scheme() -> c_int {
    CURRENT_COLOR_SCHEME.load(Ordering::Relaxed)
}

pub fn sync_color_scheme(dark: bool) {
    let scheme = ghostty_color_scheme_for_dark_mode(dark);
    CURRENT_COLOR_SCHEME.store(scheme, Ordering::Relaxed);
    let app = ghostty_app();

    unsafe {
        ghostty_app_set_color_scheme(app, scheme);
    }

    SURFACE_MAP.with(|map| {
        for surface_key in map.borrow().keys() {
            let surface = *surface_key as ghostty_surface_t;
            unsafe {
                ghostty_surface_set_color_scheme(surface, scheme);
            }
        }
    });
}

#[cfg(test)]
mod theme_tests {
    #[test]
    fn r112_c_rade_look_wins_with_the_settings_font() {
        let text = super::look_text(&Default::default(), "/l", "/d");
        assert!(text.starts_with("theme = light:/l,dark:/d\nbackground-opacity = 1\n"));
        assert!(text.contains("window-padding-x = 0\nwindow-padding-y = 0\n"));
        assert!(text.contains("font-family = \"\"\nfont-family-bold = \"\"\n"));
        assert!(
            text.contains("font-feature = \"\"\nfont-size = 10.5\n"),
            "14 px"
        );
        assert!(
            text.contains("adjust-cell-height = 15%\n"),
            "line height 1.15"
        );
        assert!(text.ends_with("font-family = Maple Mono\nfont-style = Medium\n"));
        let terminal = rade_core::config::TerminalConfig {
            font: "Geist Mono".into(),
            font_style: String::new(),
            font_size: 16.0,
            line_height: 1.0,
            ligatures: false,
            ..Default::default()
        };
        let text = super::look_text(&terminal, "/l", "/d");
        assert!(text.contains("adjust-cell-height = 0%\n"));
        assert!(text.contains("font-size = 12\nfont-family = Geist Mono\nfont-feature = -calt\n"));
        assert!(!text.contains("font-style = \n"));
    }

    #[test]
    fn r102_i_look_theme_is_the_tokens() {
        for dark in [false, true] {
            let t = super::look_theme(dark);
            assert_eq!(
                t.lines().filter(|l| l.starts_with("palette = ")).count(),
                16
            );
            let line = |k: &str| format!("{k} = {}\n", crate::tokens::rgb(k, dark));
            assert!(t.contains(&format!(
                "palette = 6={}\n",
                crate::tokens::rgb("term-6", dark)
            )));
            assert!(t.contains(&line("bg0").replace("bg0", "background")));
            assert!(t.contains(&format!(
                "cursor-color = {}\n",
                crate::tokens::rgb("acc", dark)
            )));
        }
    }

    #[test]
    fn default_theme_keeps_the_palette_and_takes_bg0_and_term_fg() {
        let base =
            "palette = 0=x\nbackground = a\nforeground = b\ncursor-text = c\ncursor-color = d\n";
        let t = super::theme_text(base, "BG", "FG");
        assert_eq!(
            t,
            "palette = 0=x\ncursor-color = d\nbackground = BG\nforeground = FG\ncursor-text = BG\n"
        );
        assert!(super::theme_text("", "BG", "FG").starts_with("background = BG"));
    }
}

#[cfg(test)]
mod scrollback_tests {
    #[test]
    fn reads_scrollback_limit_from_serialized_config() {
        let text = "font-size = 12\nscrollback-limit = 100000\ntheme = x\n";
        assert_eq!(super::scrollback_limit_of(text), Some(100000));
        assert_eq!(super::scrollback_limit_of("x = 1\n"), None);
    }

    #[test]
    fn theme_is_pinned_by_a_theme_or_a_background() {
        use super::{theme_pinned, GHOSTTY_DEFAULT_BG, GHOSTTY_DEFAULT_FG};
        // Colors spelled from bytes: no raw hex in rade-app sources (R102-b).
        let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        let (bg, fg) = (hex(GHOSTTY_DEFAULT_BG), hex(GHOSTTY_DEFAULT_FG));
        let defaults = format!("theme = \nbackground = {bg}\nforeground = {fg}\n");
        assert!(!theme_pinned(&defaults));
        assert!(!theme_pinned(""));
        assert!(theme_pinned(&format!(
            "theme = Catppuccin Mocha\nbackground = {bg}\n"
        )));
        assert!(theme_pinned(&format!(
            "theme = \nbackground = {}\n",
            hex([0x1e, 0x1e, 0x2e])
        )));
        assert!(theme_pinned(&format!(
            "theme = \nforeground = {}\n",
            hex([0x33, 0x33, 0x33])
        )));
        assert!(!theme_pinned(&format!(
            "background-opacity = 0.9\nbackground = {}\n",
            bg.to_uppercase()
        )));
    }
}
