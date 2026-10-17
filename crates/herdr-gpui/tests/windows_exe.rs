//! The Windows executable carries its icon as resource 1 beside GPUI's
//! manifest, and stays a console program so the CLI keeps its stdout and exit
//! codes.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../../test-support/pe.rs"]
mod pe;

use std::{fs, io::ErrorKind, path::Path};

const CONSOLE_SUBSYSTEM: u16 = 3;
/// The icon resource GPUI's window class loads by `LoadImageW(module, 1, ...)`.
const ICON_RESOURCE_ID: u32 = 1;

fn executable() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_herdr-gpui"))
}

#[test]
fn executable_embeds_its_icon_as_resource_one() {
    assert!(pe::has_resource_type(executable(), pe::RT_GROUP_ICON).unwrap());
    assert!(pe::has_icon_group(executable(), ICON_RESOURCE_ID).unwrap());
}

#[test]
fn executable_keeps_the_gpui_manifest() {
    assert!(pe::has_resource_type(executable(), pe::RT_MANIFEST).unwrap());
}

#[test]
fn executable_is_a_console_program() {
    assert_eq!(pe::subsystem(executable()).unwrap(), CONSOLE_SUBSYSTEM);
}

#[test]
fn reader_rejects_garbage_and_truncated_images() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("windows-exe-reader");
    fs::create_dir_all(&dir).unwrap();
    let image = fs::read(executable()).unwrap();
    let cases = [
        ("empty", Vec::new(), ErrorKind::InvalidData),
        (
            "garbage",
            b"not a portable executable".to_vec(),
            ErrorKind::InvalidData,
        ),
        (
            "dos-header-only",
            image[..0x40].to_vec(),
            ErrorKind::UnexpectedEof,
        ),
        (
            "truncated",
            image[..0x3C].to_vec(),
            ErrorKind::UnexpectedEof,
        ),
    ];
    for (name, contents, kind) in cases {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        assert_eq!(pe::subsystem(&path).unwrap_err().kind(), kind, "{name}");
        let error = pe::has_resource_type(&path, pe::RT_MANIFEST).unwrap_err();
        assert_eq!(error.kind(), kind, "{name}");
        let error = pe::has_icon_group(&path, ICON_RESOURCE_ID).unwrap_err();
        assert_eq!(error.kind(), kind, "{name}");
    }
}
