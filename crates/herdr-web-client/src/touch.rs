//! Touch gestures as the app's mouse and wheel input: a tap clicks, a drag
//! scrolls, a long press is a right click, and two fingers pinch the page's
//! zoom. A finger is not a mouse: nothing hovers, and the keyboard opens
//! only from the keys bar.
use crate::{
    app::{App, Shared},
    dom,
};
use herdr_web::wire::{Button, Delta, Input, Mods, ToHost, WindowId};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::PointerEvent;

/// How far a finger moves, in CSS pixels, before a tap becomes a drag.
const SLOP: f64 = 8.;
const LONG_PRESS_MS: i32 = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Mode {
    #[default]
    Idle,
    /// One finger down, not yet a drag or a long press.
    Pending,
    Scrolling,
    Pinching {
        distance: f64,
        zoom: f64,
    },
    /// The gesture ended early, as a long press does; the rest is ignored.
    Spent,
}

#[derive(Default)]
pub(crate) struct Touch {
    /// Pointer id and client position of each finger down.
    fingers: Vec<(i32, f64, f64)>,
    mode: Mode,
    window: Option<WindowId>,
    /// Where the gesture started, in client pixels.
    start: (f64, f64),
    /// Where the last scroll step ended.
    last: (f64, f64),
    /// Tells a long-press timer whether its gesture is still the current one.
    generation: u32,
}

fn client(event: &PointerEvent) -> (f64, f64) {
    (f64::from(event.client_x()), f64::from(event.client_y()))
}

fn spread(fingers: &[(i32, f64, f64)]) -> f64 {
    match fingers {
        [(_, ax, ay), (_, bx, by), ..] => ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt(),
        _ => 0.,
    }
}

/// A point in client pixels as app pixels in window `id`.
fn app_point(app: &App, id: WindowId, (x, y): (f64, f64)) -> Option<(f32, f32)> {
    let surface = app.surface(id)?;
    let rect = surface.area.get_bounding_client_rect();
    Some((
        ((x - rect.left()) / surface.scale) as f32,
        ((y - rect.top()) / surface.scale) as f32,
    ))
}

fn click(app: &mut App, id: WindowId, at: (f32, f32), button: Button) {
    let (x, y) = at;
    let clicks = app.input.count_click(x, y);
    let mods = Mods::default();
    app.send(&ToHost::Input {
        window: id,
        input: Input::MouseMove {
            x,
            y,
            pressed: None,
            mods,
        },
    });
    for input in [
        Input::MouseDown {
            button,
            x,
            y,
            mods,
            clicks,
        },
        Input::MouseUp {
            button,
            x,
            y,
            mods,
            clicks,
        },
    ] {
        app.send(&ToHost::Input { window: id, input });
    }
}

pub(crate) fn down(shared: &Shared, app: &mut App, id: WindowId, event: &PointerEvent) {
    let point = client(event);
    let touch = &mut app.input.touch;
    touch
        .fingers
        .retain(|(pointer, ..)| *pointer != event.pointer_id());
    touch.fingers.push((event.pointer_id(), point.0, point.1));
    match touch.fingers.len() {
        1 => {
            touch.mode = Mode::Pending;
            touch.window = Some(id);
            touch.start = point;
            touch.last = point;
            touch.generation = touch.generation.wrapping_add(1);
            schedule_long_press(shared, touch.generation);
        }
        2 => {
            touch.mode = Mode::Pinching {
                distance: spread(&touch.fingers).max(1.),
                zoom: app.zoom,
            };
        }
        _ => {}
    }
}

fn schedule_long_press(shared: &Shared, generation: u32) {
    let Some(window) = dom::window() else {
        return;
    };
    let shared = shared.clone();
    let callback = Closure::once_into_js(move || {
        let Ok(mut app) = shared.try_borrow_mut() else {
            return;
        };
        let touch = &app.input.touch;
        if touch.generation != generation || touch.mode != Mode::Pending {
            return;
        }
        let (Some(id), start) = (touch.window, touch.start) else {
            return;
        };
        app.input.touch.mode = Mode::Spent;
        if let Some(at) = app_point(&app, id, start) {
            click(&mut app, id, at, Button::Right);
        }
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.unchecked_ref(),
        LONG_PRESS_MS,
    );
}

pub(crate) fn moved(app: &mut App, event: &PointerEvent) {
    let point = client(event);
    let touch = &mut app.input.touch;
    if let Some(finger) = touch
        .fingers
        .iter_mut()
        .find(|(pointer, ..)| *pointer == event.pointer_id())
    {
        (finger.1, finger.2) = point;
    }
    match touch.mode {
        Mode::Pending => {
            let (sx, sy) = touch.start;
            if ((point.0 - sx).powi(2) + (point.1 - sy).powi(2)).sqrt() >= SLOP {
                touch.mode = Mode::Scrolling;
                scroll(app, point);
            }
        }
        Mode::Scrolling => scroll(app, point),
        Mode::Pinching { distance, zoom } => {
            let now = spread(&touch.fingers);
            if now > 0. {
                app.set_zoom(zoom * now / distance);
            }
        }
        Mode::Idle | Mode::Spent => {}
    }
}

fn scroll(app: &mut App, point: (f64, f64)) {
    let touch = &mut app.input.touch;
    let Some(id) = touch.window else {
        return;
    };
    let (dx, dy) = (point.0 - touch.last.0, point.1 - touch.last.1);
    touch.last = point;
    let start = touch.start;
    let Some((x, y)) = app_point(app, id, start) else {
        return;
    };
    // The content follows the finger, as it does in any phone app.
    let zoom = app.surface(id).map_or(app.zoom, |surface| surface.scale);
    app.send(&ToHost::Input {
        window: id,
        input: Input::Scroll {
            x,
            y,
            delta: Delta::Pixels((dx / zoom) as f32, (dy / zoom) as f32),
            mods: Mods::default(),
        },
    });
}

pub(crate) fn up(app: &mut App, event: &PointerEvent, cancelled: bool) {
    let touch = &mut app.input.touch;
    touch
        .fingers
        .retain(|(pointer, ..)| *pointer != event.pointer_id());
    let mode = touch.mode;
    let window = touch.window;
    let start = touch.start;
    if !touch.fingers.is_empty() {
        // One finger of a pinch lifted: the rest of it moves nothing.
        if matches!(mode, Mode::Pinching { .. }) {
            touch.mode = Mode::Spent;
        }
        return;
    }
    touch.mode = Mode::Idle;
    touch.generation = touch.generation.wrapping_add(1);
    if mode == Mode::Pending
        && !cancelled
        && let Some(id) = window
        && let Some(at) = app_point(app, id, start)
    {
        click(app, id, at, Button::Left);
    }
}
