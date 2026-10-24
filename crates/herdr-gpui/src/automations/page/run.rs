//! A run's page: how it went, step by step with each step's output tail,
//! the agent panes it opened, what it used, and its whole log on request.
//! The steps come from `herdr-automate runs show`, read when the page
//! opens and again with every status read while the run goes on.

use super::super::{Action, HostState, Loaded, LogKey, Opened, format, model};
use crate::HerdrWindow;
use gpui::{prelude::*, *};
use herdr_automation::report::{RunDetail, RunState, RunSummary, ScriptStatus, StepRecord, Usage};

/// Lines of a step's tail shown on the page; Full log shows the rest.
const TAIL_LINES: usize = 12;
/// Lines of a full log drawn; the log itself is bounded by the daemon.
const LOG_LINES: usize = 2000;

impl HerdrWindow {
    pub(super) fn automation_run_page(
        &self,
        host: &HostState,
        script: &ScriptStatus,
        run: i64,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let opened = Opened::Run {
            machine: host.machine.clone(),
            path: script.path.clone(),
            run,
        };
        let loaded = self.automations.runs.get(&(host.machine.clone(), run));
        let detail = match loaded {
            Some(Loaded::Read(detail)) => Some(detail.as_ref()),
            _ => None,
        };
        let summary = detail.map(|detail| &detail.run).or_else(|| {
            host.status()
                .and_then(|status| model::find_run(status, run))
        });
        let mut page = div()
            .debug_selector(|| "automations-run-page".into())
            .flex()
            .flex_col()
            .gap_3()
            .child(self.automation_crumbs(&opened, Some((host, script)), cx))
            .child(self.automation_run_heading(host, script, run, summary, cx));
        if let Some(summary) = summary {
            page = page.child(
                div()
                    .text_size(self.automation_small())
                    .text_color(rgb(theme.subtext()))
                    .child(run_line(summary, format::now())),
            );
            if let Some(why) = summary
                .detail
                .as_ref()
                .filter(|_| model::failed(summary.state))
            {
                page = page.child(
                    div()
                        .text_color(rgb(model::run_look(summary.state).color(theme)))
                        .child(why.clone()),
                );
            }
        }
        page = match loaded {
            Some(Loaded::Read(detail)) => self.automation_steps(host, detail, page, cx),
            Some(Loaded::Reading { .. }) | None => {
                page.child(div().text_color(rgb(theme.muted)).child("Reading the run…"))
            }
            Some(Loaded::Failed(error)) => page.child(
                div()
                    .debug_selector(|| "automations-run-failed".into())
                    .text_color(rgb(theme.ink(theme.palette[3])))
                    .child(error.to_string()),
            ),
        };
        let key = LogKey {
            machine: host.machine.clone(),
            run,
            step: None,
        };
        page.children(self.automation_full_log(&key, "automations-run-log"))
    }

    fn automation_run_heading(
        &self,
        host: &HostState,
        script: &ScriptStatus,
        run: i64,
        summary: Option<&RunSummary>,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let look = summary.map_or(model::Look::Paused, |run| model::run_look(run.state));
        let running = summary.is_some_and(|run| run.state == RunState::Running);
        let key = LogKey {
            machine: host.machine.clone(),
            run,
            step: None,
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(self.automation_icon(look, 16.))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(self.config.ui.size + 2.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{} #{run}", script.name)),
            )
            .child(self.automation_chip(host.label.clone(), theme.subtext()))
            .child(div().flex_1())
            .child(
                self.automation_button(
                    "automations-full-log",
                    "icons/terminal.svg",
                    if self.automations.logs.contains_key(&key) {
                        "Hide log"
                    } else {
                        "Full log"
                    },
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_automation_log(key.clone(), cx);
                })),
            )
            .when(running, |heading| {
                heading.child(self.automation_action_button(
                    "automations-stop-run",
                    "icons/x.svg",
                    "Stop run",
                    host,
                    Action::Stop { run },
                    cx,
                ))
            })
    }

    fn automation_steps(
        &self,
        host: &HostState,
        detail: &RunDetail,
        mut page: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let now = format::now();
        if detail.steps.is_empty() {
            page = page.child(
                div()
                    .text_color(rgb(theme.muted))
                    .child("No step has started."),
            );
        }
        for step in &detail.steps {
            page = page.child(self.automation_step(host, detail.run.id, step, now, cx));
        }
        let usage = total_usage(&detail.steps);
        let stat = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .child(div().text_color(rgb(theme.muted)).child(label))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(value))
        };
        let unknown = || "unknown".to_owned();
        page.child(div().h(px(8.)))
            .child(self.automation_title("Usage"))
            .child(
                div()
                    .debug_selector(|| "automations-usage".into())
                    .flex()
                    .flex_wrap()
                    .gap_5()
                    .text_size(self.automation_small())
                    .child(stat(
                        "Tokens",
                        usage
                            .tokens
                            .map_or_else(unknown, |tokens| format::tokens(tokens.total())),
                    ))
                    .child(stat("CPU", usage.cpu_ms.map_or_else(unknown, format::cpu)))
                    .child(stat(
                        "Peak memory",
                        usage.peak_memory.map_or_else(unknown, format::bytes),
                    ))
                    .child(stat("Agents", detail.run.agents.to_string())),
            )
    }

    fn automation_step(
        &self,
        host: &HostState,
        run: i64,
        step: &StepRecord,
        now: i64,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let small = self.automation_small();
        let took = format::span(step.finished_at.unwrap_or(now) - step.started_at);
        let exit = step
            .exit
            .filter(|code| *code != 0)
            .map(|code| format!(" · exit {code}"))
            .unwrap_or_default();
        let key = LogKey {
            machine: host.machine.clone(),
            run,
            step: Some(step.position),
        };
        let selector = format!("automations-step-{}", step.position);
        let pane = step
            .pane
            .as_ref()
            .and_then(|pane| self.automation_pane(host, pane));
        let mut block = div()
            .debug_selector(move || selector.clone())
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .child(self.automation_icon(model::step_look(step.state), 14.))
                    .child(
                        div()
                            .w(px(160.))
                            .flex_none()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("{}. {}", step.position, step.kind)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(small)
                            .text_color(rgb(theme.subtext()))
                            .child(format!("{} · {took}{exit}", step.summary)),
                    )
                    .children(pane.clone().map(|(endpoint, pane)| {
                        self.automation_button(
                            format!("automations-pane-{}", step.position),
                            "icons/terminal.svg",
                            "Open pane",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_automation_pane(&endpoint, &pane, window, cx);
                            },
                        ))
                    }))
                    .child(
                        self.automation_button(
                            format!("automations-step-log-{}", step.position),
                            "icons/terminal.svg",
                            if self.automations.logs.contains_key(&key) {
                                "Hide log"
                            } else {
                                "Full log"
                            },
                        )
                        .on_click(cx.listener({
                            let key = key.clone();
                            move |this, _, _, cx| this.toggle_automation_log(key.clone(), cx)
                        })),
                    ),
            );
        if let Some(worktree) = step.worktree.as_ref().filter(|_| pane.is_none()) {
            block = block.child(
                div()
                    .ml(px(26.))
                    .text_size(small)
                    .text_color(rgb(theme.muted))
                    .child(format!("Worktree {worktree}")),
            );
        }
        let full = self.automation_full_log(&key, "automations-step-full-log");
        if full.is_none() && !step.log_tail.trim().is_empty() {
            block = block.child(div().ml(px(26.)).child(self.automation_output(
                &step.log_tail,
                TAIL_LINES,
                format!("automations-step-tail-{}", step.position),
            )));
        }
        block.children(full.map(|log| div().ml(px(26.)).child(log)))
    }

    /// A full log asked for on the page, as far as it is read.
    fn automation_full_log(&self, key: &LogKey, selector: &'static str) -> Option<Div> {
        let theme = &self.theme;
        Some(match self.automations.logs.get(key)? {
            Loaded::Reading { .. } => div().text_color(rgb(theme.muted)).child("Reading the log…"),
            Loaded::Failed(error) => div()
                .text_color(rgb(theme.ink(theme.palette[3])))
                .child(error.to_string()),
            Loaded::Read(log) => div()
                .flex()
                .flex_col()
                .gap_1()
                .when(log.truncated, |block| {
                    block.child(
                        div()
                            .text_size(self.automation_small())
                            .text_color(rgb(theme.muted))
                            .child("The end of a longer log; the host keeps the rest."),
                    )
                })
                .child(self.automation_output(&log.text, LOG_LINES, selector.into())),
        })
    }

    /// The pane an agent step runs in, when this window shows it: the
    /// endpoint that reaches the step's machine has a pane by that ID.
    fn automation_pane(&self, host: &HostState, pane: &str) -> Option<(String, String)> {
        self.endpoints
            .iter()
            .enumerate()
            .find_map(|(index, endpoint)| {
                if super::super::Machine::of(&endpoint.connection.target).as_ref()
                    != Some(&host.machine)
                {
                    return None;
                }
                let snapshot = self.endpoint_snapshot(index)?;
                snapshot
                    .panes
                    .iter()
                    .any(|known| known.pane_id == pane)
                    .then(|| (endpoint.id.clone(), pane.to_owned()))
            })
    }

    /// Shows the agent's pane among the terminals; Back returns here.
    fn open_automation_pane(
        &mut self,
        endpoint: &str,
        pane: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::sidebar::ActivityPanel;
        self.show_activity_panel(ActivityPanel::Workspaces, window, cx);
        self.activity.return_to = Some(ActivityPanel::Automations);
        self.navigate_endpoint(endpoint, crate::NavigationTarget::Pane(pane), cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

/// When the run started and why, and how long it took.
fn run_line(run: &RunSummary, now: i64) -> String {
    use herdr_automation::report::Reason;
    let why = match run.reason {
        Reason::Schedule => "on schedule",
        Reason::Late => "late, after the daemon was down",
        Reason::Manual => "by hand",
    };
    let trigger = run
        .trigger_name
        .as_ref()
        .map(|name| format!(" · trigger {name}"))
        .unwrap_or_default();
    let took = match run.finished_at {
        Some(end) => format!(" · took {}", format::span(end - run.started_at)),
        None => format!(" · running for {}", format::span(now - run.started_at)),
    };
    format!(
        "Started {} {why}{trigger}{took}",
        format::when(run.started_at, now)
    )
}

/// What the run's steps used together: tokens and CPU summed, memory at its
/// peak. A measure no step took stays unknown rather than zero.
pub(crate) fn total_usage(steps: &[StepRecord]) -> Usage {
    steps.iter().fold(Usage::default(), |total, step| Usage {
        tokens: match (total.tokens, step.usage.tokens) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        },
        cpu_ms: match (total.cpu_ms, step.usage.cpu_ms) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            (a, b) => a.or(b),
        },
        peak_memory: total.peak_memory.max(step.usage.peak_memory),
    })
}
