//! Typing on a phone keyboard. Gboard and its kind edit the text they think
//! the field holds: a suggestion replaces the word typed so far, and
//! autocorrect rewrites it after a space. So on a touch screen the hidden
//! field keeps what was typed since the last Return or tap, the keyboard
//! edits it, and each change goes to the app as backspaces and new text,
//! the way a terminal takes an edit.
use crate::{app::App, dom, input};
use herdr_web::wire::{Mods, StreamInput, TextOp, ToHost};
use web_sys::{HtmlElement, InputEvent};

/// Past this many characters, the field starts over at the next space, so
/// it never grows without bound.
const MAX_FIELD: usize = 64;

/// The field's text as typed: contenteditable keeps spaces as no-break ones.
fn field_text(field: &HtmlElement) -> String {
    field
        .text_content()
        .unwrap_or_default()
        .replace('\u{a0}', " ")
}

/// Starts the field over, as after Return or a tap elsewhere. A word being
/// composed stays, or the keyboard would lose it.
pub(crate) fn reset(app: &mut App) {
    if app.input.composing {
        return;
    }
    app.input.typed.clear();
    app.field().set_text_content(None);
}

/// A key as the focused streamed page or window takes it.
fn key(app: &App, name: &str) {
    if app.pages.focused_stream().is_some() {
        let (key, code) = match name {
            "backspace" => ("Backspace", "Backspace"),
            "delete" => ("Delete", "Delete"),
            _ => ("Enter", "Enter"),
        };
        for down in [true, false] {
            app.to_stream(StreamInput::Key {
                down,
                key: key.to_owned(),
                code: code.to_owned(),
                text: None,
                mods: Mods::default(),
            });
        }
        return;
    }
    if let Some(window) = app.focused_window() {
        input::press(app, window, name, None, Mods::default());
    }
}

fn text(app: &App, text: String) {
    if app.to_stream(StreamInput::Text(text.clone())) {
        return;
    }
    if let Some(window) = app.focused_window() {
        app.send(&ToHost::Text {
            window,
            op: TextOp::Insert(text),
        });
    }
}

/// A phone's `beforeinput`: Return and edits with nothing to edit are sent
/// as keys; everything else the keyboard edits in the field, and `changed`
/// sends the difference.
pub(crate) fn before_input(app: &mut App, event: &InputEvent) {
    let held = app.input.take_sticky();
    if held != Mods::default() {
        // Ctrl from the keys bar, then a letter: the chord, not the text.
        event.prevent_default();
        dom::emit("sticky-used", &[]);
        if let Some(window) = app.focused_window()
            && let Some(data) = event.data()
        {
            for character in data.chars() {
                let key = character.to_lowercase().to_string();
                input::press(app, window, &key, None, held);
            }
        }
        reset(app);
        return;
    }
    match event.input_type().as_str() {
        "insertLineBreak" | "insertParagraph" => {
            event.prevent_default();
            key(app, "enter");
            reset(app);
        }
        "deleteContentBackward" if app.input.typed.is_empty() => {
            event.prevent_default();
            key(app, "backspace");
        }
        "deleteContentForward" => {
            event.prevent_default();
            key(app, "delete");
        }
        // The page's paste listener takes pasted text and images.
        "insertFromPaste" | "insertFromPasteAsQuotation" | "insertFromDrop" => {
            event.prevent_default();
        }
        _ => {}
    }
}

/// The field changed: sends what was removed as backspaces and what was
/// added as text.
pub(crate) fn changed(app: &mut App) {
    let now = field_text(app.field());
    let before = std::mem::take(&mut app.input.typed);
    let kept = before
        .chars()
        .zip(now.chars())
        .take_while(|(old, new)| old == new)
        .count();
    for _ in kept..before.chars().count() {
        key(app, "backspace");
    }
    let added: String = now.chars().skip(kept).collect();
    let mut lines = added.split('\n');
    if let Some(first) = lines.next().filter(|first| !first.is_empty()) {
        text(app, first.to_owned());
    }
    let mut broke = false;
    for line in lines {
        key(app, "enter");
        broke = true;
        if !line.is_empty() {
            text(app, line.to_owned());
        }
    }
    app.input.typed = now;
    if broke || (app.input.typed.chars().count() > MAX_FIELD && app.input.typed.ends_with(' ')) {
        reset(app);
    }
}
