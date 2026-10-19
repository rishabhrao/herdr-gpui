//! Browser tabs' pages over their window's canvas: local pages as
//! `<iframe>`s, and web pages as pictures of the page the serving machine
//! browses. Local pages run sandboxed on an opaque origin, so their scripts
//! cannot reach this page or its connection; they talk to the app by
//! `postMessage`.
use crate::{app::Shared, dom, stream, surface::Surface};
use herdr_web::wire::{
    PageEvent, PageId, PageOp, ToHost, WindowId, WireScene,
    region::{self, Rect},
};
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
    /// Where the app last placed it, in its pixels; `None` while hidden.
    placed: Option<Rect>,
    /// Whether the window takes every tap over it, as while a menu is open.
    covered: bool,
    /// Where it shows in the newest frame of its window, `Some(None)` if
    /// nowhere. It takes this as that frame is drawn, so it moves with the
    /// window's drawing.
    next: Option<Option<Shown>>,
    /// What it shows as now, written to the page once.
    shown: Option<Shown>,
}

/// A page's place and the parts of it nothing covers, in the app's pixels.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    bounds: Rect,
    open: Vec<Rect>,
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
    /// Each window's canvas clip as last written: the holes where its
    /// pages show through and take taps.
    clips: HashMap<WindowId, String>,
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
                        placed: None,
                        covered: false,
                        next: None,
                        shown: None,
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
                        placed: None,
                        covered: false,
                        next: None,
                        shown: None,
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
            // Both wait for the window's next frame, drawn with them.
            PageOp::Place {
                page,
                bounds,
                covered,
            } => {
                if let Some(page) = self.pages.get_mut(&page) {
                    page.size = (bounds[2], bounds[3]);
                    page.placed = Some(bounds);
                    page.covered = covered;
                }
            }
            PageOp::Hide { page: id } => {
                if let Some(page) = self.pages.get_mut(&id) {
                    page.placed = None;
                }
                self.unfocus(id);
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

    /// Notes where window `window`'s pages show in its newest frame, `scene`,
    /// drawn at `device` device pixels per app pixel: each placed page, less
    /// what the window paints over it.
    pub(crate) fn prepare(&mut self, window: WindowId, scene: &WireScene, device: f32) {
        let device = if device > 0. { device } else { 1. };
        for page in self.pages.values_mut().filter(|page| page.window == window) {
            let covered = page.covered;
            page.next = Some(page.placed.map(|bounds| {
                let area = bounds.map(|value| value * device);
                let open = if covered {
                    Vec::new()
                } else {
                    region::uncovered(area, scene.painted_over(&area))
                        .into_iter()
                        .map(|piece| piece.map(|value| value / device))
                        .collect()
                };
                Shown { bounds, open }
            }));
        }
    }

    /// Shows window `window`'s pages as its frame just drawn has them, at
    /// `scale` CSS pixels per app pixel. Pages lie beneath the canvas, which
    /// leaves them unpainted; holes cut in the canvas where nothing covers
    /// them let them take taps there, as a native page does.
    pub(crate) fn frame_drawn(&mut self, window: WindowId, scale: f64, canvas: &HtmlElement) {
        let css = |value: f32| f64::from(value) * scale;
        let mut holes = String::new();
        for page in self.pages.values_mut().filter(|page| page.window == window) {
            if let Some(next) = page.next.take()
                && page.shown != next
            {
                let element = &page.element;
                match &next {
                    Some(shown) => {
                        let [x, y, width, height] = shown.bounds.map(css);
                        dom::style(element, "left", &format!("{x}px"));
                        dom::style(element, "top", &format!("{y}px"));
                        dom::style(element, "width", &format!("{width}px"));
                        dom::style(element, "height", &format!("{height}px"));
                        dom::style(element, "display", "block");
                    }
                    None => dom::style(element, "display", "none"),
                }
                page.shown = next;
            }
            for [x, y, width, height] in page.shown.iter().flat_map(|shown| &shown.open) {
                let (x, y, width, height) = (css(*x), css(*y), css(*width), css(*height));
                holes.push_str(&format!("M{x} {y}h{width}v{height}h{}z", -width));
            }
        }
        // The canvas less the holes: the outer box reaches past any canvas.
        let clip = if holes.is_empty() {
            String::new()
        } else {
            format!("path(evenodd, \"M0 0H100000V100000H0z{holes}\")")
        };
        if self.clips.get(&window) != Some(&clip) {
            let property = if clip.is_empty() { "none" } else { &clip };
            dom::style(canvas, "clip-path", property);
            self.clips.insert(window, clip);
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
        self.clips.remove(&window);
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
