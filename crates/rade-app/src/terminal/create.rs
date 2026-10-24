// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae

//! Terminal widget creation and surface lifetime.

use super::*;

// ---------------------------------------------------------------------------
// Surface creation
// ---------------------------------------------------------------------------

pub struct TerminalCallbacks {
    pub on_title_changed: Box<TitleChangedCallback>,
    pub on_pwd_changed: Box<PwdChangedCallback>,
    pub on_desktop_notification: Box<DesktopNotificationCallback>,
    pub on_bell: Box<BellCallback>,
    pub on_close: Box<VoidCallback>,
    /// The process exited (exit code); fires before any close.
    pub on_child_exited: Box<ChildExitedCallback>,
    /// Ctrl+click on a link: Ghostty's matched text or OSC 8 URI (R7.1).
    pub on_open_url: Box<PwdChangedCallback>,
    /// OSC 133 command finished (shell integration, R10.2 fallback).
    pub on_command_finished: Box<VoidCallback>,
    /// RADE (R19.6): a right-click the program did not take (no mouse
    /// reporting, or Shift held), at widget point (x, y): show the menu.
    pub on_context_menu: Box<dyn Fn(f64, f64)>,
}

impl TerminalCallbacks {
    pub fn disconnected() -> Self {
        Self {
            on_title_changed: Box::new(|_| {}),
            on_pwd_changed: Box::new(|_| {}),
            on_desktop_notification: Box::new(|_, _, _| {}),
            on_bell: Box::new(|_| {}),
            on_close: Box::new(|| {}),
            on_child_exited: Box::new(|_| {}),
            on_open_url: Box::new(|_| {}),
            on_command_finished: Box::new(|| {}),
            on_context_menu: Box::new(|_, _| {}),
        }
    }
}

pub struct TerminalOptions {
    pub hover_focus: Rc<dyn Fn() -> bool>,
    pub copy_selection_to_clipboard: Rc<dyn Fn() -> bool>,
    pub saved_font_size: Option<f32>,
    pub startup_command: Option<String>,
    pub initial_input: Option<String>,
    /// Keep the surface open after `startup_command` exits.
    pub wait_after_command: bool,
    /// Extra environment variables to expose to the spawned shell.
    pub extra_env: Vec<(String, String)>,
    /// Asked when the surface is created: take keyboard focus? (RADE:
    /// Terminals in hidden Tabs are realized early and must not steal it.)
    pub focus_on_realize: Rc<dyn Fn() -> bool>,
    /// Agent Pane: gets the Agent scrollback budget (R2.7).
    pub agent_scrollback: bool,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            hover_focus: Rc::new(|| false),
            copy_selection_to_clipboard: Rc::new(|| true),
            saved_font_size: None,
            startup_command: None,
            initial_input: None,
            wait_after_command: false,
            extra_env: Vec::new(),
            focus_on_realize: Rc::new(|| true),
            agent_scrollback: false,
        }
    }
}

pub(super) fn free_terminal_surface(
    surface: ghostty_surface_t,
    clipboard_context_cell: &Cell<*mut ClipboardContext>,
    gl_area: &gtk::GLArea,
) {
    let surface_key = surface as usize;
    let entry = SURFACE_MAP.with(|map| map.borrow_mut().remove(&surface_key));
    let clipboard_context = entry
        .as_ref()
        .map(|entry| entry.clipboard_context)
        .unwrap_or_else(|| clipboard_context_cell.get());

    clipboard_context_cell.set(ptr::null_mut());
    if let Some(entry) = entry.as_ref() {
        unregister_surface_identity(entry.identity);
    }

    clipboard::cancel_surface_requests(surface_key);

    // The Linux C API frees GL objects in the caller's current context. A
    // shell exit runs from an idle callback, where another terminal's context
    // (or none) may be current. Bind this terminal before freeing its renderer.
    // An unrealized surface already released its GL resources in unrealize.
    if gl_area.is_realized() {
        gl_area.make_current();
    }

    // Ghostty can invoke callbacks while its worker threads stop. Keep callback
    // userdata and widgets alive until deinitialization finishes.
    unsafe { ghostty_surface_free(surface) };
    if !clipboard_context.is_null() {
        unsafe {
            drop(Box::from_raw(clipboard_context));
        }
    }
    drop(entry);
}

/// Create a new Ghostty-powered terminal widget.
/// Returns an Overlay (GLArea + toast layer) for embedding in the pane.
pub fn create_terminal(
    working_directory: Option<&str>,
    options: TerminalOptions,
    callbacks: TerminalCallbacks,
) -> TerminalWidget {
    let gl_area = gtk::GLArea::new();
    gl_area.set_hexpand(true);
    gl_area.set_vexpand(true);
    // auto_render=true ensures GTK continuously redraws the GLArea,
    // which forces its internal FBO to match the current allocation.
    // With auto_render=false, the FBO may stay at the initial size.
    gl_area.set_auto_render(true);
    gl_area.set_focusable(true);
    gl_area.set_can_focus(true);
    set_gtk_mouse_cursor(&gl_area, MouseCursorState::default());
    let wd = working_directory.map(|s| s.to_string());
    let saved_font_size = options.saved_font_size;
    let startup_command = options.startup_command;
    let initial_input = options.initial_input;
    let wait_after_command = options.wait_after_command;
    let hover_focus = options.hover_focus;
    let focus_on_realize = options.focus_on_realize;
    let copy_selection_to_clipboard = options.copy_selection_to_clipboard;
    let extra_env = options.extra_env;
    let scrollback_cap = surface_scrollback_cap(options.agent_scrollback);
    let callbacks = Rc::new(RefCell::new(callbacks));
    let surface_cell: Rc<RefCell<Option<ghostty_surface_t>>> = Rc::new(RefCell::new(None));
    let display_realized: Rc<DisplayRealizeState> = Rc::new(DisplayRealizeState::default());
    let shutting_down = Rc::new(Cell::new(false));
    let had_focus = Rc::new(Cell::new(false));
    let scrollbar_syncing = Rc::new(Cell::new(false));
    let clipboard_context_cell: Rc<Cell<*mut ClipboardContext>> =
        Rc::new(Cell::new(ptr::null_mut()));
    let signal_handlers = Rc::new(RefCell::new(Vec::new()));
    let controllers = Rc::new(RefCell::new(Vec::new()));
    let draw_count = Rc::new(Cell::new(0u64));

    // Create overlay early so closures can capture it for toast notifications
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&gl_area));
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    let scrollbar_adjustment = gtk::Adjustment::new(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let scrollbar = gtk::Scrollbar::new(gtk::Orientation::Vertical, Some(&scrollbar_adjustment));
    scrollbar.set_visible(false);
    scrollbar.set_vexpand(true);
    // The scrollbar must be an overlay child, not a sibling in the box below.
    // Ghostty emits a SCROLLBAR action whenever `total > len` flips, and we
    // toggle visibility in response. As a Box sibling that consumes layout
    // width, showing/hiding it reallocated the GLArea, changed the terminal
    // grid columns, and reflowed the text left/right — the "window bounces"
    // flicker in limux issue 136. Overlaid, the terminal keeps its full width and
    // the scrollbar just draws (or stops drawing) on top of it.
    scrollbar.set_halign(gtk::Align::End);
    scrollbar.set_valign(gtk::Align::Fill);
    overlay.add_overlay(&scrollbar);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    // RADE (R103.3): `terminal` = 10 / 12 px padding inside its Pane (CSS).
    root.add_css_class("terminal");
    root.append(&overlay);

    // RADE (R19.5): the search bar Ghostty's search actions show.
    let search = search::SearchBar::new(surface_cell.clone(), gl_area.clone());
    overlay.add_overlay(&search.root);

    let pane_ime = Rc::new(ime::create_pane_ime(&gl_area, &surface_cell));
    // Aliases for the focus / lifecycle wiring further down. Cheap
    // GObject clones, not state copies.
    let im_context = pane_ime.primary.clone();
    let im_fallback = pane_ime.fallback.clone();

    let handle = TerminalHandle {
        surface_cell: surface_cell.clone(),
        clipboard_context_cell: clipboard_context_cell.clone(),
        shutting_down: shutting_down.clone(),
        gl_area: gl_area.clone(),
        callbacks: callbacks.clone(),
        im_context: im_context.clone(),
        im_fallback: im_fallback.clone(),
        signal_handlers: signal_handlers.clone(),
        controllers: controllers.clone(),
        draw_count: draw_count.clone(),
        search: search.clone(),
    };

    {
        let surface_cell = surface_cell.clone();
        let handler = gl_area.connect_map(move |gl_area| {
            gl_area.set_auto_render(true);
            if let Some(surface) = *surface_cell.borrow() {
                unsafe { ghostty_surface_set_occlusion(surface, true) };
                refresh_surface_display(surface, gl_area);
            } else {
                gl_area.queue_render();
            }
        });
        track_signal(&signal_handlers, &gl_area, handler);
    }

    {
        let surface_cell = surface_cell.clone();
        let handler = gl_area.connect_unmap(move |gl_area| {
            gl_area.set_auto_render(false);
            if let Some(surface) = *surface_cell.borrow() {
                unsafe { ghostty_surface_set_occlusion(surface, false) };
            }
        });
        track_signal(&signal_handlers, &gl_area, handler);
    }

    {
        let surface_cell = surface_cell.clone();
        let scrollbar_syncing = scrollbar_syncing.clone();
        let handler = scrollbar_adjustment.connect_value_changed(move |adj| {
            if scrollbar_syncing.get() {
                return;
            }

            let row = adj.value().round() as usize;
            surface_action(*surface_cell.borrow(), &format!("scroll_to_row:{row}"));
        });
        track_signal(&signal_handlers, &scrollbar_adjustment, handler);
    }

    // On realize: create the Ghostty surface
    {
        let gl = gl_area.clone();
        let overlay_for_map = overlay.clone();
        let scrollbar_for_map = scrollbar.clone();
        let scrollbar_adjustment_for_map = scrollbar_adjustment.clone();
        let surface_cell = surface_cell.clone();
        let callbacks = callbacks.clone();
        let had_focus = had_focus.clone();
        let clipboard_context_cell = clipboard_context_cell.clone();
        let scrollbar_syncing = scrollbar_syncing.clone();
        let extra_env = extra_env.clone();
        let shutting_down = shutting_down.clone();
        let display_realized = display_realized.clone();
        let handler = gl_area.connect_realize(move |gl_area| {
            if shutting_down.get() {
                return;
            }
            gl_area.make_current();
            if let Some(err) = gl_area.error() {
                eprintln!("rade: GLArea error after make_current: {err}");
                return;
            }

            // If the surface already exists (reparenting from a split),
            // reinitialize the GL renderer with the new GL context while
            // preserving the terminal/pty state.
            if let Some(surface) = *surface_cell.borrow() {
                refresh_realized_surface_display(surface, gl_area, &display_realized);
                let gl_area = gl_area.clone();
                glib::idle_add_local_once(move || {
                    gl_area.queue_render();
                });
                return;
            }

            let app = ghostty_app();
            let mut config = unsafe { ghostty_surface_config_new() };
            let clipboard_context = Box::into_raw(Box::new(ClipboardContext {
                surface: Cell::new(ptr::null_mut()),
                copy_selection_to_clipboard: copy_selection_to_clipboard.clone(),
            }));
            config.platform_tag = GHOSTTY_PLATFORM_LINUX;
            config.platform = ghostty_platform_u {
                linux_platform: ghostty_platform_linux_s {
                    reserved: ptr::null_mut(),
                },
            };
            config.userdata = clipboard_context.cast();

            let scale = gl_area.scale_factor() as f64;
            config.scale_factor = scale;
            config.context = GHOSTTY_SURFACE_CONTEXT_WINDOW;

            let c_wd = wd.as_ref().and_then(|s| CString::new(s.as_str()).ok());
            if let Some(ref cwd) = c_wd {
                config.working_directory = cwd.as_ptr();
            }

            // Build env_vars array for the spawned shell. Keep the CStrings
            // and the ghostty_env_var_s array alive until after
            // ghostty_surface_new returns — Ghostty dupes the strings into
            // its own arena (see ghostty/src/apprt/embedded.zig:573), so we
            // only need the pointers valid across that single call.
            let mut env_cstrings: Vec<(CString, CString)> = Vec::with_capacity(extra_env.len());
            for (k, v) in extra_env.iter() {
                if let (Ok(k_c), Ok(v_c)) = (CString::new(k.as_str()), CString::new(v.as_str())) {
                    env_cstrings.push((k_c, v_c));
                }
            }
            let mut env_vars_raw: Vec<ghostty_env_var_s> = env_cstrings
                .iter()
                .map(|(k, v)| ghostty_env_var_s {
                    key: k.as_ptr(),
                    value: v.as_ptr(),
                })
                .collect();
            if !env_vars_raw.is_empty() {
                config.env_vars = env_vars_raw.as_mut_ptr();
                config.env_var_count = env_vars_raw.len();
            }

            let c_startup_command = startup_command
                .as_ref()
                .and_then(|command| CString::new(command.as_str()).ok());
            if let Some(ref command) = c_startup_command {
                config.command = command.as_ptr();
                eprintln!(
                    "rade: starting terminal command={}",
                    command.to_string_lossy()
                );
            }

            let c_initial_input = initial_input
                .as_ref()
                .and_then(|input| CString::new(input.as_str()).ok());
            if let Some(ref input) = c_initial_input {
                config.initial_input = input.as_ptr();
            }
            config.wait_after_command = wait_after_command;

            // The Terminal's process must not inherit the app's GPU and other
            // fds that libraries open without O_CLOEXEC (M28 acceptance, Low).
            rade_core::process::cloexec_all();
            let surface =
                unsafe { ghostty_surface_new_with_scrollback_limit(app, &config, scrollback_cap) };
            if surface.is_null() {
                unsafe {
                    drop(Box::from_raw(clipboard_context));
                }
                eprintln!("rade: failed to create ghostty surface");
                return;
            }
            let surface_key = surface as usize;
            let surface_identity = register_surface_identity(surface_key);
            unsafe {
                (*clipboard_context).surface.set(surface);
                ghostty_surface_set_color_scheme(surface, current_ghostty_color_scheme());
            }
            clipboard_context_cell.set(clipboard_context);

            // Apply saved font size (if different from ghostty default)
            if let Some(size) = saved_font_size {
                let action = format!("set_font_size:{size}");
                unsafe {
                    ghostty_surface_binding_action(
                        surface,
                        action.as_ptr() as *const c_char,
                        action.len(),
                    );
                }
            }

            // Set initial size in physical pixels. GTK allocation is logical
            // CSS pixels; Ghostty's GL renderer expects the backing FBO size.
            let alloc = (gl_area.width(), gl_area.height());
            if physical_size_for_allocation(alloc.0, alloc.1, gl_area.scale_factor()).is_some() {
                refresh_surface_display(surface, gl_area);
            }

            SURFACE_MAP.with(|map| {
                map.borrow_mut().insert(
                    surface_key,
                    SurfaceEntry {
                        _startup_command: c_startup_command,
                        identity: surface_identity,
                        gl_area: gl.clone(),
                        toast_overlay: overlay_for_map.clone(),
                        scrollbar: scrollbar_for_map.clone(),
                        scrollbar_adjustment: scrollbar_adjustment_for_map.clone(),
                        scrollbar_syncing: scrollbar_syncing.clone(),
                        search: search.clone(),
                        on_title_changed: Some(Box::new({
                            let cb = callbacks.clone();
                            move |title| (cb.borrow().on_title_changed)(title)
                        })),
                        on_pwd_changed: Some(Box::new({
                            let cb = callbacks.clone();
                            move |pwd| (cb.borrow().on_pwd_changed)(pwd)
                        })),
                        on_desktop_notification: Some(Box::new({
                            let cb = callbacks.clone();
                            move |title, body, source_focused| {
                                let callbacks = cb.borrow();
                                (callbacks.on_desktop_notification)(title, body, source_focused);
                            }
                        })),
                        on_bell: Some(Box::new({
                            let cb = callbacks.clone();
                            move |source_focused| {
                                let callbacks = cb.borrow();
                                (callbacks.on_bell)(source_focused);
                            }
                        })),
                        on_close: Some(Box::new({
                            let cb = callbacks.clone();
                            move || {
                                let callbacks = cb.borrow();
                                (callbacks.on_close)();
                            }
                        })),
                        on_child_exited: Some(Box::new({
                            let cb = callbacks.clone();
                            move |code| {
                                let callbacks = cb.borrow();
                                (callbacks.on_child_exited)(code);
                            }
                        })),
                        on_open_url: Some(Box::new({
                            let cb = callbacks.clone();
                            move |url| (cb.borrow().on_open_url)(url)
                        })),
                        on_command_finished: Some(Box::new({
                            let cb = callbacks.clone();
                            move || (cb.borrow().on_command_finished)()
                        })),
                        clipboard_context,
                        mouse_cursor: Cell::new(MouseCursorState::default()),
                    },
                );
            });

            display_realized.adopt_realized();
            *surface_cell.borrow_mut() = Some(surface);

            // Grab GTK focus so key events reach this widget.
            if focus_on_realize() {
                unsafe { ghostty_surface_set_focus(surface, true) };
                request_terminal_focus(gl_area, &had_focus);
            } else {
                unsafe { ghostty_surface_set_focus(surface, false) };
            }
            // Realized while hidden (never mapped): don't render (R4.2)
            // until `map` marks it visible.
            if !gl_area.is_mapped() {
                unsafe { ghostty_surface_set_occlusion(surface, false) };
            }
        });
        track_signal(&signal_handlers, &gl_area, handler);
    }

    // On render: draw the surface.
    {
        let surface_cell = surface_cell.clone();
        let draw_count = draw_count.clone();
        let handler = gl_area.connect_render(move |_gl_area, _context| {
            if let Some(surface) = *surface_cell.borrow() {
                draw_count.set(draw_count.get() + 1);
                unsafe { ghostty_surface_draw(surface) };
            }
            glib::Propagation::Stop
        });
        track_signal(&signal_handlers, &gl_area, handler);
    }

    // On resize: update Ghostty's terminal grid size and queue a redraw.
    // The actual GL viewport is set by GTK when the render signal fires,
    // so we must NOT call ghostty_surface_draw here — the viewport would
    // still be the old size. Instead we queue_render() and let the render
    // callback draw with the correct viewport.
    {
        let surface_cell = surface_cell.clone();
        let gl_for_resize = gl_area.clone();
        let had_focus = had_focus.clone();
        let handler = gl_area.connect_resize(move |gl_area, width, height| {
            if let Some(surface) = *surface_cell.borrow() {
                let w = width as u32;
                let h = height as u32;
                if w > 0 && h > 0 {
                    refresh_surface_display(surface, gl_area);
                }
            }

            if had_focus.get() {
                let gl_for_focus = gl_for_resize.clone();
                let had_focus = had_focus.clone();
                glib::idle_add_local_once(move || {
                    // Another pane may have received focus while layout completed.
                    if had_focus.get() {
                        gl_for_focus.grab_focus();
                    }
                });
            }
        });
        track_signal(&signal_handlers, &gl_area, handler);
    }

    // Keyboard input
    //
    // Send key events with the text field populated. Ghostty uses the
    // text field for actual character input and the keycode for bindings.
    // Do NOT use ghostty_surface_text() for regular typing — Ghostty
    // treats that as a paste, causing "pasting..." indicators in apps.
    {
        let sc_press = surface_cell.clone();
        let sc_release = surface_cell.clone();
        let pane_ime_press = pane_ime.clone();
        let pane_ime_release = pane_ime.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |ctrl, keyval, keycode, modifier| {
            if let Some(surface) = *sc_press.borrow() {
                let current_event = ctrl
                    .current_event()
                    .and_then(|event| event.downcast::<gtk::gdk::KeyEvent>().ok());
                let widget = ctrl.widget();
                let fallback_text = key_event_text(keyval);

                if let Some(current_event) = current_event.as_ref() {
                    pane_ime_press.state.borrow_mut().begin_key_event();

                    let filter_outcome =
                        ime::filter_key_event(surface, &pane_ime_press, current_event);
                    if filter_outcome == ime::ImeFilterOutcome::ConsumeForIme {
                        pane_ime_press.state.borrow_mut().finish_key_event();
                        return glib::Propagation::Stop;
                    }
                }

                let mut event = translate_key_event(
                    GHOSTTY_ACTION_PRESS,
                    widget.as_ref(),
                    current_event.as_ref(),
                    keyval,
                    keycode,
                    modifier,
                );
                let c_text = pane_ime_press
                    .state
                    .borrow_mut()
                    .take_event_text(fallback_text);
                if let Some(ref ct) = c_text {
                    event.text = ct.as_ptr();
                }

                let consumed = unsafe { ghostty_surface_key(surface, event) };
                if consumed && pane_ime_press.state.borrow().composing {
                    ime::reset_after_consumed_compose(surface, &pane_ime_press);
                }
                pane_ime_press.state.borrow_mut().finish_key_event();
                if consumed {
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });

        key_controller.connect_key_released(move |ctrl, keyval, keycode, modifier| {
            if let Some(surface) = *sc_release.borrow() {
                let current_event = ctrl
                    .current_event()
                    .and_then(|event| event.downcast::<gtk::gdk::KeyEvent>().ok());
                let widget = ctrl.widget();

                if let Some(current_event) = current_event.as_ref() {
                    pane_ime_release.state.borrow_mut().begin_key_event();

                    let filter_outcome =
                        ime::filter_key_event(surface, &pane_ime_release, current_event);
                    if filter_outcome == ime::ImeFilterOutcome::ConsumeForIme {
                        pane_ime_release.state.borrow_mut().finish_key_event();
                        return;
                    }
                }

                let event = translate_key_event(
                    GHOSTTY_ACTION_RELEASE,
                    widget.as_ref(),
                    current_event.as_ref(),
                    keyval,
                    keycode,
                    modifier,
                );
                unsafe { ghostty_surface_key(surface, event) };
                pane_ime_release.state.borrow_mut().finish_key_event();
            }
        });

        controllers
            .borrow_mut()
            .push(key_controller.clone().upcast());
        gl_area.add_controller(key_controller);
    }

    // Mouse buttons (also handles click-to-focus). RADE (R19.6): the right
    // button goes to Ghostty too, like its own GTK app: a program with mouse
    // reporting gets it; otherwise Ghostty selects the word or link there
    // and leaves it unconsumed, and the context menu opens (Shift+right-click
    // reaches it even over a mouse-reporting TUI, per mouse-shift-capture).
    {
        let surface_cell = surface_cell.clone();
        let click = gtk::GestureClick::new();
        click.set_button(0); // all buttons
        let sc = surface_cell.clone();
        let gl_for_focus = gl_area.clone();
        let had_focus = had_focus.clone();
        let menu_callbacks = callbacks.clone();
        click.connect_pressed(move |gesture, _n, x, y| {
            let btn = gesture.current_button();
            // Grab keyboard focus on any click
            request_terminal_focus(&gl_for_focus, &had_focus);
            if let Some(surface) = *sc.borrow() {
                let mods = translate_mouse_mods(gesture.current_event_state());
                let consumed = unsafe {
                    ghostty_surface_mouse_pos(surface, x, y, mods);
                    ghostty_surface_mouse_button(
                        surface,
                        GHOSTTY_MOUSE_PRESS,
                        mouse_button(btn),
                        mods,
                    )
                };
                // RADE (M14): opened in the press, not from an idle, so the
                // popover gets the keyboard (a grab needs the event).
                if btn == 3 && !consumed {
                    (menu_callbacks.borrow().on_context_menu)(x, y);
                }
            }
        });
        let sc2 = surface_cell.clone();
        let gl_for_release = gl_area.clone();
        click.connect_released(move |gesture, _n, x, y| {
            let btn = gesture.current_button();
            if let Some(surface) = *sc2.borrow() {
                let mods = translate_mouse_mods(gesture.current_event_state());
                links::before_release(); // RADE: Ctrl+click fallback (R7.1)
                                         // Ctrl+Shift+click: Open to the Side (R113.2).
                links::set_side(
                    btn == 1 && mods & GHOSTTY_MODS_CTRL != 0 && mods & GHOSTTY_MODS_SHIFT != 0,
                );
                unsafe {
                    ghostty_surface_mouse_pos(surface, x, y, mods);
                    ghostty_surface_mouse_button(
                        surface,
                        GHOSTTY_MOUSE_RELEASE,
                        mouse_button(btn),
                        mods,
                    );
                }
                if btn == 1 && mods & GHOSTTY_MODS_CTRL != 0 {
                    links::after_release(surface, &gl_for_release, x, y);
                }
            }
        });
        controllers.borrow_mut().push(click.clone().upcast());
        gl_area.add_controller(click);
    }

    // Mouse motion
    {
        let surface_cell = surface_cell.clone();
        let surface_cell_for_enter = surface_cell.clone();
        let gl_for_focus = gl_area.clone();
        let had_focus = had_focus.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter(move |ctrl, x, y| {
            if (hover_focus)() {
                // Match common Hyprland/Omarchy-style focus-follows-mouse behavior:
                // as soon as the pointer enters a terminal, focus it so typing works
                // immediately without an extra click.
                request_terminal_focus(&gl_for_focus, &had_focus);
            }

            if let Some(surface) = *surface_cell_for_enter.borrow() {
                let mods = translate_mouse_mods(ctrl.current_event_state());
                unsafe { ghostty_surface_mouse_pos(surface, x, y, mods) };
            }
        });
        let surface_cell = surface_cell.clone();
        motion.connect_motion(move |ctrl, x, y| {
            if let Some(surface) = *surface_cell.borrow() {
                let mods = translate_mouse_mods(ctrl.current_event_state());
                unsafe { ghostty_surface_mouse_pos(surface, x, y, mods) };
            }
        });
        controllers.borrow_mut().push(motion.clone().upcast());
        gl_area.add_controller(motion);
    }

    // Mouse scroll
    {
        let surface_cell = surface_cell.clone();
        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::DISCRETE,
        );
        scroll.connect_scroll(move |ctrl, dx, dy| {
            if let Some(surface) = *surface_cell.borrow() {
                let mods = translate_mouse_mods(ctrl.current_event_state());
                // GTK and Ghostty use opposite scroll conventions — negate both axes
                unsafe { ghostty_surface_mouse_scroll(surface, -dx, -dy, mods) };
            }
            glib::Propagation::Stop
        });
        controllers.borrow_mut().push(scroll.clone().upcast());
        gl_area.add_controller(scroll);
    }

    // Focus
    {
        let surface_cell = surface_cell.clone();
        let had_focus_enter = had_focus.clone();
        let had_focus_leave = had_focus.clone();
        let im_context_enter = im_context.clone();
        let im_context_leave = im_context.clone();
        let im_fallback_enter = im_fallback.clone();
        let im_fallback_leave = im_fallback.clone();
        let focus_ctrl = gtk::EventControllerFocus::new();
        let sc = surface_cell.clone();
        focus_ctrl.connect_enter(move |_| {
            had_focus_enter.set(true);
            im_context_enter.focus_in();
            im_fallback_enter.focus_in();
            if let Some(surface) = *sc.borrow() {
                unsafe { ghostty_surface_set_focus(surface, true) };
            }
        });
        focus_ctrl.connect_leave(move |_| {
            had_focus_leave.set(false);
            im_context_leave.focus_out();
            im_fallback_leave.focus_out();
            if let Some(surface) = *surface_cell.borrow() {
                unsafe { ghostty_surface_set_focus(surface, false) };
            }
        });
        controllers.borrow_mut().push(focus_ctrl.clone().upcast());
        gl_area.add_controller(focus_ctrl);
    }

    // Drops paste file paths, or save image data and paste its path (R5.2).
    {
        let drop_target = drop_target(surface_cell.clone());
        controllers.borrow_mut().push(drop_target.clone().upcast());
        gl_area.add_controller(drop_target);
    }

    // On unrealize: deinit GL resources but keep the surface alive.
    // GTK unrealizes widgets during reparenting (splits), and we need
    // the terminal/pty to survive. The GL resources will be recreated
    // in connect_realize when the widget is re-realized.
    {
        let surface_cell = surface_cell.clone();
        let display_realized = display_realized.clone();
        let handler = gl_area.connect_unrealize(move |gl_area| {
            let Some(surface) = *surface_cell.borrow() else {
                return;
            };
            // Unlike the realize path, this does not gate on
            // `gl_area.error()`. GTK destroys the context once these handlers
            // return, so staying realized after a failed `make_current` would
            // make the next realize suppress itself and leave the renderer
            // bound to a dead context.
            if display_realized.begin_unrealize() {
                gl_area.make_current();
                unsafe { ghostty_surface_display_unrealized(surface) };
            }
        });
        track_signal(&signal_handlers, &gl_area, handler);
    }

    // Explicit tab and pane teardown normally shuts down the surface first.
    // Keep widget destruction as a fallback for any future removal path.
    {
        let handle = handle.clone();
        root.connect_destroy(move |_| {
            handle.shutdown();
        });
    }

    TerminalWidget {
        root: root.upcast(),
        handle,
    }
}

fn mouse_button(gtk_button: u32) -> c_int {
    match gtk_button {
        1 => GHOSTTY_MOUSE_LEFT,
        2 => GHOSTTY_MOUSE_MIDDLE,
        3 => GHOSTTY_MOUSE_RIGHT,
        _ => GHOSTTY_MOUSE_UNKNOWN,
    }
}

pub(super) fn surface_action(surface: Option<ghostty_surface_t>, action: &str) {
    if let Some(surface) = surface {
        unsafe {
            ghostty_surface_binding_action(surface, action.as_ptr() as *const c_char, action.len());
        }
    }
}
