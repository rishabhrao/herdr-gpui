//! A window drawn by the page. Its size, focus, and input come from the
//! page; GPUI's frames go back to it, one in flight at a time.
use super::{atlas::RemoteAtlas, hub::Hub, input};
use crate::wire::{Input, TextOp, ToBrowser, WindowId, WireScene};
use futures::channel::oneshot;
use gpui::{
    AnyWindowHandle, Bounds, Capslock, DispatchEventResult, GpuSpecs, Modifiers, Pixels,
    PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point,
    PromptButton, PromptLevel, RequestFrameOptions, Scene, Size, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowVisibility,
};
use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

/// A page that has not acknowledged a frame for this long gets the next
/// one anyway, so a lost acknowledgement cannot freeze the window.
const FRAME_TIMEOUT: Duration = Duration::from_secs(1);
/// The shortest time between frames: about 60 a second. A page on a 120 Hz
/// screen asks every 8 ms, and a window that animates all the time would
/// otherwise send it twice the frames for no gain it can see. A little
/// under 1/60 s, so a 60 Hz page's ticks, which jitter, are never skipped.
const FRAME_GAP: Duration = Duration::from_millis(13);
/// What a window assumes before the page reports its pixel ratio.
const DEFAULT_SCALE: f32 = 2.;

type ResizeCallback = Box<dyn FnMut(Size<Pixels>, f32)>;

#[derive(Default)]
struct Callbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active_status_change: Option<Box<dyn FnMut(bool)>>,
    visibility_change: Option<Box<dyn FnMut(WindowVisibility)>>,
    hover_status_change: Option<Box<dyn FnMut(bool)>>,
    resize: Option<ResizeCallback>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    appearance_changed: Option<Box<dyn FnMut()>>,
    hit_test_window_control: Option<Box<dyn FnMut() -> Option<WindowControlArea>>>,
}

struct State {
    size: Size<Pixels>,
    scale: f32,
    title: String,
    input_handler: Option<PlatformInputHandler>,
    mouse: Point<Pixels>,
    modifiers: Modifiers,
    capslock: Capslock,
    active: bool,
    hovered: bool,
    visible: bool,
    background: WindowBackgroundAppearance,
    seq: u64,
    in_flight: Option<(u64, Instant)>,
    /// When the last frame was asked of GPUI.
    asked: Option<Instant>,
    /// The next frame must repaint everything: the page lost its picture.
    force: bool,
}

pub(super) struct WindowState {
    pub(super) id: WindowId,
    pub(super) handle: AnyWindowHandle,
    pub(super) main: bool,
    hub: Arc<Hub>,
    atlas: Arc<RemoteAtlas>,
    display: Option<Rc<dyn PlatformDisplay>>,
    appearance: Rc<Cell<WindowAppearance>>,
    state: RefCell<State>,
    callbacks: RefCell<Callbacks>,
}

impl WindowState {
    pub(super) fn new(
        handle: AnyWindowHandle,
        main: bool,
        title: String,
        size: Size<Pixels>,
        hub: Arc<Hub>,
        display: Option<Rc<dyn PlatformDisplay>>,
        appearance: Rc<Cell<WindowAppearance>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            id: handle.window_id().as_u64(),
            handle,
            main,
            hub,
            atlas: Arc::default(),
            display,
            appearance,
            state: RefCell::new(State {
                size,
                scale: DEFAULT_SCALE,
                title,
                input_handler: None,
                mouse: Point::default(),
                modifiers: Modifiers::default(),
                capslock: Capslock::default(),
                active: false,
                hovered: false,
                visible: true,
                background: WindowBackgroundAppearance::Opaque,
                seq: 0,
                in_flight: None,
                asked: None,
                force: true,
            }),
            callbacks: RefCell::default(),
        })
    }

    /// Runs one callback with the borrow released, so it may call back in.
    fn with_callback<C: ?Sized, R>(
        &self,
        slot: impl Fn(&mut Callbacks) -> &mut Option<Box<C>>,
        call: impl FnOnce(&mut Box<C>) -> R,
    ) -> Option<R> {
        let mut callback = slot(&mut self.callbacks.borrow_mut()).take()?;
        let result = call(&mut callback);
        let mut callbacks = self.callbacks.borrow_mut();
        let place = slot(&mut callbacks);
        if place.is_none() {
            *place = Some(callback);
        }
        Some(result)
    }

    fn with_input_handler<R>(
        &self,
        call: impl FnOnce(&mut PlatformInputHandler) -> R,
    ) -> Option<R> {
        let mut handler = self.state.borrow_mut().input_handler.take()?;
        let result = call(&mut handler);
        let mut state = self.state.borrow_mut();
        if state.input_handler.is_none() {
            state.input_handler = Some(handler);
        }
        Some(result)
    }

    /// Sends to page `to`, or to every page.
    fn post(&self, to: Option<u64>, message: &ToBrowser) {
        match to {
            Some(link) => self.hub.send_to(link, message),
            None => self.hub.send(message),
        }
    }

    fn layout(&self) -> ToBrowser {
        let state = self.state.borrow();
        ToBrowser::Layout {
            window: self.id,
            width: f32::from(state.size.width),
            height: f32::from(state.size.height),
            scale: state.scale,
        }
    }

    /// Tells a newly connected page, or every page, about this window, its
    /// size, and every tile it draws with, and repaints it in full.
    pub(super) fn announce(&self, to: Option<u64>) {
        let (title, size) = {
            let mut state = self.state.borrow_mut();
            state.in_flight = None;
            state.force = true;
            (state.title.clone(), state.size)
        };
        self.post(
            to,
            &ToBrowser::WindowOpened {
                window: self.id,
                title,
                main: self.main,
                width: f32::from(size.width),
                height: f32::from(size.height),
            },
        );
        self.post(to, &self.layout());
        let tiles = self.atlas.everything();
        if !tiles.is_empty() {
            self.post(
                to,
                &ToBrowser::Upload {
                    window: self.id,
                    tiles,
                },
            );
        }
    }

    pub(super) fn disconnected(&self) {
        self.state.borrow_mut().in_flight = None;
        self.focus(false);
    }

    pub(super) fn viewport(&self, width: f32, height: f32, scale: f32) {
        if !(width.is_finite() && height.is_finite() && scale.is_finite()) || scale <= 0. {
            return;
        }
        let size = Size {
            width: gpui::px(width.max(1.)),
            height: gpui::px(height.max(1.)),
        };
        {
            let mut state = self.state.borrow_mut();
            if state.size == size && state.scale == scale {
                return;
            }
            state.size = size;
            state.scale = scale;
            state.force = true;
        }
        self.hub.send(&self.layout());
        self.with_callback(
            |callbacks| &mut callbacks.resize,
            |resize| resize(size, scale),
        );
    }

    /// The page can show another frame: GPUI draws one if anything changed.
    pub(super) fn tick(&self) {
        let force = {
            let mut state = self.state.borrow_mut();
            if let Some((_, sent)) = state.in_flight {
                if sent.elapsed() < FRAME_TIMEOUT {
                    return;
                }
                state.in_flight = None;
            }
            let now = Instant::now();
            if state
                .asked
                .is_some_and(|asked| now.duration_since(asked) < FRAME_GAP)
            {
                return;
            }
            state.asked = Some(now);
            std::mem::take(&mut state.force)
        };
        self.with_callback(
            |callbacks| &mut callbacks.request_frame,
            |request| {
                request(RequestFrameOptions {
                    require_presentation: false,
                    force_render: force,
                })
            },
        );
    }

    pub(super) fn ack(&self, seq: u64) {
        let mut state = self.state.borrow_mut();
        if state.in_flight.is_some_and(|(sent, _)| sent == seq) {
            state.in_flight = None;
        }
    }

    pub(super) fn input(&self, input: Input) {
        let typed = match &input {
            Input::KeyDown { key_char, mods, .. } => {
                key_char.clone().filter(|_| input::inserts_text(*mods))
            }
            _ => None,
        };
        if let Input::Modifiers { mods, capslock } = &input {
            let mut state = self.state.borrow_mut();
            state.modifiers = input::modifiers(*mods);
            state.capslock = Capslock { on: *capslock };
        }
        let (event, position) = input::platform_input(input);
        if let Some(position) = position {
            self.state.borrow_mut().mouse = position;
        }
        let result = self.with_callback(|callbacks| &mut callbacks.input, |input| input(event));
        // As on macOS: a key nothing bound types its character.
        if let Some(text) = typed
            && result.is_none_or(|result| result.propagate)
        {
            self.with_input_handler(|handler| handler.replace_text_in_range(None, &text));
        }
    }

    pub(super) fn text(&self, op: TextOp) {
        self.with_input_handler(|handler| match op {
            TextOp::Insert(text) | TextOp::Commit(text) => {
                handler.replace_text_in_range(None, &text);
            }
            TextOp::Compose { text, selected } => {
                let selected = selected.map(|(start, end)| start as usize..end as usize);
                handler.replace_and_mark_text_in_range(None, &text, selected);
            }
            TextOp::Cancel => handler.unmark_text(),
        });
    }

    pub(super) fn focus(&self, focused: bool) {
        {
            let mut state = self.state.borrow_mut();
            if state.active == focused {
                return;
            }
            state.active = focused;
        }
        self.with_callback(
            |callbacks| &mut callbacks.active_status_change,
            |change| change(focused),
        );
    }

    pub(super) fn visible(&self, visible: bool) {
        {
            let mut state = self.state.borrow_mut();
            if state.visible == visible {
                return;
            }
            state.visible = visible;
            state.force |= visible;
        }
        let visibility = if visible {
            WindowVisibility::Visible
        } else {
            WindowVisibility::Hidden
        };
        self.with_callback(
            |callbacks| &mut callbacks.visibility_change,
            |change| change(visibility),
        );
    }

    pub(super) fn hover(&self, hovered: bool) {
        self.state.borrow_mut().hovered = hovered;
        self.with_callback(
            |callbacks| &mut callbacks.hover_status_change,
            |change| change(hovered),
        );
    }

    pub(super) fn appearance_changed(&self) {
        self.with_callback(
            |callbacks| &mut callbacks.appearance_changed,
            |changed| changed(),
        );
    }

    /// The page's close button: closes the window if the app agrees.
    pub(super) fn request_close(&self) {
        let close = self
            .with_callback(|callbacks| &mut callbacks.should_close, |should| should())
            .unwrap_or(true);
        if !close {
            return;
        }
        let callback = self.callbacks.borrow_mut().close.take();
        if let Some(callback) = callback {
            callback();
        }
    }
}

pub(super) struct RemoteWindow {
    pub(super) state: Rc<WindowState>,
    pub(super) platform: Rc<super::platform::Shared>,
}

impl Drop for RemoteWindow {
    fn drop(&mut self) {
        self.platform.forget(self.state.id);
        self.state.hub.send(&ToBrowser::WindowClosed {
            window: self.state.id,
        });
    }
}

impl HasWindowHandle for RemoteWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, HandleError> {
        // The window is a canvas in another process; there is no native one.
        Err(HandleError::NotSupported)
    }
}

impl HasDisplayHandle for RemoteWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::NotSupported)
    }
}

impl PlatformWindow for RemoteWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: Point::default(),
            size: self.state.state.borrow().size,
        }
    }

    fn is_maximized(&self) -> bool {
        self.state.main
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.state.state.borrow().size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        // The page lays windows out; a floating one opens at its asked size.
        if !self.state.main {
            self.state.state.borrow_mut().size = size;
        }
    }

    fn scale_factor(&self) -> f32 {
        self.state.state.borrow().scale
    }

    fn appearance(&self) -> WindowAppearance {
        self.state.appearance.get()
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.state.display.clone()
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.state.state.borrow().mouse
    }

    fn modifiers(&self) -> Modifiers {
        self.state.state.borrow().modifiers
    }

    fn capslock(&self) -> Capslock {
        self.state.state.borrow().capslock
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.state.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.state.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        let level = match level {
            PromptLevel::Info => 0,
            PromptLevel::Warning => 1,
            PromptLevel::Critical => 2,
        };
        let answers = answers
            .iter()
            .map(|answer| answer.label().to_string())
            .collect();
        Some(self.platform.prompt(level, msg, detail, answers))
    }

    fn activate(&self) {
        self.platform.set_active(Some(self.state.handle));
        self.state.hub.send(&ToBrowser::Activate {
            window: self.state.id,
        });
    }

    fn is_active(&self) -> bool {
        self.state.state.borrow().active
    }

    fn visibility(&self) -> WindowVisibility {
        if self.state.state.borrow().visible {
            WindowVisibility::Visible
        } else {
            WindowVisibility::Hidden
        }
    }

    fn is_hovered(&self) -> bool {
        self.state.state.borrow().hovered
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.state.state.borrow().background
    }

    fn set_title(&mut self, title: &str) {
        self.state.state.borrow_mut().title = title.to_owned();
        self.state.hub.send(&ToBrowser::Title {
            window: self.state.id,
            title: title.to_owned(),
        });
    }

    fn get_title(&self) -> String {
        self.state.state.borrow().title.clone()
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        // A page cannot blur what is behind the browser; it stays opaque.
        self.state.state.borrow_mut().background = background_appearance;
    }

    fn minimize(&self) {}

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {}

    fn is_fullscreen(&self) -> bool {
        false
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.state.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.state.callbacks.borrow_mut().input = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.state.callbacks.borrow_mut().active_status_change = Some(callback);
    }

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        self.state.callbacks.borrow_mut().visibility_change = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.state.callbacks.borrow_mut().hover_status_change = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.state.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.state.callbacks.borrow_mut().moved = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.state.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        self.state.callbacks.borrow_mut().hit_test_window_control = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.state.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.state.callbacks.borrow_mut().appearance_changed = Some(callback);
    }

    fn draw(&self, scene: &Scene) {
        let (uploads, removed) = self.state.atlas.take_changes();
        // A page that connects later gets every tile at once.
        if !self.state.hub.is_connected() {
            return;
        }
        let id = self.state.id;
        if !uploads.is_empty() {
            self.state.hub.send(&ToBrowser::Upload {
                window: id,
                tiles: uploads,
            });
        }
        if !removed.is_empty() {
            self.state.hub.send(&ToBrowser::Remove {
                window: id,
                tiles: removed,
            });
        }
        let seq = {
            let mut state = self.state.state.borrow_mut();
            state.seq += 1;
            state.in_flight = Some((state.seq, Instant::now()));
            state.seq
        };
        self.state.hub.send(&ToBrowser::Frame {
            window: id,
            seq,
            scene: WireScene::from_scene(scene),
        });
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.state.atlas.clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        None
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        self.state.hub.send(&ToBrowser::Ime {
            window: self.state.id,
            bounds: [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ],
        });
    }
}
