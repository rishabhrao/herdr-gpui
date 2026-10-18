//! A section's list: its column heads and virtualized lines, headings
//! between rows of issues or pull requests, each row a click to select and a
//! double-click to open. While an item is open beside the list, rows keep
//! only the columns that fit.

use super::{
    OrchestratorView,
    look::{BLUE, CYAN, GREEN, Look, MAGENTA, RED, YELLOW, age, agent_icon},
    rows::{Heading, ItemRow, Line, Next, Tab},
};
use crate::{
    fonts::StyledFont,
    orchestrator::{Access, CheckState, PrStatus, Provider, Review, SyncState},
};
use gpui::{prelude::*, *};

/// Row heights at the usual text size; [`Look::row`] grows them with it.
/// A list's headings take its rows' height, as the list is uniform.
const ISSUE_ROW: f32 = 32.;
const PULL_REQUEST_ROW: f32 = 40.;
/// The project column, while several projects are listed.
const PROJECT_WIDTH: f32 = 130.;
/// The source column, while the list mixes backends.
const SOURCE_WIDTH: f32 = 22.;
/// The column holding a row's action, such as Start agent.
const ACTION_WIDTH: f32 = 96.;
/// Checks a row names before counting the rest.
const CHECK_PILLS: usize = 3;

impl OrchestratorView {
    /// Whether rows keep only the columns that fit beside an open item.
    fn compact(&self) -> bool {
        self.detail.is_some()
    }

    /// Whether rows of several projects are listed, so each names its own.
    fn listing_projects(&self) -> bool {
        self.repos.len() > 1 && self.projects.len() != 1
    }

    /// A row's project, in its own column while several are listed.
    fn project_cell(&self, row: &ItemRow) -> Option<Div> {
        let look = &self.look;
        self.listing_projects().then(|| {
            div()
                .flex_none()
                .w(px(PROJECT_WIDTH))
                .truncate()
                .text_size(look.small())
                .text_color(rgb(look.theme.subtext()))
                .child(self.project_label(&self.repos[row.repo]))
        })
    }

    /// A row's source as a muted mark, while the list mixes backends.
    fn source_cell(&self, provider: Provider) -> Option<Div> {
        self.mixed_sources.then(|| {
            div()
                .flex_none()
                .w(px(SOURCE_WIDTH))
                .child(source_mark(&self.look, provider))
        })
    }

    fn row_height(&self) -> f32 {
        self.look.row(match self.tab {
            Tab::Issues => ISSUE_ROW,
            Tab::PullRequests => PULL_REQUEST_ROW,
        })
    }

    pub(super) fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let look = &self.look;
        let compact = self.compact();
        let sources = self.mixed_sources;
        let projects = self.listing_projects();
        let mut head: Vec<(&str, Option<f32>)> = Vec::new();
        match self.tab {
            Tab::Issues => {
                head.push(("AGE", Some(32.)));
                if sources {
                    head.push(("SRC", Some(SOURCE_WIDTH)));
                }
                head.push(("RUN", Some(30.)));
                if projects {
                    head.push(("PROJECT", Some(PROJECT_WIDTH)));
                }
                head.push(("TITLE", None));
                if !compact {
                    head.extend([
                        ("AUTHOR", Some(110.)),
                        ("\u{1f4ac}", Some(34.)),
                        ("", Some(ACTION_WIDTH)),
                    ]);
                }
            }
            Tab::PullRequests => {
                head.push(("PR", Some(48.)));
                if sources {
                    head.push(("SRC", Some(SOURCE_WIDTH)));
                }
                if projects {
                    head.push(("PROJECT", Some(PROJECT_WIDTH)));
                }
                head.push(("TITLE \u{00b7} BRANCH", None));
                if compact {
                    head.extend([("", Some(12.)), ("", Some(12.))]);
                } else {
                    head.extend([
                        ("AUTHOR", Some(100.)),
                        ("REVIEW", Some(124.)),
                        ("CHECKS", Some(150.)),
                        ("SIZE", Some(70.)),
                        ("RUN", Some(30.)),
                    ]);
                }
                head.push(("AGE", Some(32.)));
            }
        }
        let count = self.lines.len();
        let list = if count == 0 {
            self.render_empty(cx).into_any_element()
        } else {
            uniform_list(
                "orchestrator-rows",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|index| this.render_line(index, cx)).collect()
                }),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        div()
            .debug_selector(|| "orchestrator-list".into())
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(column_head(look, &head))
            .child(list)
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let what = match self.tab {
            Tab::Issues => "open issues",
            Tab::PullRequests => "pull requests",
        };
        let loading = self.repos.iter().any(|repo| {
            let snapshot = &repo.snapshot;
            snapshot.error.is_none()
                && (snapshot.repo.is_none()
                    || snapshot
                        .sources
                        .iter()
                        .any(|status| matches!(status.state, SyncState::Syncing)))
        });
        let text = if self.repos.is_empty() {
            "No repositories: open a workspace in a Git checkout on this machine or an SSH host"
                .to_owned()
        } else if loading && self.query.trim().is_empty() {
            "Loading\u{2026}".to_owned()
        } else if self.query.trim().is_empty() {
            format!("No {what} yet")
        } else {
            format!("No {what} match \u{201c}{}\u{201d}", self.query.trim())
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(look.icon("icons/search.svg", 22., look.theme.muted))
            .child(text)
            .when(!self.query.trim().is_empty(), |el| {
                el.child(
                    look.button("orchestrator-clear", "Clear filter", false)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.search.update(cx, |search, cx| search.clear(cx));
                        })),
                )
            })
    }

    fn render_line(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.lines.get(index) {
            Some(Line::Heading { heading, count }) => self.render_heading(*heading, *count),
            Some(Line::Row(row)) => match self.tab {
                Tab::Issues => self.render_issue_row(index, *row, cx),
                Tab::PullRequests => self.render_pull_request_row(index, *row, cx),
            },
            None => div().h(px(self.row_height())).into_any_element(),
        }
    }

    fn render_heading(&self, heading: Heading, count: usize) -> AnyElement {
        let look = &self.look;
        let theme = &look.theme;
        let (label, hue): (String, u32) = match heading {
            Heading::Next(next) => match next {
                Next::Review => ("Review requested".into(), look.hue(MAGENTA)),
                Next::Fix => ("Fix".into(), look.hue(RED)),
                Next::Merge => ("Ready to merge".into(), look.hue(GREEN)),
                Next::Wait => ("Waiting".into(), theme.muted),
                Next::Closed => ("Closed and merged".into(), theme.muted),
            },
            Heading::Priority(Some(priority)) => (
                format!("P{priority} \u{00b7} {}", priority_name(priority)),
                look.hue(look.priority_hue(priority)),
            ),
            Heading::Priority(None) => ("No priority".into(), theme.muted),
        };
        div()
            .w_full()
            .h(px(self.row_height()))
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .bg(rgb(theme.surface))
            .border_b_1()
            .border_color(rgb(theme.active))
            .child(look.dot(hue, 7.))
            .child(
                div()
                    .text_size(look.small())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(look.muted(count.to_string()))
            .into_any_element()
    }

    /// A row's shell: selection, click to select, double-click to open.
    /// `item` names the row for a screen reader by its identifier, title, and
    /// state.
    fn row_shell(
        &self,
        id: (&'static str, usize),
        item: &crate::orchestrator::model::Item,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let key = item.key.canonical();
        let theme = &self.look.theme;
        let selected = self.selected.as_deref() == Some(key.as_str());
        let open = self.detail.as_ref().is_some_and(|detail| detail.key == key);
        let hover = theme.surface;
        div()
            .id(id)
            .role(Role::ListItem)
            .aria_label(format!(
                "{} {}, {}",
                item.identifier, item.title, item.state
            ))
            .aria_selected(selected)
            .w_full()
            .h(px(self.row_height()))
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .border_b_1()
            .border_color(rgb(theme.active))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(hover)))
            .when(selected || open, |el| el.bg(rgb(theme.primary_wash())))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.selected = Some(key.clone());
                    this.activate(&key);
                    if event.click_count == 2 {
                        this.open_detail(key.clone(), cx);
                    }
                    cx.notify();
                }),
            )
    }

    fn render_issue_row(&self, index: usize, row: ItemRow, cx: &mut Context<Self>) -> AnyElement {
        let look = &self.look;
        let theme = &look.theme;
        let compact = self.compact();
        let item = self.row_item(&row);
        let key = item.key.canonical();
        let now = chrono::Utc::now();
        let expander = row.children.map(|open| {
            let key = key.clone();
            div()
                .id(("orchestrator-expand", index))
                .child(look.icon(
                    if open {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    },
                    10.,
                    theme.muted,
                ))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_children(key.clone(), cx);
                    }),
                )
        });
        let writable = self.repos[row.repo].snapshot.access == Some(Access::ReadWrite);
        let blocked = !item.blocked_by.is_empty() || item.state == "blocked";
        let start = (!compact && writable && row.run.is_none() && !blocked).then(|| {
            let key = key.clone();
            compact_button(look, ("orchestrator-start", index), "Start agent")
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_dispatch(key.clone(), window, cx);
                }))
        });
        self.row_shell(("orchestrator-issue", index), item, cx)
            .debug_selector(move || format!("orchestrator-row-{key}"))
            .child(
                div()
                    .flex_none()
                    .w(px(32.))
                    .text_size(look.small())
                    .text_color(rgb(theme.muted))
                    .child(age(item.updated_at, now)),
            )
            .children(self.source_cell(item.key.source.provider))
            .child(self.run_mark(&row))
            .children(self.project_cell(&row))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(px(f32::from(row.depth) * 18.))
                    .child(div().flex_none().w(px(10.)).children(expander))
                    .child(
                        look.mono(item.identifier.clone())
                            .flex_none()
                            .text_color(rgb(theme.muted)),
                    )
                    .child(div().min_w_0().truncate().child(item.title.clone()))
                    .when(!compact, |el| {
                        el.children(
                            item.labels
                                .iter()
                                .take(2)
                                .map(|label| look.chip(label.clone())),
                        )
                    })
                    .when(blocked, |el| {
                        el.child(look.icon("icons/lock.svg", 11., look.hue(YELLOW)))
                    }),
            )
            .when(!compact, |el| {
                el.child(
                    div()
                        .flex_none()
                        .w(px(110.))
                        .truncate()
                        .text_size(look.small())
                        .text_color(rgb(theme.subtext()))
                        .child(item.author.clone().unwrap_or_default()),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(34.))
                        .text_size(look.small())
                        .text_color(rgb(theme.muted))
                        .child(
                            item.activity
                                .and_then(|activity| activity.comments)
                                .filter(|count| *count > 0)
                                .map(|count| count.to_string())
                                .unwrap_or_default(),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(ACTION_WIDTH))
                        .flex()
                        .justify_end()
                        .children(start)
                        .when(blocked, |el| el.child(look.badge("blocked", YELLOW))),
                )
            })
            .into_any_element()
    }

    fn render_pull_request_row(
        &self,
        index: usize,
        row: ItemRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let look = &self.look;
        let theme = &look.theme;
        let compact = self.compact();
        let item = self.row_item(&row);
        let Some(pr) = &item.pull_request else {
            return div().h(px(self.row_height())).into_any_element();
        };
        let status = super::rows::status_of(item, &self.repos[row.repo].snapshot.pr_status);
        let fork = pr
            .head_repository
            .as_deref()
            .is_some_and(|head| head != item.key.source.repository);
        let draft = item.state == "draft";
        let key = item.key.canonical();
        self.row_shell(("orchestrator-pr", index), item, cx)
            .debug_selector(move || format!("orchestrator-row-{key}"))
            .child(
                div()
                    .flex_none()
                    .w(px(48.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(look.icon(
                        "icons/pull-request.svg",
                        12.,
                        if draft { theme.muted } else { look.hue(GREEN) },
                    ))
                    .child(
                        look.mono(item.identifier.clone())
                            .text_color(rgb(theme.muted)),
                    ),
            )
            .children(self.source_cell(item.key.source.provider))
            .children(self.project_cell(&row))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().min_w_0().truncate().child(item.title.clone()))
                            .when(draft, |el| el.child(look.badge("draft", BLUE))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(look.tiny())
                            .text_color(rgb(theme.muted))
                            .child(look.icon("icons/git-branch.svg", 10., theme.muted))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_font(&look.mono)
                                    .child(pr.head_ref.clone()),
                            )
                            .when(fork, |el| el.child(look.badge("fork", CYAN))),
                    ),
            )
            .map(|el| {
                if compact {
                    el.child(
                        div()
                            .flex_none()
                            .w(px(12.))
                            .children(review_dot(look, status)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(12.))
                            .children(checks_dot(look, status)),
                    )
                } else {
                    el.child(
                        div()
                            .flex_none()
                            .w(px(100.))
                            .truncate()
                            .text_size(look.small())
                            .text_color(rgb(theme.subtext()))
                            .child(item.author.clone().unwrap_or_default()),
                    )
                    .child(div().flex_none().w(px(124.)).flex().children(review_badge(
                        look,
                        status,
                        self.repos[row.repo].reading_login(),
                    )))
                    .child(
                        div()
                            .flex_none()
                            .w(px(150.))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .children(check_pills(look, status)),
                    )
                    .child(diff_squares(look, pr.additions, pr.deletions).w(px(70.)))
                    .child(self.run_mark(&row))
                }
            })
            .child(
                div()
                    .flex_none()
                    .w(px(32.))
                    .text_size(look.small())
                    .text_color(rgb(theme.muted))
                    .child(age(item.updated_at, chrono::Utc::now())),
            )
            .into_any_element()
    }

    fn run_mark(&self, row: &ItemRow) -> Div {
        let look = &self.look;
        div()
            .flex_none()
            .w(px(30.))
            .flex()
            .items_center()
            .gap_1()
            .when_some(row.run, |el, run| {
                let agent = &self.repos[row.repo].snapshot.runs[run.run].agent;
                el.child(look.icon(agent_icon(agent), 13., look.theme.foreground))
                    .child(look.dot(look.status_hue(run.status), 7.))
            })
    }
}

/// A backend's muted mark in a row.
pub(super) fn source_mark(look: &Look, provider: Provider) -> AnyElement {
    let icon = match provider {
        Provider::Github => "icons/github.svg",
        Provider::Beads => "icons/beads.svg",
        Provider::Gitlab => "icons/git-branch.svg",
    };
    look.icon(icon, 12., look.theme.muted).into_any_element()
}

/// A button small enough to sit in a row with room above and below.
fn compact_button(look: &Look, id: impl Into<ElementId>, label: &'static str) -> Stateful<Div> {
    let theme = &look.theme;
    let hover = theme.active;
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .flex_none()
        .px_2()
        .py(px(1.))
        .rounded(px(crate::config::corners::SMALL))
        .border_1()
        .border_color(rgb(theme.active))
        .bg(rgb(theme.surface))
        .hover(move |style| style.bg(rgb(hover)))
        .cursor_pointer()
        .text_size(look.tiny())
        .whitespace_nowrap()
        .child(label)
}

fn column_head(look: &Look, cells: &[(&str, Option<f32>)]) -> Div {
    let theme = &look.theme;
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .h(px(24.))
        .px_4()
        .border_b_1()
        .border_color(rgb(theme.active))
        .text_size(look.tiny())
        .text_color(rgb(theme.muted))
        .children(cells.iter().map(|(text, width)| {
            match width {
                Some(width) => div()
                    .flex_none()
                    .w(px(*width))
                    .whitespace_nowrap()
                    .child(SharedString::from(text.to_string())),
                None => div().flex_1().child(SharedString::from(text.to_string())),
            }
        }))
}

/// Five squares, green for added lines and red for removed, in proportion.
pub(super) fn diff_squares(look: &Look, additions: Option<u64>, deletions: Option<u64>) -> Div {
    let (added, removed) = (additions.unwrap_or(0), deletions.unwrap_or(0));
    let total = added + removed;
    let green = (added * 5 + total / 2)
        .checked_div(total)
        .map_or(0, |green| green.min(5));
    let known = additions.is_some() || deletions.is_some();
    div()
        .flex_none()
        .w(px(44.))
        .flex()
        .items_center()
        .gap(px(1.))
        .children((0..5).map(move |index| {
            let color = if !known {
                look.theme.active
            } else if index < green {
                look.hue(GREEN)
            } else {
                look.hue(RED)
            };
            div().size(px(7.)).rounded(px(1.)).bg(rgb(color))
        }))
}

fn priority_name(priority: i64) -> &'static str {
    match priority {
        0 => "urgent",
        1 => "high",
        2 => "normal",
        3 => "low",
        _ => "backlog",
    }
}

fn check_hue(look: &Look, state: CheckState) -> u32 {
    match state {
        CheckState::Failed => look.hue(RED),
        CheckState::Pending => look.hue(YELLOW),
        CheckState::Passed => look.hue(GREEN),
        CheckState::Skipped => look.theme.muted,
    }
}

/// The review state's word, or that the user is asked to review.
fn review_badge(look: &Look, status: Option<&PrStatus>, login: Option<&str>) -> Option<Div> {
    let status = status?;
    if login.is_some_and(|login| status.requests(login)) {
        return Some(look.badge("your review", MAGENTA));
    }
    Some(match status.review? {
        Review::Approved => look.badge("approved", GREEN),
        Review::ChangesRequested => look.badge("changes requested", RED),
        Review::Required => look.badge("review required", BLUE),
    })
}

fn review_dot(look: &Look, status: Option<&PrStatus>) -> Option<Div> {
    let hue = match status?.review? {
        Review::Approved => GREEN,
        Review::ChangesRequested => RED,
        Review::Required => BLUE,
    };
    Some(look.dot(look.hue(hue), 7.))
}

fn checks_dot(look: &Look, status: Option<&PrStatus>) -> Option<Div> {
    let state = status?.checks_state()?;
    Some(look.dot(check_hue(look, state), 7.))
}

/// The first checks as short colored pills, failures first, then a count of
/// the rest.
fn check_pills(look: &Look, status: Option<&PrStatus>) -> Vec<Div> {
    let Some(status) = status else {
        return Vec::new();
    };
    let mut checks: Vec<_> = status.checks.iter().collect();
    checks.sort_by_key(|check| check.state);
    let mut pills: Vec<Div> = checks
        .iter()
        .take(CHECK_PILLS)
        .map(|check| {
            let hue = check_hue(look, check.state);
            let short: String = check.name.chars().take(4).collect();
            div()
                .flex_none()
                .px(px(4.))
                .rounded(px(crate::config::corners::SMALL))
                .bg(rgb(crate::config::mix(look.theme.background, hue, 18)))
                .text_color(rgb(hue))
                .text_size(look.tiny())
                .child(short)
        })
        .collect();
    if checks.len() > CHECK_PILLS {
        pills.push(look.muted(format!("+{}", checks.len() - CHECK_PILLS)));
    }
    pills
}
