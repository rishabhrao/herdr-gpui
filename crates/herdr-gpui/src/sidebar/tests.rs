#![allow(clippy::unwrap_used)]

use super::{
    STATUS_DOT_UNKNOWN, STATUS_WIDTH, agent_name,
    agents::{Indicators, agent_labels, agent_place, status_indicator, status_style},
    layout_tests,
    render::header,
    workspace_label,
    workspaces::workspace_entries,
};
use crate::{
    config::{FontConfig, StatusIcons, Theme},
    contrast::Contrast,
};
use herdr_client::protocol::{
    AgentStatus, ClientShellAgent, ClientShellSnapshot, ClientShellWorkspace,
};

/// Every status, most urgent first.
const STATUSES: [AgentStatus; 5] = [
    AgentStatus::Working,
    AgentStatus::Blocked,
    AgentStatus::Done,
    AgentStatus::Idle,
    AgentStatus::Unknown,
];

mod agent_order;
mod hierarchy;
mod host_mark;
mod row_text;
mod statuses;
mod sticky_hosts;
mod working_marks;
