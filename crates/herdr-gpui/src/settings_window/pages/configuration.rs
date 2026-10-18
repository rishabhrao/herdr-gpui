//! Configuration: the files behind these settings, and reload.
use super::{SettingsWindow, ui};
use gpui::{prelude::*, *};
use std::path::PathBuf;

/// Reveals `path` in the file manager, or opens it in the default editor.
/// Spawned on the background executor: waiting on the helper never blocks the UI.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(in crate::settings_window) fn open_path(reveal: bool, path: PathBuf, cx: &mut App) {
    cx.background_executor()
        .spawn(async move {
            #[cfg(target_os = "macos")]
            let mut command = {
                let mut command = herdr_client::new_command("open");
                if reveal {
                    command.arg("-R");
                }
                command.arg(&path);
                command
            };
            #[cfg(target_os = "linux")]
            let mut command = {
                let mut command = herdr_client::new_command("xdg-open");
                command.arg(if reveal {
                    path.parent().unwrap_or(&path)
                } else {
                    &path
                });
                command
            };
            if let Err(error) = command.status() {
                tracing::warn!("Unable to open {}: {error}", path.display());
            }
        })
        .detach();
}

impl SettingsWindow {
    pub(in crate::settings_window) fn render_configuration(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let shared_path = self.shared.as_ref().map(|shared| shared.path.clone());
        div()
            .flex()
            .flex_col()
            .child(ui::section(theme, "Files", None))
            .child(self.file_row(
                "config-local",
                "Herdr UI settings",
                ui::Source::HerdrUi,
                self.controls.local_path.clone(),
                self.controls.local_file.clone(),
            ))
            .child(self.gate_shared(self.file_row(
                "config-shared",
                "Herdr configuration",
                ui::Source::Herdr,
                shared_path.as_ref().map_or_else(
                    || "Unavailable".to_owned(),
                    |path| path.display().to_string(),
                ),
                shared_path.clone(),
            )))
            .child(
                ui::row(theme, "config-reload", "Reload configuration")
                    .description("Re-reads both files. Does not restart the daemon.")
                    .control(
                        ui::button(theme, "config-reload-button", "Reload", true)
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    ),
            )
    }

    fn file_row_base(
        &self,
        id: &'static str,
        title: &'static str,
        source: ui::Source,
        path: String,
    ) -> ui::Row {
        ui::row(&self.theme, id, title)
            .tag(source)
            .description(path)
    }

    /// A file's row with Reveal and Open buttons, which only exist where the
    /// platform has something to hand the path to.
    fn file_row(
        &self,
        id: &'static str,
        title: &'static str,
        source: ui::Source,
        text: String,
        file: Option<PathBuf>,
    ) -> ui::Row {
        let row = self.file_row_base(id, title, source, text);
        self.file_buttons(row, id, file)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(in crate::settings_window) fn file_buttons(
        &self,
        mut row: ui::Row,
        id: &'static str,
        file: Option<PathBuf>,
    ) -> ui::Row {
        for (suffix, label, reveal) in [("reveal", "Reveal", true), ("open", "Open", false)] {
            let enabled = file.is_some();
            let file = file.clone();
            row = row.control(
                ui::button(&self.theme, format!("{id}-{suffix}"), label, enabled).when(
                    enabled,
                    |button| {
                        button.on_click(move |_, _, cx| {
                            if let Some(file) = file.clone() {
                                open_path(reveal, file, cx);
                            }
                        })
                    },
                ),
            );
        }
        row
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(in crate::settings_window) fn file_buttons(
        &self,
        row: ui::Row,
        _id: &'static str,
        _file: Option<PathBuf>,
    ) -> ui::Row {
        row
    }
}
