//! The native platform's services, with windows that live in the page. The
//! run loop, executors, text system, clipboard, keychain, and file dialogs
//! stay native: the page is on this machine, so they are the user's own.
use super::{
    hub::{Hub, Inbound},
    peers::{Left, Peers},
    window::{RemoteWindow, WindowState},
};
use crate::wire::{Input, PROTOCOL, ToBrowser, ToHost, WindowId};
use futures::channel::oneshot;
use gpui::{
    Action, ActivityGuard, AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle,
    ForegroundExecutor, Keymap, Menu, MenuItem, OwnedMenu, PathPromptOptions, Platform,
    PlatformDisplay, PlatformKeyboardLayout, PlatformKeyboardMapper, PlatformTextSystem,
    PlatformWindow, SystemNotification, SystemNotificationResponse, Task, ThermalState,
    WindowAppearance, WindowKind, WindowParams,
};
use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::Arc,
};

/// What the platform and its windows share on the UI thread.
pub(super) struct Shared {
    hub: Arc<Hub>,
    windows: RefCell<Vec<Weak<WindowState>>>,
    active: Cell<Option<AnyWindowHandle>>,
    appearance: Rc<Cell<WindowAppearance>>,
    /// Every connected page, and the one used last, which the windows
    /// lay out for.
    peers: RefCell<Peers>,
    /// The remote page's clipboard as this app knows it: what the app
    /// copied, or what the page sent before a paste. The Mac's own
    /// pasteboard is left alone while a phone is the page used last.
    remote_clipboard: RefCell<Option<ClipboardItem>>,
    /// A page just sent its clipboard for a paste: the next read takes it,
    /// even from a page on this Mac, whose image would otherwise be lost.
    page_paste: Cell<bool>,
    cursor: RefCell<&'static str>,
    /// Questions the page shows as its own dialogs, until answered. A page
    /// that connects later is shown them too.
    prompts: RefCell<Vec<Prompt>>,
    next_prompt: Cell<u64>,
}

pub(super) struct Prompt {
    id: u64,
    message: ToBrowser,
    answer: oneshot::Sender<usize>,
}

impl Shared {
    fn live(&self) -> Vec<Rc<WindowState>> {
        let mut windows = self.windows.borrow_mut();
        windows.retain(|window| window.strong_count() > 0);
        let mut live: Vec<_> = windows.iter().filter_map(Weak::upgrade).collect();
        // The main window first, so the page lays the others over it.
        live.sort_by_key(|window| !window.main);
        live
    }

    fn find(&self, id: WindowId) -> Option<Rc<WindowState>> {
        self.live().into_iter().find(|window| window.id == id)
    }

    pub(super) fn forget(&self, id: WindowId) {
        self.windows
            .borrow_mut()
            .retain(|window| window.upgrade().is_some_and(|window| window.id != id));
        if self
            .active
            .get()
            .is_some_and(|active| active.window_id().as_u64() == id)
        {
            self.active.set(None);
        }
    }

    pub(super) fn set_active(&self, handle: Option<AnyWindowHandle>) {
        self.active.set(handle);
    }

    /// Shows a question as the page's own dialog. GPUI's drawn fallback
    /// dialog never gets its clicks under this app, whose own handlers
    /// take the mouse first.
    pub(super) fn prompt(
        &self,
        level: u8,
        message: &str,
        detail: Option<&str>,
        answers: Vec<String>,
    ) -> oneshot::Receiver<usize> {
        let id = self.next_prompt.get() + 1;
        self.next_prompt.set(id);
        let (answer, receiver) = oneshot::channel();
        let message = ToBrowser::Prompt {
            id,
            level,
            message: message.to_owned(),
            detail: detail.map(str::to_owned),
            answers,
        };
        self.hub.send(&message);
        self.prompts.borrow_mut().push(Prompt {
            id,
            message,
            answer,
        });
        receiver
    }

    /// Closes the page's dialogs whose asker stopped waiting.
    fn sweep_prompts(&self) {
        let closed: Vec<u64> = {
            let mut prompts = self.prompts.borrow_mut();
            let closed = prompts
                .iter()
                .filter(|prompt| prompt.answer.is_canceled())
                .map(|prompt| prompt.id)
                .collect();
            prompts.retain(|prompt| !prompt.answer.is_canceled());
            closed
        };
        for id in closed {
            self.hub.send(&ToBrowser::PromptClosed { id });
        }
    }

    /// Whether the page used last is on another device.
    fn is_remote(&self) -> bool {
        self.peers.borrow().driver_is_remote()
    }

    /// Sends to the page used last: a link to open, a copy, a note.
    fn to_driver(&self, message: &ToBrowser) {
        let driver = self.peers.borrow().driver();
        if let Some(driver) = driver {
            self.hub.send_to(driver, message);
        }
    }

    fn toast(&self, text: &str) {
        self.to_driver(&ToBrowser::Toast {
            text: text.to_owned(),
        });
    }

    /// Page `link` is the one used now: the windows take its size, light
    /// or dark, and focus.
    fn take_control(&self, link: u64) {
        let Some(peer) = self.peers.borrow_mut().drive(link) else {
            return;
        };
        for window in self.live() {
            if let Some((width, height, scale)) = peer.viewports.get(&window.id) {
                window.viewport(*width, *height, *scale);
            }
        }
        if let Some(dark) = peer.dark {
            self.set_dark(dark);
        }
    }

    fn set_dark(&self, dark: bool) {
        let appearance = if dark {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        };
        if self.appearance.replace(appearance) != appearance {
            for window in self.live() {
                window.appearance_changed();
            }
        }
    }

    /// A file dialog would open on the Mac, out of a remote user's sight:
    /// answer as if it was cancelled, and say why.
    fn no_dialog<T>(&self) -> oneshot::Receiver<anyhow::Result<Option<T>>> {
        self.toast("File pickers open on the Mac; use this from the Mac to choose a file.");
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Ok(None));
        receiver
    }

    fn handle(&self, inbound: Inbound) {
        self.sweep_prompts();
        match inbound {
            Inbound::Connected(link, visitor) => {
                self.peers.borrow_mut().add(link, visitor.remote);
                // Caught up alone, then it gets what every page gets.
                self.hub
                    .send_to(link, &ToBrowser::Hello { protocol: PROTOCOL });
                self.hub.send_to(
                    link,
                    &ToBrowser::Cursor {
                        css: (*self.cursor.borrow()).to_owned(),
                    },
                );
                for window in self.live() {
                    window.announce(Some(link));
                }
                for prompt in self.prompts.borrow().iter() {
                    self.hub.send_to(link, &prompt.message);
                }
                self.hub.replay_pages(link);
                self.hub.ready(link);
            }
            Inbound::Disconnected(link) => {
                let left = self.peers.borrow_mut().remove(link);
                match left {
                    // The page used before it takes over again.
                    Left::Driver(next) => self.take_control(next),
                    Left::Last => {
                        for window in self.live() {
                            window.disconnected();
                        }
                    }
                    Left::Unknown | Left::Other => {}
                }
                self.update_visible();
            }
            Inbound::Message(link, message) => {
                if self.peers.borrow().contains(link) {
                    self.message(link, message);
                }
            }
        }
    }

    /// The windows are visible while any page shows them.
    fn update_visible(&self) {
        let visible = self.peers.borrow().any_visible();
        for window in self.live() {
            window.visible(visible);
        }
    }

    fn message(&self, link: u64, message: ToHost) {
        // Settings' "this device" actions reach the page being handled.
        self.hub.set_acting(Some(link));
        self.handle_message(link, message);
        self.hub.set_acting(None);
    }

    fn handle_message(&self, link: u64, message: ToHost) {
        let driving = self.peers.borrow().driver() == Some(link);
        match message {
            ToHost::Hello { .. } | ToHost::Page(_) => {}
            ToHost::PromptAnswer { id, answer } => {
                let found = {
                    let mut prompts = self.prompts.borrow_mut();
                    prompts
                        .iter()
                        .position(|prompt| prompt.id == id)
                        .map(|index| prompts.remove(index))
                };
                if let Some(prompt) = found {
                    let _ = prompt.answer.send(answer as usize);
                    // The other pages close the same dialog.
                    self.hub.send(&ToBrowser::PromptClosed { id });
                }
            }
            ToHost::ClipboardImage { mime, bytes } => {
                self.take_control(link);
                if let Some(format) = image_format(&mime) {
                    *self.remote_clipboard.borrow_mut() =
                        Some(gpui::Image::from_bytes(format, bytes).into());
                    self.page_paste.set(true);
                }
            }
            ToHost::Clipboard { text } => {
                // A paste follows, from this page.
                self.take_control(link);
                *self.remote_clipboard.borrow_mut() = Some(ClipboardItem::new_string(text));
                self.page_paste.set(true);
            }
            ToHost::Viewport {
                window,
                width,
                height,
                scale,
            } => {
                self.peers
                    .borrow_mut()
                    .set_viewport(link, window, (width, height, scale));
                if driving && let Some(window) = self.find(window) {
                    window.viewport(width, height, scale);
                }
            }
            ToHost::Tick { window } => {
                if let Some(window) = self.find(window) {
                    window.tick();
                }
            }
            ToHost::Ack { window, seq } => {
                if let Some(window) = self.find(window) {
                    window.ack(seq);
                }
            }
            ToHost::Input { window, input } => {
                // Moving the pointer over another page does not take the
                // app; a key does, at once. A click does once it ends, so
                // it lands where that page showed it, before the layout
                // changes to fit that page.
                let after_click = !driving && matches!(input, Input::MouseUp { .. });
                if !driving {
                    match input {
                        Input::KeyDown { .. } => self.take_control(link),
                        Input::MouseMove { .. }
                        | Input::MouseExit { .. }
                        | Input::Modifiers { .. } => return,
                        _ => {}
                    }
                }
                if let Some(window) = self.find(window) {
                    window.input(input);
                }
                if after_click {
                    self.take_control(link);
                }
            }
            ToHost::Text { window, op } => {
                self.take_control(link);
                if let Some(window) = self.find(window) {
                    window.text(op);
                }
            }
            ToHost::Focus { window, focused } => {
                if focused {
                    self.take_control(link);
                } else if !driving {
                    // Another page losing focus says nothing about this one.
                    return;
                }
                for other in self.live() {
                    other.focus(focused && other.id == window);
                    if focused && other.id == window {
                        self.active.set(Some(other.handle));
                    }
                }
            }
            ToHost::Visible { visible } => {
                self.peers.borrow_mut().set_visible(link, visible);
                if visible {
                    self.take_control(link);
                }
                self.update_visible();
            }
            ToHost::Hover { window, hovered } => {
                if driving && let Some(window) = self.find(window) {
                    window.hover(hovered);
                }
            }
            ToHost::Close { window } => {
                if let Some(window) = self.find(window) {
                    window.request_close();
                }
            }
            ToHost::Dark { dark } => {
                self.peers.borrow_mut().set_dark(link, dark);
                if driving {
                    self.set_dark(dark);
                }
            }
        }
    }
}

pub(super) struct RemotePlatform {
    inner: Rc<dyn Platform>,
    shared: Rc<Shared>,
    inbound: RefCell<Option<async_channel::Receiver<Inbound>>>,
}

impl RemotePlatform {
    pub(super) fn new(
        inner: Rc<dyn Platform>,
        hub: Arc<Hub>,
        inbound: async_channel::Receiver<Inbound>,
    ) -> Self {
        let appearance = Rc::new(Cell::new(inner.window_appearance()));
        Self {
            inner,
            shared: Rc::new(Shared {
                hub,
                windows: RefCell::default(),
                active: Cell::new(None),
                appearance,
                peers: RefCell::default(),
                remote_clipboard: RefCell::new(None),
                page_paste: Cell::new(false),
                cursor: RefCell::new("default"),
                prompts: RefCell::default(),
                next_prompt: Cell::new(0),
            }),
            inbound: RefCell::new(Some(inbound)),
        }
    }
}

/// The image format of a pasted image's MIME type.
fn image_format(mime: &str) -> Option<gpui::ImageFormat> {
    use gpui::ImageFormat;
    Some(match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        "image/bmp" => ImageFormat::Bmp,
        "image/tiff" => ImageFormat::Tiff,
        _ => return None,
    })
}

fn cursor_css(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::Arrow => "default",
        CursorStyle::IBeam => "text",
        CursorStyle::Crosshair => "crosshair",
        CursorStyle::ClosedHand => "grabbing",
        CursorStyle::OpenHand => "grab",
        CursorStyle::PointingHand => "pointer",
        CursorStyle::ResizeLeft | CursorStyle::ResizeRight | CursorStyle::ResizeLeftRight => {
            "ew-resize"
        }
        CursorStyle::ResizeUp | CursorStyle::ResizeDown | CursorStyle::ResizeUpDown => "ns-resize",
        CursorStyle::ResizeUpLeftDownRight => "nesw-resize",
        CursorStyle::ResizeUpRightDownLeft => "nwse-resize",
        CursorStyle::ResizeColumn => "col-resize",
        CursorStyle::ResizeRow => "row-resize",
        CursorStyle::IBeamCursorForVerticalLayout => "vertical-text",
        CursorStyle::OperationNotAllowed => "not-allowed",
        CursorStyle::DragLink => "alias",
        CursorStyle::DragCopy => "copy",
        CursorStyle::ContextualMenu => "context-menu",
    }
}

impl Platform for RemotePlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.inner.background_executor()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.inner.foreground_executor()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.inner.text_system()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        if let Some(inbound) = self.inbound.borrow_mut().take() {
            let shared = self.shared.clone();
            // Applies the page's messages in order, on the UI thread.
            self.inner
                .foreground_executor()
                .spawn(async move {
                    while let Ok(message) = inbound.recv().await {
                        shared.handle(message);
                    }
                })
                .detach();
        }
        self.inner.run(on_finish_launching);
    }

    fn quit(&self) {
        self.inner.quit();
    }

    fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<OsString>) {
        self.inner.restart(binary_path, arguments);
    }

    // The app has no Dock presence or windows of its own to raise or hide.
    fn activate(&self, _ignoring_other_apps: bool) {}

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.inner.displays()
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.inner.primary_display()
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.shared.active.get()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        let main = matches!(options.kind, WindowKind::Normal)
            && !self.shared.live().iter().any(|window| window.main);
        let title = options
            .titlebar
            .as_ref()
            .and_then(|titlebar| titlebar.title.as_ref())
            .map(ToString::to_string)
            .unwrap_or_default();
        let state = WindowState::new(
            handle,
            main,
            title,
            options.bounds.size,
            self.shared.hub.clone(),
            self.inner.primary_display(),
            self.shared.appearance.clone(),
        );
        self.shared.windows.borrow_mut().push(Rc::downgrade(&state));
        if options.focus || self.shared.active.get().is_none() {
            self.shared.active.set(Some(handle));
        }
        if self.shared.hub.is_connected() {
            state.announce(None);
        }
        Ok(Box::new(RemoteWindow {
            state,
            platform: self.shared.clone(),
        }))
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.shared.appearance.get()
    }

    fn open_url(&self, url: &str) {
        if self.shared.is_remote() {
            self.shared.to_driver(&ToBrowser::OpenUrl {
                url: url.to_owned(),
            });
            return;
        }
        self.inner.open_url(url);
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.inner.on_open_urls(callback);
    }

    fn register_url_scheme(&self, url: &str) -> Task<anyhow::Result<()>> {
        self.inner.register_url_scheme(url)
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>> {
        if self.shared.is_remote() {
            return self.shared.no_dialog();
        }
        self.inner.prompt_for_paths(options)
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<anyhow::Result<Option<PathBuf>>> {
        if self.shared.is_remote() {
            return self.shared.no_dialog();
        }
        self.inner.prompt_for_new_path(directory, suggested_name)
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        self.inner.can_select_mixed_files_and_dirs()
    }

    fn reveal_path(&self, path: &Path) {
        if self.shared.is_remote() {
            self.shared
                .toast("Finder is on the Mac; it cannot open here.");
            return;
        }
        self.inner.reveal_path(path);
    }

    fn open_with_system(&self, path: &Path) {
        if self.shared.is_remote() {
            self.shared
                .toast("This file opens with an app on the Mac; it cannot open here.");
            return;
        }
        self.inner.open_with_system(path);
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.inner.on_quit(callback);
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_reopen(callback);
    }

    fn on_system_sleep(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_sleep(callback);
    }

    fn on_system_wake(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_wake(callback);
    }

    // The menu bar belongs to the browser; key bindings still work.
    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        None
    }

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.inner.on_app_menu_action(callback);
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_will_open_app_menu(callback);
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.inner.on_validate_app_menu_command(callback);
    }

    fn thermal_state(&self) -> ThermalState {
        self.inner.thermal_state()
    }

    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_thermal_state_change(callback);
    }

    fn prevent_idle_sleep(&self, reason: &str) -> Task<anyhow::Result<ActivityGuard>> {
        self.inner.prevent_idle_sleep(reason)
    }

    fn set_app_identity(&self, identifier: &str, name: &str) {
        self.inner.set_app_identity(identifier, name);
    }

    fn show_system_notification(&self, notification: SystemNotification) {
        if self.shared.is_remote() {
            self.shared.to_driver(&ToBrowser::Notify {
                title: notification.title.to_string(),
                body: notification.body.to_string(),
            });
        }
        self.inner.show_system_notification(notification);
    }

    fn dismiss_system_notification(&self, tag: &str) {
        self.inner.dismiss_system_notification(tag);
    }

    fn on_system_notification_response(
        &self,
        callback: Box<dyn FnMut(SystemNotificationResponse)>,
    ) {
        self.inner.on_system_notification_response(callback);
    }

    fn compositor_name(&self) -> &'static str {
        "browser"
    }

    fn app_path(&self) -> anyhow::Result<PathBuf> {
        self.inner.app_path()
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> anyhow::Result<PathBuf> {
        self.inner.path_for_auxiliary_executable(name)
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        let css = cursor_css(style);
        if self.shared.cursor.replace(css) != css {
            self.shared.hub.send(&ToBrowser::Cursor {
                css: css.to_owned(),
            });
        }
    }

    fn hide_cursor_until_mouse_moves(&self) {}

    fn is_cursor_visible(&self) -> bool {
        true
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        self.inner.should_auto_hide_scrollbars()
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        if self.shared.page_paste.replace(false) || self.shared.is_remote() {
            return self.shared.remote_clipboard.borrow().clone();
        }
        self.inner.read_from_clipboard()
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        if self.shared.is_remote() {
            if let Some(text) = item.text() {
                self.shared.to_driver(&ToBrowser::Clipboard { text });
            }
            *self.shared.remote_clipboard.borrow_mut() = Some(item);
            return;
        }
        self.inner.write_to_clipboard(item);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.inner.read_from_primary()
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn write_to_primary(&self, item: ClipboardItem) {
        self.inner.write_to_primary(item);
    }

    #[cfg(target_os = "macos")]
    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
        self.inner.read_from_find_pasteboard()
    }

    #[cfg(target_os = "macos")]
    fn write_to_find_pasteboard(&self, item: ClipboardItem) {
        self.inner.write_to_find_pasteboard(item);
    }

    fn write_credentials(
        &self,
        url: &str,
        username: &str,
        password: &[u8],
    ) -> Task<anyhow::Result<()>> {
        self.inner.write_credentials(url, username, password)
    }

    fn read_credentials(&self, url: &str) -> Task<anyhow::Result<Option<(String, Vec<u8>)>>> {
        self.inner.read_credentials(url)
    }

    fn delete_credentials(&self, url: &str) -> Task<anyhow::Result<()>> {
        self.inner.delete_credentials(url)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        self.inner.keyboard_layout()
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        self.inner.keyboard_mapper()
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_keyboard_layout_change(callback);
    }
}
