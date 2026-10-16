//! Small DOM helpers. Elements the page's HTML provides are looked up by id;
//! a missing one means a broken page, reported once as text.
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{AddEventListenerOptions, Document, EventTarget, HtmlElement, Window};

pub(crate) fn window() -> Option<Window> {
    web_sys::window()
}

pub(crate) fn document() -> Option<Document> {
    window()?.document()
}

pub(crate) fn by_id<T: JsCast>(id: &str) -> Option<T> {
    document()?.get_element_by_id(id)?.dyn_into().ok()
}

pub(crate) fn create<T: JsCast>(tag: &str) -> Option<T> {
    document()?.create_element(tag).ok()?.dyn_into().ok()
}

pub(crate) fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// Shows `text` in the status box, or hides the box for an empty text.
pub(crate) fn show_status(text: &str) {
    let Some(status) = by_id::<HtmlElement>("herdr-status") else {
        return;
    };
    status.set_text_content(Some(text));
    status.set_hidden(text.is_empty());
}

pub(crate) fn log(text: &str) {
    web_sys::console::warn_1(&JsValue::from_str(text));
}

/// A DOM listener that is removed when dropped.
pub(crate) struct Listener {
    target: EventTarget,
    name: &'static str,
    closure: Closure<dyn FnMut(JsValue)>,
}

impl Listener {
    pub(crate) fn new(
        target: &EventTarget,
        name: &'static str,
        passive: bool,
        handler: impl FnMut(JsValue) + 'static,
    ) -> Self {
        let closure = Closure::<dyn FnMut(JsValue)>::new(handler);
        let options = AddEventListenerOptions::new();
        options.set_passive(passive);
        let _ = target.add_event_listener_with_callback_and_add_event_listener_options(
            name,
            closure.as_ref().unchecked_ref(),
            &options,
        );
        Self {
            target: target.clone(),
            name,
            closure,
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.closure.as_ref().unchecked_ref());
    }
}

pub(crate) fn now() -> f64 {
    window()
        .and_then(|window| window.performance())
        .map_or(0., |performance| performance.now())
}

/// Tells `chrome.js` something: a `herdr` event whose detail is
/// `{ type: kind, ...fields }`.
pub(crate) fn emit(kind: &str, fields: &[(&str, &str)]) {
    let Some(window) = window() else {
        return;
    };
    let detail = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &detail,
        &JsValue::from_str("type"),
        &JsValue::from_str(kind),
    );
    for (key, value) in fields {
        let _ = js_sys::Reflect::set(&detail, &JsValue::from_str(key), &JsValue::from_str(value));
    }
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict("herdr", &init) {
        let _ = window.dispatch_event(&event);
    }
}

/// A touch screen: the keyboard opens only when the user asks for it.
pub(crate) fn coarse_pointer() -> bool {
    window()
        .and_then(|window| window.match_media("(pointer: coarse)").ok().flatten())
        .is_some_and(|query| query.matches())
}
