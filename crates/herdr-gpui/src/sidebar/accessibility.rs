//! The names the sidebar's rows give screen readers. A row paints its parts
//! as plain text, which assistive technology never sees, so each row is one
//! list item whose name says what the row shows, most telling part first.

use crate::{HerdrWindow, accessibility::update, window::chrome_focus::Region};
use gpui::{AccessibleAction, Context, Role, StatefulInteractiveElement, WeakEntity, Window};
use herdr_client::protocol::AgentStatus;

/// Gives a row that folds its children the expanded state and the Expand and
/// Collapse actions, which run the same `toggle` as its arrow. AccessKit only
/// asks for the action that changes the state, so both toggle.
pub(super) fn expandable<E: StatefulInteractiveElement>(
    row: E,
    expanded: bool,
    view: WeakEntity<HerdrWindow>,
    toggle: impl Fn(&mut HerdrWindow, &mut Window, &mut Context<HerdrWindow>) + Clone + 'static,
) -> E {
    row.aria_expanded(expanded)
        .on_a11y_action(
            AccessibleAction::Expand,
            update(view.clone(), toggle.clone()),
        )
        .on_a11y_action(AccessibleAction::Collapse, update(view, toggle))
}

impl HerdrWindow {
    /// A sidebar button: its name, and its place among the sidebar's stops.
    pub(super) fn sidebar_button<E: StatefulInteractiveElement>(
        &self,
        button: E,
        label: &'static str,
    ) -> E {
        self.chrome_stop(Region::Sidebar, button.role(Role::Button).aria_label(label))
    }

    /// A workspace row as a list item named `name`. A repository's row folds
    /// its linked worktrees: `folds` is whether they are folded, and the
    /// endpoint and repository key that fold them.
    pub(super) fn workspace_item<E: StatefulInteractiveElement>(
        &self,
        row: E,
        name: String,
        selected: bool,
        folds: Option<(bool, (String, String))>,
        cx: &mut Context<Self>,
    ) -> E {
        let row = row
            .role(Role::ListItem)
            .aria_label(name)
            .aria_selected(selected);
        let row = self.chrome_stop(Region::Sidebar, row);
        let Some((collapsed, (endpoint, key))) = folds else {
            return row;
        };
        expandable(
            row,
            !collapsed,
            cx.entity().downgrade(),
            move |this, _, cx| {
                this.toggle_repository(&endpoint, &key, cx);
            },
        )
    }
}

/// A workspace row: its label, then its status. A row on another device
/// names that device too, since the list is flat.
pub(super) fn workspace_name(label: &str, status: AgentStatus, host: Option<&str>) -> String {
    join([Some(label), status_word(status), host])
}

/// An agent row: the agent, its state (the agent's own label for it, when it
/// gave one), and where it runs.
pub(super) fn agent_name(
    name: &str,
    state: &str,
    place: Option<(&str, Option<&str>)>,
    host: Option<&str>,
) -> String {
    let (workspace, tab) = place.unzip();
    join([Some(name), Some(state), workspace, tab.flatten(), host])
}

/// A device's header: its name, whether it is connected, and, in the Devices
/// layout, how many of its agents work.
pub(super) fn device_name(label: &str, connected: bool, working: Option<&str>) -> String {
    let working = working.map(|count| format!("{count} working"));
    join([
        Some(label),
        Some(if connected {
            "connected"
        } else {
            "disconnected"
        }),
        working.as_deref(),
    ])
}

/// `Unknown` says nothing a listener could use.
fn status_word(status: AgentStatus) -> Option<&'static str> {
    (status != AgentStatus::Unknown).then(|| super::agents::status_text(status))
}

fn join<'a>(parts: impl IntoIterator<Item = Option<&'a str>>) -> String {
    parts
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_names_its_status_and_a_remote_device() {
        assert_eq!(
            workspace_name("herdr", AgentStatus::Blocked, None),
            "herdr, blocked"
        );
        assert_eq!(
            workspace_name("herdr", AgentStatus::Unknown, Some("devbox")),
            "herdr, devbox"
        );
    }

    #[test]
    fn an_agent_names_its_state_and_place() {
        assert_eq!(
            agent_name("claude", "working", Some(("herdr", Some("tests"))), None),
            "claude, working, herdr, tests"
        );
        assert_eq!(
            agent_name("codex", "needs input", None, Some("devbox")),
            "codex, needs input, devbox"
        );
    }

    #[test]
    fn a_device_names_its_connection_and_working_agents() {
        assert_eq!(device_name("Local", true, None), "Local, connected");
        assert_eq!(
            device_name("devbox", false, Some("2/5")),
            "devbox, disconnected, 2/5 working"
        );
    }

    #[test]
    fn blank_parts_leave_no_empty_commas() {
        assert_eq!(agent_name("claude", " ", Some(("", None)), None), "claude");
    }
}
