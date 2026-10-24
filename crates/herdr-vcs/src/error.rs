//! Git, checkpoint, and code index failures. The app folds each back into
//! its own variant of the same name, so callers match one set of variants
//! wherever a failure came from.

use std::io;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("That checkpoint was taken on another branch. Check that branch out to restore it.")]
    CheckpointBranch,
    #[error("A merge, rebase, cherry-pick, or revert is in progress. Finish or abort it first.")]
    CheckpointBusy,
    #[error("No checkout of this branch was found on its host.")]
    CheckpointCheckout,
    #[error("That checkpoint no longer exists.")]
    CheckpointMissing,
    #[error("Checkpoint output was not understood.")]
    CheckpointOutput,
    #[error("Checkpoint Git commands failed")]
    CheckpointScript(#[source] herdr_client::Error),
    #[error("Could not start the checkpoint worker")]
    CheckpointThread(#[source] io::Error),
    #[error("This checkout has no commit yet, so it has no checkpoints.")]
    CheckpointUnborn,
    #[error("The checkpoint worker stopped.")]
    CheckpointWorker,
    /// Only CPU and memory are sampled on an Azure machine so far.
    #[cfg(feature = "azure")]
    #[error("This is not available for cloud machines yet.")]
    CloudHostUnsupported,
    #[error("Reading the checkout was cancelled")]
    CodeIndexCancelled,
    #[error("Only a local Git checkout can be searched for files and symbols")]
    CodeIndexRoot,
    #[error("A Git operation is already running.")]
    GitBusy,
    #[error("Enter a commit message of at most 4096 characters.")]
    GitCommitMessage,
    #[error("git {operation} failed: {details}")]
    GitFailed {
        operation: &'static str,
        details: String,
    },
    #[error(transparent)]
    GitHub(#[from] herdr_github::Error),
    #[error("No local checkout is focused.")]
    GitNoCheckout,
    #[error("Nothing to commit: the checkout has no changes.")]
    GitNothingToCommit,
    #[error("Could not {operation}.")]
    GitProcess {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("This branch is the repository default branch; no pull request can be opened from it.")]
    GitPullRequestBase,
    #[error("The branch has no commit to describe. Commit before opening a pull request.")]
    GitPullRequestTitle,
    #[error("Git worker stopped. Retry the operation.")]
    GitWorker,
    // keep-sorted end
}
