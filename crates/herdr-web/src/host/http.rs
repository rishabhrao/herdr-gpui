//! Request heads, small bodies, cookies, and plain responses.
use crate::{Error, Result};
use std::io::{Read, Write};

/// The most a request head may take; the page sends small ones.
const MAX_HEAD: usize = 16 * 1024;
/// The most a request body may take: a pairing label or a device id.
pub(super) const MAX_BODY: usize = 4 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// A query parameter, without percent-decoding: the server only puts
    /// plain hex and digits there.
    pub(super) fn param(&self, name: &str) -> Option<&str> {
        self.query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    }

    pub(super) fn cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("cookie"))
            .flat_map(|(_, value)| value.split(';'))
            .find_map(|pair| {
                let (key, value) = pair.trim().split_once('=')?;
                (key == name).then_some(value)
            })
    }

    pub(super) fn wants_websocket(&self) -> bool {
        self.header("upgrade")
            .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"))
    }

    fn content_length(&self) -> Result<usize> {
        match self.header("content-length") {
            None => Ok(0),
            Some(value) => value.trim().parse().map_err(|_| Error::BadRequest),
        }
    }
}

/// Reads one request head. Bytes past it are returned for the next reader.
pub(super) fn read_head(stream: &mut impl Read) -> Result<(Request, Vec<u8>)> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0; 1024];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(Error::BadRequest);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let request = parse(&buffer[..end + 4])?;
            return Ok((request, buffer[end + 4..].to_vec()));
        }
        if buffer.len() > MAX_HEAD {
            return Err(Error::BadRequest);
        }
    }
}

/// Reads the request's body after `leftover`, refusing one past `MAX_BODY`.
pub(super) fn read_body(
    stream: &mut impl Read,
    request: &Request,
    mut leftover: Vec<u8>,
) -> Result<Vec<u8>> {
    let length = request.content_length()?;
    if length > MAX_BODY {
        return Err(Error::BadRequest);
    }
    if leftover.len() < length {
        let mut rest = vec![0; length - leftover.len()];
        stream.read_exact(&mut rest)?;
        leftover.extend_from_slice(&rest);
    }
    leftover.truncate(length);
    Ok(leftover)
}

fn parse(head: &[u8]) -> Result<Request> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    match request.parse(head) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err(Error::BadRequest),
    }
    let (Some(method), Some(target)) = (request.method, request.path) else {
        return Err(Error::BadRequest);
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers = request
        .headers
        .iter()
        .map(|header| {
            Ok((
                header.name.to_owned(),
                std::str::from_utf8(header.value)
                    .map_err(|_| Error::BadRequest)?
                    .to_owned(),
            ))
        })
        .collect::<Result<_>>()?;
    Ok(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        headers,
    })
}

/// A cookie only this server reads: not to scripts, not to other sites, and
/// over a tunnel only on HTTPS.
pub(super) fn set_cookie(name: &str, value: &str, max_age: u64, secure: bool) -> String {
    format!(
        "{name}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Strict{}",
        if secure { "; Secure" } else { "" }
    )
}

/// A complete response; the connection closes after it.
pub(super) struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub cache: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub(super) fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type,
            cache: "no-store",
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub(super) fn text(status: u16, body: &str) -> Self {
        Self::new(status, "text/plain; charset=utf-8", body)
    }

    pub(super) fn json(status: u16, value: &serde_json::Value) -> Self {
        Self::new(status, "application/json", value.to_string())
    }

    pub(super) fn redirect(location: &str) -> Self {
        Self::new(302, "text/plain; charset=utf-8", "").with("Location", location)
    }

    pub(super) fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// May be kept, and must be checked again before each use.
    pub(super) fn revalidated(mut self) -> Self {
        self.cache = "private, no-cache";
        self
    }

    pub(super) fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            302 => "Found",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            429 => "Too Many Requests",
            503 => "Service Unavailable",
            _ => "Error",
        };
        let mut head = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n",
            self.status,
            self.content_type,
            self.body.len(),
            self.cache,
        );
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        writer.write_all(head.as_bytes())?;
        writer.write_all(&self.body)?;
        writer.flush()
    }
}
