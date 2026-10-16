//! The page's files. They hold no secrets, so they are served to anyone the
//! front door lets through; each carries an `ETag`, so a phone that already
//! has the 4 MB of WebAssembly checks it rather than downloading it again.
use crate::{Error, Result};
use sha2::{Digest as _, Sha256};
use std::{collections::HashMap, path::Path, sync::Arc};

const SCRIPT: &str = "herdr_web_client.js";
const WASM: &str = "herdr_web_client_bg.wasm";

/// Built into the server: path, content type, bytes.
const BUILT_IN: [(&str, &str, &[u8]); 9] = [
    (
        "/",
        "text/html; charset=utf-8",
        include_bytes!("page/index.html"),
    ),
    (
        "/apple-touch-icon.png",
        "image/png",
        include_bytes!("page/apple-touch-icon.png"),
    ),
    (
        "/boot.js",
        "text/javascript; charset=utf-8",
        include_bytes!("page/boot.js"),
    ),
    (
        "/chrome.js",
        "text/javascript; charset=utf-8",
        include_bytes!("page/chrome.js"),
    ),
    (
        "/icon-192.png",
        "image/png",
        include_bytes!("page/icon-192.png"),
    ),
    (
        "/icon-512.png",
        "image/png",
        include_bytes!("page/icon-512.png"),
    ),
    (
        "/manifest.webmanifest",
        "application/manifest+json",
        include_bytes!("page/manifest.webmanifest"),
    ),
    (
        "/style.css",
        "text/css; charset=utf-8",
        include_bytes!("page/style.css"),
    ),
    (
        "/sw.js",
        "text/javascript; charset=utf-8",
        include_bytes!("page/sw.js"),
    ),
];

pub(crate) struct Asset {
    pub(crate) kind: &'static str,
    pub(crate) etag: String,
    pub(crate) bytes: Arc<[u8]>,
}

impl Asset {
    fn new(kind: &'static str, bytes: Vec<u8>) -> Self {
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .take(12)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self {
            kind,
            etag: format!("\"{digest}\""),
            bytes: bytes.into(),
        }
    }
}

pub(crate) struct Assets(HashMap<&'static str, Asset>);

impl Assets {
    /// The built-in files and the WebAssembly build in `client_dir`.
    pub(crate) fn load(client_dir: &Path) -> Result<Self> {
        let read = |name: &str| {
            let path = client_dir.join(name);
            std::fs::read(&path).map_err(|source| Error::MissingClient { path, source })
        };
        let mut assets: HashMap<&'static str, Asset> = BUILT_IN
            .iter()
            .map(|(path, kind, bytes)| (*path, Asset::new(kind, bytes.to_vec())))
            .collect();
        assets.insert(
            "/herdr_web_client.js",
            Asset::new("text/javascript; charset=utf-8", read(SCRIPT)?),
        );
        assets.insert(
            "/herdr_web_client_bg.wasm",
            Asset::new("application/wasm", read(WASM)?),
        );
        Ok(Self(assets))
    }

    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        let mut assets: HashMap<&'static str, Asset> = BUILT_IN
            .iter()
            .map(|(path, kind, bytes)| (*path, Asset::new(kind, bytes.to_vec())))
            .collect();
        assets.insert(
            "/herdr_web_client_bg.wasm",
            Asset::new("application/wasm", b"\0asm".to_vec()),
        );
        Self(assets)
    }

    pub(crate) fn get(&self, path: &str) -> Option<&Asset> {
        self.0.get(path)
    }
}
