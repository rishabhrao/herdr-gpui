//! Settings in a narrow window, such as a phone's browser: the section
//! list and a page take the whole width in turn, with a way back to the list.
use super::*;

/// Below this width, in app pixels, Settings shows one column.
const NARROW_WIDTH: f32 = 560.;

impl SettingsWindow {
    pub(super) fn narrow(&self) -> bool {
        self.viewport_width > 0. && self.viewport_width < NARROW_WIDTH
    }

    /// Whether the page shows: always when wide; when narrow, once a
    /// section or a search was chosen.
    pub(super) fn page_shown(&self) -> bool {
        !self.narrow() || self.narrow_page || self.search_results.is_some()
    }

    /// Whether the section list shows beside or instead of the page.
    pub(super) fn navigation_shown(&self) -> bool {
        !self.narrow() || !self.page_shown()
    }

    /// The narrow page's way back to the section list.
    pub(super) fn back_to_sections(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.narrow() {
            return None;
        }
        let theme = &self.theme;
        Some(
            div()
                .id("settings-back")
                .debug_selector(|| "settings-back".into())
                .flex()
                .items_center()
                .gap(px(6.))
                .mb(px(12.))
                .min_h(px(36.))
                .text_color(rgb(theme.primary()))
                .cursor_pointer()
                .child("\u{2039} All settings")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.narrow_page = false;
                    this.clear_search(cx);
                    cx.notify();
                })),
        )
    }
}
