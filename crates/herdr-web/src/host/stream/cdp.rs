//! A DevTools protocol connection: commands go out on a writer thread, so
//! no caller blocks on the socket, and replies and events come back on a
//! reader thread, to the callbacks that asked and the sessions that listen.
use super::super::ws;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead as _, BufReader, Cursor, Read as _, Write as _},
    net::TcpStream,
    process::Child,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
};

/// The largest DevTools message read: a picture, in base64, fits easily.
const MAX_MESSAGE: usize = 32 * 1024 * 1024;
/// Commands waiting for the socket; more means Chrome stopped reading.
const OUTGOING: usize = 1024;
const MAX_HANDSHAKE: usize = 16 * 1024;

/// What a command's reply is handed: its `result`, or `None` on an error.
pub(super) type Reply = Box<dyn FnOnce(Option<Value>) + Send>;
/// A session's event handler.
pub(super) type Route = Arc<dyn Fn(&str, &Value) + Send + Sync>;

pub(in crate::host) struct Cdp {
    outgoing: SyncSender<Vec<u8>>,
    next: AtomicU64,
    pending: Mutex<HashMap<u64, Reply>>,
    routes: Mutex<HashMap<String, Route>>,
    alive: AtomicBool,
    /// Chrome itself, stopped when the last handle goes.
    child: Mutex<Option<Child>>,
}

impl Drop for Cdp {
    fn drop(&mut self) {
        if let Some(mut child) = self
            .child
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `ws://host:port/path` as the address to dial and the path to ask for.
pub(super) fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("ws://")?;
    let slash = rest.find('/')?;
    Some((&rest[..slash], &rest[slash..]))
}

fn handshake(stream: &mut TcpStream, address: &str, path: &str) -> crate::Result<Vec<u8>> {
    let mut key = [0; 16];
    getrandom::fill(&mut key).map_err(|_| crate::Error::Random)?;
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, key);
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )?;
    stream.flush()?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || head.len() > MAX_HANDSHAKE {
            return Err(crate::Error::BadRequest);
        }
        let end = line == "\r\n";
        head.push_str(&line);
        if end {
            break;
        }
    }
    if !head.starts_with("HTTP/1.1 101") {
        return Err(crate::Error::BadRequest);
    }
    // Bytes the buffered reader took past the head belong to the first frame.
    Ok(reader.buffer().to_vec())
}

impl Cdp {
    /// Connects to Chrome's browser socket at `url`.
    pub(super) fn connect(url: &str) -> crate::Result<Arc<Self>> {
        let (address, path) = split_url(url).ok_or(crate::Error::BadRequest)?;
        let mut stream = TcpStream::connect(address)?;
        let leftover = handshake(&mut stream, address, path)?;
        let (outgoing, queue) = mpsc::sync_channel::<Vec<u8>>(OUTGOING);
        let cdp = Arc::new(Self {
            outgoing,
            next: AtomicU64::new(1),
            pending: Mutex::default(),
            routes: Mutex::default(),
            alive: AtomicBool::new(true),
            child: Mutex::new(None),
        });
        let mut writer = stream.try_clone()?;
        std::thread::Builder::new()
            .name("web-cdp-write".into())
            .spawn(move || {
                for message in queue {
                    let mut mask = [0; 4];
                    if getrandom::fill(&mut mask).is_err()
                        || ws::write_masked(&mut writer, ws::TEXT, &message, mask).is_err()
                    {
                        return;
                    }
                }
            })?;
        let reading = Arc::downgrade(&cdp);
        std::thread::Builder::new()
            .name("web-cdp-read".into())
            .spawn(move || {
                let mut reader =
                    ws::Reader::client(Cursor::new(leftover).chain(stream), MAX_MESSAGE);
                while let Ok(Some(message)) = reader.next() {
                    let Some(cdp) = reading.upgrade() else {
                        return;
                    };
                    if let ws::Message::Text(text) = message
                        && let Ok(value) = serde_json::from_slice::<Value>(&text)
                    {
                        cdp.received(&value);
                    }
                }
                if let Some(cdp) = reading.upgrade() {
                    cdp.alive.store(false, Ordering::Release);
                    tracing::warn!("Chrome's DevTools connection closed");
                }
            })?;
        Ok(cdp)
    }

    /// Makes this connection own `child`, which stops with it.
    pub(super) fn adopt(&self, child: Child) {
        *self.child.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
    }

    pub(super) fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// Queues `method` on `session`, or on the browser when `None`; `reply`
    /// gets its result.
    pub(super) fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        reply: Option<Reply>,
    ) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        if let Some(reply) = reply {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, reply);
        }
        let Ok(bytes) = serde_json::to_vec(&message) else {
            return;
        };
        match self.outgoing.try_send(bytes) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!(method, "Chrome is not reading; a command was dropped");
                self.pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
            }
            Err(TrySendError::Disconnected(_)) => self.alive.store(false, Ordering::Release),
        }
    }

    /// Sends `session`'s events to `route`.
    pub(super) fn route(&self, session: &str, route: Route) {
        self.routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session.to_owned(), route);
    }

    pub(super) fn forget(&self, session: &str) {
        self.routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(session);
    }

    fn received(&self, value: &Value) {
        if let Some(id) = value["id"].as_u64() {
            let reply = self
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            if let Some(reply) = reply {
                reply(
                    value
                        .get("result")
                        .filter(|_| value.get("error").is_none())
                        .cloned(),
                );
            }
            return;
        }
        let (Some(method), Some(session)) = (value["method"].as_str(), value["sessionId"].as_str())
        else {
            return;
        };
        let route = self
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session)
            .cloned();
        if let Some(route) = route {
            route(method, &value["params"]);
        }
    }
}
