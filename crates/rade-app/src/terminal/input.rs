// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae

//! Key, mouse, and drop translation to Ghostty input.

use super::*;

// ---------------------------------------------------------------------------
// Key translation
// ---------------------------------------------------------------------------

pub(super) fn translate_key_event(
    action: c_int,
    widget: Option<&gtk::Widget>,
    key_event: Option<&gtk::gdk::KeyEvent>,
    keyval: gtk::gdk::Key,
    keycode: u32,
    modifier: gtk::gdk::ModifierType,
) -> ghostty_input_key_s {
    let mut mods: c_int = GHOSTTY_MODS_NONE;
    if modifier.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        mods |= GHOSTTY_MODS_SHIFT;
    }
    if modifier.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
        mods |= GHOSTTY_MODS_CTRL;
    }
    if modifier.contains(gtk::gdk::ModifierType::ALT_MASK) {
        mods |= GHOSTTY_MODS_ALT;
    }
    if modifier.contains(gtk::gdk::ModifierType::SUPER_MASK) {
        mods |= GHOSTTY_MODS_SUPER;
    }

    let unshifted = widget
        .zip(key_event)
        .and_then(|(widget, key_event)| keyval_unicode_unshifted(widget, key_event, keycode))
        .unwrap_or_else(|| fallback_unshifted_codepoint(keyval));

    let consumed = key_event
        .map(translate_consumed_mods)
        .unwrap_or_else(|| fallback_consumed_mods(keyval, modifier));
    let keycode = ghostty_keycode_with_keyval_remap(keyval, keycode);

    ghostty_input_key_s {
        action,
        mods,
        consumed_mods: consumed,
        keycode,
        text: ptr::null(),
        unshifted_codepoint: unshifted,
        composing: false,
        key: ghostty_key_for_keyval(keyval),
    }
}

pub(super) fn ghostty_keycode_with_keyval_remap(keyval: gtk::gdk::Key, keycode: u32) -> u32 {
    const XKB_KEYCODE_ESCAPE: u32 = 9;
    const XKB_KEYCODE_CAPS_LOCK: u32 = 66;

    match keyval {
        gtk::gdk::Key::Escape => XKB_KEYCODE_ESCAPE,
        gtk::gdk::Key::Caps_Lock => XKB_KEYCODE_CAPS_LOCK,
        _ => keycode,
    }
}

/// Keypad keyvals as Ghostty keys, matching Ghostty's GTK apprt.
///
/// Embedded Ghostty derives its key from the XKB keycode, so with Num Lock off
/// keypad 1 (`KP_End`) would still resolve to `numpad_1`. Passing the keyval's
/// key keeps keypad identity (`numpad_end`) for `kp_*` keybinds and Kitty
/// keyboard sequences while still producing navigation.
pub(super) fn ghostty_key_for_keyval(keyval: gtk::gdk::Key) -> c_int {
    use gtk::gdk::Key;

    match keyval {
        Key::KP_0 => GHOSTTY_KEY_NUMPAD_0,
        Key::KP_1 => GHOSTTY_KEY_NUMPAD_1,
        Key::KP_2 => GHOSTTY_KEY_NUMPAD_2,
        Key::KP_3 => GHOSTTY_KEY_NUMPAD_3,
        Key::KP_4 => GHOSTTY_KEY_NUMPAD_4,
        Key::KP_5 => GHOSTTY_KEY_NUMPAD_5,
        Key::KP_6 => GHOSTTY_KEY_NUMPAD_6,
        Key::KP_7 => GHOSTTY_KEY_NUMPAD_7,
        Key::KP_8 => GHOSTTY_KEY_NUMPAD_8,
        Key::KP_9 => GHOSTTY_KEY_NUMPAD_9,
        Key::KP_Decimal => GHOSTTY_KEY_NUMPAD_DECIMAL,
        Key::KP_Divide => GHOSTTY_KEY_NUMPAD_DIVIDE,
        Key::KP_Multiply => GHOSTTY_KEY_NUMPAD_MULTIPLY,
        Key::KP_Subtract => GHOSTTY_KEY_NUMPAD_SUBTRACT,
        Key::KP_Add => GHOSTTY_KEY_NUMPAD_ADD,
        Key::KP_Enter => GHOSTTY_KEY_NUMPAD_ENTER,
        Key::KP_Equal => GHOSTTY_KEY_NUMPAD_EQUAL,
        Key::KP_Separator => GHOSTTY_KEY_NUMPAD_SEPARATOR,
        Key::KP_Left => GHOSTTY_KEY_NUMPAD_LEFT,
        Key::KP_Right => GHOSTTY_KEY_NUMPAD_RIGHT,
        Key::KP_Up => GHOSTTY_KEY_NUMPAD_UP,
        Key::KP_Down => GHOSTTY_KEY_NUMPAD_DOWN,
        Key::KP_Page_Up => GHOSTTY_KEY_NUMPAD_PAGE_UP,
        Key::KP_Page_Down => GHOSTTY_KEY_NUMPAD_PAGE_DOWN,
        Key::KP_Home => GHOSTTY_KEY_NUMPAD_HOME,
        Key::KP_End => GHOSTTY_KEY_NUMPAD_END,
        Key::KP_Insert => GHOSTTY_KEY_NUMPAD_INSERT,
        Key::KP_Delete => GHOSTTY_KEY_NUMPAD_DELETE,
        Key::KP_Begin => GHOSTTY_KEY_NUMPAD_BEGIN,
        _ => GHOSTTY_KEY_UNIDENTIFIED,
    }
}

pub(super) fn key_event_text(keyval: gtk::gdk::Key) -> Option<CString> {
    let ch = keyval.to_unicode()?;
    if ch.is_control() {
        return None;
    }

    let mut buf = [0u8; 4];
    let s = ch.encode_utf8(&mut buf);
    CString::new(s.as_bytes()).ok()
}

pub(super) fn keyval_unicode_unshifted(
    widget: &gtk::Widget,
    key_event: &gtk::gdk::KeyEvent,
    keycode: u32,
) -> Option<u32> {
    widget
        .display()
        .map_keycode(keycode)
        .and_then(|entries| {
            entries
                .into_iter()
                .find(|(keymap_key, _)| {
                    keymap_key.group() == key_event.layout() as i32 && keymap_key.level() == 0
                })
                .and_then(|(_, key)| key.to_unicode())
        })
        .map(|ch| ch as u32)
        .filter(|codepoint| *codepoint != 0)
}

pub(super) fn translate_consumed_mods(key_event: &gtk::gdk::KeyEvent) -> c_int {
    let consumed = key_event.consumed_modifiers() & gtk::gdk::MODIFIER_MASK;
    translate_mouse_mods(consumed)
}

pub(super) fn fallback_consumed_mods(
    keyval: gtk::gdk::Key,
    modifier: gtk::gdk::ModifierType,
) -> c_int {
    let mut consumed: c_int = GHOSTTY_MODS_NONE;
    if modifier.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        let shifted = keyval.to_unicode().map(|c| c as u32).unwrap_or(0);
        let unshifted = fallback_unshifted_codepoint(keyval);
        if shifted != 0 && shifted != unshifted {
            consumed |= GHOSTTY_MODS_SHIFT;
        }
    }
    consumed
}

pub(super) fn fallback_unshifted_codepoint(keyval: gtk::gdk::Key) -> u32 {
    match keyval.to_unicode() {
        Some('!') => '1' as u32,
        Some('@') => '2' as u32,
        Some('#') => '3' as u32,
        Some('$') => '4' as u32,
        Some('%') => '5' as u32,
        Some('^') => '6' as u32,
        Some('&') => '7' as u32,
        Some('*') => '8' as u32,
        Some('(') => '9' as u32,
        Some(')') => '0' as u32,
        Some('_') => '-' as u32,
        Some('+') => '=' as u32,
        Some('{') => '[' as u32,
        Some('}') => ']' as u32,
        Some('|') => '\\' as u32,
        Some(':') => ';' as u32,
        Some('"') => '\'' as u32,
        Some('<') => ',' as u32,
        Some('>') => '.' as u32,
        Some('?') => '/' as u32,
        Some('~') => '`' as u32,
        Some(ch) => ch.to_lowercase().next().map(|c| c as u32).unwrap_or(0),
        None => 0,
    }
}

pub(super) fn translate_mouse_mods(state: gtk::gdk::ModifierType) -> c_int {
    let mut mods: c_int = GHOSTTY_MODS_NONE;
    if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        mods |= GHOSTTY_MODS_SHIFT;
    }
    if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
        mods |= GHOSTTY_MODS_CTRL;
    }
    if state.contains(gtk::gdk::ModifierType::ALT_MASK) {
        mods |= GHOSTTY_MODS_ALT;
    }
    if state.contains(gtk::gdk::ModifierType::SUPER_MASK) {
        mods |= GHOSTTY_MODS_SUPER;
    }
    mods
}
