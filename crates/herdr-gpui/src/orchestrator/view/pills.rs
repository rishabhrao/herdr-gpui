//! The pills under a section's search: one per backend and one per project
//! listed, each with how many of the shown list's items it holds. Picking
//! some narrows the list to them; none picked lists everything. A row of
//! pills shows only when there is more than one to choose between.

use super::{
    OrchestratorView,
    inbox::{provider_mark, provider_name},
    rows::Tab,
};
use crate::{config::corners, orchestrator::Provider};
use gpui::{prelude::*, *};

impl OrchestratorView {
    pub(super) fn render_pills(&self, cx: &mut Context<Self>) -> Option<Div> {
        let look = &self.look;
        let theme = &look.theme;
        let tab = self.tab;
        fn rows(repo: &super::Repo, tab: Tab) -> &[super::rows::ItemRow] {
            match tab {
                Tab::Issues => &repo.issue_rows,
                Tab::PullRequests => &repo.pull_request_rows,
            }
        }
        let mut backends: Vec<(Provider, usize)> = Vec::new();
        for repo in &self.repos {
            for row in rows(repo, tab).iter().filter(|row| row.depth == 0) {
                let provider = repo.snapshot.items[row.item].key.source.provider;
                match backends.iter_mut().find(|(found, _)| *found == provider) {
                    Some((_, count)) => *count += 1,
                    None => backends.push((provider, 1)),
                }
            }
        }
        backends.sort_by_key(|(provider, _)| provider_name(*provider));
        let projects: Vec<(String, String, usize)> = self
            .repos
            .iter()
            .map(|repo| {
                let count = rows(repo, tab).iter().filter(|row| row.depth == 0).count();
                (repo.key.clone(), self.project_label(repo), count)
            })
            .collect();
        if backends.len() < 2 && projects.len() < 2 {
            return None;
        }
        let pill = |id: SharedString, on: bool| {
            let hover = theme.active;
            div()
                .id(id.clone())
                .debug_selector(move || id.to_string())
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py(px(2.))
                .rounded_full()
                .border_1()
                .border_color(rgb(if on { theme.primary() } else { theme.active }))
                .when(on, |el| el.bg(rgb(theme.primary_wash())))
                .when(!on, |el| el.hover(move |style| style.bg(rgb(hover))))
                .cursor_pointer()
                .text_size(look.small())
        };
        let count = |count: usize| div().text_color(rgb(theme.muted)).child(count.to_string());
        let mut row = div()
            .debug_selector(|| "orchestrator-pills".into())
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_4()
            .pb_2();
        if backends.len() > 1 {
            for (provider, items) in backends {
                let on = self.backends.contains(&provider);
                row = row.child(
                    pill(
                        format!("orchestrator-pill-backend-{}", provider_name(provider)).into(),
                        on,
                    )
                    .child(provider_mark(look, provider))
                    .child(provider_name(provider))
                    .child(count(items))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.backends.remove(&provider) {
                            this.backends.insert(provider);
                        }
                        this.refresh_rows();
                        cx.notify();
                    })),
                );
            }
            if projects.len() > 1 {
                row = row.child(div().w(px(1.)).h(px(16.)).bg(rgb(theme.active)));
            }
        }
        if projects.len() > 1 {
            for (index, (key, label, items)) in projects.into_iter().enumerate() {
                let on = self.projects.contains(&key);
                row = row.child(
                    pill(format!("orchestrator-pill-project-{index}").into(), on)
                        .rounded(px(corners::CONTROL))
                        .child(look.icon("icons/git-branch.svg", 11., theme.subtext()))
                        .child(label)
                        .child(count(items))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.projects.remove(&key) {
                                this.projects.insert(key.clone());
                            }
                            this.refresh_rows();
                            cx.notify();
                        })),
                );
            }
        }
        Some(row)
    }

    /// A project's name, with its host's when the same name is listed from
    /// another host too.
    pub(super) fn project_label(&self, repo: &super::Repo) -> String {
        let name = repo.name();
        let shared = self
            .repos
            .iter()
            .filter(|other| other.name() == name)
            .count()
            > 1;
        if shared {
            format!("{name} \u{00b7} {}", repo.host)
        } else {
            name
        }
    }
}
