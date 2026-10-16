//! Browser tabs' pages, as `<iframe>`s over their window's canvas. Local
//! pages run sandboxed on an opaque origin, so their scripts cannot reach
//! this page or its connection; they talk to the app by `postMessage`.
use crate::{app::Shared, dom, surface::Surface};
use herdr_web::wire::{PageEvent, PageId, PageOp, ToHost, WindowId};
use std::collections::HashMap;
use wasm_bindgen::JsValue;
use web_sys::{HtmlIFrameElement, MessageEvent};

/// Local pages may script and post; nothing may navigate this page.
const LOCAL_SANDBOX: &str = "allow-scripts allow-forms allow-popups allow-modals allow-downloads";
const WEB_SANDBOX: &str =
    "allow-scripts allow-same-origin allow-forms allow-popups allow-modals allow-downloads";

struct Page {
    window: WindowId,
    frame: HtmlIFrameElement,
    local: bool,
    _load: dom::Listener,
}

#[derive(Default)]
pub(crate) struct Pages {
    pages: HashMap<PageId, Page>,
}

impl Pages {
    /// Each window's scale maps the app's bounds onto the page's CSS pixels.
    pub(crate) fn apply(&mut self, op: PageOp, shared: &Shared, surfaces: &[Surface]) {
        match op {
            PageOp::Open {
                window,
                page,
                url,
                local,
            } => {
                let Some(surface) = surfaces.iter().find(|surface| surface.id == window) else {
                    return;
                };
                let Some(frame) = dom::create::<HtmlIFrameElement>("iframe") else {
                    return;
                };
                let _ =
                    frame.set_attribute("sandbox", if local { LOCAL_SANDBOX } else { WEB_SANDBOX });
                let _ = frame.set_attribute("allow", "clipboard-write; fullscreen");
                frame.set_src(&url);
                dom::style(&frame, "display", "none");
                let _ = surface.area.append_child(&frame);
                let app = shared.clone();
                let loaded = frame.clone();
                let load = dom::Listener::new(frame.as_ref(), "load", true, move |_| {
                    // Another origin's address cannot be read; the asked one stands in.
                    let url = loaded
                        .content_window()
                        .and_then(|window| window.location().href().ok())
                        .unwrap_or_else(|| loaded.src());
                    if let Ok(app) = app.try_borrow_mut() {
                        app.send(&ToHost::Page(PageEvent::Loaded { page, url }));
                    }
                });
                self.pages.insert(
                    page,
                    Page {
                        window,
                        frame,
                        local,
                        _load: load,
                    },
                );
            }
            PageOp::Place { page, bounds } => {
                if let Some(page) = self.pages.get(&page) {
                    let zoom = surfaces
                        .iter()
                        .find(|surface| surface.id == page.window)
                        .map_or(1., |surface| surface.scale);
                    let [x, y, width, height] = bounds.map(|value| f64::from(value) * zoom);
                    let frame = &page.frame;
                    dom::style(frame, "left", &format!("{x}px"));
                    dom::style(frame, "top", &format!("{y}px"));
                    dom::style(frame, "width", &format!("{width}px"));
                    dom::style(frame, "height", &format!("{height}px"));
                    dom::style(frame, "display", "block");
                }
            }
            PageOp::Hide { page } => {
                if let Some(page) = self.pages.get(&page) {
                    dom::style(&page.frame, "display", "none");
                }
            }
            PageOp::Close { page } => {
                if let Some(page) = self.pages.remove(&page) {
                    page.frame.remove();
                }
            }
            PageOp::Load { page, url } => {
                if let Some(page) = self.pages.get(&page) {
                    page.frame.set_src(&url);
                }
            }
            PageOp::Reload { page } => {
                if let Some(page) = self.pages.get(&page) {
                    let url = page.frame.src();
                    page.frame.set_src(&url);
                }
            }
            PageOp::Script { page, script } => self.post(page, "herdrScript", &script),
            PageOp::Back { page } => self.post(page, "herdrScript", "history.back()"),
            PageOp::Forward { page } => self.post(page, "herdrScript", "history.forward()"),
            PageOp::Focus { page } => {
                if let Some(page) = self.pages.get(&page) {
                    let _ = page.frame.focus();
                }
            }
            PageOp::Blur { .. } => {}
        }
    }

    /// Sends a local page `{ key: value }`; its bridge acts on it.
    fn post(&self, page: PageId, key: &str, value: &str) {
        let Some(page) = self.pages.get(&page).filter(|page| page.local) else {
            return;
        };
        let Some(target) = page.frame.content_window() else {
            return;
        };
        let message = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&message, &JsValue::from_str(key), &JsValue::from_str(value));
        let _ = target.post_message(&message, "*");
    }

    /// A message a page posted: only local pages' own messages count.
    pub(crate) fn event(&self, message: &MessageEvent) -> Option<PageEvent> {
        let source = message.source()?;
        let (id, _) = self.pages.iter().find(|(_, page)| {
            page.local
                && page
                    .frame
                    .content_window()
                    .is_some_and(|window| JsValue::from(window) == JsValue::from(source.clone()))
        })?;
        let data = message.data();
        let text = |key: &str| {
            js_sys::Reflect::get(&data, &JsValue::from_str(key))
                .ok()
                .and_then(|value| value.as_string())
        };
        if let Some(body) = text("herdrPost") {
            return Some(PageEvent::Posted { page: *id, body });
        }
        text("herdrTitle").map(|title| PageEvent::Title { page: *id, title })
    }

    /// Drops the pages of a closed window.
    pub(crate) fn close_window(&mut self, window: WindowId) {
        self.pages.retain(|_, page| {
            let keep = page.window != window;
            if !keep {
                page.frame.remove();
            }
            keep
        });
    }
}
