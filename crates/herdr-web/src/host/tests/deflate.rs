use crate::host::deflate;
use flate2::{Decompress, FlushDecompress};

/// About 430 KB that compresses 10:1, like a frame of a busy window. This
/// one made `write::DeflateEncoder::flush` end the stream mid-block.
fn frame_like(mut seed: u64) -> Vec<u8> {
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..427_849)
        .map(|index| {
            if random() % 12 == 0 {
                random() as u8
            } else {
                (index % 80) as u8
            }
        })
        .collect()
}

#[test]
fn every_message_ends_where_the_next_one_can_start() {
    let messages = [frame_like(9), frame_like(10), b"frame ".repeat(500)];
    // A browser inflates every message through one inflater, adding the
    // tail the extension leaves off each.
    let mut inflater = Decompress::new(false);
    for message in &messages {
        let mut input = deflate::compress(message).unwrap();
        input.extend_from_slice(&[0x00, 0x00, 0xff, 0xff]);
        let start = inflater.total_in();
        let mut out = Vec::with_capacity(message.len() + 1024);
        while ((inflater.total_in() - start) as usize) < input.len() {
            if out.len() == out.capacity() {
                out.reserve(1 << 16);
            }
            let read = (inflater.total_in() - start) as usize;
            inflater
                .decompress_vec(&input[read..], &mut out, FlushDecompress::Sync)
                .unwrap();
        }
        assert_eq!(out.len(), message.len());
        assert!(out == *message);
    }
}
