// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae

//! Ghostty runtime callbacks (C ABI) and mouse-cursor sync.

use super::*;

pub(super) fn gtk_cursor_name_for_ghostty_shape(
    shape: ghostty_action_mouse_shape_e,
) -> &'static str {
    match shape {
        GHOSTTY_MOUSE_SHAPE_DEFAULT => "default",
        GHOSTTY_MOUSE_SHAPE_CONTEXT_MENU => "context-menu",
        GHOSTTY_MOUSE_SHAPE_HELP => "help",
        GHOSTTY_MOUSE_SHAPE_POINTER => "pointer",
        GHOSTTY_MOUSE_SHAPE_PROGRESS => "progress",
        GHOSTTY_MOUSE_SHAPE_WAIT => "wait",
        GHOSTTY_MOUSE_SHAPE_CELL => "cell",
        GHOSTTY_MOUSE_SHAPE_CROSSHAIR => "crosshair",
        GHOSTTY_MOUSE_SHAPE_TEXT => "text",
        GHOSTTY_MOUSE_SHAPE_VERTICAL_TEXT => "vertical-text",
        GHOSTTY_MOUSE_SHAPE_ALIAS => "alias",
        GHOSTTY_MOUSE_SHAPE_COPY => "copy",
        GHOSTTY_MOUSE_SHAPE_MOVE => "move",
        GHOSTTY_MOUSE_SHAPE_NO_DROP => "no-drop",
        GHOSTTY_MOUSE_SHAPE_NOT_ALLOWED => "not-allowed",
        GHOSTTY_MOUSE_SHAPE_GRAB => "grab",
        GHOSTTY_MOUSE_SHAPE_GRABBING => "grabbing",
        GHOSTTY_MOUSE_SHAPE_ALL_SCROLL => "all-scroll",
        GHOSTTY_MOUSE_SHAPE_COL_RESIZE => "col-resize",
        GHOSTTY_MOUSE_SHAPE_ROW_RESIZE => "row-resize",
        GHOSTTY_MOUSE_SHAPE_N_RESIZE => "n-resize",
        GHOSTTY_MOUSE_SHAPE_E_RESIZE => "e-resize",
        GHOSTTY_MOUSE_SHAPE_S_RESIZE => "s-resize",
        GHOSTTY_MOUSE_SHAPE_W_RESIZE => "w-resize",
        GHOSTTY_MOUSE_SHAPE_NE_RESIZE => "ne-resize",
        GHOSTTY_MOUSE_SHAPE_NW_RESIZE => "nw-resize",
        GHOSTTY_MOUSE_SHAPE_SE_RESIZE => "se-resize",
        GHOSTTY_MOUSE_SHAPE_SW_RESIZE => "sw-resize",
        GHOSTTY_MOUSE_SHAPE_EW_RESIZE => "ew-resize",
        GHOSTTY_MOUSE_SHAPE_NS_RESIZE => "ns-resize",
        GHOSTTY_MOUSE_SHAPE_NESW_RESIZE => "nesw-resize",
        GHOSTTY_MOUSE_SHAPE_NWSE_RESIZE => "nwse-resize",
        GHOSTTY_MOUSE_SHAPE_ZOOM_IN => "zoom-in",
        GHOSTTY_MOUSE_SHAPE_ZOOM_OUT => "zoom-out",
        _ => "default",
    }
}

pub(super) fn set_gtk_mouse_cursor(widget: &impl IsA<gtk::Widget>, state: MouseCursorState) {
    widget.set_cursor_from_name(Some(state.gtk_cursor_name()));
}

pub(super) fn apply_mouse_cursor_update(identity: SurfaceIdentity, update: MouseCursorUpdate) {
    SURFACE_MAP.with(|map| {
        let map = map.borrow();
        let Some(entry) = map.get(&identity.surface_key) else {
            return;
        };
        if entry.identity != identity {
            return;
        }
        let state = match update {
            MouseCursorUpdate::Shape(shape) => entry.mouse_cursor.get().update_shape(shape),
            MouseCursorUpdate::Visibility(visibility) => {
                entry.mouse_cursor.get().update_visibility(visibility)
            }
        };
        entry.mouse_cursor.set(state);
        set_gtk_mouse_cursor(&entry.gl_area, state);
    });
}

pub(super) fn dispatch_mouse_cursor_update(surface_key: usize, update: MouseCursorUpdate) {
    let Some(identity) = current_surface_identity(surface_key) else {
        return;
    };

    if glib::MainContext::default().is_owner() {
        apply_mouse_cursor_update(identity, update);
    } else {
        // Capture copied payload and surface generation. GTK access happens
        // later, only if this pointer still identifies the same surface.
        glib::idle_add_once(move || apply_mouse_cursor_update(identity, update));
    }
}

// ---------------------------------------------------------------------------
// Runtime callbacks (C ABI)
// ---------------------------------------------------------------------------

pub(super) fn claim_wakeup_idle_slot(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::AcqRel)
}

pub(super) fn release_wakeup_idle_slot(flag: &AtomicBool) {
    flag.store(false, Ordering::Release);
}

pub(super) unsafe extern "C" fn ghostty_wakeup_cb(_userdata: *mut c_void) {
    // Collapse renderer wakeups to a single pending idle source so text floods
    // do not enqueue unbounded GTK callbacks on the main thread.
    if claim_wakeup_idle_slot(&WAKEUP_IDLE_QUEUED) {
        glib::idle_add_once(|| {
            release_wakeup_idle_slot(&WAKEUP_IDLE_QUEUED);
            let app = ghostty_app();
            unsafe { ghostty_app_tick(app) };
        });
    }
    glib::MainContext::default().wakeup();
}

pub(super) unsafe extern "C" fn ghostty_action_cb(
    app: ghostty_app_t,
    target: ghostty_target_s,
    action: ghostty_action_s,
) -> bool {
    let tag = action.tag;

    match tag {
        GHOSTTY_ACTION_SCROLLBAR => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let scrollbar = unsafe { action.action.scrollbar };
                SURFACE_MAP.with(|map| {
                    if let Some(entry) = map.borrow().get(&surface_key) {
                        entry.scrollbar_syncing.set(true);
                        entry.scrollbar_adjustment.configure(
                            scrollbar.offset as f64,
                            0.0,
                            scrollbar.total as f64,
                            1.0,
                            scrollbar.len as f64,
                            scrollbar.len as f64,
                        );
                        entry.scrollbar_syncing.set(false);
                        entry.scrollbar.set_visible(
                            CURRENT_SCROLLBAR_ENABLED.load(Ordering::Relaxed)
                                && scrollbar.total > scrollbar.len,
                        );
                    }
                });
            }
            true
        }
        GHOSTTY_ACTION_RENDER => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                SURFACE_MAP.with(|map| {
                    if let Some(entry) = map.borrow().get(&surface_key) {
                        entry.gl_area.queue_render();
                    }
                });
            }
            true
        }
        GHOSTTY_ACTION_SET_TITLE => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let title_ptr = unsafe { action.action.set_title.title };
                if !title_ptr.is_null() {
                    let title = unsafe { std::ffi::CStr::from_ptr(title_ptr) }
                        .to_str()
                        .unwrap_or("")
                        .to_string();
                    SURFACE_MAP.with(|map| {
                        if let Some(entry) = map.borrow().get(&surface_key) {
                            if let Some(cb) = &entry.on_title_changed {
                                cb(&title);
                            }
                        }
                    });
                }
            }
            true
        }
        GHOSTTY_ACTION_DESKTOP_NOTIFICATION => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let title_ptr = unsafe { action.action.desktop_notification.title };
                let body_ptr = unsafe { action.action.desktop_notification.body };
                let title = if title_ptr.is_null() {
                    String::new()
                } else {
                    unsafe { std::ffi::CStr::from_ptr(title_ptr) }
                        .to_str()
                        .unwrap_or("")
                        .to_string()
                };
                let body = if body_ptr.is_null() {
                    String::new()
                } else {
                    unsafe { std::ffi::CStr::from_ptr(body_ptr) }
                        .to_str()
                        .unwrap_or("")
                        .to_string()
                };
                SURFACE_MAP.with(|map| {
                    if let Some(entry) = map.borrow().get(&surface_key) {
                        if let Some(cb) = &entry.on_desktop_notification {
                            cb(&title, &body, entry.gl_area.is_focus());
                        }
                    }
                });
            }
            true
        }
        GHOSTTY_ACTION_PWD => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let pwd_ptr = unsafe { action.action.pwd.pwd };
                if !pwd_ptr.is_null() {
                    let pwd = unsafe { std::ffi::CStr::from_ptr(pwd_ptr) }
                        .to_str()
                        .unwrap_or("")
                        .to_string();
                    SURFACE_MAP.with(|map| {
                        if let Some(entry) = map.borrow().get(&surface_key) {
                            if let Some(cb) = &entry.on_pwd_changed {
                                cb(&pwd);
                            }
                        }
                    });
                }
            }
            true
        }
        // RADE (R7.1): Ctrl+click on a link. The URL is Ghostty's matched
        // text (a path it resolved against its pwd if that exists), or an
        // OSC 8 URI; the app decides between Viewer and browser.
        GHOSTTY_ACTION_OPEN_URL => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let open = unsafe { action.action.open_url };
                if !open.url.is_null() && open.len > 0 {
                    super::links::mark_opened();
                    let bytes =
                        unsafe { std::slice::from_raw_parts(open.url.cast::<u8>(), open.len) };
                    let url = String::from_utf8_lossy(bytes).into_owned();
                    glib::idle_add_local_once(move || {
                        SURFACE_MAP.with(|map| {
                            if let Some(cb) = map
                                .borrow()
                                .get(&surface_key)
                                .and_then(|e| e.on_open_url.as_ref())
                            {
                                cb(&url);
                            }
                        });
                    });
                }
            }
            true
        }
        GHOSTTY_ACTION_MOUSE_SHAPE => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let shape = unsafe { action.action.mouse_shape };
                dispatch_mouse_cursor_update(surface_key, MouseCursorUpdate::Shape(shape));
            }
            true
        }
        GHOSTTY_ACTION_MOUSE_VISIBILITY => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let visibility = unsafe { action.action.mouse_visibility };
                dispatch_mouse_cursor_update(
                    surface_key,
                    MouseCursorUpdate::Visibility(visibility),
                );
            }
            true
        }
        GHOSTTY_ACTION_RING_BELL => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                SURFACE_MAP.with(|map| {
                    if let Some(entry) = map.borrow().get(&surface_key) {
                        if let Some(cb) = &entry.on_bell {
                            cb(entry.gl_area.is_focus());
                        }
                    }
                });
            }
            true
        }
        // RADE (R10.2): the shell's foreground command ended (OSC 133 D).
        // Mid-tick: report from the main loop.
        GHOSTTY_ACTION_COMMAND_FINISHED => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                glib::idle_add_local_once(move || {
                    SURFACE_MAP.with(|map| {
                        if let Some(cb) = map
                            .borrow()
                            .get(&surface_key)
                            .and_then(|e| e.on_command_finished.as_ref())
                        {
                            cb();
                        }
                    });
                });
            }
            true
        }
        // RADE: report the exit, then let Ghostty print its own "Process
        // exited" line (return false). With `wait_after_command` the
        // surface stays until a key press closes it (close_surface_cb);
        // otherwise Ghostty closes it right away.
        GHOSTTY_ACTION_SHOW_CHILD_EXITED => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let code = unsafe { action.action.child_exited.exit_code };
                glib::idle_add_local_once(move || {
                    SURFACE_MAP.with(|map| {
                        if let Some(entry) = map.borrow().get(&surface_key) {
                            if let Some(cb) = &entry.on_child_exited {
                                cb(code);
                            }
                        }
                    });
                });
            }
            false
        }
        // RADE (R19.5): Ghostty's search, shown in the Terminal's search bar
        // (from the main loop: the surface map may be borrowed here).
        GHOSTTY_ACTION_START_SEARCH
        | GHOSTTY_ACTION_END_SEARCH
        | GHOSTTY_ACTION_SEARCH_TOTAL
        | GHOSTTY_ACTION_SEARCH_SELECTED => {
            if target.tag != GHOSTTY_TARGET_SURFACE {
                return false;
            }
            let surface_key = unsafe { target.target.surface } as usize;
            let needle = (tag == GHOSTTY_ACTION_START_SEARCH)
                .then_some(unsafe { action.action.start_search.needle })
                .filter(|p| !p.is_null())
                .map(|p| {
                    unsafe { std::ffi::CStr::from_ptr(p) }
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_default();
            let count = match tag {
                GHOSTTY_ACTION_SEARCH_TOTAL | GHOSTTY_ACTION_SEARCH_SELECTED => unsafe {
                    action.action.search_count
                },
                _ => -1,
            };
            glib::idle_add_local_once(move || {
                let bar = SURFACE_MAP
                    .with(|map| map.borrow().get(&surface_key).map(|e| e.search.clone()));
                let Some(bar) = bar else { return };
                match tag {
                    GHOSTTY_ACTION_START_SEARCH => bar.open(&needle),
                    GHOSTTY_ACTION_END_SEARCH => bar.ended(),
                    GHOSTTY_ACTION_SEARCH_TOTAL => bar.set_total(count),
                    _ => bar.set_selected(count),
                }
            });
            true
        }
        GHOSTTY_ACTION_RELOAD_CONFIG => {
            let config = load_ghostty_config();
            CURRENT_SCROLLBAR_ENABLED.store(load_scrollbar_enabled(config), Ordering::Relaxed);
            match target.tag {
                GHOSTTY_TARGET_APP => unsafe {
                    ghostty_app_update_config(app, config);
                },
                GHOSTTY_TARGET_SURFACE => {
                    let surface = unsafe { target.target.surface };
                    unsafe {
                        ghostty_surface_update_config(surface, config);
                    }
                }
                _ => {}
            }
            unsafe {
                ghostty_config_free(config);
            }
            true
        }
        _ => false,
    }
}

pub(super) unsafe extern "C" fn ghostty_close_surface_cb(
    userdata: *mut c_void,
    _process_alive: bool,
) {
    let Some(surface_key) =
        (unsafe { clipboard_surface_from_userdata(userdata) }).map(|surface| surface as usize)
    else {
        return;
    };
    glib::idle_add_local_once(move || {
        SURFACE_MAP.with(|map| {
            if let Some(entry) = map.borrow().get(&surface_key) {
                if let Some(cb) = &entry.on_close {
                    cb();
                }
            }
        });
    });
}
