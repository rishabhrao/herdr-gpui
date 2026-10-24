//! Errors from `herdr-github`, folded back into the app's variants of the
//! same name, so callers match `Error::GitHubRateLimit`, `Error::PrOrigin`,
//! and the rest wherever a failure came from.

use super::Error;
use herdr_client::local_path::PathError;
use herdr_github::Error as GitHub;
pub use herdr_github::{GitHubAccountsError, GitHubJsonError};

impl From<GitHub> for Error {
    fn from(error: GitHub) -> Self {
        match error {
            GitHub::Client(value) => Self::Client(value),
            GitHub::ClientIdEncoding => Self::ClientIdEncoding,
            GitHub::Credentials(value) => Self::Credentials(value),
            GitHub::GitHubAccounts(value) => Self::GitHubAccounts(value),
            GitHub::GitHubAuthentication => Self::GitHubAuthentication,
            GitHub::GitHubAuthorization => Self::GitHubAuthorization,
            GitHub::GitHubCliMissing => Self::GitHubCliMissing,
            GitHub::GitHubCliProcess(error) => Self::GitHubCliProcess(Box::new((*error).into())),
            GitHub::GitHubCliRejected => Self::GitHubCliRejected,
            GitHub::GitHubCliSignedOut => Self::GitHubCliSignedOut,
            GitHub::GitHubCliTimeout => Self::GitHubCliTimeout,
            GitHub::GitHubDenied => Self::GitHubDenied,
            GitHub::GitHubDevice => Self::GitHubDevice,
            GitHub::GitHubEncoding(value) => Self::GitHubEncoding(value),
            GitHub::GitHubExpired => Self::GitHubExpired,
            GitHub::GitHubForbidden => Self::GitHubForbidden,
            GitHub::GitHubHeader(value) => Self::GitHubHeader(value),
            GitHub::GitHubJson(value) => Self::GitHubJson(value),
            GitHub::GitHubNetwork(value) => Self::GitHubNetwork(value),
            GitHub::GitHubQuery => Self::GitHubQuery,
            GitHub::GitHubRateLimit { reset } => Self::GitHubRateLimit { reset },
            GitHub::GitHubRead(value) => Self::GitHubRead(value),
            GitHub::GitHubRejected(value) => Self::GitHubRejected(value),
            GitHub::GitHubRepositoryHidden {
                repository,
                app_token,
            } => Self::GitHubRepositoryHidden {
                repository,
                app_token,
            },
            GitHub::GitHubSize => Self::GitHubSize,
            GitHub::GitHubStatus(value) => Self::GitHubStatus(value),
            GitHub::GitHubToken => Self::GitHubToken,
            GitHub::GitHubTokenType => Self::GitHubTokenType,
            GitHub::GitHubWorker(value) => Self::GitHubWorker(value),
            GitHub::InvalidClientId(value) => Self::InvalidClientId(value),
            GitHub::Path(PathError::MissingHome) => Self::MissingHome,
            GitHub::Path(PathError::RelativeConfigRoot) => Self::RelativeConfigRoot,
            GitHub::PrAbsolutePath => Self::PrAbsolutePath,
            GitHub::PrActionBusy => Self::PrActionBusy,
            GitHub::PrActionProcess { source } => Self::PrActionProcess { source },
            GitHub::PrActionTarget => Self::PrActionTarget,
            GitHub::PrActionWorker => Self::PrActionWorker,
            GitHub::PrAmbiguous => Self::PrAmbiguous,
            GitHub::PrAmbiguousWorktree => Self::PrAmbiguousWorktree,
            GitHub::PrBranch => Self::PrBranch,
            GitHub::PrBranchChanged => Self::PrBranchChanged,
            GitHub::PrCancelled => Self::PrCancelled,
            GitHub::PrCheckout => Self::PrCheckout,
            GitHub::PrCommentBody => Self::PrCommentBody,
            GitHub::PrEncoding(value) => Self::PrEncoding(value),
            GitHub::PrIdentity => Self::PrIdentity,
            GitHub::PrMergeChanged => Self::PrMergeChanged,
            GitHub::PrMergeMethod => Self::PrMergeMethod,
            GitHub::PrMetadata => Self::PrMetadata,
            GitHub::PrMissingWorktree => Self::PrMissingWorktree,
            GitHub::PrOrigin => Self::PrOrigin,
            GitHub::PrProcess { operation, source } => Self::PrProcess { operation, source },
            GitHub::PrRepository => Self::PrRepository,
            GitHub::PrRepositoryMismatch => Self::PrRepositoryMismatch,
            GitHub::PrSize => Self::PrSize,
            GitHub::PrTimeout => Self::PrTimeout,
            GitHub::PrUntrustedEndpoint => Self::PrUntrustedEndpoint,
            GitHub::PrWorkspaceRepository => Self::PrWorkspaceRepository,
            GitHub::PrWorktreeLookup => Self::PrWorktreeLookup,
        }
    }
}
