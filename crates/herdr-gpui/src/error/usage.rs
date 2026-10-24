//! Errors from `herdr-usage`. Its shell is shared with the listening-port and
//! system-load scans, so the failures they share with the rest of the app
//! keep the app's own variants; the rest stay usage errors.

use super::Error;
use herdr_usage::Error as Usage;

impl From<Usage> for Error {
    fn from(error: Usage) -> Self {
        match error {
            Usage::Client(error) => Self::Client(error),
            #[cfg(feature = "azure")]
            Usage::CloudHostUnsupported => Self::CloudHostUnsupported,
            Usage::WslHostUnsupported => Self::WslHostUnsupported,
            error => Self::Usage(error),
        }
    }
}
