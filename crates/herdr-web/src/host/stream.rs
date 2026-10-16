//! Web pages browsed on this machine and streamed to the page. A browser
//! tab's web page would load on whichever device shows the app, where an
//! `http` page inside the tunnel's `https` page is blocked and `localhost`
//! names the phone. Instead a headless Chrome here opens it, its pictures go
//! to every page, and their pointer, keys, and text come back to it.
//!
//! One Chrome serves every streamed page, each its own target, over the
//! DevTools protocol. It starts with the first streamed page, off the UI
//! thread, and keeps a profile in the state folder so sign-ins last.
mod cdp;
mod chrome;
mod input;

use super::hub::Hub;
use crate::wire::{PageEvent, PageId, PageOp, StreamInput, WindowId};
use cdp::Cdp;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, PoisonError, Weak},
};

/// Device pixels per CSS pixel in the pictures: sharp on phones and Retina.
const PICTURE_SCALE: f32 = 2.;
/// The binding a page's `window.ipc.postMessage` calls, as a native web
/// view's does; what it posts reaches the app as untrusted text.
const IPC_BINDING: &str = "herdrIpc";
const IPC_SHIM: &str = "window.ipc = { postMessage: function (body) { herdrIpc(String(body)); } };";
const PICTURE_QUALITY: u8 = 70;

/// The app's handler for a page's reports.
pub(super) type Events = Arc<dyn Fn(PageEvent) + Send + Sync>;

/// The Chrome to stream with, if this machine has one.
pub(super) fn available() -> bool {
    chrome::binary().is_some()
}

/// The running Chrome, started on first use and again after it exits.
static BROWSER: Mutex<Option<Arc<Cdp>>> = Mutex::new(None);
static PROFILE: OnceLock<PathBuf> = OnceLock::new();

/// Where Chrome keeps its profile; set once, when serving starts.
pub(super) fn set_profile(dir: PathBuf) {
    let _ = PROFILE.set(dir);
}

fn browser() -> crate::Result<Arc<Cdp>> {
    let mut browser = BROWSER.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(cdp) = browser.as_ref().filter(|cdp| cdp.alive()) {
        return Ok(cdp.clone());
    }
    let profile = PROFILE.get().cloned().ok_or(crate::Error::Unsupported)?;
    let cdp = chrome::launch(&profile)?;
    *browser = Some(cdp.clone());
    Ok(cdp)
}

#[derive(Default)]
struct State {
    window: Option<WindowId>,
    /// The address to open once the target exists.
    url: Option<String>,
    /// The page's size in CSS pixels, from the app's layout.
    size: Option<(f32, f32)>,
    shown: bool,
    cdp: Option<Arc<Cdp>>,
    target: Option<String>,
    session: Option<String>,
    casting: bool,
    /// Asked to close before its target existed.
    closed: bool,
}

/// One streamed page: what the app asked of it, and its Chrome target.
pub(super) struct Stream {
    page: PageId,
    hub: Weak<Hub>,
    events: Events,
    state: Mutex<State>,
}

impl Stream {
    pub(super) fn new(page: PageId, hub: Weak<Hub>, events: Events) -> Arc<Self> {
        Arc::new(Self {
            page,
            hub,
            events,
            state: Mutex::new(State::default()),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn show(&self, op: PageOp) {
        if let Some(hub) = self.hub.upgrade() {
            hub.send_page(op);
        }
    }

    /// Sends a command on this page's session, once it has one.
    fn call(&self, state: &State, method: &str, params: Value) {
        if let (Some(cdp), Some(session)) = (&state.cdp, &state.session) {
            cdp.call(Some(session), method, params, None);
        }
    }

    /// An operation the app sent for this page. The page shows the picture
    /// and its placement; Chrome does everything else.
    pub(super) fn op(self: &Arc<Self>, op: PageOp) {
        match op {
            PageOp::Open { window, url, .. } => {
                {
                    let mut state = self.state();
                    state.window = Some(window);
                    state.url = Some(url);
                }
                self.show(PageOp::Stream {
                    window,
                    page: self.page,
                });
                self.start();
            }
            PageOp::Place { page, bounds } => {
                self.show(PageOp::Place { page, bounds });
                let size = (bounds[2].max(1.), bounds[3].max(1.));
                let mut state = self.state();
                state.shown = true;
                if state.size != Some(size) {
                    state.size = Some(size);
                    self.resize(&mut state);
                } else if !state.casting {
                    self.cast(&mut state);
                }
            }
            PageOp::Hide { page } => {
                self.show(PageOp::Hide { page });
                let mut state = self.state();
                state.shown = false;
                if state.casting {
                    state.casting = false;
                    self.call(&state, "Page.stopScreencast", json!({}));
                }
            }
            PageOp::Close { page } => {
                self.show(PageOp::Close { page });
                let mut state = self.state();
                state.closed = true;
                if let (Some(cdp), Some(target)) = (&state.cdp, &state.target) {
                    cdp.call(
                        None,
                        "Target.closeTarget",
                        json!({ "targetId": target }),
                        None,
                    );
                    if let Some(session) = &state.session {
                        cdp.forget(session);
                    }
                }
            }
            PageOp::Load { url, .. } => {
                let mut state = self.state();
                if state.session.is_some() {
                    self.call(&state, "Page.navigate", json!({ "url": url }));
                } else {
                    state.url = Some(url);
                }
            }
            PageOp::Back { .. } => self.evaluate("history.back()"),
            PageOp::Forward { .. } => self.evaluate("history.forward()"),
            // The app's scripts run in the page, as in a native web view.
            PageOp::Script { script, .. } => self.evaluate(&script),
            PageOp::Reload { .. } => {
                let state = self.state();
                self.call(&state, "Page.reload", json!({}));
            }
            // Keys reach the picture once the page focuses it.
            op @ (PageOp::Focus { .. } | PageOp::Blur { .. }) => self.show(op),
            PageOp::Stream { .. } | PageOp::Frame { .. } => {}
        }
    }

    fn evaluate(&self, expression: &str) {
        let state = self.state();
        self.call(
            &state,
            "Runtime.evaluate",
            json!({ "expression": expression }),
        );
    }

    /// Opens the target off the UI thread: starting Chrome takes a moment.
    fn start(self: &Arc<Self>) {
        let stream = self.clone();
        let spawned = std::thread::Builder::new()
            .name("web-stream-open".into())
            .spawn(move || {
                let cdp = match browser() {
                    Ok(cdp) => cdp,
                    Err(error) => {
                        tracing::warn!(%error, "Could not start Chrome for a browser tab");
                        stream.show(PageOp::Frame {
                            page: stream.page,
                            jpeg: Vec::new(),
                        });
                        return;
                    }
                };
                let url = {
                    let mut state = stream.state();
                    state.cdp = Some(cdp.clone());
                    state.url.take().unwrap_or_else(|| "about:blank".into())
                };
                let opened = stream.clone();
                cdp.call(
                    None,
                    "Target.createTarget",
                    json!({ "url": url }),
                    Some(Box::new(move |result| opened.created(result))),
                );
            });
        if spawned.is_err() {
            tracing::warn!("Could not start a thread for a browser tab");
        }
    }

    fn created(self: &Arc<Self>, result: Option<Value>) {
        let Some(target) = result
            .as_ref()
            .and_then(|result| result["targetId"].as_str())
            .map(str::to_owned)
        else {
            return;
        };
        let cdp = {
            let mut state = self.state();
            state.target = Some(target.clone());
            if state.closed {
                if let Some(cdp) = &state.cdp {
                    cdp.call(
                        None,
                        "Target.closeTarget",
                        json!({ "targetId": target }),
                        None,
                    );
                }
                return;
            }
            state.cdp.clone()
        };
        let Some(cdp) = cdp else {
            return;
        };
        let attached = self.clone();
        cdp.call(
            None,
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
            Some(Box::new(move |result| attached.attached(result))),
        );
    }

    fn attached(self: &Arc<Self>, result: Option<Value>) {
        let Some(session) = result
            .as_ref()
            .and_then(|result| result["sessionId"].as_str())
            .map(str::to_owned)
        else {
            return;
        };
        let mut state = self.state();
        let Some(cdp) = state.cdp.clone() else {
            return;
        };
        let stream = Arc::downgrade(self);
        cdp.route(
            &session,
            Arc::new(move |method: &str, params: &Value| {
                if let Some(stream) = stream.upgrade() {
                    stream.event(method, params);
                }
            }),
        );
        state.session = Some(session);
        for (method, params) in [
            ("Page.enable", json!({})),
            ("Runtime.enable", json!({})),
            ("Runtime.addBinding", json!({ "name": IPC_BINDING })),
            (
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": IPC_SHIM }),
            ),
            ("Runtime.evaluate", json!({ "expression": IPC_SHIM })),
            (
                "Emulation.setFocusEmulationEnabled",
                json!({ "enabled": true }),
            ),
        ] {
            self.call(&state, method, params);
        }
        if let Some(url) = state.url.take() {
            self.call(&state, "Page.navigate", json!({ "url": url }));
        }
        self.resize(&mut state);
    }

    /// Lays the page out at the app's size and restarts its pictures.
    fn resize(&self, state: &mut State) {
        let Some((width, height)) = state.size else {
            return;
        };
        self.call(
            state,
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width.round() as u32,
                "height": height.round() as u32,
                "deviceScaleFactor": PICTURE_SCALE,
                "mobile": false,
            }),
        );
        if state.casting {
            self.call(state, "Page.stopScreencast", json!({}));
            state.casting = false;
        }
        self.cast(state);
    }

    fn cast(&self, state: &mut State) {
        let Some((width, height)) = state.size.filter(|_| state.shown) else {
            return;
        };
        if state.session.is_none() {
            return;
        }
        self.call(
            state,
            "Page.startScreencast",
            json!({
                "format": "jpeg",
                "quality": PICTURE_QUALITY,
                "maxWidth": (width * PICTURE_SCALE).round() as u32,
                "maxHeight": (height * PICTURE_SCALE).round() as u32,
            }),
        );
        state.casting = true;
    }

    /// A DevTools event on this page's session.
    fn event(&self, method: &str, params: &Value) {
        match method {
            "Page.screencastFrame" => {
                let state = self.state();
                // Chrome sends the next picture once this one is taken.
                if let Some(frame) = params["sessionId"].as_u64() {
                    self.call(
                        &state,
                        "Page.screencastFrameAck",
                        json!({ "sessionId": frame }),
                    );
                }
                drop(state);
                if let Some(jpeg) = params["data"].as_str().and_then(input::decode_picture) {
                    self.show(PageOp::Frame {
                        page: self.page,
                        jpeg,
                    });
                }
            }
            "Page.frameNavigated" if params["frame"]["parentId"].is_null() => {
                if let Some(url) = params["frame"]["url"].as_str() {
                    (self.events)(PageEvent::Loaded {
                        page: self.page,
                        url: url.to_owned(),
                    });
                }
                // A page Back restores from Chrome's cache loads nothing,
                // so its title is read here as well as after a load.
                self.read_title();
            }
            "Page.loadEventFired" | "Page.domContentEventFired" => self.read_title(),
            "Runtime.bindingCalled" if params["name"] == IPC_BINDING => {
                if let Some(body) = params["payload"].as_str() {
                    (self.events)(PageEvent::Posted {
                        page: self.page,
                        body: body.to_owned(),
                    });
                }
            }
            "Page.navigatedWithinDocument" => {
                if let Some(url) = params["url"].as_str() {
                    (self.events)(PageEvent::Loaded {
                        page: self.page,
                        url: url.to_owned(),
                    });
                }
            }
            _ => {}
        }
    }

    /// Reports the page's title to the app once it has one.
    fn read_title(&self) {
        let events = self.events.clone();
        let page = self.page;
        let state = self.state();
        let (Some(cdp), Some(session)) = (&state.cdp, &state.session) else {
            return;
        };
        cdp.call(
            Some(session),
            "Runtime.evaluate",
            json!({ "expression": "document.title", "returnByValue": true }),
            Some(Box::new(move |result| {
                if let Some(title) = result
                    .as_ref()
                    .and_then(|result| result["result"]["value"].as_str())
                    .filter(|title| !title.is_empty())
                {
                    events(PageEvent::Title {
                        page,
                        title: title.to_owned(),
                    });
                }
            })),
        );
    }

    /// Pointer, keys, or text a page sent for this one.
    pub(super) fn input(&self, input: StreamInput) {
        let state = self.state();
        for (method, params) in input::commands(&input) {
            self.call(&state, method, params);
        }
    }
}
