//! A narrow served window, such as a phone's: the rail stays beside the
//! terminal, and the full sidebar opens over the terminal as a drawer
//! instead of taking half the width from it. Choosing a place closes it.
//! Wider windows keep the sidebar as it is, so the same app driven from a
//! laptop gets its column back.
use super::rail::RAIL_WIDTH;
use crate::HerdrWindow;
use gpui::{prelude::*, *};

/// Below this width, in app pixels, a served window is compact.
pub(crate) const COMPACT_WIDTH: f32 = 600.;
/// The drawer's widest, and the strip of terminal it leaves to tap away.
const DRAWER_MAX: f32 = 360.;
const DRAWER_GAP: f32 = 56.;

/// The drawer's width in a window `window_width` wide.
pub(crate) fn drawer_width(window_width: f32) -> f32 {
    (window_width - DRAWER_GAP).clamp(160., DRAWER_MAX)
}

impl HerdrWindow {
    /// Whether this window lays out for a narrow screen.
    pub(crate) fn compact(&self) -> bool {
        crate::served::active() && self.viewport_width > 0. && self.viewport_width < COMPACT_WIDTH
    }

    pub(crate) fn drawer_open(&self) -> bool {
        self.sidebar_drawer && self.compact()
    }

    /// Closes the drawer; whether it was open.
    pub(crate) fn close_drawer(&mut self) -> bool {
        std::mem::take(&mut self.sidebar_drawer)
    }

    /// The column beside the terminal while the drawer is open: its room,
    /// `width` wide and empty, so the terminal keeps its size under the drawer.
    pub(crate) fn drawer_spacer(&self, width: f32) -> AnyElement {
        div()
            .w(px(width))
            .flex_none()
            .h_full()
            .bg(self.fill().panel(self.theme.sidebar_background()))
            .into_any_element()
    }

    /// The expanded sidebar's width in the drawer, which the section tabs
    /// sit above rather than beside.
    pub(crate) fn drawer_sidebar_width(&self, window_width: f32) -> f32 {
        drawer_width(window_width)
    }

    /// The open drawer over the window body, and the shade beside it that
    /// closes it.
    pub(crate) fn render_drawer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.drawer_open() {
            return None;
        }
        let width = drawer_width(self.viewport_width).max(RAIL_WIDTH);
        Some(
            div()
                .debug_selector(|| "sidebar-drawer".into())
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .occlude()
                .child(
                    div()
                        .w(px(width))
                        .flex_none()
                        .h_full()
                        .bg(rgb(self.theme.background))
                        .border_r_1()
                        .border_color(rgb(self.theme.active))
                        .shadow_lg()
                        .child(crate::sidebar::cached_view(&self.sidebar_view, width)),
                )
                .child(
                    div()
                        .id("sidebar-drawer-shade")
                        .flex_1()
                        .h_full()
                        .bg(hsla(0., 0., 0., 0.6))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.close_drawer();
                                cx.notify();
                            }),
                        ),
                )
                .into_any_element(),
        )
    }
}
