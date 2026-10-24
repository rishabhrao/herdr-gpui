//! An alert's tabs: Overview (the fix first, then the facts and what
//! happened), Advisory (scores, weaknesses, identifiers, references), and
//! Dependency path (what GitHub says about how the package is pulled in).

use super::{
    Read, arrange, body, callout, date, facts_list, heading, link, mono, plain, severity_text,
};
use crate::{
    HerdrWindow,
    orchestrator::Look,
    security::{
        Alert, Item, Repo, Tab,
        actions::{manifest_path, upgrade_command},
        page::{AlertPage, Relationship, Scope},
    },
    window::Flash,
};
use gpui::{prelude::*, *};

impl HerdrWindow {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn security_alert_body(
        &self,
        look: &Look,
        tab: Tab,
        repo: &Repo,
        alert: &Alert,
        page: Read<'_, AlertPage>,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let item = Item::Alert {
            repo: repo.name.clone(),
            number: alert.number,
        };
        match tab {
            Tab::Advisory => advisory(look, alert, page, cx),
            Tab::Dependency => self.dependency(look, &item, repo, alert, page, cx),
            _ => {
                let lead = self.upgrade(look, &item, repo, alert, cx);
                let text = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(heading("What happened"))
                    .child(body(
                        look,
                        "security-alert-description",
                        page.get().map(|page| page.description.as_slice()),
                        page.waiting(look),
                    ));
                arrange(look, wide, Some(lead), facts(look, alert, page.get()), text)
            }
        }
    }

    /// The callout: the version to move to, how, and the actions.
    fn upgrade(
        &self,
        look: &Look,
        item: &Item,
        repo: &Repo,
        alert: &Alert,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &look.theme;
        let (hue, mark, title) = match &alert.patched {
            Some(patched) => (
                look.hue(2),
                "icons/check.svg",
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(heading("Upgrade to fix"))
                    .child(
                        look.mono(alert.vulnerable.clone())
                            .text_color(rgb(theme.muted)),
                    )
                    .child(look.icon("icons/arrow-right.svg", 11., theme.muted))
                    .child(look.mono(patched.clone()).text_color(rgb(look.hue(2)))),
            ),
            None => (
                look.hue(3),
                "icons/error.svg",
                heading("No fixed version yet"),
            ),
        };
        let command = upgrade_command(alert).map(|command| {
            let copied = command.clone();
            div()
                .id("security-upgrade-command")
                .debug_selector(|| "security-upgrade-command".into())
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(crate::config::corners::CONTROL))
                .bg(rgb(theme.surface))
                .cursor_pointer()
                .child(look.mono(command).flex_1().min_w_0().truncate())
                .child(look.icon("icons/copy.svg", 12., theme.muted))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                    this.show_flash(Flash::success("Copied the command"), cx);
                }))
        });
        let explain = match alert.patched {
            Some(_) => format!("{} pulls in an affected version.", alert.manifest),
            None => {
                "GitHub knows no fixed version: remove the dependency or mitigate it.".to_owned()
            }
        };
        let fix = item.clone();
        let open = item.clone();
        let url = repo.alert_url(alert);
        let actions = div()
            .child(
                look.button("security-alert-agent", "Fix with agent", true)
                    .debug_selector(|| "security-alert-agent".into())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.security_agent(&fix, window, cx);
                    })),
            )
            .when(manifest_path(repo, alert).is_some(), |el| {
                el.child(
                    look.button(
                        "security-alert-manifest",
                        format!("Open {}", file_name(&alert.manifest)),
                        false,
                    )
                    .debug_selector(|| "security-alert-manifest".into())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_security_manifest(&open, window, cx);
                    })),
                )
            })
            .child(div().flex_1())
            .child(
                look.button("security-alert-dismiss", "Dismiss on GitHub", false)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_security_page(&url, window, cx);
                    })),
            );
        callout(
            look,
            hue,
            mark,
            title,
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(look.muted(explain))
                .children(command),
            actions,
        )
    }

    fn dependency(
        &self,
        look: &Look,
        item: &Item,
        repo: &Repo,
        alert: &Alert,
        page: Read<'_, AlertPage>,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(page) = page.get() else {
            return page.waiting(look);
        };
        let relationship = match page.relationship {
            Relationship::Direct => "Direct: the manifest names it",
            Relationship::Transitive => "Transitive: another dependency pulls it in",
            Relationship::Unknown => "GitHub could not tell",
        };
        let scope = match page.scope {
            Scope::Runtime => "Runtime: it ships with the project",
            Scope::Development => "Development: it only builds or tests the project",
            Scope::Unknown => "GitHub could not tell",
        };
        let mut rows = vec![
            (
                "Package",
                mono(look, format!("{} ({})", alert.package, alert.ecosystem)),
            ),
            ("Relationship", plain(relationship)),
            ("Scope", plain(scope)),
            ("Manifest", mono(look, alert.manifest.clone())),
        ];
        if let Some(requirement) = &page.requirement {
            rows.push(("Requirement", mono(look, requirement.clone())));
        }
        rows.push(("Affected versions", mono(look, alert.vulnerable.clone())));
        let mut column = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(facts_list(look, rows));
        if let Some(number) = page.update {
            let url = format!("https://github.com/{}/pull/{number}", repo.name);
            column = column.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(heading("Dependabot's fix"))
                    .child(link(look, "security-alert-update", &url, cx)),
            );
        }
        let open = item.clone();
        if page.relationship == Relationship::Transitive {
            column = column.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(look.muted(
                        "GitHub does not say which dependency pulls it in; the lockfile does.",
                    ))
                    .when(manifest_path(repo, alert).is_some(), |el| {
                        el.child(
                            look.button("security-dependency-manifest", "Open the lockfile", false)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_security_manifest(&open, window, cx);
                                })),
                        )
                    }),
            );
        }
        column
    }
}

fn facts(look: &Look, alert: &Alert, page: Option<&AlertPage>) -> Vec<(&'static str, AnyElement)> {
    let cvss = page
        .and_then(|page| page.cvss.as_ref())
        .map(|cvss| cvss.score);
    let mut rows = vec![("Severity", severity_text(look, alert.severity, cvss))];
    rows.push(("Affected versions", mono(look, alert.vulnerable.clone())));
    if let Some(requirement) = page.and_then(|page| page.requirement.clone()) {
        rows.push(("Installed", mono(look, requirement)));
    }
    rows.push(("Manifest", mono(look, alert.manifest.clone())));
    let mut identifiers = vec![alert.advisory.clone()];
    if let Some(page) = page {
        identifiers.extend(
            page.identifiers
                .iter()
                .filter(|id| **id != alert.advisory)
                .cloned(),
        );
    }
    rows.push((
        "Identifiers",
        div()
            .flex()
            .flex_col()
            .children(identifiers.into_iter().map(|id| look.mono(id)))
            .into_any_element(),
    ));
    if let Some(weakness) = page.and_then(|page| page.weaknesses.first()) {
        rows.push((
            "Weakness",
            plain(format!("{} {}", weakness.id, weakness.name)),
        ));
    }
    if let Some(at) = alert.created_at {
        rows.push(("Opened", plain(date(at))));
    }
    rows
}

fn advisory(
    look: &Look,
    alert: &Alert,
    page: Read<'_, AlertPage>,
    cx: &mut Context<HerdrWindow>,
) -> Div {
    let Some(page) = page.get() else {
        return page.waiting(look);
    };
    let mut rows = Vec::new();
    match &page.cvss {
        Some(cvss) => {
            rows.push((
                "Score",
                severity_text(look, alert.severity, Some(cvss.score)),
            ));
            rows.push((
                "Vector",
                mono(look, format!("CVSS {} {}", cvss.version, cvss.vector)),
            ));
        }
        None => rows.push(("Score", severity_text(look, alert.severity, None))),
    }
    if let Some(at) = page.published_at {
        rows.push(("Published", plain(date(at))));
    }
    for weakness in &page.weaknesses {
        rows.push((
            "Weakness",
            plain(format!("{} {}", weakness.id, weakness.name)),
        ));
    }
    let mut column = div()
        .flex()
        .flex_col()
        .gap_4()
        .child(facts_list(look, rows));
    if !page.references.is_empty() {
        column = column.child(
            div()
                .debug_selector(|| "security-references".into())
                .flex()
                .flex_col()
                .gap_1()
                .child(heading("References"))
                .children(
                    page.references
                        .iter()
                        .enumerate()
                        .map(|(index, url)| link(look, ("security-reference", index), url, cx)),
                ),
        );
    }
    column
}

/// `Cargo.lock` of `crates/app/Cargo.lock`.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
