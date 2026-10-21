#![allow(clippy::unwrap_used)]
//! Shared fixtures; each topic's tests are in `tests/`.
use super::{
    assets::Assets,
    devices::{Devices, PairingRequest},
    gate::Doors,
    http::Request,
    hub::{Hub, Setup, random_hex},
};
use std::{path::PathBuf, sync::Arc};

mod access;
mod deflate;
mod devices;
mod gate;
mod hub;
mod io;
mod peers;
mod routes;

pub(super) const PORT: u16 = 4321;
pub(super) const PUBLIC: &str = "herdr.example.com";

/// A private, empty state folder, removed with the guard.
pub(super) struct Dir(pub(super) PathBuf);

impl Dir {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "herdr-web-{name}-{}-{}",
            std::process::id(),
            random_hex(4).unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) struct Fixture {
    pub(super) hub: Arc<Hub>,
    pub(super) approvals: async_channel::Receiver<PairingRequest>,
    pub(super) token: String,
    pub(super) _dir: Dir,
}

pub(super) fn fixture(name: &str) -> Fixture {
    let dir = Dir::new(name);
    let token = random_hex(32).unwrap();
    let (hub, _, approvals) = Hub::new(Setup {
        doors: Doors::new(PORT, &[format!("https://{PUBLIC}/")]),
        token: token.clone(),
        assets: Assets::fixture(),
        devices: Devices::new(&dir.0),
        access: None,
    });
    Fixture {
        hub,
        approvals,
        token,
        _dir: dir,
    }
}

pub(super) fn request(method: &str, path: &str, query: &str, headers: &[(&str, &str)]) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        query: query.into(),
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

/// The headers this machine's own page sends.
pub(super) fn local(extra: &[(&'static str, String)]) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("Host", format!("localhost:{PORT}")),
        ("Origin", format!("http://localhost:{PORT}")),
    ];
    headers.extend(extra.iter().cloned());
    headers
}

/// The headers a phone's page sends through the tunnel.
pub(super) fn public(extra: &[(&'static str, String)]) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("Host", PUBLIC.to_owned()),
        ("Origin", format!("https://{PUBLIC}")),
        ("Cf-Connecting-Ip", "203.0.113.9".to_owned()),
    ];
    headers.extend(extra.iter().cloned());
    headers
}

pub(super) fn with(
    method: &str,
    path: &str,
    query: &str,
    headers: &[(&'static str, String)],
) -> Request {
    let pairs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    request(method, path, query, &pairs)
}

/// The value a response sets for cookie `name`.
pub(super) fn set_cookie(response: &super::http::Response, name: &str) -> Option<String> {
    response
        .headers
        .iter()
        .filter(|(header, _)| *header == "Set-Cookie")
        .find_map(|(_, value)| {
            let (key, rest) = value.split_once('=')?;
            (key == name).then(|| rest.split(';').next().unwrap_or_default().to_owned())
        })
}
