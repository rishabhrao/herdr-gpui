//! What `chrome.js` calls: the keys bar, paste, zoom, and taking over.
use crate::{app, keys};
use herdr_web::wire::{Input, Mods, TextOp, ToHost};
use wasm_bindgen::prelude::wasm_bindgen;

/// A key from the keys bar, with the modifiers held there.
#[wasm_bindgen]
pub fn send_key(key: String, control: bool, alt: bool, shift: bool, platform: bool) {
    app::with_app(|_, app| {
        let Some(window) = app.focused_window() else {
            return;
        };
        let mods = keys::merge(
            Mods {
                control,
                alt,
                shift,
                platform,
            },
            app.input.take_sticky(),
        );
        if mods != Mods::default() || app.input.sticky_was_set() {
            crate::dom::emit("sticky-used", &[]);
        }
        app.send(&ToHost::Input {
            window,
            input: Input::KeyDown {
                key: key.clone(),
                key_char: None,
                mods,
                held: false,
            },
        });
        app.send(&ToHost::Input {
            window,
            input: Input::KeyUp {
                key,
                key_char: None,
                mods,
            },
        });
    });
}

/// Text to type as it is.
#[wasm_bindgen]
pub fn send_text(text: String) {
    app::with_app(|_, app| {
        if let Some(window) = app.focused_window() {
            app.send(&ToHost::Text {
                window,
                op: TextOp::Insert(text),
            });
        }
    });
}

/// Pastes this device's clipboard text into the app.
#[wasm_bindgen]
pub fn paste(text: String) {
    app::with_app(|_, app| {
        let Some(window) = app.focused_window() else {
            return;
        };
        app.send(&ToHost::Clipboard { text });
        let mods = Mods {
            platform: true,
            ..Mods::default()
        };
        app.send(&ToHost::Input {
            window,
            input: Input::KeyDown {
                key: "v".into(),
                key_char: None,
                mods,
                held: false,
            },
        });
        app.send(&ToHost::Input {
            window,
            input: Input::KeyUp {
                key: "v".into(),
                key_char: None,
                mods,
            },
        });
    });
}

/// Modifiers for the next key typed on the phone keyboard.
#[wasm_bindgen]
pub fn set_sticky(control: bool, alt: bool, platform: bool) {
    app::with_app(|_, app| {
        app.input.set_sticky(Mods {
            control,
            alt,
            shift: false,
            platform,
        });
    });
}

#[wasm_bindgen]
pub fn zoom() -> f64 {
    let mut zoom = 1.;
    app::with_app(|_, app| zoom = app.zoom);
    zoom
}

#[wasm_bindgen]
pub fn set_zoom(zoom: f64) {
    app::with_app(|_, app| app.set_zoom(zoom));
}

/// The answer chosen in one of the app's dialogs.
#[wasm_bindgen]
pub fn prompt_answer(id: f64, answer: u32) {
    app::with_app(|_, app| {
        app.send(&ToHost::PromptAnswer {
            id: id as u64,
            answer,
        });
    });
}

#[wasm_bindgen]
pub fn keyboard(show: bool) {
    app::with_app(|_, app| app.show_keyboard(show));
}
