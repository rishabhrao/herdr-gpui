//! Asks before opening a workspace for a checkout a `herdr-gpui://` link
//! named. Any web page or program can send such a link, and a workspace
//! starts a shell in its folder, so the user sees the folder first and
//! nothing opens unless they say so. A checkout already open is focused
//! without asking (see `link::follow`).

use super::Page;
use crate::{HerdrWindow, endpoint::LOCAL, fonts::StyledFont};
use gpui::{prelude::*, *};
use herdr_client::Method;
use std::path::PathBuf;

impl HerdrWindow {
    /// Shows the question for `path`, unless another page holds the window.
    pub(crate) fn ask_to_open_checkout(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.page.is_some() || !self.open_menu(window, cx) {
            self.local_error = Some("Open link: close the open dialog first".into());
            return;
        }
        self.menu.open_checkout = Some(path);
        self.menu.page = Some(Page::OpenCheckout);
    }

    /// Opens a workspace in the folder on this computer's Herdr, and closes
    /// the question. The repository's own scripts are not trusted by it.
    pub(in crate::menu) fn confirm_open_checkout(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.menu.open_checkout.take();
        self.dismiss_menu(window, cx);
        let Some(path) = path else {
            return;
        };
        if !self.select_endpoint(LOCAL, cx) {
            self.local_error = Some("Open link: this computer's Herdr is not available".into());
            return;
        }
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let params = serde_json::json!({
            "cwd": path,
            "label": label,
            "focus": true,
            "trust_repository": false,
        });
        if !self.request_focus_change("Open workspace", None, |handle, boot| {
            handle.request(boot, Method::WorkspaceCreate, params)
        }) && self.local_error.is_none()
        {
            self.local_error = Some("Open link: not connected to this computer's Herdr".into());
        }
        cx.notify();
    }

    pub(in crate::menu) fn render_open_checkout(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let Some(path) = &self.menu.open_checkout else {
            return div();
        };
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .p(px(8.))
                .rounded(px(crate::config::corners::CONTROL))
                .when(primary, |button| button.bg(rgb(theme.active)))
                .when(!primary, |button| button.hover(|s| s.bg(rgb(theme.active))))
                .cursor_pointer()
                .child(label)
        };
        div()
            .debug_selector(|| "open-checkout".into())
            .child(
                div()
                    .p(px(8.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Open a workspace in this folder?"),
            )
            .child(
                div().px(px(8.)).child(
                    div()
                        .debug_selector(|| "open-checkout-path".into())
                        .rounded(px(crate::config::corners::CONTROL))
                        .bg(rgb(theme.active))
                        .px(px(10.))
                        .py(px(6.))
                        .overflow_hidden()
                        .text_font(&self.config.terminal)
                        .child(path.to_string_lossy().into_owned()),
                ),
            )
            .child(
                div().p(px(8.)).text_color(rgb(theme.muted)).child(
                    "A link from another application asked for it. A new workspace starts a shell there; the repository's scripts do not run.",
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .p(px(8.))
                    .child(button("open-checkout-yes", "Open", true).on_click(cx.listener(
                        |this, _, window, cx| {
                            cx.stop_propagation();
                            this.confirm_open_checkout(window, cx);
                        },
                    )))
                    .child(button("open-checkout-no", "Cancel", false).on_click(cx.listener(
                        |this, _, window, cx| {
                            cx.stop_propagation();
                            this.dismiss_menu(window, cx);
                        },
                    ))),
            )
    }
}
