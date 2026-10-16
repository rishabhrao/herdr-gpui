//! A page's input as DevTools commands, and pictures as bytes.
use crate::wire::{Button, Mods, MouseKind, StreamInput};
use base64::Engine as _;
use serde_json::{Value, json};

/// The bit mask DevTools takes for held modifiers.
pub(super) fn modifiers(mods: Mods) -> u8 {
    u8::from(mods.alt)
        | u8::from(mods.control) << 1
        | u8::from(mods.platform) << 2
        | u8::from(mods.shift) << 3
}

fn button(button: Option<Button>) -> &'static str {
    match button {
        Some(Button::Left) => "left",
        Some(Button::Middle) => "middle",
        Some(Button::Right) => "right",
        Some(Button::Back) => "back",
        Some(Button::Forward) => "forward",
        None => "none",
    }
}

/// The Windows virtual key code DevTools needs for keys that act rather
/// than type, such as Enter and the arrows.
pub(super) fn key_code(key: &str) -> u32 {
    match key {
        "Backspace" => 8,
        "Tab" => 9,
        "Enter" => 13,
        "Escape" => 27,
        " " => 32,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Delete" => 46,
        key => match key.chars().next() {
            Some(character) if key.chars().count() == 1 && character.is_ascii_alphanumeric() => {
                u32::from(character.to_ascii_uppercase())
            }
            _ => 0,
        },
    }
}

/// The DevTools commands for one input.
pub(super) fn commands(input: &StreamInput) -> Vec<(&'static str, Value)> {
    match input {
        StreamInput::Mouse {
            kind,
            x,
            y,
            button: pressed,
            clicks,
            mods,
        } => {
            let kind = match kind {
                MouseKind::Down => "mousePressed",
                MouseKind::Up => "mouseReleased",
                MouseKind::Move => "mouseMoved",
            };
            vec![(
                "Input.dispatchMouseEvent",
                json!({
                    "type": kind,
                    "x": x,
                    "y": y,
                    "button": button(*pressed),
                    "clickCount": clicks,
                    "modifiers": modifiers(*mods),
                }),
            )]
        }
        StreamInput::Wheel { x, y, dx, dy } => vec![(
            "Input.dispatchMouseEvent",
            json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy }),
        )],
        StreamInput::Key {
            down,
            key,
            code,
            text,
            mods,
        } => {
            // A key that types needs its text; Enter types a return.
            let text = text
                .clone()
                .or_else(|| (key == "Enter").then(|| "\r".to_owned()));
            let kind = match (down, &text) {
                (false, _) => "keyUp",
                (true, Some(_)) => "keyDown",
                (true, None) => "rawKeyDown",
            };
            let mut params = json!({
                "type": kind,
                "key": key,
                "code": code,
                "windowsVirtualKeyCode": key_code(key),
                "modifiers": modifiers(*mods),
            });
            if *down && let Some(text) = text {
                params["text"] = json!(text);
            }
            vec![("Input.dispatchKeyEvent", params)]
        }
        StreamInput::Text(text) => vec![("Input.insertText", json!({ "text": text }))],
    }
}

/// A screencast picture's base64 data as JPEG bytes.
pub(super) fn decode_picture(data: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

#[cfg(test)]
mod tests;
