//! Browser tabs' pages over their window's canvas: local pages as
//! `<iframe>`s, and web pages as pictures of the page the serving machine
//! browses. Local pages run sandboxed on an opaque origin, so their scripts
//! cannot reach this page or its connection; they talk to the app by
//! `postMessage`.
use crate::{app::Shared, dom, stream, surface::Surface};
use herdr_web::wire::{PageEvent, PageId, PageOp, ToHost, WindowId};
use std::collections::HashMap;
use wasm_bindgen::JsValue;
use web_sys::{
    Blob, BlobPropertyBag, HtmlElement, HtmlIFrameElement, HtmlImageElement, MessageEvent, Url,
};

/// Local pages may script and post; nothing may navigate this page.
const LOCAL_SANDBOX: &str = "allow-scripts allow-forms allow-popups allow-modals allow-downloads";
const WEB_SANDBOX: &str =
    "allow-scripts allow-same-origin allow-forms allow-popups allow-modals allow-downloads";

enum Kind {
    Frame {
        frame: HtmlIFrameElement,
        local: bool,
        _load: dom::Listener,
    },
    Stream {
        image: HtmlImageElement,
        /// The object URL of the picture shown, freed when replaced.
        picture: Option<String>,
        _listeners: Vec<dom::Listener>,
    },
}

struct Page {
    window: WindowId,
    element: HtmlElement,
    kind: Kind,
    /// The page's size in the app's pixels, which are a streamed page's
    /// CSS pixels.
    size: (f32, f32),
}

impl Page {
    fn frame(&self) -> Option<&HtmlIFrameElement> {
        match &self.kind {
            Kind::Frame { frame, .. } => Some(frame),
            Kind::Stream { .. } => None,
        }
    }

    fn local(&self) -> bool {
        matches!(self.kind, Kind::Frame { local: true, .. })
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        if let Kind::Stream {
            picture: Some(url), ..
        } = &self.kind
        {
            let _ = Url::revoke_object_url(url);
        }
        self.element.remove();
    }
}

#[derive(Default)]
pub(crate) struct Pages {
    pages: HashMap<PageId, Page>,
    /// The streamed page keys go to, until the app's canvas is used.
    focused: Option<PageId>,
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
                        element: frame.clone().into(),
                        kind: Kind::Frame {
                            frame,
                            local,
                            _load: load,
                        },
                        size: (0., 0.),
                    },
                );
            }
            PageOp::Stream { window, page } => {
                let Some(surface) = surfaces.iter().find(|surface| surface.id == window) else {
                    return;
                };
                let Some((image, listeners)) = stream::element(shared, page) else {
                    return;
                };
                dom::style(&image, "display", "none");
                let _ = surface.area.append_child(&image);
                self.pages.insert(
                    page,
                    Page {
                        window,
                        element: image.clone().into(),
                        kind: Kind::Stream {
                            image,
                            picture: None,
                            _listeners: listeners,
                        },
                        size: (0., 0.),
                    },
                );
            }
            PageOp::Frame { page, jpeg } => {
                let Some(Page {
                    kind: Kind::Stream { image, picture, .. },
                    ..
                }) = self.pages.get_mut(&page)
                else {
                    return;
                };
                let bytes = js_sys::Uint8Array::from(jpeg.as_slice());
                let parts = js_sys::Array::of1(&bytes);
                let options = BlobPropertyBag::new();
                options.set_type("image/jpeg");
                let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&parts, &options)
                else {
                    return;
                };
                let Ok(url) = Url::create_object_url_with_blob(&blob) else {
                    return;
                };
                image.set_src(&url);
                if let Some(old) = picture.replace(url) {
                    let _ = Url::revoke_object_url(&old);
                }
            }
            PageOp::Place { page, bounds } => {
                if let Some(page) = self.pages.get_mut(&page) {
                    page.size = (bounds[2], bounds[3]);
                    let zoom = surfaces
                        .iter()
                        .find(|surface| surface.id == page.window)
                        .map_or(1., |surface| surface.scale);
                    let [x, y, width, height] = bounds.map(|value| f64::from(value) * zoom);
                    let frame = &page.element;
                    dom::style(frame, "left", &format!("{x}px"));
                    dom::style(frame, "top", &format!("{y}px"));
                    dom::style(frame, "width", &format!("{width}px"));
                    dom::style(frame, "height", &format!("{height}px"));
                    dom::style(frame, "display", "block");
                }
            }
            PageOp::Hide { page } => {
                if let Some(shown) = self.pages.get(&page) {
                    dom::style(&shown.element, "display", "none");
                }
                self.unfocus(page);
            }
            PageOp::Close { page } => {
                self.pages.remove(&page);
                self.unfocus(page);
            }
            PageOp::Load { page, url } => {
                if let Some(frame) = self.pages.get(&page).and_then(Page::frame) {
                    frame.set_src(&url);
                }
            }
            PageOp::Reload { page } => {
                if let Some(frame) = self.pages.get(&page).and_then(Page::frame) {
                    let url = frame.src();
                    frame.set_src(&url);
                }
            }
            PageOp::Script { page, script } => self.post(page, "herdrScript", &script),
            PageOp::Back { page } => self.post(page, "herdrScript", "history.back()"),
            PageOp::Forward { page } => self.post(page, "herdrScript", "history.forward()"),
            PageOp::Focus { page } => match self.pages.get(&page) {
                Some(Page {
                    kind: Kind::Stream { .. },
                    ..
                }) => self.focused = Some(page),
                Some(page) => {
                    let _ = page.element.focus();
                }
                None => {}
            },
            PageOp::Blur { page } => self.unfocus(page),
        }
    }

    /// Sends a local page `{ key: value }`; its bridge acts on it.
    fn post(&self, page: PageId, key: &str, value: &str) {
        let Some(frame) = self
            .pages
            .get(&page)
            .filter(|page| page.local())
            .and_then(Page::frame)
        else {
            return;
        };
        let Some(target) = frame.content_window() else {
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
            page.local()
                && page
                    .frame()
                    .and_then(HtmlIFrameElement::content_window)
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
        self.pages.retain(|_, page| page.window != window);
        if self
            .focused
            .is_some_and(|page| !self.pages.contains_key(&page))
        {
            self.focused = None;
        }
    }

    /// The streamed page keys go to, if one has focus.
    pub(crate) fn focused_stream(&self) -> Option<PageId> {
        self.focused
    }

    pub(crate) fn set_focus(&mut self, page: Option<PageId>) {
        self.focused = page;
    }

    fn unfocus(&mut self, page: PageId) {
        if self.focused == Some(page) {
            self.focused = None;
        }
    }

    /// A point in client pixels as streamed page `page`'s CSS pixels.
    pub(crate) fn stream_point(&self, page: PageId, (x, y): (f64, f64)) -> Option<(f32, f32)> {
        let page = self.pages.get(&page)?;
        let rect = page.element.get_bounding_client_rect();
        if rect.width() <= 0. || rect.height() <= 0. {
            return None;
        }
        Some((
            ((x - rect.left()) / rect.width() * f64::from(page.size.0)) as f32,
            ((y - rect.top()) / rect.height() * f64::from(page.size.1)) as f32,
        ))
    }

    /// The page's CSS pixels per client pixel.
    pub(crate) fn stream_scale(&self, page: PageId) -> Option<f64> {
        let page = self.pages.get(&page)?;
        let width = page.element.get_bounding_client_rect().width();
        (width > 0.).then(|| f64::from(page.size.0) / width)
    }
}
