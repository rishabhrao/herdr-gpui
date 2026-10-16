//! A web page browsed on the serving machine: its pictures in an `<img>`
//! over the window, and this page's pointer, wheel, and swipes sent back in
//! the page's own CSS pixels. Keys reach it from the keyboard listeners while
//! it has focus.
use crate::{app::Shared, dom, fling};
use herdr_web::wire::{Button, Mods, MouseKind, PageEvent, PageId, StreamInput, ToHost};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{HtmlImageElement, MouseEvent, PointerEvent, WheelEvent};

/// How far a finger moves before a tap becomes a swipe, in CSS pixels.
const TAP_SLOP: f64 = 8.;

/// The picture element of streamed page `page`, and its listeners.
pub(crate) fn element(
    shared: &Shared,
    page: PageId,
) -> Option<(HtmlImageElement, Vec<dom::Listener>)> {
    let image: HtmlImageElement = dom::create("img")?;
    image.set_class_name("herdr-stream");
    image.set_draggable(false);
    let _ = image.set_attribute("alt", "");
    let target: &web_sys::EventTarget = image.as_ref();
    let mut listeners = Vec::new();
    let touch = Rc::new(RefCell::new(Touch::default()));

    let app = shared.clone();
    listeners.push(dom::Listener::new(
        target,
        "mousedown",
        false,
        move |event| {
            let event: MouseEvent = event.unchecked_into();
            event.prevent_default();
            mouse(&app, page, &event, MouseKind::Down);
        },
    ));
    let app = shared.clone();
    listeners.push(dom::Listener::new(target, "mouseup", false, move |event| {
        let event: MouseEvent = event.unchecked_into();
        mouse(&app, page, &event, MouseKind::Up);
    }));
    let app = shared.clone();
    listeners.push(dom::Listener::new(
        target,
        "mousemove",
        true,
        move |event| {
            let event: MouseEvent = event.unchecked_into();
            mouse(&app, page, &event, MouseKind::Move);
        },
    ));
    listeners.push(dom::Listener::new(target, "contextmenu", false, |event| {
        let event: web_sys::Event = event.unchecked_into();
        event.prevent_default();
    }));
    let app = shared.clone();
    listeners.push(dom::Listener::new(target, "wheel", false, move |event| {
        let event: WheelEvent = event.unchecked_into();
        event.prevent_default();
        let at = (f64::from(event.client_x()), f64::from(event.client_y()));
        wheel(&app, page, at, (event.delta_x(), event.delta_y()));
    }));

    // Fingers: a tap clicks, a swipe scrolls and keeps its momentum.
    let (app, state) = (shared.clone(), touch.clone());
    listeners.push(dom::Listener::new(
        target,
        "pointerdown",
        false,
        move |event| {
            let event: PointerEvent = event.unchecked_into();
            if event.pointer_type() != "touch" {
                return;
            }
            event.prevent_default();
            fling::stop();
            let at = (f64::from(event.client_x()), f64::from(event.client_y()));
            let mut touch = state.borrow_mut();
            *touch = Touch {
                pointer: Some(event.pointer_id()),
                start: at,
                last: at,
                swiping: false,
                tracker: fling::Tracker::default(),
            };
            touch.tracker.push(fling::now(), at.0, at.1);
            focus(&app, page);
        },
    ));
    let (app, state) = (shared.clone(), touch.clone());
    listeners.push(dom::Listener::new(
        target,
        "pointermove",
        false,
        move |event| {
            let event: PointerEvent = event.unchecked_into();
            let mut touch = state.borrow_mut();
            if touch.pointer != Some(event.pointer_id()) {
                return;
            }
            event.prevent_default();
            let at = (f64::from(event.client_x()), f64::from(event.client_y()));
            touch.tracker.push(fling::now(), at.0, at.1);
            let moved = (at.0 - touch.start.0).hypot(at.1 - touch.start.1);
            if !touch.swiping && moved < TAP_SLOP {
                return;
            }
            touch.swiping = true;
            let (dx, dy) = (touch.last.0 - at.0, touch.last.1 - at.1);
            touch.last = at;
            let start = touch.start;
            drop(touch);
            wheel(&app, page, start, (dx, dy));
        },
    ));
    let (app, state) = (shared.clone(), touch);
    let release = move |event: JsValue| {
        let event: PointerEvent = event.unchecked_into();
        let mut touch = state.borrow_mut();
        if touch.pointer.take() != Some(event.pointer_id()) {
            return;
        }
        let start = touch.start;
        if !touch.swiping {
            drop(touch);
            if event.type_() == "pointerup" {
                tap(&app, page, start);
            }
            return;
        }
        let Some((vx, vy)) = touch.tracker.velocity() else {
            return;
        };
        drop(touch);
        let app = app.clone();
        // The content moves against the finger, as the swipe did.
        fling::start((-vx, -vy), move |dx, dy| {
            wheel(&app, page, start, (dx, dy));
            true
        });
    };
    listeners.push(dom::Listener::new(
        target,
        "pointerup",
        false,
        release.clone(),
    ));
    listeners.push(dom::Listener::new(target, "pointercancel", false, release));
    Some((image, listeners))
}

#[derive(Default)]
struct Touch {
    pointer: Option<i32>,
    start: (f64, f64),
    last: (f64, f64),
    swiping: bool,
    tracker: fling::Tracker,
}

fn send(shared: &Shared, page: PageId, input: StreamInput) {
    if let Ok(app) = shared.try_borrow_mut() {
        app.send(&ToHost::Page(PageEvent::Input { page, input }));
    }
}

/// Keys go to this page until the app's own canvas is used.
fn focus(shared: &Shared, page: PageId) {
    if let Ok(mut app) = shared.try_borrow_mut() {
        app.focus_stream(page);
    }
}

fn point(shared: &Shared, page: PageId, at: (f64, f64)) -> Option<(f32, f32)> {
    let app = shared.try_borrow().ok()?;
    app.pages.stream_point(page, at)
}

fn mods(event: &MouseEvent) -> Mods {
    Mods {
        control: event.ctrl_key(),
        alt: event.alt_key(),
        shift: event.shift_key(),
        platform: event.meta_key(),
    }
}

fn mouse(shared: &Shared, page: PageId, event: &MouseEvent, kind: MouseKind) {
    let Some((x, y)) = point(
        shared,
        page,
        (f64::from(event.client_x()), f64::from(event.client_y())),
    ) else {
        return;
    };
    if kind == MouseKind::Down {
        focus(shared, page);
    }
    let button = match (kind, event.button()) {
        (MouseKind::Move, _) => (event.buttons() & 1 == 1).then_some(Button::Left),
        (_, 1) => Some(Button::Middle),
        (_, 2) => Some(Button::Right),
        _ => Some(Button::Left),
    };
    let clicks = u8::try_from(event.detail().max(1)).unwrap_or(1);
    send(
        shared,
        page,
        StreamInput::Mouse {
            kind,
            x,
            y,
            button,
            clicks,
            mods: mods(event),
        },
    );
}

fn tap(shared: &Shared, page: PageId, at: (f64, f64)) {
    let Some((x, y)) = point(shared, page, at) else {
        return;
    };
    for kind in [MouseKind::Move, MouseKind::Down, MouseKind::Up] {
        send(
            shared,
            page,
            StreamInput::Mouse {
                kind,
                x,
                y,
                button: (kind != MouseKind::Move).then_some(Button::Left),
                clicks: 1,
                mods: Mods::default(),
            },
        );
    }
}

/// Scrolls by `delta` CSS pixels of this page, at `at`.
fn wheel(shared: &Shared, page: PageId, at: (f64, f64), delta: (f64, f64)) {
    let Some((x, y)) = point(shared, page, at) else {
        return;
    };
    let scale = shared
        .try_borrow()
        .ok()
        .and_then(|app| app.pages.stream_scale(page))
        .unwrap_or(1.);
    send(
        shared,
        page,
        StreamInput::Wheel {
            x,
            y,
            dx: (delta.0 * scale) as f32,
            dy: (delta.1 * scale) as f32,
        },
    );
}
