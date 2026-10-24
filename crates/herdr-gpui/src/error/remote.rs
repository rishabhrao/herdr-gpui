//! Errors from `herdr-remote`, folded into the app's own variants so callers
//! match `Error::Provider`, `Error::Cloud`, and the rest wherever they come
//! from, and a config failure comes back as the app's own error.

use super::Error;
use herdr_remote::Error as Remote;

impl From<Remote> for Error {
    fn from(error: Remote) -> Self {
        match error {
            Remote::Client(error) => Self::Client(error),
            #[cfg(feature = "cloud")]
            Remote::Cloud(error) => Self::Cloud(error),
            // The app's loader boxed its own error; anything else stays boxed.
            Remote::Config(error) => match error.downcast::<Error>() {
                Ok(error) => *error,
                Err(error) => Self::Remote(Remote::Config(error)),
            },
            Remote::Credentials(error) => Self::Credentials(error),
            Remote::Io(error) => Self::Io(error),
            Remote::Json(error) => Self::Json(error),
            Remote::Path(herdr_client::local_path::PathError::MissingHome) => Self::MissingHome,
            Remote::Path(herdr_client::local_path::PathError::RelativeConfigRoot) => {
                Self::RelativeConfigRoot
            }
            Remote::PreferencesPath => Self::PreferencesPath,
            #[cfg(feature = "cloud")]
            Remote::Provider(error) => Self::Provider(error),
            #[cfg(feature = "cloud")]
            Remote::SandboxImage(error) => Self::SandboxImage(error),
            Remote::Teleport(error) => Self::Teleport(error),
            error @ Remote::NoConfig => Self::Remote(error),
        }
    }
}

/// A provider's own error, as `herdr_remote` itself converts it.
macro_rules! provider_errors {
    ($($feature:literal $module:ident),* $(,)?) => {
        $(#[cfg(feature = $feature)]
        impl From<herdr_remote::$module::Error> for Error {
            fn from(error: herdr_remote::$module::Error) -> Self {
                Remote::from(error).into()
            }
        })*
    };
}

provider_errors! {
    // keep-sorted start
    "azure" azure,
    "boat" boat,
    "coder" coder,
    "container" container,
    "daytona" daytona,
    "opensandbox" opensandbox,
    "railway" railway,
    "sprites" sprites,
    // keep-sorted end
}
