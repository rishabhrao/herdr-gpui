//! The providers' saved secrets, in Herdr UI's credential store beside the
//! GUI config, under each provider's own plaintext opt-in.

use crate::{Error, Result};
pub use herdr_credentials::Store;
use secrecy::SecretString;
use std::path::PathBuf;

pub type Entry = herdr_credentials::Entry<Error>;

/// The directory the GUI config, and so the private credential files, live in.
pub fn directory() -> Result<PathBuf> {
    Ok(herdr_client::local_path::config_root()?.join("herdr"))
}

/// Read `entry` from `store`. Callers run on a background worker.
pub fn read_entry(store: Store, entry: &Entry) -> Result<Option<SecretString>> {
    herdr_credentials::read_entry(store, entry, &directory()?)
}

/// Write or remove `entry` in `store`. Callers run on a background worker.
pub fn save_entry(store: Store, entry: &Entry, token: Option<&SecretString>) -> Result<()> {
    herdr_credentials::save_entry(store, entry, &directory()?, token)
}
