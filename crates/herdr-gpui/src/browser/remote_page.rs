//! Browser tabs in a served app. The page shows each tab's page where the
//! native view would be: a local page as an `<iframe>`, and a web page as
//! the pictures of the page the server browses with Chrome on this machine.
//! This side places it each frame and forwards its reports to the window's
//! event queue, as a native view's handlers do.
use super::{
    Location, Tab, TabId,
    native::{Event, Source},
};
use gpui::{AnyElement, Bounds, IntoElement, Pixels, Styled as _, Window, canvas};
use herdr_web::{
    host::{PageFiles, PageHandle, Pages as ServedPages},
    wire::{PageEvent, PageOp},
};
use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError, mpsc::SyncSender},
};

/// Gives a local page `window.ipc` as wry does, over `postMessage`, and runs
/// the app's own scripts sent the same way. Only the app's page, its parent,
/// may send them.
const SHIM: &str = r#"(function () {
  var app = window.parent;
  window.ipc = { postMessage: function (body) { app.postMessage({ herdrPost: String(body) }, "*"); } };
  window.addEventListener("message", function (event) {
    if (event.source !== app || !event.data || typeof event.data.herdrScript !== "string") return;
    (0, eval)(event.data.herdrScript);
  });
  window.addEventListener("load", function () { app.postMessage({ herdrTitle: document.title }, "*"); });
})();"#;

/// Where the page is and whether it shows, shared with its element.
struct Placement {
    handle: PageHandle,
    shown: Cell<bool>,
    placed: Cell<Option<[f32; 4]>>,
}

impl Placement {
    fn place(&self, bounds: Bounds<Pixels>) {
        if !self.shown.get() {
            return;
        }
        let rect = [
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ];
        if self.placed.replace(Some(rect)) != Some(rect) {
            self.handle.send(PageOp::Place {
                page: self.handle.id(),
                bounds: rect,
            });
        }
    }

    fn set_shown(&self, shown: bool) {
        if self.shown.replace(shown) == shown {
            return;
        }
        if !shown {
            self.placed.set(None);
            self.handle.send(PageOp::Hide {
                page: self.handle.id(),
            });
        }
    }
}

/// A served page's address: local files come from the server under the
/// page's own secret path.
fn served_url(location: &Location, handle: &PageHandle) -> String {
    let url = location.page_url();
    match location {
        Location::Local { .. } => {
            let path = url.strip_prefix("herdr-preview://localhost").unwrap_or("/");
            format!("{}{path}", handle.files_url())
        }
        _ => url,
    }
}

/// Serves a local page's folder, with the shim and the app's bridge in each
/// HTML file, and the kit in the window's theme.
fn files(root: std::path::PathBuf, theme: Arc<Mutex<String>>) -> PageFiles {
    Arc::new(move |path: &str| {
        let theme = theme.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(asset) = super::kit::asset(path, &theme) {
            return Some(asset);
        }
        let (body, kind) = super::preview::read(&root, path).ok()?;
        if !kind.starts_with("text/html") {
            return Some((body, kind));
        }
        let page = String::from_utf8_lossy(&body);
        let scripts = format!(
            "<script>{SHIM}</script><script>{}</script>",
            super::annotate::BRIDGE
        );
        let lower = page.to_ascii_lowercase();
        let at = lower
            .find("<head>")
            .map(|index| index + "<head>".len())
            .unwrap_or(0);
        Some((
            format!("{}{scripts}{}", &page[..at], &page[at..]).into_bytes(),
            kind,
        ))
    })
}

/// One window's served pages.
#[derive(Default)]
pub(crate) struct Remote {
    pages: HashMap<TabId, Rc<Placement>>,
    theme: Arc<Mutex<String>>,
}

impl Remote {
    pub(crate) fn contains(&self, id: TabId) -> bool {
        self.pages.contains_key(&id)
    }

    pub(crate) fn ids(&self) -> impl Iterator<Item = TabId> + '_ {
        self.pages.keys().copied()
    }

    /// What lays tab `id`'s page out: an empty element that places the
    /// page in the browser wherever it is drawn.
    pub(crate) fn view(&self, id: TabId) -> Option<AnyElement> {
        let placement = self.pages.get(&id)?.clone();
        Some(
            canvas(move |bounds, _, _| placement.place(bounds), |_, _, _, _| {})
                .size_full()
                .into_any_element(),
        )
    }

    pub(crate) fn set_theme(&self, css: &str) {
        *self.theme.lock().unwrap_or_else(PoisonError::into_inner) = css.to_owned();
    }

    /// Opens tab `tab`'s page in the browser, hidden until presented.
    pub(crate) fn ensure(
        &mut self,
        tab: &Tab,
        location: &Location,
        sender: &SyncSender<Event<Source>>,
        window: &Window,
    ) -> crate::Result<()> {
        let Some(served) = ServedPages::get() else {
            return Ok(());
        };
        let id = tab.id;
        let events = sender.clone();
        let local = match location {
            Location::Local { file } => Some(files(file.root().to_owned(), self.theme.clone())),
            _ => None,
        };
        let is_local = local.is_some();
        // A local page is the app's preview; a web page, streamed from
        // Chrome, posts as untrusted as any other site.
        let from = if is_local {
            super::PostedFrom::Preview
        } else {
            super::PostedFrom::Elsewhere
        };
        let report = move |event: PageEvent| {
            let event = match event {
                PageEvent::Loaded { url, .. } => Event::Loaded(Source::Tab(id), url),
                PageEvent::Title { title, .. } => Event::Title(Source::Tab(id), title),
                PageEvent::Posted { body, .. } => Event::Posted(Source::Tab(id), body, from),
                // The server hands a streamed page's input to its browser.
                PageEvent::Input { .. } => return,
            };
            let _ = events.try_send(event);
        };
        let handle = served.register(report, local)?;
        handle.send(PageOp::Open {
            window: window.window_handle().window_id().as_u64(),
            page: handle.id(),
            url: served_url(location, &handle),
            local: is_local,
        });
        let placement = Rc::new(Placement {
            handle,
            shown: Cell::new(false),
            placed: Cell::new(None),
        });
        self.pages.insert(id, placement);
        Ok(())
    }

    pub(crate) fn show(&self, id: TabId, shown: bool) {
        if let Some(placement) = self.pages.get(&id) {
            placement.set_shown(shown);
        }
    }

    /// Dropping the handle closes the page.
    pub(crate) fn close(&mut self, id: TabId) {
        self.pages.remove(&id);
    }

    pub(crate) fn load(&self, id: TabId, location: &Location) {
        if let Some(placement) = self.pages.get(&id) {
            let handle = &placement.handle;
            handle.send(PageOp::Load {
                page: handle.id(),
                url: served_url(location, handle),
            });
        }
    }

    /// Sends one of `op`'s operations to tab `id`'s page.
    pub(crate) fn send(&self, id: TabId, op: impl FnOnce(u64) -> PageOp) {
        if let Some(placement) = self.pages.get(&id) {
            placement.handle.send(op(placement.handle.id()));
        }
    }
}
