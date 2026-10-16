//! Shows a GPUI app's windows in a browser tab. The app keeps running on this
//! machine on a [`host`] platform that has no native windows: each frame's
//! scene and the glyph images it uses go to the page over a WebSocket, and the
//! page sends input back. Both sides share the [`wire`] format; the page
//! itself is the `herdr-web-client` crate.
mod error;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
pub mod host;
pub mod wire;

pub use error::{Error, Result};
