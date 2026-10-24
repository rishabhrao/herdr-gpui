//! Errors from `herdr-vcs`, folded back into the app's variants of the same
//! name, so callers match `Error::GitBusy`, `Error::CheckpointMissing`, and
//! the rest wherever a failure came from.

use super::Error;
use herdr_vcs::Error as Vcs;

impl From<Vcs> for Error {
    fn from(error: Vcs) -> Self {
        match error {
            Vcs::CheckpointBranch => Self::CheckpointBranch,
            Vcs::CheckpointBusy => Self::CheckpointBusy,
            Vcs::CheckpointCheckout => Self::CheckpointCheckout,
            Vcs::CheckpointMissing => Self::CheckpointMissing,
            Vcs::CheckpointOutput => Self::CheckpointOutput,
            Vcs::CheckpointScript(value) => Self::CheckpointScript(value),
            Vcs::CheckpointThread(value) => Self::CheckpointThread(value),
            Vcs::CheckpointUnborn => Self::CheckpointUnborn,
            Vcs::CheckpointWorker => Self::CheckpointWorker,
            #[cfg(feature = "azure")]
            Vcs::CloudHostUnsupported => Self::CloudHostUnsupported,
            Vcs::CodeIndexCancelled => Self::CodeIndexCancelled,
            Vcs::CodeIndexRoot => Self::CodeIndexRoot,
            Vcs::GitBusy => Self::GitBusy,
            Vcs::GitCommitMessage => Self::GitCommitMessage,
            Vcs::GitFailed { operation, details } => Self::GitFailed { operation, details },
            Vcs::GitHub(error) => error.into(),
            Vcs::GitNoCheckout => Self::GitNoCheckout,
            Vcs::GitNothingToCommit => Self::GitNothingToCommit,
            Vcs::GitProcess { operation, source } => Self::GitProcess { operation, source },
            Vcs::GitPullRequestBase => Self::GitPullRequestBase,
            Vcs::GitPullRequestTitle => Self::GitPullRequestTitle,
            Vcs::GitWorker => Self::GitWorker,
        }
    }
}
