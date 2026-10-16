//! `permessage-deflate` (RFC 7692) without context takeover: each message is
//! compressed on its own, so neither side keeps a window between messages.
//! Frames are what crosses a tunnel to a phone, and they compress well.
use flate2::{Compression, Decompress, FlushDecompress, Status, write::DeflateEncoder};
use std::io::Write as _;

/// What a sync flush ends with; the extension leaves it off the wire.
const TAIL: [u8; 4] = [0x00, 0x00, 0xff, 0xff];
/// Smaller messages are sent as they are.
pub(crate) const MIN_COMPRESSED: usize = 256;

/// The response header value that accepts the page's offer.
pub(crate) const RESPONSE: &str =
    "permessage-deflate; server_no_context_takeover; client_no_context_takeover";

/// Whether the page offered the extension.
pub(crate) fn offered(header: Option<&str>) -> bool {
    header.is_some_and(|value| {
        value
            .split(',')
            .any(|offer| offer.trim().starts_with("permessage-deflate"))
    })
}

pub(crate) fn compress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder =
        DeflateEncoder::new(Vec::with_capacity(data.len() / 3 + 64), Compression::fast());
    encoder.write_all(data)?;
    // A sync flush ends on a byte boundary with the tail, which is dropped.
    encoder.flush()?;
    let mut out = std::mem::take(encoder.get_mut());
    if out.ends_with(&TAIL) {
        out.truncate(out.len() - TAIL.len());
    }
    Ok(out)
}

/// Inflates one message, refusing one that would grow past `limit`.
pub(crate) fn decompress(data: &[u8], limit: usize) -> crate::Result<Vec<u8>> {
    let mut input = Vec::with_capacity(data.len() + TAIL.len());
    input.extend_from_slice(data);
    input.extend_from_slice(&TAIL);
    let mut inflater = Decompress::new(false);
    let mut out = Vec::with_capacity((data.len() * 4).clamp(1024, limit.max(1024)));
    loop {
        let consumed = inflater.total_in() as usize;
        if out.len() == out.capacity() {
            if out.len() > limit {
                return Err(crate::Error::FrameTooLarge(out.len() as u64));
            }
            out.reserve(out.len().max(1024));
        }
        let produced = out.len();
        let status = inflater
            .decompress_vec(&input[consumed..], &mut out, FlushDecompress::Sync)
            .map_err(|_| crate::Error::BadRequest)?;
        if out.len() > limit {
            return Err(crate::Error::FrameTooLarge(out.len() as u64));
        }
        let done = inflater.total_in() as usize == input.len() && out.len() < out.capacity();
        let stuck = inflater.total_in() as usize == consumed && out.len() == produced;
        if done || status == Status::StreamEnd || (stuck && out.len() < out.capacity()) {
            return Ok(out);
        }
    }
}
