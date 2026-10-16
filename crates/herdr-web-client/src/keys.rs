//! DOM keys as GPUI keystroke names. Adapted from GPUI's own browser
//! platform (`gpui_web`, Apache-2.0), with Option chords named by the
//! physical key as macOS names them.
use herdr_web::wire::Mods;
use web_sys::KeyboardEvent;

/// Both sets of modifiers held at once.
pub(crate) fn merge(left: Mods, right: Mods) -> Mods {
    Mods {
        control: left.control || right.control,
        alt: left.alt || right.alt,
        shift: left.shift || right.shift,
        platform: left.platform || right.platform,
    }
}

pub(crate) fn mods(event: &KeyboardEvent) -> Mods {
    Mods {
        control: event.ctrl_key(),
        alt: event.alt_key(),
        shift: event.shift_key(),
        platform: event.meta_key(),
    }
}

pub(crate) fn is_modifier(key: &str) -> bool {
    matches!(
        key,
        "control" | "alt" | "shift" | "platform" | "capslock" | "compose" | "process" | "fn"
    )
}

/// The key's GPUI name: `enter`, `left`, `f5`, or the character in lower case.
pub(crate) fn key(event: &KeyboardEvent) -> String {
    let key = event.key();
    let named = match key.as_str() {
        "Enter" => "enter",
        "Backspace" => "backspace",
        "Tab" => "tab",
        "Escape" => "escape",
        "Delete" => "delete",
        " " => "space",
        "ArrowLeft" => "left",
        "ArrowRight" => "right",
        "ArrowUp" => "up",
        "ArrowDown" => "down",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pageup",
        "PageDown" => "pagedown",
        "Insert" => "insert",
        "Control" => "control",
        "Alt" => "alt",
        "Shift" => "shift",
        "Meta" => "platform",
        "CapsLock" => "capslock",
        "Fn" => "fn",
        "Process" | "Unidentified" | "Dead" => "",
        _ => {
            if let Some(number) = key
                .strip_prefix('F')
                .and_then(|rest| rest.parse::<u8>().ok())
                && (1..=35).contains(&number)
            {
                return format!("f{number}");
            }
            // Option composes a character; bindings name the key under it.
            if event.alt_key()
                && let Some(physical) = physical(&event.code())
            {
                return physical;
            }
            return key.to_lowercase();
        }
    };
    if named.is_empty() {
        // Dead keys and IME keys: the physical key is the best name.
        return physical(&event.code()).unwrap_or_default();
    }
    named.to_owned()
}

/// `KeyA` → `a`, `Digit1` → `1`, and the punctuation keys of a US layout.
fn physical(code: &str) -> Option<String> {
    if let Some(letter) = code.strip_prefix("Key") {
        return Some(letter.to_lowercase());
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return Some(digit.to_owned());
    }
    let symbol = match code {
        "Minus" => "-",
        "Equal" => "=",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Backquote" => "`",
        _ => return None,
    };
    Some(symbol.to_owned())
}

/// The text a key press types, if any. Command and Control chords type
/// nothing; Option composes on macOS, so its character counts.
pub(crate) fn key_char(event: &KeyboardEvent, key: &str, mods: Mods) -> Option<String> {
    if mods.platform || mods.control || is_modifier(key) {
        return None;
    }
    if key == "space" {
        return Some(" ".to_owned());
    }
    let raw = event.key();
    (raw.chars().count() == 1).then_some(raw)
}

/// Cmd-[ is Back and Cmd-] is Forward in the browser: `Some(true)` for Back.
pub(crate) fn history_step(key: &str, mods: Mods) -> Option<bool> {
    if !mods.platform || mods.control || mods.alt || mods.shift {
        return None;
    }
    match key {
        "[" => Some(true),
        "]" => Some(false),
        _ => None,
    }
}

/// Keys the browser keeps: the developer tools stay reachable.
pub(crate) fn browser_keeps(key: &str, mods: Mods) -> bool {
    key == "f12" || (mods.platform && mods.alt && matches!(key, "i" | "j" | "c"))
}
