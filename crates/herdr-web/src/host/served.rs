//! What the served app shows about its own serving: its addresses, the
//! pages connected now, and the paired devices, which it can forget.
use super::{devices::Device, hub::Hub};
use std::sync::{Arc, OnceLock};

static SERVED: OnceLock<Served> = OnceLock::new();

pub(super) fn install(hub: &Arc<Hub>) {
    let _ = SERVED.set(Served(hub.clone()));
}

/// A page connected now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub label: String,
    /// Through a tunnel rather than at this machine.
    pub remote: bool,
}

/// The server of a served app. Only exists while serving.
pub struct Served(Arc<Hub>);

impl Served {
    /// The served app's server, or `None` when the app has native windows.
    pub fn get() -> Option<&'static Served> {
        SERVED.get()
    }

    /// The address for a browser on this machine.
    pub fn local_address(&self) -> String {
        format!("http://localhost:{}", self.0.doors.port())
    }

    /// The names the app is served at through a tunnel.
    pub fn public_hosts(&self) -> Vec<String> {
        self.0.doors.public_hosts().to_vec()
    }

    pub fn connections(&self) -> Vec<Connection> {
        self.0
            .visitors()
            .into_iter()
            .map(|visitor| Connection {
                label: visitor.label,
                remote: visitor.remote,
            })
            .collect()
    }

    /// The paired devices. Reads the devices file when it changed, so call
    /// it off the UI thread.
    pub fn devices(&self) -> Vec<Device> {
        self.0.devices.list()
    }

    /// Forgets device `id` and closes its pages. Writes the devices file.
    pub fn forget(&self, id: &str) -> crate::Result<bool> {
        let removed = self.0.devices.forget(id)?;
        self.0.drop_device(id);
        Ok(removed)
    }
}
