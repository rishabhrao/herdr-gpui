//! Following a parsed link in a window: switching to a device, or showing a
//! local checkout. A checkout already open in a workspace is focused; any
//! other is only opened after the user agrees in the window, since a link can
//! come from any web page and opening one starts a shell in that directory.

use super::Link;
use crate::{
    HerdrWindow, NavigationTarget,
    control::{ErrorCode, Response},
    endpoint::LOCAL,
};
use gpui::{App, Context, Global, Window, WindowHandle};
use herdr_client::protocol::ClientShellSnapshot;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("{} could not be read", .path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{} is not a folder", .0.display())]
    NotFolder(PathBuf),
    #[error("{} is not a Git checkout or worktree", .0.display())]
    NotCheckout(PathBuf),
    /// Only Windows paths can name another computer's share.
    #[cfg(windows)]
    #[error("{} is on another computer", .0.display())]
    Remote(PathBuf),
}

/// A checkout a link names, and the workspace already showing it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Checkout {
    pub path: PathBuf,
    pub workspace: Option<String>,
}

/// A link the app was started with, followed once a window can.
#[derive(Default)]
pub(crate) struct Pending(Option<Link>);

impl Global for Pending {}

pub(crate) fn defer(link: Link, cx: &mut App) {
    cx.set_global(Pending(Some(link)));
}

/// Follows the link the app was started with once this window has what it
/// needs: a device once the saved devices are loaded (a window starts with
/// Local alone), a checkout once this computer's Herdr is connected. A notification from before the app started is not known, so
/// starting the app was all its click could do.
pub(crate) fn poll(view: &HerdrWindow, window: &Window, cx: &mut Context<HerdrWindow>) {
    let ready = match cx
        .try_global::<Pending>()
        .and_then(|pending| pending.0.as_ref())
    {
        None => return,
        Some(Link::Notification { .. }) => {
            cx.global_mut::<Pending>().0 = None;
            return;
        }
        Some(Link::Device { id }) => {
            view.catalog.loaded() || view.endpoints.iter().any(|endpoint| &endpoint.id == id)
        }
        Some(Link::Open { .. }) => local_snapshot(view).is_some(),
    };
    let Some(handle) = window.window_handle().downcast::<HerdrWindow>() else {
        return;
    };
    if !ready {
        return;
    }
    let Some(link) = cx.global_mut::<Pending>().0.take() else {
        return;
    };
    cx.defer(move |cx| {
        let response = follow(link, handle, cx);
        if let Response::Error { message, .. } = response {
            let _ = handle.update(cx, |view, _, cx| {
                view.local_error = Some(format!("Open link: {message}"));
                cx.notify();
            });
        }
    });
}

pub(crate) fn follow(link: Link, handle: WindowHandle<HerdrWindow>, cx: &mut App) -> Response {
    let closed = || Response::error(ErrorCode::NoWindow, "The window closed");
    match link {
        Link::Notification { tag } => {
            if crate::window::system_notifications::open(&tag, cx) {
                Response::done("Opened the notification's pane")
            } else {
                Response::error(ErrorCode::NotFound, "That notification is no longer known")
            }
        }
        Link::Device { id } => handle
            .update(cx, |view, window, cx| {
                if view.menu.page.is_some() {
                    return Response::error(
                        ErrorCode::WindowBusy,
                        "A dialog is open in the window; close it first",
                    );
                }
                if !view.select_endpoint(&id, cx) {
                    return Response::error(ErrorCode::NotFound, format!("No device {id}"));
                }
                bring_forward(window, cx);
                Response::done(format!(
                    "Showing {}",
                    view.endpoints[view.selected_endpoint].label
                ))
            })
            .unwrap_or_else(|_| closed()),
        Link::Open { path } => {
            let Ok(snapshot) = handle.read_with(cx, |view, _| local_snapshot(view)) else {
                return closed();
            };
            let Some(snapshot) = snapshot else {
                return Response::error(
                    ErrorCode::Busy,
                    "Herdr UI is not connected to this computer's Herdr",
                );
            };
            let message = format!("Opening {}", path.display());
            let check = cx
                .background_executor()
                .spawn(async move { checkout_at(&path, &snapshot) });
            cx.spawn(async move |cx| {
                let found = check.await;
                let _ = handle.update(cx, |view, window, cx| {
                    view.show_checkout(found, window, cx);
                });
            })
            .detach();
            Response::done(message)
        }
    }
}

/// Whether the window is connected to this computer's Herdr, so it can open
/// a checkout.
pub(crate) fn shows_local(view: &HerdrWindow) -> bool {
    local_snapshot(view).is_some()
}

/// The snapshot of this computer's Herdr, when the window is connected to it.
fn local_snapshot(view: &HerdrWindow) -> Option<Arc<ClientShellSnapshot>> {
    let index = view.endpoints.iter().position(|e| e.id == LOCAL)?;
    let live = if index == view.selected_endpoint {
        &view.live
    } else {
        &view.endpoints[index].live
    };
    live.status
        .is_connected()
        .then(|| live.snapshot.clone())
        .flatten()
}

fn bring_forward(window: &mut Window, cx: &mut App) {
    cx.activate(true);
    window.activate_window();
}

/// Checks a link's folder on disk, off the UI thread, and finds the
/// workspace already showing it.
fn checkout_at(path: &Path, snapshot: &ClientShellSnapshot) -> Result<Checkout, Error> {
    let path = canonical(path)?;
    if !path.is_dir() {
        return Err(Error::NotFolder(path));
    }
    // A main checkout has a `.git` folder, a linked worktree a `.git` file.
    if std::fs::symlink_metadata(path.join(".git")).is_err() {
        return Err(Error::NotCheckout(path));
    }
    let workspace = snapshot.workspaces.iter().find_map(|workspace| {
        let root = Path::new(crate::palette::launch_root(
            snapshot,
            &workspace.workspace_id,
        )?);
        (root.is_absolute() && canonical(root).is_ok_and(|root| root == path))
            .then(|| workspace.workspace_id.clone())
    });
    Ok(Checkout { path, workspace })
}

/// The path with links resolved, in the form Herdr reports workspace
/// directories in: on Windows a plain drive path, never the `\\?\` form
/// `canonicalize` returns, and never another computer's share.
fn canonical(path: &Path) -> Result<PathBuf, Error> {
    let resolved = std::fs::canonicalize(path).map_err(|source| Error::Unreadable {
        path: path.to_owned(),
        source,
    })?;
    #[cfg(windows)]
    {
        let text = resolved.to_string_lossy();
        if let Some(drive) = text.strip_prefix(r"\\?\")
            && drive.as_bytes().get(1) == Some(&b':')
        {
            return Ok(PathBuf::from(drive));
        }
        if text.starts_with(r"\\") {
            return Err(Error::Remote(resolved));
        }
    }
    Ok(resolved)
}

impl HerdrWindow {
    fn show_checkout(
        &mut self,
        found: Result<Checkout, Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        bring_forward(window, cx);
        // An earlier link's complaint no longer applies.
        if self
            .local_error
            .as_deref()
            .is_some_and(|error| error.starts_with("Open link:"))
        {
            self.local_error = None;
        }
        match found {
            Ok(Checkout {
                workspace: Some(workspace),
                ..
            }) => {
                if !self.navigate_endpoint(LOCAL, NavigationTarget::Workspace(&workspace), cx) {
                    self.local_error = Some("Open link: close the open dialog first".into());
                }
            }
            Ok(Checkout {
                path,
                workspace: None,
            }) => self.ask_to_open_checkout(path, window, cx),
            Err(error) => self.local_error = Some(format!("Open link: {error}")),
        }
        cx.notify();
    }
}

#[cfg(test)]
#[path = "follow_tests.rs"]
mod tests;
