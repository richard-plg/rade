// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae

//! Clipboard consent and the lifetime of Ghostty's pending C requests.

use super::*;
use rade_core::paste;

struct PendingRequest {
    identity: SurfaceIdentity,
    state: Option<*mut c_void>,
    cancellable: gtk::gio::Cancellable,
}

#[derive(Default)]
struct PendingRequests {
    next_id: u64,
    requests: HashMap<u64, PendingRequest>,
}

impl PendingRequests {
    fn insert(&mut self, request: PendingRequest) -> u64 {
        self.next_id += 1;
        self.requests.insert(self.next_id, request);
        self.next_id
    }

    fn take(&mut self, id: u64) -> Option<PendingRequest> {
        self.requests.remove(&id)
    }

    fn drain_surface(&mut self, surface_key: usize) -> Vec<PendingRequest> {
        let ids: Vec<_> = self
            .requests
            .iter()
            .filter(|(_, request)| request.identity.surface_key == surface_key)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter().filter_map(|id| self.take(id)).collect()
    }
}

thread_local! {
    static PENDING: RefCell<PendingRequests> = RefCell::new(PendingRequests::default());
}

fn track(identity: SurfaceIdentity, state: Option<*mut c_void>) -> (u64, gtk::gio::Cancellable) {
    let cancellable = gtk::gio::Cancellable::new();
    let id = PENDING.with(|pending| {
        pending.borrow_mut().insert(PendingRequest {
            identity,
            state,
            cancellable: cancellable.clone(),
        })
    });
    (id, cancellable)
}

fn take_current(id: u64) -> Option<PendingRequest> {
    let request = PENDING.with(|pending| pending.borrow_mut().take(id))?;
    (current_surface_identity(request.identity.surface_key) == Some(request.identity))
        .then_some(request)
}

// A confirmed empty completion is Ghostty's available cancellation mechanism:
// paste does nothing, and OSC 52 receives an empty reply. It also releases the
// C request allocation without prompting again. Never free that allocation here.
fn complete(request: PendingRequest, text: &std::ffi::CStr, confirmed: bool) {
    if let Some(state) = request.state {
        unsafe {
            ghostty_surface_complete_clipboard_request(
                request.identity.surface_key as ghostty_surface_t,
                text.as_ptr(),
                state,
                confirmed,
            );
        }
    }
}

pub(super) fn cancel_surface_requests(surface_key: usize) {
    // Remove every request before cancellation can schedule its async callback.
    // The caller still owns the live Ghostty surface until this returns.
    let requests = PENDING.with(|pending| pending.borrow_mut().drain_surface(surface_key));
    for request in requests {
        request.cancellable.cancel();
        complete(request, c"", true);
    }
}

unsafe fn identity_from_userdata(userdata: *mut c_void) -> Option<SurfaceIdentity> {
    let surface = unsafe { clipboard_surface_from_userdata(userdata) }?;
    current_surface_identity(surface as usize)
}

pub(super) unsafe extern "C" fn ghostty_read_clipboard_cb(
    userdata: *mut c_void,
    clipboard_type: c_int,
    state: *mut c_void,
) -> bool {
    let Some(identity) = (unsafe { identity_from_userdata(userdata) }) else {
        return false;
    };
    let Some(display) = gtk::gdk::Display::default() else {
        return false;
    };
    let clipboard = clipboard_from_type(&display, clipboard_type);
    let formats = clipboard.formats();
    let mut sources = paste::clip_sources(
        formats.mime_types().iter().map(|m| m.as_str()),
        formats.contains_type(gtk::gdk::Texture::static_type()),
    );
    // Middle-click (primary selection) pastes text only, as before (R5).
    if clipboard_type == GHOSTTY_CLIPBOARD_SELECTION {
        sources.retain(|s| *s == paste::Clip::Text);
    }
    if sources.is_empty() {
        return false;
    }

    let (id, _) = track(identity, Some(state));
    glib::spawn_future_local(async move {
        let text = clipboard_paste_text(&clipboard, &sources).await;
        // The Pane may have closed meanwhile: its request is already completed.
        let Some(request) = take_current(id) else {
            return;
        };
        match text {
            // Let Ghostty enforce clipboard-read and paste-protection policy.
            // It may synchronously transfer this state to the confirm callback.
            Some(text) => complete(request, &clipboard_read_text_cstring(Some(&text)), false),
            None => complete(request, c"", true),
        }
    });
    true
}

/// The clipboard as pasted text (R5.1): copied files → their escaped paths,
/// else text, else an image saved as PNG → its path. Ghostty then pastes it
/// like any text (bracketed when the program asked for it).
async fn clipboard_paste_text(
    clipboard: &gtk::gdk::Clipboard,
    sources: &[paste::Clip],
) -> Option<String> {
    for source in sources {
        let text = match source {
            paste::Clip::Text => clipboard
                .read_text_future()
                .await
                .ok()
                .flatten()
                .map(String::from),
            paste::Clip::Files => read_all(clipboard, "text/uri-list")
                .await
                .and_then(|t| paste::escape_paths(&paste::parse_uri_list(&t))),
            paste::Clip::Image => match clipboard.read_texture_future().await {
                Ok(Some(texture)) => save_texture(&texture),
                _ => None,
            },
        };
        if text.as_deref().is_some_and(|t| !t.is_empty()) {
            return text;
        }
    }
    None
}

async fn read_all(clipboard: &gtk::gdk::Clipboard, mime: &str) -> Option<String> {
    let (stream, _) = clipboard
        .read_future(&[mime], glib::Priority::DEFAULT)
        .await
        .ok()?;
    let mut out = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await
            .ok()?;
        if chunk.is_empty() {
            break;
        }
        out.extend_from_slice(&chunk);
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Save image data to the paste folder (ADR-0004); its escaped path.
// ponytail: PNG encoding runs on the UI thread (~tens of ms for a 4K
// screenshot); move to gio::spawn_blocking if pastes ever feel slow.
fn save_texture(texture: &gtk::gdk::Texture) -> Option<String> {
    let dir = rade_core::paths::paste_dir(&rade_core::paths::process_env);
    match paste::save(
        &dir,
        &texture.save_to_png_bytes(),
        std::time::SystemTime::now(),
    ) {
        Ok(path) => paste::escape_paths(&[path]),
        Err(e) => {
            eprintln!("rade: paste image to {}: {e}", dir.display());
            None
        }
    }
}

/// Text a drop pastes (R5.2): files → escaped paths, image data → saved PNG path.
pub(super) fn dropped_text(value: &glib::Value) -> Option<String> {
    if let Ok(files) = value.get::<gtk::gdk::FileList>() {
        let paths: Vec<_> = files.files().iter().filter_map(|f| f.path()).collect();
        return paste::escape_paths(&paths);
    }
    save_texture(&value.get::<gtk::gdk::Texture>().ok()?)
}

/// Drop target for a Terminal: files from a file manager or image data (a
/// browser) paste as paths. Image data is preferred: a browser's image drag
/// also offers a uri-list, of `https://` URLs; a file manager offers no image.
pub(super) fn drop_target(surface_cell: Rc<RefCell<Option<ghostty_surface_t>>>) -> gtk::DropTarget {
    let target = gtk::DropTarget::new(glib::Type::INVALID, gtk::gdk::DragAction::COPY);
    target.set_types(&[
        gtk::gdk::Texture::static_type(),
        gtk::gdk::FileList::static_type(),
    ]);
    target.connect_drop(move |_, value, _, _| {
        let Some(surface) = *surface_cell.borrow() else {
            return false;
        };
        let Some(text) = dropped_text(value) else {
            return false;
        };
        unsafe { ghostty_surface_text(surface, text.as_ptr() as *const c_char, text.len()) };
        true
    });
    target
}

fn consent_text(text: &std::ffi::CStr, choice: Option<i32>) -> &std::ffi::CStr {
    if choice == Some(1) {
        text
    } else {
        c""
    }
}

fn ask_consent(
    identity: SurfaceIdentity,
    state: Option<*mut c_void>,
    message: &str,
    detail: &str,
    allow_label: &str,
    on_choice: impl FnOnce(PendingRequest, Option<i32>) + 'static,
) {
    let parent = SURFACE_MAP.with(|map| {
        map.borrow()
            .get(&identity.surface_key)
            .filter(|entry| entry.identity == identity)
            .and_then(|entry| entry.gl_area.root())
            .and_then(|root| root.downcast::<gtk::Window>().ok())
    });
    let (id, cancellable) = track(identity, state);
    let Some(parent) = parent else {
        if let Some(request) = take_current(id) {
            on_choice(request, None);
        }
        return;
    };
    let dialog = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons(["Cancel", allow_label])
        .default_button(0)
        .cancel_button(0)
        .modal(true)
        .build();
    dialog.choose(Some(&parent), Some(&cancellable), move |result| {
        if let Some(request) = take_current(id) {
            on_choice(request, result.ok());
        }
    });
}

pub(super) unsafe extern "C" fn ghostty_confirm_read_clipboard_cb(
    userdata: *mut c_void,
    text: *const c_char,
    state: *mut c_void,
    request_type: c_int,
) {
    let Some(identity) = (unsafe { identity_from_userdata(userdata) }) else {
        return;
    };
    // Ghostty only borrows this string for the duration of this callback.
    let text = unsafe { std::ffi::CStr::from_ptr(clipboard_completion_text_ptr(text)) }.to_owned();
    let (message, detail, allow_label) = match request_type {
        GHOSTTY_CLIPBOARD_REQUEST_PASTE => (
            "Paste potentially unsafe text?",
            "The clipboard contains text that may execute commands in this terminal.",
            "Paste",
        ),
        GHOSTTY_CLIPBOARD_REQUEST_OSC_52_READ => (
            "Allow this terminal to read the clipboard?",
            "A program in this terminal wants to read your clipboard contents.",
            "Allow Read",
        ),
        _ => {
            let (id, _) = track(identity, Some(state));
            if let Some(request) = take_current(id) {
                complete(request, c"", true);
            }
            return;
        }
    };
    ask_consent(
        identity,
        Some(state),
        message,
        detail,
        allow_label,
        move |request, choice| complete(request, consent_text(&text, choice), true),
    );
}

pub(super) unsafe extern "C" fn ghostty_write_clipboard_cb(
    userdata: *mut c_void,
    clipboard_type: c_int,
    contents: *const ghostty_clipboard_content_s,
    count: usize,
    confirm: bool,
) {
    if count == 0 || contents.is_null() {
        return;
    }
    let content = unsafe { &*contents };
    if content.data.is_null() {
        return;
    }
    let text = unsafe { std::ffi::CStr::from_ptr(content.data) }
        .to_str()
        .unwrap_or("")
        .to_string();
    let Some(context) = (unsafe { clipboard_context_from_userdata(userdata) }) else {
        return;
    };
    let Some(identity) = (unsafe { identity_from_userdata(userdata) }) else {
        return;
    };
    let policy = clipboard_write_policy(clipboard_type, (context.copy_selection_to_clipboard)());
    if confirm {
        ask_consent(
            identity,
            None,
            "Allow this terminal to replace the clipboard?",
            "A program in this terminal wants to overwrite your clipboard contents.",
            "Allow Write",
            move |request, choice| {
                if choice == Some(1) {
                    write_text(request.identity, &text, policy);
                }
            },
        );
    } else {
        write_text(identity, &text, policy);
    }
}

fn write_text(identity: SurfaceIdentity, text: &str, policy: ClipboardWritePolicy) {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    if policy.write_clipboard {
        display.clipboard().set_text(text);
    }
    if policy.write_primary {
        display.primary_clipboard().set_text(text);
    }
    if policy.show_toast {
        SURFACE_MAP.with(|map| {
            if let Some(entry) = map.borrow().get(&identity.surface_key) {
                show_clipboard_toast(&entry.toast_overlay);
            }
        });
    }
}

pub(super) unsafe fn clipboard_surface_from_userdata(
    userdata: *mut c_void,
) -> Option<ghostty_surface_t> {
    if userdata.is_null() {
        return None;
    }
    let context = unsafe { &*(userdata as *const ClipboardContext) };
    let surface = context.surface.get();
    if surface.is_null() {
        None
    } else {
        Some(surface)
    }
}

pub(super) unsafe fn clipboard_context_from_userdata(
    userdata: *mut c_void,
) -> Option<&'static ClipboardContext> {
    if userdata.is_null() {
        return None;
    }
    Some(unsafe { &*(userdata as *const ClipboardContext) })
}

pub(super) fn clipboard_read_text_cstring(text: Option<&str>) -> CString {
    CString::new(text.unwrap_or_default().replace('\0', ""))
        .expect("clipboard text should not contain NUL bytes")
}

pub(super) fn clipboard_completion_text_ptr(text: *const c_char) -> *const c_char {
    if text.is_null() {
        EMPTY_CLIPBOARD_TEXT.as_ptr().cast()
    } else {
        text
    }
}

pub(super) fn clipboard_from_type(
    display: &gtk::gdk::Display,
    clipboard_type: c_int,
) -> gtk::gdk::Clipboard {
    if clipboard_type == GHOSTTY_CLIPBOARD_SELECTION {
        display.primary_clipboard()
    } else {
        display.clipboard()
    }
}

pub(super) fn clipboard_write_policy(
    clipboard_type: c_int,
    copy_selection_to_clipboard: bool,
) -> ClipboardWritePolicy {
    if clipboard_type == GHOSTTY_CLIPBOARD_SELECTION {
        ClipboardWritePolicy {
            write_clipboard: copy_selection_to_clipboard,
            write_primary: true,
            show_toast: copy_selection_to_clipboard,
        }
    } else {
        ClipboardWritePolicy {
            write_clipboard: true,
            write_primary: true,
            show_toast: true,
        }
    }
}

pub(super) fn show_clipboard_toast(overlay: &gtk::Overlay) {
    let toast = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    toast.set_halign(gtk::Align::Center);
    toast.set_valign(gtk::Align::End);
    toast.set_margin_bottom(12);

    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "box.rade-toast { \
            background: rgba(45, 45, 45, 0.95); \
            color: white; \
            border-radius: 6px; \
            padding: 6px 14px; \
            font-size: 12px; \
        } \
        box.rade-toast label { color: white; } \
        box.rade-toast button { \
            color: rgba(255,255,255,0.5); \
            border: none; \
            background: none; \
            min-height: 0; min-width: 0; \
            padding: 0 2px; \
        } \
        box.rade-toast button:hover { color: white; }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    toast.add_css_class("rade-toast");
    let label = gtk::Label::new(Some("Copied to clipboard"));
    let close_btn = gtk::Button::with_label("\u{00D7}"); // ×
    toast.append(&label);
    toast.append(&close_btn);
    toast.set_can_target(false);

    overlay.add_overlay(&toast);

    // Close button dismisses immediately
    {
        let t = toast.clone();
        let o = overlay.clone();
        close_btn.set_can_target(true);
        close_btn.connect_clicked(move |_| {
            o.remove_overlay(&t);
        });
    }

    // Auto-dismiss after 2 seconds
    {
        let t = toast.clone();
        let o = overlay.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
            if t.parent().is_some() {
                o.remove_overlay(&t);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_consent_releases_clipboard_text() {
        for choice in [None, Some(0), Some(-1), Some(2)] {
            assert_eq!(consent_text(c"secret\ncommand", choice), c"");
        }
        assert_eq!(
            consent_text(c"secret\ncommand", Some(1)),
            c"secret\ncommand"
        );
    }

    fn request(identity: SurfaceIdentity) -> PendingRequest {
        PendingRequest {
            identity,
            state: Some(std::ptr::dangling_mut()),
            cancellable: gtk::gio::Cancellable::new(),
        }
    }

    #[test]
    fn teardown_takes_pending_requests_once_and_preserves_other_surfaces() {
        let mut identities = SurfaceIdentityRegistry::default();
        let old_surface = identities.register(10);
        let other_surface = identities.register(20);
        let mut pending = PendingRequests::default();
        let read = pending.insert(request(old_surface));
        let prompt = pending.insert(request(old_surface));
        let other = pending.insert(request(other_surface));

        assert_eq!(pending.drain_surface(10).len(), 2);
        assert!(pending.take(read).is_none());
        assert!(pending.take(prompt).is_none());
        assert!(pending.drain_surface(10).is_empty());
        assert!(pending.take(other).is_some());
        assert!(pending.take(other).is_none());

        identities.unregister(old_surface);
        let replacement = identities.register(10);
        assert_ne!(old_surface, replacement);
        let new_request = pending.insert(request(replacement));
        assert_ne!(read, new_request);
        assert!(pending.take(read).is_none());
        assert_eq!(pending.take(new_request).unwrap().identity, replacement);
    }

    #[test]
    fn cancelled_prompt_cannot_complete_later() {
        let identity = register_surface_identity(0xc11b_0001);
        let (id, cancellable) = track(identity, None);
        cancel_surface_requests(identity.surface_key);
        assert!(cancellable.is_cancelled());
        assert!(take_current(id).is_none());
        cancel_surface_requests(identity.surface_key);
        unregister_surface_identity(identity);
    }

    #[test]
    fn late_callback_cannot_target_a_reused_surface_address() {
        let old = register_surface_identity(0xc11b_0002);
        let (id, _) = track(old, None);
        unregister_surface_identity(old);
        let replacement = register_surface_identity(old.surface_key);

        assert!(take_current(id).is_none());
        let (new_id, _) = track(replacement, None);
        assert_eq!(take_current(new_id).unwrap().identity, replacement);
        assert!(take_current(new_id).is_none());
        unregister_surface_identity(replacement);
    }

    #[test]
    fn read_can_transfer_c_state_to_confirmation_without_double_completion() {
        let identity = register_surface_identity(0xc11b_0003);
        let state = std::ptr::dangling_mut();
        let (read_id, _) = track(identity, Some(state));

        // An unconfirmed completion removes the read before Ghostty calls the
        // confirmation callback synchronously with the same opaque C state.
        let read = take_current(read_id).unwrap();
        let (prompt_id, _) = track(read.identity, read.state);
        assert!(take_current(read_id).is_none());

        // Closing the surface must reclaim exactly the confirmation's state.
        let drained =
            PENDING.with(|pending| pending.borrow_mut().drain_surface(identity.surface_key));
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].state, Some(state));
        assert!(take_current(prompt_id).is_none());
        assert!(take_current(read_id).is_none());
        unregister_surface_identity(identity);
    }
}
