//! The little of RFC 6455 a single page needs: the handshake, masked frames
//! from the page with fragments joined, unmasked frames to it, and
//! `permessage-deflate` when the page offers it. The same reader, as a
//! client, reads a browser's DevTools socket, which speaks text.
use super::deflate;
use crate::{Error, Result};
use base64::Engine as _;
use sha1::{Digest as _, Sha1};
use std::io::{self, Read, Write};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
pub(super) const BINARY: u8 = 0x2;
const CONTINUATION: u8 = 0x0;
pub(super) const TEXT: u8 = 0x1;
pub(super) const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;
pub(super) const PONG: u8 = 0xA;
/// The first reserved bit: a compressed message.
const RSV1: u8 = 0x40;

/// The `Sec-WebSocket-Accept` answer to the page's key.
pub(super) fn accept_key(key: &str) -> String {
    let digest = Sha1::digest(format!("{}{GUID}", key.trim()).as_bytes());
    base64::engine::general_purpose::STANDARD.encode(digest)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Message {
    Binary(Vec<u8>),
    /// Only a client reader returns text; a server reader skips it.
    Text(Vec<u8>),
    Ping(Vec<u8>),
    /// The status code the peer closed with, when it gave one.
    Close(Option<u16>),
}

struct Frame {
    fin: bool,
    compressed: bool,
    opcode: u8,
    payload: Vec<u8>,
}

/// Reads whole messages; pongs, and a server's text messages, are skipped.
pub(super) struct Reader<R> {
    inner: R,
    limit: usize,
    deflate: bool,
    /// A client: frames come unmasked, and text is returned.
    client: bool,
}

impl<R: Read> Reader<R> {
    /// `deflate` when the handshake accepted `permessage-deflate`.
    pub(super) fn new(inner: R, limit: usize, deflate: bool) -> Self {
        Self {
            inner,
            limit,
            deflate,
            client: false,
        }
    }

    /// Reads a server's unmasked frames, text included.
    pub(super) fn client(inner: R, limit: usize) -> Self {
        Self {
            inner,
            limit,
            deflate: false,
            client: true,
        }
    }

    /// The next message, or `None` at the end of the stream.
    pub(super) fn next(&mut self) -> Result<Option<Message>> {
        // The opcode, whether it is compressed, and the payload so far.
        let mut message: Option<(u8, bool, Vec<u8>)> = None;
        loop {
            let Some(frame) = self.frame()? else {
                return Ok(None);
            };
            match frame.opcode {
                PING => return Ok(Some(Message::Ping(frame.payload))),
                PONG => continue,
                CLOSE => {
                    let code = frame
                        .payload
                        .first_chunk::<2>()
                        .map(|code| u16::from_be_bytes(*code));
                    return Ok(Some(Message::Close(code)));
                }
                CONTINUATION => {
                    let Some((_, _, buffer)) = message.as_mut() else {
                        return Err(Error::BadRequest);
                    };
                    if buffer.len() + frame.payload.len() > self.limit {
                        return Err(Error::FrameTooLarge(
                            (buffer.len() + frame.payload.len()) as u64,
                        ));
                    }
                    buffer.extend_from_slice(&frame.payload);
                }
                BINARY | TEXT => {
                    if message.is_some() {
                        return Err(Error::BadRequest);
                    }
                    if frame.compressed && !self.deflate {
                        return Err(Error::BadRequest);
                    }
                    message = Some((frame.opcode, frame.compressed, frame.payload));
                }
                other => return Err(Error::Opcode(other)),
            }
            // A server takes a finished text message and drops it.
            if frame.fin
                && self.client
                && message.as_ref().is_some_and(|(opcode, ..)| *opcode == TEXT)
                && let Some((_, _, payload)) = message.take()
            {
                return Ok(Some(Message::Text(payload)));
            }
            if frame.fin
                && let Some((opcode, compressed, payload)) = message.take()
                && opcode == BINARY
            {
                let payload = if compressed {
                    deflate::decompress(&payload, self.limit)?
                } else {
                    payload
                };
                return Ok(Some(Message::Binary(payload)));
            }
        }
    }

    fn frame(&mut self) -> Result<Option<Frame>> {
        let mut head = [0; 2];
        match self.inner.read_exact(&mut head) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let fin = head[0] & 0x80 != 0;
        let compressed = head[0] & RSV1 != 0;
        let opcode = head[0] & 0x0F;
        let masked = head[1] & 0x80 != 0;
        if !masked && !self.client {
            return Err(Error::Unmasked);
        }
        let length = match head[1] & 0x7F {
            126 => {
                let mut bytes = [0; 2];
                self.inner.read_exact(&mut bytes)?;
                u64::from(u16::from_be_bytes(bytes))
            }
            127 => {
                let mut bytes = [0; 8];
                self.inner.read_exact(&mut bytes)?;
                u64::from_be_bytes(bytes)
            }
            short => u64::from(short),
        };
        if length > self.limit as u64 {
            return Err(Error::FrameTooLarge(length));
        }
        let mut mask = [0; 4];
        if masked {
            self.inner.read_exact(&mut mask)?;
        }
        let mut payload = vec![0; length as usize];
        self.inner.read_exact(&mut payload)?;
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
        Ok(Some(Frame {
            fin,
            compressed,
            opcode,
            payload,
        }))
    }
}

/// Writes one final frame masked with `mask`, as a client must.
pub(super) fn write_masked(
    writer: &mut impl Write,
    opcode: u8,
    payload: &[u8],
    mask: [u8; 4],
) -> io::Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | opcode);
    match payload.len() {
        length @ 0..=125 => frame.push(0x80 | length as u8),
        length @ 126..=0xFFFF => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        }
        length => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    writer.write_all(&frame)?;
    writer.flush()
}

/// Writes one final, unmasked frame.
pub(super) fn write_frame(writer: &mut impl Write, opcode: u8, payload: &[u8]) -> io::Result<()> {
    write_frame_with(writer, opcode, payload, false)
}

/// Writes a binary message, compressed when `deflate` and worth it.
pub(super) fn write_message(
    writer: &mut impl Write,
    payload: &[u8],
    deflate: bool,
) -> io::Result<()> {
    if deflate
        && payload.len() >= deflate::MIN_COMPRESSED
        && let Some(compressed) = deflate::compress(payload)
    {
        return write_frame_with(writer, BINARY, &compressed, true);
    }
    write_frame_with(writer, BINARY, payload, false)
}

fn write_frame_with(
    writer: &mut impl Write,
    opcode: u8,
    payload: &[u8],
    compressed: bool,
) -> io::Result<()> {
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | if compressed { RSV1 } else { 0 } | opcode);
    match payload.len() {
        length @ 0..=125 => head.push(length as u8),
        length @ 126..=0xFFFF => {
            head.push(126);
            head.extend_from_slice(&(length as u16).to_be_bytes());
        }
        length => {
            head.push(127);
            head.extend_from_slice(&(length as u64).to_be_bytes());
        }
    }
    writer.write_all(&head)?;
    writer.write_all(payload)?;
    writer.flush()
}
