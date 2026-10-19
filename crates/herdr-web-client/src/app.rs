//! The page's state: its connection, windows, and pages. Every DOM callback
//! borrows it with `try_borrow_mut` and skips a re-entrant event rather than
//! panicking, which on WebAssembly would wedge the page.
use crate::{
    dom::{self, Listener},
    input,
    pages::Pages,
    surface::Surface,
};
use gpui_wgpu::{WebBackendPreference, WgpuContext, wgpu};
use herdr_web::wire::{self, Button, Input, PROTOCOL, ToBrowser, ToHost, WindowId};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{BinaryType, HtmlCanvasElement, HtmlElement, MessageEvent, WebSocket};

pub(crate) type Shared = Rc<RefCell<App>>;

/// How long the page waits before trying a lost connection again.
/// The first wait before trying a lost connection again; each failure
/// doubles it, up to `RETRY_MAX_MS`.
const RETRY_MS: i32 = 1000;
const RETRY_MAX_MS: i32 = 10_000;
/// The page's UI scale, kept per browser.
const ZOOM_KEY: &str = "herdr-zoom";
pub(crate) const MIN_ZOOM: f64 = 0.5;
pub(crate) const MAX_ZOOM: f64 = 2.;

thread_local! {
    /// The page's state, for the functions `chrome.js` calls.
    static APP: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

/// Runs `call` on the page's state, unless it is busy or not started.
pub(crate) fn with_app(call: impl FnOnce(&Shared, &mut App)) {
    let Some(shared) = APP.with(|app| app.borrow().clone()) else {
        return;
    };
    if let Ok(mut app) = shared.try_borrow_mut() {
        call(&shared, &mut app);
    }
}

fn saved_zoom() -> f64 {
    dom::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(ZOOM_KEY).ok().flatten())
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|zoom| zoom.is_finite())
        .map_or(1., |zoom| zoom.clamp(MIN_ZOOM, MAX_ZOOM))
}

pub(crate) struct App {
    context: Rc<WgpuContext>,
    /// The canvas the GPU was set up on, for the first window.
    spare: Option<(HtmlCanvasElement, wgpu::Surface<'static>)>,
    root: HtmlElement,
    /// The hidden field keys and text arrive in: contenteditable, since a
    /// phone keyboard inserts images only into rich text.
    text: HtmlElement,
    socket: Option<WebSocket>,
    socket_handlers: Vec<Listener>,
    /// Bottom to top; the main window first.
    surfaces: Vec<Surface>,
    focused: Option<WindowId>,
    pub(crate) pages: Pages,
    pub(crate) input: input::State,
    pub(crate) dragging: Option<(WindowId, (f64, f64))>,
    /// Whether this page has been connected; a later connection reloads it,
    /// so each one starts from a clean page.
    connected_once: bool,
    /// The app was updated and this page is too old; it no longer reconnects.
    replaced: bool,
    /// Connections tried since the last one that opened.
    attempts: u32,
    /// A retry is waiting on its timer; coming back to the page runs it now.
    retry_pending: bool,
    /// CSS pixels per app pixel: below 1 fits more of the app, above 1
    /// makes it larger to touch.
    pub(crate) zoom: f64,
    cursor: String,
    _listeners: Vec<Listener>,
}

pub(crate) async fn run() -> Result<(), String> {
    let root: HtmlElement = dom::by_id("herdr-root").ok_or("the page has no root")?;
    let text: HtmlElement = dom::by_id("herdr-input").ok_or("the page has no input")?;
    let canvas: HtmlCanvasElement = dom::create("canvas").ok_or("no document")?;
    let prepared = WgpuContext::new_web(&canvas, WebBackendPreference::Auto)
        .await
        .map_err(|error| format!("{error:#}"))?;
    let shared: Shared = Rc::new(RefCell::new(App {
        context: Rc::new(prepared.context),
        spare: Some((canvas, prepared.surface)),
        root,
        text: text.clone(),
        socket: None,
        socket_handlers: Vec::new(),
        surfaces: Vec::new(),
        focused: None,
        pages: Pages::default(),
        input: input::State::default(),
        dragging: None,
        connected_once: false,
        replaced: false,
        attempts: 0,
        retry_pending: false,
        zoom: saved_zoom(),
        cursor: "default".into(),
        _listeners: Vec::new(),
    }));
    let mut listeners = input::install(&shared, &text);
    listeners.extend(history_listener(&shared));
    if let Ok(mut app) = shared.try_borrow_mut() {
        app._listeners = listeners;
    }
    APP.with(|app| *app.borrow_mut() = Some(shared.clone()));
    if !dom::coarse_pointer() {
        let _ = text.focus();
    }
    animate(shared.clone());
    connect(&shared);
    Ok(())
}

fn connect(shared: &Shared) {
    let Some(window) = dom::window() else {
        return;
    };
    let location = window.location();
    let host = location.host().unwrap_or_default();
    // An HTTPS page, as through a tunnel, may only open a secure socket.
    let scheme = if location
        .protocol()
        .is_ok_and(|protocol| protocol == "https:")
    {
        "wss"
    } else {
        "ws"
    };
    let Ok(mut app) = shared.try_borrow_mut() else {
        return;
    };
    let url = format!("{scheme}://{host}/ws");
    let socket = match WebSocket::new(&url) {
        Ok(socket) => socket,
        Err(_) => {
            drop(app);
            retry(shared);
            return;
        }
    };
    socket.set_binary_type(BinaryType::Arraybuffer);
    let target: &web_sys::EventTarget = socket.as_ref();
    let mut handlers = Vec::new();

    let opened = shared.clone();
    handlers.push(Listener::new(target, "open", true, move |_| {
        let Ok(mut app) = opened.try_borrow_mut() else {
            return;
        };
        if app.connected_once {
            // A fresh page is simpler than undoing the old connection's state.
            if let Some(window) = dom::window() {
                let _ = window.location().reload();
            }
            return;
        }
        app.connected_once = true;
        app.attempts = 0;
        trap_history();
        app.send(&ToHost::Hello { protocol: PROTOCOL });
        let dark = dom::window()
            .and_then(|window| window.match_media("(prefers-color-scheme: dark)").ok())
            .flatten()
            .is_some_and(|query| query.matches());
        app.send(&ToHost::Dark { dark });
        dom::show_status("Waiting for Herdr UI\u{2026}");
    }));

    let received = shared.clone();
    handlers.push(Listener::new(target, "message", true, move |event| {
        let event: MessageEvent = event.unchecked_into();
        let bytes = js_sys::Uint8Array::new(&event.data()).to_vec();
        match wire::decode::<ToBrowser>(&bytes) {
            Ok(message) => receive(&received, message),
            Err(error) => dom::log(&format!(
                "Herdr UI sent a message this page cannot read: {error}"
            )),
        }
    }));

    let closed = shared.clone();
    handlers.push(Listener::new(target, "close", true, move |_| {
        if let Ok(mut app) = closed.try_borrow_mut() {
            app.socket = None;
            app.socket_handlers.clear();
            if app.replaced {
                return;
            }
            if app.connected_once {
                dom::show_status("Herdr UI stopped serving this page. Reconnecting\u{2026}");
                // After a few failures the page checks why: a sign-in that
                // ended or a device that was removed needs a reload, not
                // retries forever.
                dom::emit("disconnected", &[("attempts", &app.attempts.to_string())]);
            }
        }
        retry(&closed);
    }));

    app.socket = Some(socket);
    app.socket_handlers = handlers;
}

/// The history entry the page rests on, between one for Back and one for
/// Forward. Leaving it is a step in the app, and the page returns to it.
const REST: f64 = 1.;

/// Gives the page three history entries and rests on the middle one, so the
/// browser's Back and Forward, swipes, and Cmd-[ and Cmd-] each move one
/// entry away and back. A reloaded page already has them.
fn trap_history() {
    let Some(history) = dom::window().and_then(|window| window.history().ok()) else {
        return;
    };
    if history.state().ok().and_then(|state| state.as_f64()) == Some(REST) {
        return;
    }
    let _ = history.replace_state(&JsValue::from_f64(REST - 1.), "");
    let _ = history.push_state(&JsValue::from_f64(REST), "");
    let _ = history.push_state(&JsValue::from_f64(REST + 1.), "");
    let _ = history.back();
}

pub(crate) fn step_history(back: bool) {
    if let Some(history) = dom::window().and_then(|window| window.history().ok()) {
        let _ = if back {
            history.back()
        } else {
            history.forward()
        };
    }
}

fn history_listener(shared: &Shared) -> Option<Listener> {
    let app = shared.clone();
    let target: web_sys::EventTarget = dom::window()?.into();
    Some(Listener::new(&target, "popstate", true, move |event| {
        let event: web_sys::PopStateEvent = event.unchecked_into();
        let Some(entry) = event.state().as_f64() else {
            return;
        };
        if entry == REST {
            return;
        }
        let back = entry < REST;
        if let Ok(app) = app.try_borrow_mut() {
            app.navigate(back);
        }
        // Back to resting, which reports `REST` and is ignored.
        step_history(!back);
    }))
}

fn retry(shared: &Shared) {
    let Some(window) = dom::window() else {
        return;
    };
    let delay = {
        let Ok(mut app) = shared.try_borrow_mut() else {
            return;
        };
        app.retry_pending = true;
        app.attempts = app.attempts.saturating_add(1);
        let doubled = RETRY_MS.saturating_mul(1 << app.attempts.min(4).saturating_sub(1));
        doubled.min(RETRY_MAX_MS)
    };
    let again = shared.clone();
    let callback = Closure::once_into_js(move || {
        if take_retry(&again) {
            connect(&again);
        }
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), delay);
}

/// Claims the waiting retry, so it runs once, from its timer or early.
fn take_retry(shared: &Shared) -> bool {
    shared
        .try_borrow_mut()
        .is_ok_and(|mut app| std::mem::take(&mut app.retry_pending))
}

/// Tries a lost connection now rather than at its timer: the page came back
/// into view, or the device back online.
pub(crate) fn reconnect_now(shared: &Shared) {
    if take_retry(shared) {
        connect(shared);
    }
}

fn receive(shared: &Shared, message: ToBrowser) {
    // Creating a window attaches listeners that hold the shared state.
    if let ToBrowser::WindowOpened {
        window,
        title,
        main,
        width,
        height,
    } = message
    {
        open_window(shared, window, &title, main, width, height);
        return;
    }
    let Ok(mut app) = shared.try_borrow_mut() else {
        return;
    };
    match message {
        ToBrowser::Hello { protocol } => {
            if protocol != PROTOCOL {
                // The server's build changed; reloading fetches the new page.
                app.replaced = true;
                dom::emit("protocol", &[]);
                if let Some(socket) = &app.socket {
                    let _ = socket.close();
                }
            }
        }
        ToBrowser::Layout {
            window,
            width,
            height,
            scale,
        } => {
            if let Some(surface) = app.surface_mut(window) {
                surface.set_layout((width, height, scale));
            }
        }
        ToBrowser::OpenUrl { url } => dom::emit("open-url", &[("url", &url)]),
        ToBrowser::Clipboard { text } => dom::emit("clipboard", &[("text", &text)]),
        ToBrowser::Notify { title, body } => {
            dom::emit("notify", &[("title", &title), ("body", &body)]);
        }
        ToBrowser::Toast { text } => dom::emit("toast", &[("text", &text)]),
        ToBrowser::Prompt {
            id,
            level,
            message,
            detail,
            answers,
        } => {
            let answers = answers.join("\u{1f}");
            dom::emit(
                "prompt",
                &[
                    ("id", &id.to_string()),
                    ("level", &level.to_string()),
                    ("message", &message),
                    ("detail", detail.as_deref().unwrap_or("")),
                    ("answers", &answers),
                ],
            );
        }
        ToBrowser::PromptClosed { id } => dom::emit("prompt-closed", &[("id", &id.to_string())]),
        ToBrowser::Keys { prefix } => {
            let flag = |on: bool| if on { "1" } else { "0" };
            match prefix {
                Some(chord) => dom::emit(
                    "keys",
                    &[
                        ("key", &chord.key),
                        ("control", flag(chord.mods.control)),
                        ("alt", flag(chord.mods.alt)),
                        ("shift", flag(chord.mods.shift)),
                        ("platform", flag(chord.mods.platform)),
                    ],
                ),
                None => dom::emit("keys", &[]),
            }
        }
        ToBrowser::Device(action) => {
            let action = match action {
                wire::DeviceAction::TextSmaller => "text-smaller",
                wire::DeviceAction::TextReset => "text-reset",
                wire::DeviceAction::TextLarger => "text-larger",
                wire::DeviceAction::AllowNotifications => "allow-notifications",
                wire::DeviceAction::SignOut => "sign-out",
                wire::DeviceAction::HideKeyboard => "hide-keyboard",
            };
            dom::emit("device", &[("action", action)]);
        }
        ToBrowser::WindowOpened { .. } => {}
        ToBrowser::WindowClosed { window } => {
            app.pages.close_window(window);
            app.surfaces.retain(|surface| surface.id != window);
            if app.focused == Some(window) {
                app.focused = None;
            }
        }
        ToBrowser::Title { window, title } => {
            if let Some(surface) = app.surface(window) {
                surface.set_title(&title);
                if surface.main
                    && let Some(document) = dom::document()
                {
                    document.set_title(if title.is_empty() { "Herdr UI" } else { &title });
                }
            }
        }
        ToBrowser::Activate { window } => {
            app.raise(window);
            app.focus_window(window);
        }
        ToBrowser::Upload { window, tiles } => {
            if let Some(surface) = app.surface_mut(window) {
                surface.upload(tiles);
            }
        }
        ToBrowser::Remove { window, tiles } => {
            if let Some(surface) = app.surface_mut(window) {
                surface.remove(tiles);
            }
        }
        ToBrowser::Frame { window, seq, scene } => {
            if let Some(surface) = app.surface_mut(window) {
                surface.frame(seq, scene);
            }
        }
        ToBrowser::Cursor { css } => {
            for surface in &app.surfaces {
                dom::style(&surface.canvas, "cursor", &css);
            }
            app.cursor = css;
        }
        ToBrowser::Ime { window, bounds } => {
            if let Some(((left, top), scale)) = app
                .surface(window)
                .map(|surface| (surface.origin(), surface.scale))
            {
                let [x, y, _, height] = bounds.map(|value| f64::from(value) * scale);
                let text = &app.text;
                dom::style(text, "left", &format!("{}px", left + x));
                dom::style(text, "top", &format!("{}px", top + y + height));
            }
        }
        ToBrowser::Page(op) => {
            let App {
                pages, surfaces, ..
            } = &mut *app;
            pages.apply(op, shared, surfaces);
        }
    }
}

fn open_window(shared: &Shared, id: WindowId, title: &str, main: bool, width: f32, height: f32) {
    let surface = {
        let Ok(mut app) = shared.try_borrow_mut() else {
            return;
        };
        app.surfaces.retain(|surface| surface.id != id);
        // The GPU's own canvas suits the main window best.
        let spare = if main { app.spare.take() } else { None };
        match Surface::new(id, main, width, height, &app.context, spare, &app.root) {
            Ok(surface) => surface,
            Err(error) => {
                dom::show_status(&format!("Herdr UI could not open a window: {error}"));
                return;
            }
        }
    };
    let listeners = input::attach(shared, &surface);
    let Ok(mut app) = shared.try_borrow_mut() else {
        return;
    };
    let mut surface = surface;
    surface.listeners = listeners;
    surface.set_title(title);
    dom::style(&surface.canvas, "cursor", &app.cursor);
    if main {
        app.surfaces.insert(0, surface);
        dom::show_status("");
        app.focus_window(id);
    } else {
        app.surfaces.push(surface);
        app.raise(id);
        app.focus_window(id);
    }
}

/// The animation-frame callback, which schedules itself again.
type FrameLoop = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

/// Every animation frame: reports sizes, draws new frames, and asks for more.
fn animate(shared: Shared) {
    let Some(window) = dom::window() else {
        return;
    };
    let next: FrameLoop = Rc::new(RefCell::new(None));
    let again = next.clone();
    *next.borrow_mut() = Some(Closure::new(move || {
        if let Ok(mut app) = shared.try_borrow_mut() {
            app.frame();
        }
        if let (Some(window), Some(callback)) = (dom::window(), again.borrow().as_ref()) {
            let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }));
    if let Some(callback) = next.borrow().as_ref() {
        let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
    }
    // The loop holds itself for the life of the page.
    std::mem::forget(next);
}

impl App {
    pub(crate) fn send(&self, message: &ToHost) {
        let Some(socket) = self
            .socket
            .as_ref()
            .filter(|socket| socket.ready_state() == 1)
        else {
            return;
        };
        match wire::encode(message) {
            Ok(bytes) => {
                let _ = socket.send_with_u8_array(&bytes);
            }
            Err(error) => dom::log(&format!("Could not encode a message: {error}")),
        }
    }

    fn frame(&mut self) {
        self.flush_move();
        let ratio = dom::window().map_or(1., |window| window.device_pixel_ratio());
        let mut messages = Vec::new();
        for surface in &mut self.surfaces {
            let (viewport, drawn) = surface.animate(ratio, self.zoom);
            messages.extend(viewport);
            if let Some(seq) = drawn {
                messages.push(ToHost::Ack {
                    window: surface.id,
                    seq,
                });
            }
            messages.push(ToHost::Tick { window: surface.id });
        }
        for message in &messages {
            self.send(message);
        }
    }

    pub(crate) fn set_zoom(&mut self, zoom: f64) {
        if !zoom.is_finite() {
            return;
        }
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if let Some(storage) =
            dom::window().and_then(|window| window.local_storage().ok().flatten())
        {
            let _ = storage.set_item(ZOOM_KEY, &self.zoom.to_string());
        }
    }

    /// Sends keys to streamed page `page` until the app's canvas is used.
    pub(crate) fn focus_stream(&mut self, page: wire::PageId) {
        self.pages.set_focus(Some(page));
        if !dom::coarse_pointer() {
            self.keep_keyboard();
        }
    }

    /// Sends `input` to the focused streamed page; whether one has focus.
    pub(crate) fn to_stream(&self, input: wire::StreamInput) -> bool {
        let Some(page) = self.pages.focused_stream() else {
            return false;
        };
        self.send(&ToHost::Page(wire::PageEvent::Input { page, input }));
        true
    }

    /// Shows or hides the phone keyboard, which follows the hidden input.
    pub(crate) fn field(&self) -> &HtmlElement {
        &self.text
    }

    pub(crate) fn show_keyboard(&self, show: bool) {
        if show {
            // A field still focused after the keyboard was closed opens it
            // again only by focusing anew.
            let _ = self.text.blur();
            let _ = self.text.focus();
        } else {
            let _ = self.text.blur();
        }
    }

    pub(crate) fn flush_move(&mut self) {
        if let Some((window, input)) = self.input.pending_move.take() {
            self.send(&ToHost::Input { window, input });
        }
    }

    pub(crate) fn surface(&self, id: WindowId) -> Option<&Surface> {
        self.surfaces.iter().find(|surface| surface.id == id)
    }

    fn surface_mut(&mut self, id: WindowId) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|surface| surface.id == id)
    }

    /// The window keys go to: the focused one, else the main one.
    pub(crate) fn focused_window(&self) -> Option<WindowId> {
        self.focused
            .or_else(|| self.surfaces.first().map(|surface| surface.id))
    }

    pub(crate) fn focus_window(&mut self, id: WindowId) {
        if self.focused == Some(id) {
            return;
        }
        self.focused = Some(id);
        self.send(&ToHost::Focus {
            window: id,
            focused: true,
        });
    }

    /// The browser window gained or lost focus.
    pub(crate) fn page_focus(&mut self, focused: bool) {
        let Some(window) = self.focused_window() else {
            return;
        };
        if focused && !dom::coarse_pointer() {
            self.keep_keyboard();
        }
        self.focused = focused.then_some(window);
        self.send(&ToHost::Focus { window, focused });
    }

    /// Puts the keyboard back on the hidden input.
    pub(crate) fn keep_keyboard(&self) {
        let _ = self.text.focus();
    }

    /// Stacks floating window `id` above the others.
    pub(crate) fn raise(&mut self, id: WindowId) {
        let Some(index) = self
            .surfaces
            .iter()
            .position(|surface| surface.id == id && !surface.main)
        else {
            return;
        };
        let surface = self.surfaces.remove(index);
        self.surfaces.push(surface);
        for (order, surface) in self.surfaces.iter().enumerate() {
            dom::style(&surface.element, "z-index", &order.to_string());
        }
    }

    /// The browser's Back or Forward, as the mouse's side button the app
    /// already takes for its own Back and Forward.
    fn navigate(&self, back: bool) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let (x, y) = match self.input.pointer {
            Some((pointer_window, x, y)) if pointer_window == window => (x, y),
            _ => self.surface(window).map_or((1., 1.), |surface| {
                let rect = surface.area.get_bounding_client_rect();
                (
                    (rect.width() / 2. / surface.scale) as f32,
                    (rect.height() / 2. / surface.scale) as f32,
                )
            }),
        };
        let button = if back { Button::Back } else { Button::Forward };
        let mods = Default::default();
        self.send(&ToHost::Input {
            window,
            input: Input::MouseDown {
                button,
                x,
                y,
                mods,
                clicks: 1,
            },
        });
        self.send(&ToHost::Input {
            window,
            input: Input::MouseUp {
                button,
                x,
                y,
                mods,
                clicks: 1,
            },
        });
    }

    pub(crate) fn page_message(&self, message: &MessageEvent) {
        if let Some(event) = self.pages.event(message) {
            self.send(&ToHost::Page(event));
        }
    }
}
