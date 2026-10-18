//! The app's side: a GPUI platform whose windows live in a browser tab, and
//! the loopback server the tab talks to, directly or through a tunnel.
//!
//! Threads: the accept loop and one reader and one writer per connection
//! never touch GPUI. Readers queue what the page sends on a bounded channel
//! that a foreground task drains in order; the windows send frames through
//! the [`hub::Hub`], whose bounded queue a writer drains to the socket.
/// Writes one line of the server's log to standard error, after the time
/// of day in UTC, so a run of disconnects can be told apart from one.
macro_rules! note {
    ($($arg:tt)*) => {
        eprintln!("{} {}", crate::host::clock(), format_args!($($arg)*))
    };
}

mod access;
mod assets;
mod atlas;
mod deflate;
mod devices;
mod gate;
mod http;
mod hub;
mod input;
mod peers;
mod platform;
mod routes;
mod served;
mod server;
mod stream;
mod window;
mod ws;

pub use access::AccessOptions;
pub use devices::{Device, PairingRequest, forget as forget_device, list as list_devices};
pub use hub::{PageFiles, PageHandle, Pages};
pub use served::{Connection, Served};

use crate::{Error, Result};
use gpui::Platform;
use std::{net::TcpListener, path::PathBuf, rc::Rc, sync::Arc};

/// What `herdr-gpui serve` was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub port: u16,
    /// Where `just web-build` put the page's script and WebAssembly.
    pub client_dir: PathBuf,
    /// Where the access token and the paired devices are kept.
    pub state_dir: PathBuf,
    /// Hosts a tunnel on this machine forwards, such as `herdr.example.com`.
    pub public_hosts: Vec<String>,
    /// The Cloudflare Access application to check requests through a public
    /// host against. Without one, the first properly signed token names it.
    pub access: Option<AccessOptions>,
    /// Requires Cloudflare Access on public hosts; off only on request.
    pub check_access: bool,
}

/// Where the learned Cloudflare Access application is kept; deleting it
/// makes the next request through the tunnel name it again.
pub const ACCESS_FILE: &str = "web-access.json";

/// Where `just web-build` puts the page in this checkout, unless
/// `HERDR_WEB_CLIENT_DIR` names another folder.
pub fn default_client_dir() -> PathBuf {
    std::env::var_os("HERDR_WEB_CLIENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/web-client")
        })
}

/// A bound server, not yet answering anything a window needs.
pub struct Server {
    hub: Arc<hub::Hub>,
    port: u16,
    inbound: async_channel::Receiver<hub::Inbound>,
    approvals: Approvals,
}

/// Reads the page, binds `127.0.0.1:port`, and starts answering requests.
/// Fails before any window exists, so a taken port or a missing build is
/// reported at once.
pub fn bind(options: &Options) -> Result<Server> {
    let assets = assets::Assets::load(&options.client_dir)?;
    let listener =
        TcpListener::bind(("127.0.0.1", options.port)).map_err(|source| Error::Bind {
            port: options.port,
            source,
        })?;
    let port = listener.local_addr()?.port();
    let access = (options.check_access && !options.public_hosts.is_empty())
        .then(|| {
            access::Access::start(
                options.access.clone(),
                Some(options.state_dir.join(ACCESS_FILE)),
            )
        })
        .transpose()?;
    stream::set_profile(options.state_dir.join("web-browser"));
    let (hub, inbound, requests) = hub::Hub::new(hub::Setup {
        doors: gate::Doors::new(port, &options.public_hosts),
        token: hub::saved_token(&options.state_dir.join("web-token"))?,
        assets,
        devices: devices::Devices::new(&options.state_dir),
        access,
    });
    server::start(listener, hub.clone())?;
    let approvals = Approvals {
        hub: hub.clone(),
        requests,
    };
    Ok(Server {
        hub,
        port,
        inbound,
        approvals,
    })
}

impl Server {
    /// The address that pairs this machine's own browser.
    pub fn url(&self) -> String {
        self.hub.local_url(self.port)
    }

    /// The public hosts this server answers, as normalized.
    pub fn public_hosts(&self) -> Vec<String> {
        self.hub.doors.public_hosts().to_vec()
    }

    /// Pairing requests for the app to show to whoever uses it.
    pub fn approvals(&self) -> Approvals {
        self.approvals.clone()
    }

    /// `inner`'s services with windows that live in the page.
    pub fn into_platform(self, inner: Rc<dyn Platform>) -> Rc<dyn Platform> {
        Rc::new(platform::RemotePlatform::new(inner, self.hub, self.inbound))
    }
}

/// Browsers asking to pair, and the way to answer them.
#[derive(Clone)]
pub struct Approvals {
    hub: Arc<hub::Hub>,
    requests: async_channel::Receiver<PairingRequest>,
}

impl Approvals {
    /// The next request, or `None` once the server is gone.
    pub async fn next(&self) -> Option<PairingRequest> {
        self.requests.recv().await.ok()
    }

    /// Allows or denies request `id`. False when it is no longer waiting.
    pub fn answer(&self, id: u64, allow: bool) -> Result<bool> {
        self.hub.devices.answer(id, allow)
    }

    /// Whether request `id` still waits for an answer.
    pub fn is_waiting(&self, id: u64) -> bool {
        self.hub.devices.is_waiting(id)
    }

    /// Allows or denies the waiting request with `code`. False when none
    /// waits with it.
    pub fn answer_code(&self, code: &str, allow: bool) -> Result<bool> {
        match self.hub.devices.waiting_with_code(code) {
            Some(id) => self.hub.devices.answer(id, allow),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests;

/// The time of day in UTC, as `HH:MM:SSZ`.
pub(crate) fn clock() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
        % 86_400;
    format!(
        "{:02}:{:02}:{:02}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}
