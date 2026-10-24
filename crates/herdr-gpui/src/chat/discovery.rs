//! Which panes run a chat program this window may attach to.

use super::{MAX_CHATS, Route, Target};
use crate::state::LiveState;
use herdr_chat_protocol::{EndpointId, TOKEN};
use herdr_client::{ConnectTarget, protocol::ClientShellSnapshot};
use std::collections::HashSet;

/// Whether the daemon behind `target` runs on this machine as this user, so
/// its panes' chat endpoints are this user's own: the trust boundary local Git
/// and checkpoints use. A socket given by path may be a forward from another
/// host, and every remote or cloud daemon's programs listen elsewhere.
///
/// Windows cannot verify the daemon's peer, so there the check falls to the
/// chat pipe itself, which is refused unless this user owns it.
pub(crate) fn local(target: &ConnectTarget, live: &LiveState) -> bool {
    match target {
        ConnectTarget::Local | ConnectTarget::Session { .. } => {
            (live.local_daemon_peer || cfg!(windows)) && live.status.is_connected()
        }
        _ => false,
    }
}

/// How this window reaches the chat programs of the daemon behind `target`:
/// on this machine as for [`local`], or over SSH on the host an SSH
/// connection runs the daemon on. A socket given by path, a WSL
/// distribution, and a cloud machine have no route yet.
pub(crate) fn route(target: &ConnectTarget, live: &LiveState) -> Option<Route> {
    match target {
        _ if local(target, live) => Some(Route::Local),
        ConnectTarget::Ssh { target, .. } if live.status.is_connected() => {
            Some(Route::Ssh(target.clone()))
        }
        _ => None,
    }
}

/// The program a pane's metadata names, when its token holds a valid id.
pub(crate) fn endpoint(tokens: &[(String, String)]) -> Option<EndpointId> {
    tokens
        .iter()
        .find(|(name, _)| name == TOKEN)
        .and_then(|(_, value)| value.parse().ok())
}

/// The chat panes of a reachable daemon's snapshot, in its order, one per
/// pane and at most [`MAX_CHATS`], each reached by `route`. A daemon with
/// no route has none.
pub(crate) fn targets(snapshot: &ClientShellSnapshot, route: Option<&Route>) -> Vec<Target> {
    let Some(route) = route else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    snapshot
        .agents
        .iter()
        .filter_map(|agent| {
            Some(Target {
                endpoint: endpoint(&agent.tokens)?,
                pane_id: agent.pane_id.clone(),
                route: route.clone(),
            })
        })
        .filter(|target| seen.insert(target.pane_id.clone()))
        .take(MAX_CHATS)
        .collect()
}
