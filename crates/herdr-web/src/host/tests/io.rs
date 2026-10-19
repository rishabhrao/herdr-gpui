use crate::host::{
    atlas::RemoteAtlas,
    http::{self, Response},
    hub::same_secret,
    ws,
};
use crate::wire::{TileUpload, WireTile};
use gpui::{
    AtlasKey, DevicePixels, ImageId, PlatformAtlas, RenderImageParams, RenderSvgParams, size,
};
use std::{borrow::Cow, io::Cursor};

#[test]
fn request_heads_are_parsed_and_bounded() {
    let mut stream = Cursor::new(
        b"GET /ws?token=ab&x=1 HTTP/1.1\r\nHost: localhost:4321\r\nUpgrade: websocket\r\nCookie: a=1; herdr_device=xyz\r\n\r\nrest"
            .to_vec(),
    );
    let (request, leftover) = http::read_head(&mut stream).unwrap();
    assert_eq!(request.path, "/ws");
    assert_eq!(request.param("token"), Some("ab"));
    assert_eq!(request.param("x"), Some("1"));
    assert!(request.wants_websocket());
    assert_eq!(leftover, b"rest");
    assert_eq!(request.cookie("herdr_device"), Some("xyz"));
    assert_eq!(request.cookie("missing"), None);

    let huge = format!("GET / HTTP/1.1\r\nX: {}\r\n", "a".repeat(20_000));
    assert!(http::read_head(&mut Cursor::new(huge.into_bytes())).is_err());
    assert!(http::read_head(&mut Cursor::new(b"GET / HTTP/1.1\r\n".to_vec())).is_err());
}

#[test]
fn the_token_is_kept_across_restarts_and_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("herdr-web-token-{}", std::process::id()));
    let path = dir.join("web-token");
    let first = crate::host::hub::saved_token(&path).unwrap();
    assert_eq!(first.len(), 64);
    assert_eq!(crate::host::hub::saved_token(&path).unwrap(), first);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    std::fs::write(&path, "short").unwrap();
    let replaced = crate::host::hub::saved_token(&path).unwrap();
    assert_ne!(replaced, "short");
    assert_eq!(replaced.len(), 64);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn secrets_compare_whole() {
    assert!(same_secret("abcd", "abcd"));
    assert!(!same_secret("abcd", "abce"));
    assert!(!same_secret("abc", "abcd"));
}

#[test]
fn responses_carry_length_and_no_store() {
    let mut out = Vec::new();
    Response::text(200, "hi")
        .with("X-Test", "1")
        .write(&mut out)
        .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(text.contains("Content-Length: 2\r\n"));
    assert!(text.contains("Cache-Control: no-store\r\n"));
    assert!(text.contains("X-Test: 1\r\n"));
    assert!(text.ends_with("\r\n\r\nhi"));
}

/// A masked frame as a browser sends it.
fn masked(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mask = [1, 2, 3, 4];
    let mut frame = vec![if fin { 0x80 } else { 0 } | opcode];
    match payload.len() {
        length @ 0..=125 => frame.push(0x80 | length as u8),
        length => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ mask[i % 4]),
    );
    frame
}

#[test]
fn websocket_messages_are_unmasked_joined_and_bounded() {
    let mut bytes = masked(false, ws::BINARY, b"hel");
    bytes.extend(masked(true, 0, b"lo"));
    bytes.extend(masked(true, 0x9, b"p"));
    bytes.extend(masked(true, 0x1, b"text is skipped"));
    bytes.extend(masked(true, ws::BINARY, &[7; 300]));
    bytes.extend(masked(true, ws::CLOSE, b""));
    bytes.extend(masked(true, ws::CLOSE, &[0x03, 0xe9, b'b', b'y', b'e']));
    let mut reader = ws::Reader::new(Cursor::new(bytes), 1024, false);
    assert_eq!(
        reader.next().unwrap(),
        Some(ws::Message::Binary(b"hello".to_vec()))
    );
    assert_eq!(
        reader.next().unwrap(),
        Some(ws::Message::Ping(b"p".to_vec()))
    );
    assert_eq!(
        reader.next().unwrap(),
        Some(ws::Message::Binary(vec![7; 300]))
    );
    assert_eq!(reader.next().unwrap(), Some(ws::Message::Close(None)));
    // A page that is left closes with 1001, "going away".
    assert_eq!(reader.next().unwrap(), Some(ws::Message::Close(Some(1001))));
    assert_eq!(reader.next().unwrap(), None);

    let mut big = ws::Reader::new(Cursor::new(masked(true, ws::BINARY, &[0; 300])), 100, false);
    assert!(matches!(big.next(), Err(crate::Error::FrameTooLarge(300))));
    let mut unmasked = ws::Reader::new(Cursor::new(vec![0x82, 0x01, 0x00]), 100, false);
    assert!(matches!(unmasked.next(), Err(crate::Error::Unmasked)));
}

#[test]
fn websocket_frames_use_the_right_length_form() {
    for (length, head) in [(5, 2), (300, 4), (70_000, 10)] {
        let mut out = Vec::new();
        ws::write_frame(&mut out, ws::BINARY, &vec![1; length]).unwrap();
        assert_eq!(out.len(), head + length);
        assert_eq!(out[0], 0x82);
    }
    // RFC 6455's own example key.
    assert_eq!(
        ws::accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
}

fn svg(name: &str) -> AtlasKey {
    AtlasKey::Svg(RenderSvgParams {
        path: name.to_owned().into(),
        size: size(DevicePixels(2), DevicePixels(2)),
    })
}

#[test]
fn the_atlas_numbers_tiles_and_reports_changes_once() {
    let atlas = RemoteAtlas::default();
    let builds = std::cell::Cell::new(0);
    let mut build = || {
        builds.set(builds.get() + 1);
        Ok(Some((
            size(DevicePixels(2), DevicePixels(2)),
            Cow::Borrowed(&[1u8, 2, 3, 4][..]),
        )))
    };
    let first = atlas
        .get_or_insert_with(svg("a"), &mut build)
        .unwrap()
        .unwrap();
    let again = atlas
        .get_or_insert_with(svg("a"), &mut build)
        .unwrap()
        .unwrap();
    assert_eq!(first, again);
    let image = AtlasKey::Image(RenderImageParams {
        image_id: ImageId(1),
        frame_index: 0,
    });
    let second = atlas
        .get_or_insert_with(image.clone(), &mut build)
        .unwrap()
        .unwrap();
    assert_ne!(first.tile_id, second.tile_id);
    assert_eq!(builds.get(), 2);

    let (uploads, removed) = atlas.take_changes();
    let kinds: Vec<_> = uploads.iter().map(|tile| (tile.id, tile.kind)).collect();
    assert_eq!(
        kinds,
        [
            (first.tile_id.0, WireTile::Mono),
            (second.tile_id.0, WireTile::Poly)
        ]
    );
    assert!(removed.is_empty());
    assert_eq!(atlas.take_changes(), (Vec::<TileUpload>::new(), Vec::new()));

    atlas.remove(&image);
    assert_eq!(atlas.take_changes().1, [second.tile_id.0]);
    // A tile added and removed before the page heard needs neither message.
    atlas.get_or_insert_with(svg("b"), &mut build).unwrap();
    atlas.remove(&svg("b"));
    assert_eq!(atlas.take_changes(), (Vec::new(), Vec::new()));
    // A page that connects late gets every live tile.
    assert_eq!(atlas.everything().len(), 1);
    assert_eq!(atlas.len(), 1);
}

#[test]
fn compressed_messages_round_trip_and_are_bounded() {
    use crate::host::deflate;
    let message = b"frame ".repeat(500);
    let compressed = deflate::compress(&message).unwrap();
    assert!(compressed.len() < message.len() / 4);
    assert_eq!(deflate::decompress(&compressed, 1 << 20).unwrap(), message);
    assert!(deflate::decompress(&compressed, 100).is_err());
    assert!(deflate::decompress(b"\xff\xff not deflate", 1 << 20).is_err());

    // A page's compressed frame, with the first reserved bit set.
    let mut frame = masked(true, ws::BINARY, &compressed);
    frame[0] |= 0x40;
    let mut reader = ws::Reader::new(Cursor::new(frame.clone()), 1 << 20, true);
    assert_eq!(
        reader.next().unwrap(),
        Some(ws::Message::Binary(message.clone()))
    );
    // Without the extension agreed, a compressed frame is refused.
    let mut plain = ws::Reader::new(Cursor::new(frame), 1 << 20, false);
    assert!(plain.next().is_err());

    let mut out = Vec::new();
    ws::write_message(&mut out, &message, true).unwrap();
    assert_eq!(out[0], 0x80 | 0x40 | 0x2);
    let mut small = Vec::new();
    ws::write_message(&mut small, b"tiny", true).unwrap();
    assert_eq!(small[0], 0x82);
    assert!(deflate::offered(Some(
        "permessage-deflate; client_max_window_bits"
    )));
    assert!(!deflate::offered(Some("x-webkit-deflate-frame")));
    assert!(!deflate::offered(None));
}
