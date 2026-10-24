//! An agent's compaction summary as one quiet card: its gist folded, its
//! non-empty sections when opened.

use super::super::summary::Summary;
use crate::{HerdrWindow, config::corners, release_notes};
use gpui::{prelude::*, *};
use herdr_chat_protocol::Entry;

impl HerdrWindow {
    pub(super) fn render_chat_summary(
        &self,
        pane_id: &str,
        entry: &Entry,
        summary: &Summary,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let small = px((self.chat_text_size() - 1.).max(10.));
        let pane_id = pane_id.to_owned();
        let id = entry.id;
        let card = div()
            .id(("chat-summary", id.0))
            .debug_selector(move || format!("chat-summary-{}", id.0))
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(corners::CONTROL))
            .bg(rgb(theme.surface))
            .cursor_pointer()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(small)
                    .text_color(rgb(theme.subtext()))
                    .child(
                        svg()
                            .flex_none()
                            .path("icons/note.svg")
                            .size(px(11.))
                            .text_color(rgb(theme.subtext())),
                    )
                    .child("Context summary")
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme.primary()))
                            .child(if expanded { "Hide" } else { "Show" }),
                    ),
            )
            .map(|card| {
                if expanded {
                    card.child(release_notes::render(
                        "chat-summary",
                        &summary.lines,
                        theme,
                        &self.config.terminal,
                    ))
                } else {
                    card.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(small)
                            .child(summary.gist.clone()),
                    )
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if let Some(pane) = this.chats.pane_mut(&pane_id) {
                    if !pane.expanded.remove(&id) {
                        pane.expanded.insert(id);
                    }
                    pane.remeasure(id);
                }
                cx.notify();
            }));
        div().child(card)
    }
}
