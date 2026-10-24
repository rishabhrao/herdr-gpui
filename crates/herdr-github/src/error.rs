//! GitHub sign-in, pull request, and `[github]` config failures. The app
//! folds each back into its own variant of the same name, so callers match
//! one set of variants wherever a failure came from.

use std::io;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error(transparent)]
    Client(#[from] herdr_client::Error),
    #[error("HERDR_GITHUB_OAUTH_CLIENT_ID must be UTF-8")]
    ClientIdEncoding,
    #[error(transparent)]
    Credentials(#[from] herdr_credentials::Error),
    #[error(transparent)]
    GitHubAccounts(#[from] GitHubAccountsError),
    #[error(
        "GitHub authentication required. Use menu > GitHub sign-in or set GH_TOKEN / GITHUB_TOKEN."
    )]
    GitHubAuthentication,
    #[error("GitHub authorization failed. Check OAuth application settings.")]
    GitHubAuthorization,
    #[error("GitHub CLI (gh) was not found. Install it and run `gh auth login`, or sign in here.")]
    GitHubCliMissing,
    #[error("Could not run the GitHub CLI (gh).")]
    GitHubCliProcess(#[source] Box<Error>),
    #[error(
        "GitHub rejected the GitHub CLI's token. Run `gh auth login` to renew it, then try again."
    )]
    GitHubCliRejected,
    #[error("GitHub CLI is not signed in to github.com. Run `gh auth login`, then try again.")]
    GitHubCliSignedOut,
    #[error("GitHub CLI did not answer within 10 seconds. Check `gh auth status`, then try again.")]
    GitHubCliTimeout,
    #[error("GitHub authorization denied.")]
    GitHubDenied,
    #[error("Invalid GitHub device authorization response.")]
    GitHubDevice,
    #[error("Invalid GitHub credential encoding.")]
    GitHubEncoding(#[source] std::str::Utf8Error),
    #[error("GitHub code expired. Sign in again.")]
    GitHubExpired,
    #[error("GitHub denied access: check token permissions or SSO authorization.")]
    GitHubForbidden,
    #[error("Invalid GitHub authorization header.")]
    GitHubHeader(#[source] ureq::http::header::InvalidHeaderValue),
    #[error("Invalid GitHub JSON response.")]
    GitHubJson(#[source] GitHubJsonError),
    #[error("GitHub network request failed or timed out ({0}).")]
    GitHubNetwork(#[source] ureq::Error),
    #[error("GitHub query failed. Check token repository permissions and rate limits.")]
    GitHubQuery,
    /// The account's (or, signed out, this address's) API quota is used up.
    #[error("GitHub rate limit reached. {}.", herdr_updater::retry_hint(*reset))]
    GitHubRateLimit {
        reset: Option<chrono::DateTime<chrono::Utc>>,
    },
    #[error("GitHub response could not be read within the size/time limit.")]
    GitHubRead(#[source] io::Error),
    /// GitHub's own refusal of a requested change, cleaned and bounded.
    #[error("GitHub refused the request: {0}")]
    GitHubRejected(String),
    /// GraphQL answers a repository the credential cannot reach as missing.
    #[error("GitHub cannot see {repository} with this sign-in. {}", crate::hidden_hint(*app_token))]
    GitHubRepositoryHidden { repository: String, app_token: bool },
    #[error("GitHub response exceeded the size limit.")]
    GitHubSize,
    #[error("GitHub request failed (HTTP {0}). Check network and repository access.")]
    GitHubStatus(u16),
    #[error("Invalid GitHub token. Replace the configured credential.")]
    GitHubToken,
    #[error("Unsupported GitHub token type.")]
    GitHubTokenType,
    #[error("GitHub {0} worker stopped.")]
    GitHubWorker(&'static str),
    #[error(
        "{0} must be 1..256 ASCII letters, digits, '.', '_' or '-' (public client ID, not a secret)"
    )]
    InvalidClientId(&'static str),
    #[error(transparent)]
    Path(#[from] herdr_client::local_path::PathError),
    #[error("Daemon did not provide an absolute checkout and repository key.")]
    PrAbsolutePath,
    #[error("A pull request action is already running.")]
    PrActionBusy,
    #[error("Could not start the pull request worker.")]
    PrActionProcess {
        #[source]
        source: io::Error,
    },
    #[error(
        "This pull request cannot be acted on here: it is not open, or its details are incomplete. Refresh and try again."
    )]
    PrActionTarget,
    #[error("Pull request worker stopped. Check the pull request on GitHub before retrying.")]
    PrActionWorker,
    #[error("Multiple PRs match this branch; no PR selected.")]
    PrAmbiguous,
    #[error("Multiple local worktrees match the daemon branch.")]
    PrAmbiguousWorktree,
    #[error("No supported branch available.")]
    PrBranch,
    #[error("Checkout branch changed. Waiting for daemon metadata.")]
    PrBranchChanged,
    #[error("PR lookup cancelled.")]
    PrCancelled,
    #[error("Local checkout unavailable or not a trusted Git repository.")]
    PrCheckout,
    #[error("Enter a comment of at most 4096 characters.")]
    PrCommentBody,
    #[error("Invalid process text.")]
    PrEncoding(#[source] std::str::Utf8Error),
    #[error("PR identity does not match the repository and branch.")]
    PrIdentity,
    #[error("The branch changed since this dialog opened. Review the pull request again.")]
    PrMergeChanged,
    #[error("The repository does not allow this merge method.")]
    PrMergeMethod,
    #[error("No repository metadata.")]
    PrMetadata,
    #[error("No local worktree matches the daemon branch.")]
    PrMissingWorktree,
    #[error("PR lookup supports GitHub.com origins only.")]
    PrOrigin,
    #[error("Could not {operation} for Git PR lookup.")]
    PrProcess {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("GitHub repository unavailable. Check repository access and token permissions.")]
    PrRepository,
    #[error("Local repository does not match daemon metadata.")]
    PrRepositoryMismatch,
    #[error("PR response exceeded the size limit.")]
    PrSize,
    #[error("PR lookup timed out (15 seconds).")]
    PrTimeout,
    #[error(
        "PR lookup requires your owned local session socket or a saved SSH device. Other socket locations are unsupported."
    )]
    PrUntrustedEndpoint,
    #[error("Workspace directory is not in a Git repository.")]
    PrWorkspaceRepository,
    #[error("Local repository unavailable for worktree lookup.")]
    PrWorktreeLookup,
    // keep-sorted end
}

impl Error {
    pub fn github_json(source: serde_json::Error) -> Self {
        Self::GitHubJson(GitHubJsonError(source))
    }
}

/// Parser diagnostics can quote credential-bearing fields; expose the cause only
/// to explicit source inspection, never ordinary Display or Debug formatting.
#[derive(thiserror::Error)]
#[error("Invalid GitHub JSON response.")]
pub struct GitHubJsonError(#[source] serde_json::Error);

impl std::fmt::Debug for GitHubJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GitHubJsonError([REDACTED])")
    }
}

/// Why `[github] accounts`, or a change to it, was refused.
#[derive(Debug, thiserror::Error)]
pub enum GitHubAccountsError {
    #[error("[github] accounts may list at most {0} accounts")]
    TooMany(usize),
    #[error("[github] accounts id must be 32 lowercase hexadecimal digits")]
    Id,
    #[error("[github] accounts lists the id {0} twice")]
    DuplicateId(String),
    #[error("[github] accounts must be a list of account tables")]
    NotAList,
    #[error("[github] accounts owner {0:?} is not a GitHub user or organization name")]
    Owner(String),
    #[error("Could not generate a GitHub account ID")]
    Random(#[source] getrandom::Error),
}
