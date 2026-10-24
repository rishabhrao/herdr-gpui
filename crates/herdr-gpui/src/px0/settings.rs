//! One serial loader owns the shared workbench configuration, independent of
//! every window's deferred reloads. Invalid edits retain the last good config.
use super::Launcher;
use crate::config::{
    Config as AppConfig,
    watch::{Watch, fingerprint_all},
};
use gpui::{App, Task};
use std::time::Duration;

pub(super) fn watch(cx: &mut App) -> Task<()> {
    cx.spawn(async move |cx| {
        let mut watch = Watch::default();
        loop {
            let sampled = cx
                .background_executor()
                .spawn(async {
                    Ok::<_, crate::Error>(fingerprint_all([
                        AppConfig::path()?,
                        AppConfig::local_path()?,
                    ]))
                })
                .await;
            match sampled {
                Ok(sample) if watch.observe(sample) => {
                    let loaded = cx
                        .background_executor()
                        .spawn(async { AppConfig::load().map(|config| config.px0) })
                        .await;
                    // Acknowledging the sampled bytes leaves any later edit
                    // pending. No overlapping loads can publish out of order.
                    watch.accept(sample);
                    match loaded {
                        Ok(config) => cx.update(|cx| Launcher::apply_config(cx, config)),
                        Err(error) => tracing::warn!(%error, "Cannot reload px0 settings"),
                    }
                }
                Err(error) => tracing::warn!(%error, "Cannot watch px0 settings"),
                _ => {}
            }
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
        }
    })
}
