//! What the threads share: the connected pages, the front doors and the
//! devices allowed through them, and the browser tabs' pages.
use super::stream::{self, Stream};
use super::{
    access::Access,
    assets::Assets,
    devices::{Devices, PairingRequest},
    gate::Doors,
};
use crate::wire::{self, PageEvent, PageId, PageOp, ToBrowser, ToHost, WindowId};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
};

/// Messages the page sent that wait for the UI thread.
const INBOUND: usize = 1024;
/// Messages waiting for the socket. Uploads and frames are one message each
/// and one frame is in flight per window, so a full queue means a page that
/// stopped reading; it is dropped and reconnects.
pub(super) const OUTBOUND: usize = 1024;
/// Pages connected at once, from every device together.
const MAX_LINKS: usize = 8;
/// Pairing requests waiting for the app to ask about them.
const APPROVALS: usize = 16;

static PAGES: OnceLock<Pages> = OnceLock::new();

/// Who a connected page is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Visitor {
    pub(super) device: String,
    pub(super) label: String,
    /// Through a tunnel rather than at this machine: links, the clipboard,
    /// and file pickers must reach the page instead of the Mac.
    pub(super) remote: bool,
}

pub(super) enum Inbound {
    Connected(u64, Visitor),
    Message(u64, ToHost),
    Disconnected(u64),
}

/// What a picture shows, so a page that falls behind skips to the newest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Picture {
    Window(WindowId),
    Page(PageId),
}

/// One message for a page's socket. A picture names what it shows.
pub(super) struct Outbound {
    pub(super) frame: Option<Picture>,
    pub(super) bytes: Vec<u8>,
}

struct Link {
    id: u64,
    visitor: Visitor,
    sender: SyncSender<Outbound>,
    /// Gets what every page gets. A new page is caught up first, alone.
    ready: bool,
}

/// A local page's files: the response body and content type for a path.
pub type PageFiles = Arc<dyn Fn(&str) -> Option<(Vec<u8>, &'static str)> + Send + Sync>;

struct PageEntry {
    secret: String,
    events: stream::Events,
    files: Option<PageFiles>,
    /// A web page browsed here and streamed, which takes its input.
    stream: Option<Arc<Stream>>,
    shown: Shown,
}

/// What the pages were told about one page, for a page that connects later.
#[derive(Default)]
struct Shown {
    /// `Open` with the latest address, or `Stream`.
    opened: Option<PageOp>,
    placed: Option<PageOp>,
    /// A streamed page's latest picture.
    picture: Option<PageOp>,
}

impl Shown {
    fn record(&mut self, op: &PageOp) {
        match op {
            PageOp::Open { .. } | PageOp::Stream { .. } => self.opened = Some(op.clone()),
            PageOp::Load { url, .. } => {
                if let Some(PageOp::Open { url: shown, .. }) = &mut self.opened {
                    shown.clone_from(url);
                }
            }
            PageOp::Place { .. } => self.placed = Some(op.clone()),
            PageOp::Hide { .. } => self.placed = None,
            PageOp::Frame { .. } => self.picture = Some(op.clone()),
            PageOp::Close { .. }
            | PageOp::Script { .. }
            | PageOp::Back { .. }
            | PageOp::Forward { .. }
            | PageOp::Reload { .. }
            | PageOp::Focus { .. }
            | PageOp::Blur { .. } => {}
        }
    }
}

/// What the server needs beyond the hub's own state.
pub(super) struct Setup {
    pub(super) doors: Doors,
    pub(super) token: String,
    pub(super) assets: Assets,
    pub(super) devices: Devices,
    pub(super) access: Option<Arc<Access>>,
}

pub(super) struct Hub {
    pub(super) doors: Doors,
    token: String,
    pub(super) assets: Assets,
    pub(super) devices: Devices,
    pub(super) access: Option<Arc<Access>>,
    links: Mutex<Vec<Link>>,
    next_link: AtomicU64,
    /// The page whose input the app is handling now, so an action it takes
    /// for "this device" reaches that page.
    acting: Mutex<Option<u64>>,
    inbound: async_channel::Sender<Inbound>,
    approvals: async_channel::Sender<PairingRequest>,
    pages: Mutex<HashMap<PageId, PageEntry>>,
    next_page: AtomicU64,
}

pub(super) fn random_hex(bytes: usize) -> crate::Result<String> {
    let mut buffer = vec![0; bytes];
    getrandom::fill(&mut buffer).map_err(|_| crate::Error::Random)?;
    Ok(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The token saved at `path`, or a new one saved there, readable only by
/// this user.
pub(super) fn saved_token(path: &std::path::Path) -> crate::Result<String> {
    use std::{fs::OpenOptions, io::Write as _, os::unix::fs::OpenOptionsExt as _};
    if let Ok(saved) = std::fs::read_to_string(path) {
        let saved = saved.trim();
        if saved.len() == 64 && saved.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(saved.to_owned());
        }
    }
    let token = random_hex(32)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    Ok(token)
}

/// Equal strings, compared without stopping at the first difference.
pub(super) fn same_secret(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0
}

impl Hub {
    pub(super) fn new(
        setup: Setup,
    ) -> (
        Arc<Self>,
        async_channel::Receiver<Inbound>,
        async_channel::Receiver<PairingRequest>,
    ) {
        let (inbound, receiver) = async_channel::bounded(INBOUND);
        let (approvals, requests) = async_channel::bounded(APPROVALS);
        let hub = Arc::new(Self {
            doors: setup.doors,
            token: setup.token,
            assets: setup.assets,
            devices: setup.devices,
            access: setup.access,
            links: Mutex::default(),
            acting: Mutex::new(None),
            next_link: AtomicU64::new(1),
            inbound,
            approvals,
            pages: Mutex::new(HashMap::new()),
            next_page: AtomicU64::new(1),
        });
        let _ = PAGES.set(Pages(hub.clone()));
        super::served::install(&hub);
        (hub, receiver, requests)
    }

    /// The address that pairs this machine's own browser, with the token.
    pub(super) fn local_url(&self, port: u16) -> String {
        format!("http://localhost:{port}/?token={}", self.token)
    }

    pub(super) fn token_matches(&self, token: &str) -> bool {
        same_secret(token, &self.token)
    }

    fn links(&self) -> std::sync::MutexGuard<'_, Vec<Link>> {
        self.links.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds a page. It gets only what is sent to it alone until
    /// [`Hub::ready`]; `None` when too many pages are connected.
    pub(super) fn attach(&self, sender: SyncSender<Outbound>, visitor: Visitor) -> Option<u64> {
        let mut links = self.links();
        if links.len() >= MAX_LINKS {
            return None;
        }
        let id = self.next_link.fetch_add(1, Ordering::Relaxed);
        links.push(Link {
            id,
            visitor,
            sender,
            ready: false,
        });
        Some(id)
    }

    /// Page `id` has been caught up and now gets everything.
    pub(super) fn ready(&self, id: u64) {
        if let Some(link) = self.links().iter_mut().find(|link| link.id == id) {
            link.ready = true;
        }
    }

    pub(super) fn detach(&self, id: u64) {
        self.links().retain(|link| link.id != id);
    }

    /// Drops the pages of a device that was forgotten.
    pub(super) fn drop_device(&self, device: &str) {
        self.links().retain(|link| link.visitor.device != device);
    }

    pub(super) fn set_acting(&self, link: Option<u64>) {
        *self.acting.lock().unwrap_or_else(PoisonError::into_inner) = link;
    }

    pub(super) fn acting(&self) -> Option<u64> {
        *self.acting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Who each connected page is, oldest first.
    pub(super) fn visitors(&self) -> Vec<Visitor> {
        self.links()
            .iter()
            .filter(|link| link.ready)
            .map(|link| link.visitor.clone())
            .collect()
    }

    /// Whether any caught-up page is connected.
    pub(super) fn is_connected(&self) -> bool {
        self.links().iter().any(|link| link.ready)
    }

    fn encode(message: &ToBrowser) -> Option<Outbound> {
        let frame = match message {
            ToBrowser::Frame { window, .. } => Some(Picture::Window(*window)),
            ToBrowser::Page(PageOp::Frame { page, .. }) => Some(Picture::Page(*page)),
            _ => None,
        };
        match wire::encode(message) {
            Ok(bytes) => Some(Outbound { frame, bytes }),
            Err(error) => {
                tracing::warn!(%error, "Could not encode a message for the web page");
                None
            }
        }
    }

    /// Queues for one page; false when the page is cut off. A page that
    /// falls a full queue behind is cut off rather than buffered without
    /// bound.
    fn deliver(link: &Link, outbound: Outbound) -> bool {
        match link.sender.try_send(outbound) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                eprintln!(
                    "{} did not read fast enough; dropped its connection.",
                    link.visitor.label
                );
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Queues `message` for every caught-up page.
    pub(super) fn send(&self, message: &ToBrowser) {
        let Some(outbound) = Self::encode(message) else {
            return;
        };
        self.links().retain(|link| {
            !link.ready
                || Self::deliver(
                    link,
                    Outbound {
                        frame: outbound.frame,
                        bytes: outbound.bytes.clone(),
                    },
                )
        });
    }

    /// Queues `message` for page `id` alone.
    pub(super) fn send_to(&self, id: u64, message: &ToBrowser) {
        let Some(outbound) = Self::encode(message) else {
            return;
        };
        let mut links = self.links();
        if let Some(index) = links.iter().position(|link| link.id == id)
            && !Self::deliver(&links[index], outbound)
        {
            links.remove(index);
        }
    }

    /// Sends a page operation to every page, remembering what a page that
    /// connects later needs to show the same.
    pub(super) fn send_page(&self, op: PageOp) {
        let page = match &op {
            PageOp::Open { page, .. }
            | PageOp::Place { page, .. }
            | PageOp::Hide { page }
            | PageOp::Close { page }
            | PageOp::Load { page, .. }
            | PageOp::Script { page, .. }
            | PageOp::Back { page }
            | PageOp::Forward { page }
            | PageOp::Reload { page }
            | PageOp::Focus { page }
            | PageOp::Blur { page }
            | PageOp::Stream { page, .. }
            | PageOp::Frame { page, .. } => *page,
        };
        if let Some(entry) = self
            .pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&page)
        {
            entry.shown.record(&op);
        }
        self.send(&ToBrowser::Page(op));
    }

    /// Shows page `link`, which just connected, every page the others show.
    pub(super) fn replay_pages(&self, link: u64) {
        let ops: Vec<PageOp> = self
            .pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .flat_map(|entry| {
                [
                    &entry.shown.opened,
                    &entry.shown.placed,
                    &entry.shown.picture,
                ]
                .into_iter()
                .flatten()
                .cloned()
            })
            .collect();
        for op in ops {
            self.send_to(link, &ToBrowser::Page(op));
        }
    }

    /// Hands a reader's message to the UI thread, waiting while it is busy.
    /// Returns false once the app has stopped listening.
    pub(super) fn receive(&self, inbound: Inbound) -> bool {
        self.inbound.send_blocking(inbound).is_ok()
    }

    /// Asks the app to show a pairing request to whoever uses it.
    pub(super) fn ask_approval(&self, request: PairingRequest) {
        if self.approvals.try_send(request).is_err() {
            tracing::warn!("Too many pairing requests are waiting; one was dropped");
        }
    }

    /// A tab's page reported something; its owner hears it on this thread.
    pub(super) fn page_event(&self, event: PageEvent) {
        let page = match &event {
            PageEvent::Loaded { page, .. }
            | PageEvent::Title { page, .. }
            | PageEvent::Posted { page, .. }
            | PageEvent::Input { page, .. } => *page,
        };
        let entry = {
            let pages = self.pages.lock().unwrap_or_else(PoisonError::into_inner);
            pages
                .get(&page)
                .map(|entry| (entry.events.clone(), entry.stream.clone()))
        };
        let Some((events, stream)) = entry else {
            return;
        };
        match (event, stream) {
            (PageEvent::Input { input, .. }, Some(stream)) => stream.input(input),
            // Only a streamed page takes input.
            (PageEvent::Input { .. }, None) => {}
            (event, _) => events(event),
        }
    }

    /// The files of local page `page` at `path`, if `secret` is the page's.
    pub(super) fn page_files(&self, page: PageId, secret: &str) -> Option<PageFiles> {
        let pages = self.pages.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = pages.get(&page)?;
        same_secret(secret, &entry.secret)
            .then(|| entry.files.clone())
            .flatten()
    }
}

/// The browser tabs' pages of a served app. Only exists while serving.
#[derive(Clone)]
pub struct Pages(Arc<Hub>);

impl Pages {
    /// The served app's pages, or `None` when the app has native windows.
    pub fn get() -> Option<&'static Pages> {
        PAGES.get()
    }

    /// Registers a page whose reports go to `events`, and whose local files,
    /// if it shows any, `files` reads. A web page, with no files, is browsed
    /// here and streamed when this machine has Chrome; otherwise each page
    /// loads it itself. Dropping the handle closes it.
    pub fn register(
        &self,
        events: impl Fn(PageEvent) + Send + Sync + 'static,
        files: Option<PageFiles>,
    ) -> crate::Result<PageHandle> {
        let id = self.0.next_page.fetch_add(1, Ordering::Relaxed);
        let secret = random_hex(16)?;
        let base = format!("/page/{id}/{secret}");
        let events: stream::Events = Arc::new(events);
        let stream = (files.is_none() && stream::available())
            .then(|| Stream::new(id, Arc::downgrade(&self.0), events.clone()));
        self.0
            .pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id,
                PageEntry {
                    secret,
                    events,
                    files,
                    stream: stream.clone(),
                    shown: Shown::default(),
                },
            );
        Ok(PageHandle {
            hub: self.0.clone(),
            id,
            base,
            stream,
        })
    }
}

/// One registered page. Its operations reach the page in order with frames.
pub struct PageHandle {
    hub: Arc<Hub>,
    id: PageId,
    base: String,
    stream: Option<Arc<Stream>>,
}

impl PageHandle {
    pub fn id(&self) -> PageId {
        self.id
    }

    /// Where the server answers with this page's local files, to which a
    /// file's path is appended.
    pub fn files_url(&self) -> &str {
        &self.base
    }

    pub fn send(&self, op: PageOp) {
        match &self.stream {
            Some(stream) => stream.op(op),
            None => self.hub.send_page(op),
        }
    }
}

impl Drop for PageHandle {
    fn drop(&mut self) {
        self.send(PageOp::Close { page: self.id });
        self.hub
            .pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.id);
    }
}
