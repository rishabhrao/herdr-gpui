//! The page's input as app input. Keys go through a hidden text area that
//! keeps focus, so the IME and dictation work; pointer input comes from each
//! window's canvas.
use crate::{
    app::Shared,
    dom::{self, Listener},
    keys,
    surface::Surface,
};
use herdr_web::wire::{Button, Delta, Input, Mods, TextOp, ToHost, WindowId};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    CompositionEvent, EventTarget, HtmlTextAreaElement, InputEvent, KeyboardEvent, PointerEvent,
    WheelEvent,
};

/// Presses closer together than this, in milliseconds and CSS pixels, count
/// as one double or triple click.
const CLICK_TIME: f64 = 400.;
const CLICK_DISTANCE: f32 = 5.;

#[derive(Default)]
pub(crate) struct State {
    mods: Mods,
    capslock: bool,
    composing: bool,
    pressed: Option<Button>,
    clicks: (u8, f64, f32, f32),
    /// The newest pointer move, sent once per animation frame.
    pub(crate) pending_move: Option<(WindowId, Input)>,
    /// Where the pointer last was, for Back and Forward from the browser.
    pub(crate) pointer: Option<(WindowId, f32, f32)>,
    hovered: Option<WindowId>,
    /// Modifiers the keys bar holds for the next key.
    sticky: Mods,
    pub(crate) touch: crate::touch::Touch,
}

impl State {
    pub(crate) fn set_sticky(&mut self, mods: Mods) {
        self.sticky = mods;
    }

    pub(crate) fn sticky_was_set(&self) -> bool {
        self.sticky != Mods::default()
    }

    /// The held modifiers, released by the key that uses them.
    pub(crate) fn take_sticky(&mut self) -> Mods {
        std::mem::take(&mut self.sticky)
    }

    /// One more click at `(x, y)`, or the first of a new series.
    pub(crate) fn count_click(&mut self, x: f32, y: f32) -> u8 {
        let now = dom::now();
        let (count, time, last_x, last_y) = self.clicks;
        let near = ((x - last_x).powi(2) + (y - last_y).powi(2)).sqrt() < CLICK_DISTANCE;
        let clicks = if now - time < CLICK_TIME && near {
            count.saturating_add(1)
        } else {
            1
        };
        self.clicks = (clicks, now, x, y);
        clicks
    }
}

fn button(code: i16) -> Button {
    match code {
        1 => Button::Middle,
        2 => Button::Right,
        3 => Button::Back,
        4 => Button::Forward,
        _ => Button::Left,
    }
}

fn mouse_mods(event: &web_sys::MouseEvent) -> Mods {
    Mods {
        control: event.ctrl_key(),
        alt: event.alt_key(),
        shift: event.shift_key(),
        platform: event.meta_key(),
    }
}

/// The pointer's place in window `id`, in app pixels.
fn position(
    event: &web_sys::MouseEvent,
    app: &crate::app::App,
    id: WindowId,
) -> Option<(f32, f32)> {
    let surface = app.surface(id)?;
    let rect = surface.area.get_bounding_client_rect();
    Some((
        ((f64::from(event.client_x()) - rect.left()) / surface.scale) as f32,
        ((f64::from(event.client_y()) - rect.top()) / surface.scale) as f32,
    ))
}

fn is_touch(event: &PointerEvent) -> bool {
    event.pointer_type() == "touch"
}

/// A key pressed and released, as the keys bar and a phone keyboard send them.
fn press(app: &crate::app::App, window: WindowId, key: &str, key_char: Option<String>, mods: Mods) {
    app.send(&ToHost::Input {
        window,
        input: Input::KeyDown {
            key: key.to_owned(),
            key_char: key_char.clone(),
            mods,
            held: false,
        },
    });
    app.send(&ToHost::Input {
        window,
        input: Input::KeyUp {
            key: key.to_owned(),
            key_char,
            mods,
        },
    });
}

/// Keyboard, focus, visibility, and appearance listeners for the page.
pub(crate) fn install(shared: &Shared, text: &HtmlTextAreaElement) -> Vec<Listener> {
    let target: &EventTarget = text.as_ref();
    let mut listeners = Vec::new();

    let app = shared.clone();
    listeners.push(Listener::new(target, "keydown", false, move |event| {
        let event: KeyboardEvent = event.unchecked_into();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        if event.is_composing() || event.key_code() == 229 {
            return;
        }
        let key = keys::key(&event);
        let mods = keys::mods(&event);
        let capslock = event.get_modifier_state("CapsLock");
        let window = app.focused_window();
        if app.input.mods != mods || app.input.capslock != capslock {
            app.input.mods = mods;
            app.input.capslock = capslock;
            if let Some(window) = window {
                app.send(&ToHost::Input {
                    window,
                    input: Input::Modifiers { mods, capslock },
                });
            }
        }
        if keys::is_modifier(&key) || key.is_empty() || keys::browser_keeps(&key, mods) {
            return;
        }
        event.prevent_default();
        let held = app.input.take_sticky();
        if held != Mods::default() {
            dom::emit("sticky-used", &[]);
        }
        let mods = keys::merge(mods, held);
        // Cmd-[ and Cmd-] walk the browser's history, like its toolbar.
        if let Some(back) = keys::history_step(&key, mods) {
            drop(app);
            crate::app::step_history(back);
            return;
        }
        let Some(window) = window else {
            return;
        };
        let key_char = keys::key_char(&event, &key, mods);
        app.send(&ToHost::Input {
            window,
            input: Input::KeyDown {
                key,
                key_char,
                mods,
                held: event.repeat(),
            },
        });
    }));

    let app = shared.clone();
    listeners.push(Listener::new(target, "keyup", false, move |event| {
        let event: KeyboardEvent = event.unchecked_into();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        let key = keys::key(&event);
        let mods = keys::mods(&event);
        let Some(window) = app.focused_window() else {
            return;
        };
        if app.input.mods != mods {
            app.input.mods = mods;
            let capslock = app.input.capslock;
            app.send(&ToHost::Input {
                window,
                input: Input::Modifiers { mods, capslock },
            });
        }
        if keys::is_modifier(&key) || key.is_empty() || app.input.composing {
            return;
        }
        let key_char = keys::key_char(&event, &key, mods);
        app.send(&ToHost::Input {
            window,
            input: Input::KeyUp {
                key,
                key_char,
                mods,
            },
        });
    }));

    // Text that arrives without a key press the app saw: dictation, the
    // emoji picker, autocorrect.
    let app = shared.clone();
    let area = text.clone();
    listeners.push(Listener::new(target, "beforeinput", false, move |event| {
        let event: InputEvent = event.unchecked_into();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        if app.input.composing || event.is_composing() {
            return;
        }
        event.prevent_default();
        area.set_value("");
        let Some(window) = app.focused_window() else {
            return;
        };
        let kind = event.input_type();
        // A phone keyboard sends its keys this way rather than as key presses.
        let key = match kind.as_str() {
            "deleteContentBackward" => Some("backspace"),
            "deleteContentForward" => Some("delete"),
            "insertLineBreak" | "insertParagraph" => Some("enter"),
            _ => None,
        };
        let held = app.input.take_sticky();
        if held != Mods::default() {
            dom::emit("sticky-used", &[]);
        }
        if let Some(key) = key {
            press(&app, window, key, None, held);
            return;
        }
        let Some(data) = event.data().filter(|data| !data.is_empty()).filter(|_| {
            matches!(
                kind.as_str(),
                "insertText" | "insertReplacementText" | "insertFromDrop"
            )
        }) else {
            return;
        };
        if held != Mods::default() {
            // Ctrl from the keys bar, then a letter: the chord, not the text.
            for character in data.chars() {
                let key = character.to_lowercase().to_string();
                press(&app, window, &key, None, held);
            }
            return;
        }
        app.send(&ToHost::Text {
            window,
            op: TextOp::Insert(data),
        });
    }));

    let app = shared.clone();
    listeners.push(Listener::new(target, "compositionstart", true, move |_| {
        if let Ok(mut app) = app.try_borrow_mut() {
            app.input.composing = true;
        }
    }));

    let app = shared.clone();
    listeners.push(Listener::new(
        target,
        "compositionupdate",
        true,
        move |event| {
            let event: CompositionEvent = event.unchecked_into();
            let Ok(app) = app.try_borrow_mut() else {
                return;
            };
            let Some(window) = app.focused_window() else {
                return;
            };
            let text = event.data().unwrap_or_default();
            let end = text.encode_utf16().count() as u32;
            app.send(&ToHost::Text {
                window,
                op: TextOp::Compose {
                    text,
                    selected: Some((end, end)),
                },
            });
        },
    ));

    let app = shared.clone();
    let area = text.clone();
    listeners.push(Listener::new(
        target,
        "compositionend",
        true,
        move |event| {
            let event: CompositionEvent = event.unchecked_into();
            let Ok(mut app) = app.try_borrow_mut() else {
                return;
            };
            app.input.composing = false;
            area.set_value("");
            let Some(window) = app.focused_window() else {
                return;
            };
            let op = match event.data().filter(|data| !data.is_empty()) {
                Some(text) => TextOp::Commit(text),
                None => TextOp::Cancel,
            };
            app.send(&ToHost::Text { window, op });
        },
    ));

    if let Some(window) = dom::window() {
        let target: &EventTarget = window.as_ref();
        let app = shared.clone();
        listeners.push(Listener::new(target, "focus", true, move |_| {
            if let Ok(mut app) = app.try_borrow_mut() {
                app.page_focus(true);
            }
        }));
        let app = shared.clone();
        listeners.push(Listener::new(target, "blur", true, move |_| {
            if let Ok(mut app) = app.try_borrow_mut() {
                app.page_focus(false);
            }
        }));
        let app = shared.clone();
        listeners.push(Listener::new(target, "message", true, move |event| {
            let event: web_sys::MessageEvent = event.unchecked_into();
            if let Ok(app) = app.try_borrow_mut() {
                app.page_message(&event);
            }
        }));
        if let Ok(Some(query)) = window.match_media("(prefers-color-scheme: dark)") {
            let app = shared.clone();
            let watched = query.clone();
            listeners.push(Listener::new(query.as_ref(), "change", true, move |_| {
                if let Ok(app) = app.try_borrow_mut() {
                    app.send(&ToHost::Dark {
                        dark: watched.matches(),
                    });
                }
            }));
        }
    }
    if let Some(document) = dom::document() {
        let target: &EventTarget = document.as_ref();
        let app = shared.clone();
        listeners.push(Listener::new(target, "visibilitychange", true, move |_| {
            let visible = dom::document().is_some_and(|document| !document.hidden());
            if let Ok(app) = app.try_borrow_mut() {
                app.send(&ToHost::Visible { visible });
            }
        }));
        // Dropped files reach the page only as blobs, never as paths the
        // app could open; keep the browser from navigating to them.
        for name in ["dragover", "drop"] {
            listeners.push(Listener::new(target, name, false, |event| {
                let event: web_sys::Event = event.unchecked_into();
                event.prevent_default();
            }));
        }
    }
    listeners
}

/// Pointer listeners for one window's canvas, and its title bar's.
pub(crate) fn attach(shared: &Shared, surface: &Surface) -> Vec<Listener> {
    let id = surface.id;
    let target: &EventTarget = surface.canvas.as_ref();
    let mut listeners = Vec::new();

    let app = shared.clone();
    listeners.push(Listener::new(target, "pointerdown", false, move |event| {
        let event: PointerEvent = event.unchecked_into();
        event.prevent_default();
        let shared = app.clone();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        app.raise(id);
        app.focus_window(id);
        if let Some(canvas) = app.surface(id).map(|surface| surface.canvas.clone()) {
            let _ = canvas.set_pointer_capture(event.pointer_id());
        }
        if is_touch(&event) {
            crate::touch::down(&shared, &mut app, id, &event);
            return;
        }
        app.keep_keyboard();
        let Some((x, y)) = position(&event, &app, id) else {
            return;
        };
        if let Some(canvas) = app.surface(id).map(|surface| surface.canvas.clone()) {
            let _ = canvas.set_pointer_capture(event.pointer_id());
        }
        let clicks = app.input.count_click(x, y);
        let pressed = button(event.button());
        app.input.pointer = Some((id, x, y));
        app.input.pressed = Some(pressed);
        app.flush_move();
        app.send(&ToHost::Input {
            window: id,
            input: Input::MouseDown {
                button: pressed,
                x,
                y,
                mods: mouse_mods(&event),
                clicks,
            },
        });
    }));

    let app = shared.clone();
    listeners.push(Listener::new(target, "pointerup", false, move |event| {
        let event: PointerEvent = event.unchecked_into();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        if is_touch(&event) {
            crate::touch::up(&mut app, &event, false);
            return;
        }
        let Some((x, y)) = position(&event, &app, id) else {
            return;
        };
        if let Some(canvas) = app.surface(id).map(|surface| surface.canvas.clone()) {
            let _ = canvas.release_pointer_capture(event.pointer_id());
        }
        app.input.pressed = None;
        app.flush_move();
        let clicks = app.input.clicks.0;
        app.send(&ToHost::Input {
            window: id,
            input: Input::MouseUp {
                button: button(event.button()),
                x,
                y,
                mods: mouse_mods(&event),
                clicks,
            },
        });
    }));

    let app = shared.clone();
    listeners.push(Listener::new(target, "pointermove", true, move |event| {
        let event: PointerEvent = event.unchecked_into();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        if is_touch(&event) {
            crate::touch::moved(&mut app, &event);
            return;
        }
        let Some((x, y)) = position(&event, &app, id) else {
            return;
        };
        app.input.pointer = Some((id, x, y));
        if app.input.hovered != Some(id) {
            app.input.hovered = Some(id);
            app.send(&ToHost::Hover {
                window: id,
                hovered: true,
            });
        }
        let pressed = app.input.pressed;
        app.input.pending_move = Some((
            id,
            Input::MouseMove {
                x,
                y,
                pressed,
                mods: mouse_mods(&event),
            },
        ));
    }));

    let app = shared.clone();
    let cancel = shared.clone();
    listeners.push(Listener::new(target, "pointercancel", true, move |event| {
        let event: PointerEvent = event.unchecked_into();
        if is_touch(&event)
            && let Ok(mut app) = cancel.try_borrow_mut()
        {
            crate::touch::up(&mut app, &event, true);
        }
    }));

    listeners.push(Listener::new(target, "pointerleave", true, move |event| {
        let event: PointerEvent = event.unchecked_into();
        if is_touch(&event) {
            return;
        }
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        let Some((x, y)) = position(&event, &app, id) else {
            return;
        };
        app.flush_move();
        if app.input.hovered == Some(id) {
            app.input.hovered = None;
        }
        let pressed = app.input.pressed;
        app.send(&ToHost::Input {
            window: id,
            input: Input::MouseExit {
                x,
                y,
                pressed,
                mods: mouse_mods(&event),
            },
        });
        app.send(&ToHost::Hover {
            window: id,
            hovered: false,
        });
    }));

    let app = shared.clone();
    listeners.push(Listener::new(target, "wheel", false, move |event| {
        let event: WheelEvent = event.unchecked_into();
        event.prevent_default();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        let Some((x, y)) = position(&event, &app, id) else {
            return;
        };
        app.flush_move();
        let mods = mouse_mods(&event);
        // A trackpad pinch reaches the page as a wheel with Control held.
        let input = if event.ctrl_key() && event.delta_mode() == 0 {
            Input::Pinch {
                x,
                y,
                delta: (-event.delta_y() / 100.) as f32,
                mods: Mods {
                    control: false,
                    ..mods
                },
            }
        } else {
            let (dx, dy) = (-event.delta_x() as f32, -event.delta_y() as f32);
            Input::Scroll {
                x,
                y,
                delta: if event.delta_mode() == 1 {
                    Delta::Lines(dx, dy)
                } else {
                    Delta::Pixels(dx, dy)
                },
                mods,
            }
        };
        app.send(&ToHost::Input { window: id, input });
    }));

    // A mouse's side buttons reach the app directly; the browser must not
    // also walk its history for them.
    listeners.push(Listener::new(target, "mouseup", false, |event| {
        let event: web_sys::MouseEvent = event.unchecked_into();
        if matches!(event.button(), 3 | 4) {
            event.prevent_default();
        }
    }));

    listeners.push(Listener::new(target, "contextmenu", false, |event| {
        let event: web_sys::Event = event.unchecked_into();
        event.prevent_default();
    }));

    if let Some(close) = &surface.close {
        let app = shared.clone();
        listeners.push(Listener::new(close.as_ref(), "click", true, move |_| {
            if let Ok(app) = app.try_borrow_mut() {
                app.send(&ToHost::Close { window: id });
            }
        }));
    }
    if let Some(bar) = &surface.title_bar {
        listeners.extend(drag(shared, id, bar.as_ref()));
    }
    listeners
}

/// Moves a floating window by its title bar.
fn drag(shared: &Shared, id: WindowId, bar: &EventTarget) -> Vec<Listener> {
    let mut listeners = Vec::new();
    let app = shared.clone();
    listeners.push(Listener::new(bar, "pointerdown", false, move |event| {
        let event: PointerEvent = event.unchecked_into();
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        app.raise(id);
        app.focus_window(id);
        let Some(element) = app.surface(id).map(|surface| surface.element.clone()) else {
            return;
        };
        let start = (
            f64::from(event.client_x()) - f64::from(element.offset_left()),
            f64::from(event.client_y()) - f64::from(element.offset_top()),
        );
        app.dragging = Some((id, start));
        if let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        {
            let _ = target.set_pointer_capture(event.pointer_id());
        }
    }));
    let app = shared.clone();
    listeners.push(Listener::new(bar, "pointermove", true, move |event| {
        let event: PointerEvent = event.unchecked_into();
        let Ok(app) = app.try_borrow_mut() else {
            return;
        };
        let Some((dragged, (dx, dy))) = app.dragging.filter(|(dragged, _)| *dragged == id) else {
            return;
        };
        if let Some(surface) = app.surface(dragged) {
            let left = (f64::from(event.client_x()) - dx).max(0.);
            let top = (f64::from(event.client_y()) - dy).max(0.);
            dom::style(&surface.element, "left", &format!("{left}px"));
            dom::style(&surface.element, "top", &format!("{top}px"));
        }
    }));
    let app = shared.clone();
    listeners.push(Listener::new(bar, "pointerup", true, move |_: JsValue| {
        if let Ok(mut app) = app.try_borrow_mut() {
            app.dragging = None;
        }
    }));
    listeners
}
