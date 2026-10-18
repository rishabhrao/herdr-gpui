//! The loopback server. Every request passes the front door first: a known
//! host, then Cloudflare Access for a public host when configured. Pages and
//! the socket also need a paired device.
use super::{
    access::Verdict,
    deflate,
    gate::Door,
    http::{self, Request, Response},
    hub::{Hub, Inbound, OUTBOUND, Outbound, Visitor},
    routes, ws,
};
use crate::{
    Result,
    wire::{self, MAX_TO_HOST_BYTES, PROTOCOL, ToBrowser, ToHost},
};
use std::{
    collections::HashSet,
    io::{Cursor, Read as _, Write as _},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

/// Connections served at once; the page needs two or three.
const MAX_CONNECTIONS: usize = 32;
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a closing socket waits for the page to close its side.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Ends a socket the WebSocket way: a close frame, then this side's half,
/// then whatever the page still sends. Dropping a socket with unread data
/// resets it, and through a tunnel a reset can lose the last messages,
/// such as the reason the page is closed.
fn close_politely(stream: &TcpStream) {
    let mut writer = stream;
    let _ = ws::write_frame(&mut writer, ws::CLOSE, &[]);
    let _ = stream.shutdown(Shutdown::Write);
    let _ = stream.set_read_timeout(Some(CLOSE_TIMEOUT));
    let mut reader = stream;
    let mut buffer = [0; 4096];
    while matches!(reader.read(&mut buffer), Ok(read) if read > 0) {}
    let _ = stream.shutdown(Shutdown::Both);
}

/// Writes queued messages in order. Frames waiting behind a newer frame of
/// the same window are skipped: a slow page catches up instead of falling
/// further behind.
fn write_queued(
    outbound: mpsc::Receiver<Outbound>,
    socket: &Mutex<TcpStream>,
    deflate: bool,
) -> bool {
    while let Ok(first) = outbound.recv() {
        let mut batch = vec![first];
        batch.extend(outbound.try_iter());
        let mut newest = HashSet::new();
        let keep: Vec<bool> = batch
            .iter()
            .rev()
            .map(|message| message.frame.is_none_or(|picture| newest.insert(picture)))
            .collect();
        let mut socket = socket.lock().unwrap_or_else(PoisonError::into_inner);
        for (message, keep) in batch.iter().zip(keep.iter().rev()) {
            if *keep && ws::write_message(&mut *socket, &message.bytes, deflate).is_err() {
                return false;
            }
        }
    }
    true
}

pub(super) fn start(listener: TcpListener, hub: Arc<Hub>) -> Result<()> {
    let open = Arc::new(AtomicUsize::new(0));
    // Accepts for the life of the process; nothing joins it.
    thread::Builder::new()
        .name("web-accept".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                if open.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                    open.fetch_sub(1, Ordering::AcqRel);
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let hub = hub.clone();
                let open = open.clone();
                let spawned =
                    thread::Builder::new()
                        .name("web-connection".into())
                        .spawn(move || {
                            if let Err(error) = serve(stream, &hub) {
                                tracing::debug!(%error, "Web connection ended");
                            }
                            open.fetch_sub(1, Ordering::AcqRel);
                        });
                if spawned.is_err() {
                    tracing::warn!("Could not start a web connection thread");
                }
            }
        })?;
    Ok(())
}

/// The Access token a request carries: the header `cloudflared` adds, or the
/// cookie Access sets on the browser.
fn access_token(request: &Request) -> Option<&str> {
    request
        .header("cf-access-jwt-assertion")
        .or_else(|| request.cookie("CF_Authorization"))
}

/// Whether a request may pass the door it came through.
pub(super) fn admit(hub: &Hub, request: &Request) -> std::result::Result<Door, Response> {
    let Some(door) = hub.doors.door(request) else {
        return Err(Response::text(403, "Unknown host"));
    };
    if door == Door::Public {
        // Without an `access`, serving without the check was asked for.
        if let Some(access) = &hub.access {
            match access.check(access_token(request)) {
                Verdict::Valid => {}
                Verdict::Unready => {
                    return Err(Response::text(
                        503,
                        "Herdr UI is still fetching the Cloudflare Access keys. Try again shortly.",
                    ));
                }
                Verdict::Invalid(reason) => {
                    tracing::warn!(reason, "Refused a request without a valid Access token");
                    return Err(Response::text(
                        403,
                        "Cloudflare Access did not sign this request",
                    ));
                }
            }
        }
    }
    Ok(door)
}

fn serve(mut stream: TcpStream, hub: &Arc<Hub>) -> Result<()> {
    stream.set_read_timeout(Some(HEAD_TIMEOUT))?;
    let (request, leftover) = http::read_head(&mut stream)?;
    let door = match admit(hub, &request) {
        Ok(door) => door,
        Err(response) => return Ok(response.write(&mut stream)?),
    };
    let response = match request.method.as_str() {
        "GET" if request.path == "/ws" && request.wants_websocket() => {
            return websocket(stream, leftover, &request, door, hub);
        }
        "GET" => routes::get(hub, &request, door),
        "POST" => {
            let body = http::read_body(&mut stream, &request, leftover)?;
            routes::post(hub, &request, door, &body)
        }
        _ => Response::text(405, "Only GET and POST"),
    };
    Ok(response.write(&mut stream)?)
}

fn websocket(
    stream: TcpStream,
    leftover: Vec<u8>,
    request: &Request,
    door: Door,
    hub: &Arc<Hub>,
) -> Result<()> {
    let mut stream = stream;
    if !hub.doors.origin_ok(request, door) {
        return Ok(Response::text(403, "Forbidden").write(&mut stream)?);
    }
    let Some(device) = routes::device(hub, request, door) else {
        note!("A browser opened the app socket without a paired device; refused.");
        return Ok(Response::text(401, "Pair this device first").write(&mut stream)?);
    };
    let Some(key) = request.header("sec-websocket-key") else {
        return Ok(Response::text(400, "Not a WebSocket request").write(&mut stream)?);
    };
    let deflate = deflate::offered(request.header("sec-websocket-extensions"));
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n{}\r\n",
        ws::accept_key(key),
        if deflate {
            format!("Sec-WebSocket-Extensions: {}\r\n", deflate::RESPONSE)
        } else {
            String::new()
        }
    )?;
    stream.flush()?;
    stream.set_read_timeout(None)?;
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let mut reader = ws::Reader::new(
        Cursor::new(leftover).chain(stream.try_clone()?),
        MAX_TO_HOST_BYTES,
        deflate,
    );
    let reply = |message: &ToBrowser| -> Result<()> {
        let bytes = wire::encode(message)?;
        let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
        ws::write_message(&mut *writer, &bytes, deflate)?;
        Ok(())
    };

    // The page says which build it is before anything else.
    let Some(ws::Message::Binary(hello)) = reader.next()? else {
        return Ok(());
    };
    match wire::decode(&hello) {
        Ok(ToHost::Hello { protocol }) if protocol == PROTOCOL => {}
        // An older page: it is told to reload, and fetches the new one.
        Ok(ToHost::Hello { .. }) | Err(_) => {
            note!("{} runs an older page; told it to reload.", device.label);
            reply(&ToBrowser::Hello { protocol: PROTOCOL })?;
            close_politely(&stream);
            return Ok(());
        }
        // A page that reconnected reloads itself rather than say hello; it
        // may send a message or two first.
        Ok(_) => {
            close_politely(&stream);
            return Ok(());
        }
    }

    let visitor = Visitor {
        device: device.id.clone(),
        label: device.label.clone(),
        remote: door == Door::Public,
    };
    let (sender, outbound) = mpsc::sync_channel::<Outbound>(OUTBOUND);
    let Some(link) = hub.attach(sender, visitor.clone()) else {
        note!("{} was refused: too many pages are open.", visitor.label);
        reply(&ToBrowser::Toast {
            text: "Too many pages show Herdr UI. Close one and reload this page.".into(),
        })?;
        close_politely(&stream);
        return Ok(());
    };
    note!("{} connected.", visitor.label);
    let label = visitor.label.clone();
    let socket = writer.clone();
    thread::Builder::new()
        .name("web-writer".into())
        .spawn(move || {
            // Ends when the hub drops this link's sender: the page left, or
            // was cut off, and is told so before the socket closes.
            if write_queued(outbound, &socket, deflate) {
                let socket = socket.lock().unwrap_or_else(PoisonError::into_inner);
                close_politely(&socket);
            } else {
                let socket = socket.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = socket.shutdown(Shutdown::Both);
            }
        })?;
    let ended = if hub.receive(Inbound::Connected(link, visitor)) {
        read_messages(&mut reader, &writer, hub, link)
    } else {
        "the app stopped"
    };
    note!("{label} disconnected: {ended}.");
    hub.detach(link);
    hub.receive(Inbound::Disconnected(link));
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn read_messages(
    reader: &mut ws::Reader<impl std::io::Read>,
    writer: &Mutex<TcpStream>,
    hub: &Hub,
    link: u64,
) -> &'static str {
    loop {
        let message = match reader.next() {
            Ok(Some(message)) => message,
            Ok(None) => return "the connection closed",
            Err(error) => {
                tracing::debug!(%error, "Web page sent a bad frame");
                return "the page sent a bad frame";
            }
        };
        match message {
            ws::Message::Binary(bytes) => match wire::decode::<ToHost>(&bytes) {
                Ok(ToHost::Page(event)) => hub.page_event(event),
                Ok(message) => {
                    if !hub.receive(Inbound::Message(link, message)) {
                        return "the app stopped";
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "Web page sent a message this build cannot read");
                    return "the page sent a message this build cannot read";
                }
            },
            ws::Message::Text(_) => {}
            ws::Message::Ping(payload) => {
                let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
                if ws::write_frame(&mut *writer, ws::PONG, &payload).is_err() {
                    return "the connection closed";
                }
            }
            ws::Message::Close => {
                let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = ws::write_frame(&mut *writer, ws::CLOSE, &[]);
                return "the page closed it";
            }
        }
    }
}
