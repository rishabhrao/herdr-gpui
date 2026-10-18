//! The section list and the footer: grouped sections under a search box, and
//! the save status line.
use super::*;

impl SettingsWindow {
    pub(super) fn navigation(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        use crate::panel_resize::{PanelDrag, Side, handle};
        let theme = &self.theme;
        div()
            .id("settings-navigation")
            .role(Role::Navigation)
            .aria_label("Settings sections")
            .relative()
            .map(|navigation| {
                // Narrow, the list is the whole window until a page opens.
                if self.narrow() {
                    navigation.flex_1()
                } else {
                    navigation
                        .w(px(self.navigation_width.width(self.viewport_width)))
                        .flex_none()
                }
            })
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<PanelDrag>, _, cx| {
                    if *event.drag(cx) == PanelDrag::SettingsNavigation
                        && this.navigation_width.drag(
                            Side::Left,
                            event.bounds,
                            event.event.position.x,
                        )
                    {
                        cx.notify();
                    }
                }),
            )
            .child(handle(
                "settings-navigation-resize",
                Side::Left,
                PanelDrag::SettingsNavigation,
                cx.listener(|this, _, _, cx| {
                    this.navigation_width.reset();
                    cx.notify();
                }),
            ))
            .h_full()
            .flex()
            .flex_col()
            .p(px(12.))
            .gap(px(2.))
            .bg(rgb(mix(theme.surface, theme.background, 25)))
            .border_r_1()
            .border_color(rgb(theme.active))
            .child(
                div()
                    .debug_selector(|| "settings-nav-search".into())
                    .mb(px(8.))
                    .child(self.nav_search.clone()),
            )
            .child(
                self.scroll_area(
                    scrollbar::Pane::Sections,
                    div()
                        .id("settings-sections")
                        .debug_selector(|| "settings-sections".into())
                        // Picking a section swaps the page beside it, as a tab does.
                        .role(Role::TabList)
                        .aria_label("Sections")
                        .aria_orientation(Orientation::Vertical)
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .overflow_y_scroll()
                        .track_scroll(&self.navigation_scroll)
                        // Flat, so `Section::nav_item` can scroll to a page.
                        .children(std::iter::once(None).chain(Group::ALL.map(Some)).flat_map(
                            |group| {
                                group
                                    .map(|group| self.group_label(group).into_any_element())
                                    .into_iter()
                                    .chain(
                                        Section::ALL
                                            .iter()
                                            .copied()
                                            .enumerate()
                                            .filter(move |(_, section)| section.group() == group)
                                            .map(|(index, section)| {
                                                self.navigation_row(index, section, cx)
                                                    .into_any_element()
                                            })
                                            .collect::<Vec<_>>(),
                                    )
                            },
                        )),
                    cx,
                ),
            )
    }

    fn group_label(&self, group: Group) -> Div {
        div()
            .debug_selector(move || format!("settings-group-{}", group.label().to_lowercase()))
            .px(px(10.))
            .pt(px(10.))
            .pb(px(4.))
            .text_size(px(10.))
            .text_color(rgb(self.theme.subtext()))
            .child(group.label().to_uppercase())
    }

    fn navigation_row(
        &self,
        index: usize,
        section: Section,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let selected = self.section == section;
        div()
            .id(("settings-section", index))
            .role(Role::Tab)
            .aria_label(section.label())
            .aria_selected(selected)
            .relative()
            .map(|row| {
                #[cfg(all(feature = "integration-test", target_os = "macos"))]
                let row = row.child(native::probe(index));
                row
            })
            .debug_selector(move || section.selector())
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(corners::CONTROL))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme.primary_wash())))
            .hover(|el| el.bg(rgb(theme.active)))
            .child(
                svg()
                    .path(section.icon())
                    .size(px(17.))
                    .flex_none()
                    .text_color(rgb(if selected {
                        theme.primary()
                    } else {
                        theme.subtext()
                    })),
            )
            .child(section.label())
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_section(section, window, cx)),
            )
    }

    fn pending_drafts(&self) -> Vec<&'static str> {
        [
            (self.theme_dirty(), "theme"),
            (self.layout_intent.is_some(), "layout"),
            (self.sidebar_dirty(), "sidebar"),
        ]
        .into_iter()
        .filter_map(|(dirty, name)| dirty.then_some(name))
        .collect()
    }

    pub(super) fn footer_text(&self) -> String {
        let pending = self.pending_drafts();
        if let Some(error) = &self.error {
            error.clone()
        } else if self.loading {
            "Loading preferences...".into()
        } else if let Some(status) = &self.status {
            status.clone()
        } else if pending.is_empty() {
            match self.section {
                // Its account fields are a form with their own Save button.
                #[cfg(feature = "cloud")]
                Section::CloudDevices => {
                    "Account fields save with Save; sign-in and removal apply at once.".into()
                }
                _ => "All changes saved.".into(),
            }
        } else {
            format!(
                "Unsaved: {} — saved when this window closes",
                pending.join(", ")
            )
        }
    }

    pub(super) fn footer(&self, cx: &mut Context<Self>) -> Div {
        let pending = self.pending_drafts();
        let text = self.footer_text();
        div()
            .flex_none()
            .min_h(px(36.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(12.))
            .border_t_1()
            .border_color(rgb(self.theme.active))
            .text_size(px(11.))
            .when(!pending.is_empty() && self.error.is_none(), |footer| {
                footer.child(
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(rgb(self.theme.ink(0xe3b341))),
                )
            })
            .child(
                div()
                    .id("settings-footer-status")
                    .debug_selector(|| "settings-footer-status".into())
                    .role(Role::Status)
                    .aria_label(SharedString::from(text.clone()))
                    .flex_1()
                    .min_w_0()
                    .child(text),
            )
            .when(!pending.is_empty(), |footer| {
                footer.child(
                    self.control_choice("settings-footer-save", "Save now", false, !self.busy())
                        .debug_selector(|| "settings-footer-save".into())
                        .role(Role::Button)
                        .aria_label("Save now")
                        .flex_none()
                        .px(px(10.))
                        .py(px(3.))
                        .on_click(cx.listener(|this, _, _, cx| this.save_pending_drafts(cx))),
                )
            })
            .child(
                self.control_choice("settings-footer-reload", "Reload", false, true)
                    .debug_selector(|| "settings-footer-reload".into())
                    .role(Role::Button)
                    .aria_label("Reload")
                    .flex_none()
                    .px(px(10.))
                    .py(px(3.))
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
    }
}
