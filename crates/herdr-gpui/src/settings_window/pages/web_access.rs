//! Web access: where `herdr-gpui serve` shows this app, the pages open on
//! it now, and the paired devices, which can be forgotten here. The same
//! page shows in every browser, so nothing here acts on "this browser".
use super::{SettingsWindow, ui};
use crate::settings_window::section::Section;
use gpui::{prelude::*, *};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often the open page rereads who is connected.
const REFRESH: Duration = Duration::from_secs(2);

/// What the server reported last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::settings_window) struct WebAccess {
    served: bool,
    local_address: String,
    public_hosts: Vec<String>,
    /// Each open page's device and whether it came through the tunnel.
    connections: Vec<(String, bool)>,
    devices: Vec<Paired>,
    /// Why the last Forget failed, for the page to say.
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Paired {
    id: String,
    label: String,
    remote: bool,
    /// Seconds since the Unix epoch.
    last_seen: u64,
}

/// What the server reports now. Reads the devices file, so it runs off the
/// UI thread.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn snapshot() -> WebAccess {
    let Some(served) = herdr_web::host::Served::get() else {
        return WebAccess::default();
    };
    WebAccess {
        served: true,
        local_address: served.local_address(),
        public_hosts: served.public_hosts(),
        connections: served
            .connections()
            .into_iter()
            .map(|connection| (connection.label, connection.remote))
            .collect(),
        devices: served
            .devices()
            .into_iter()
            .map(|device| Paired {
                id: device.id,
                label: device.label,
                remote: device.remote,
                last_seen: device.last_seen,
            })
            .collect(),
        error: None,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
fn snapshot() -> WebAccess {
    WebAccess::default()
}

/// Forgets device `id`; the error's text when that failed.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn forget(id: &str) -> Option<String> {
    let served = herdr_web::host::Served::get()?;
    served.forget(id).err().map(|error| error.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
fn forget(_: &str) -> Option<String> {
    None
}

/// When a device was last seen, from `now`, both in Unix seconds.
pub(in crate::settings_window) fn seen(last_seen: u64, now: u64) -> String {
    let elapsed = now.saturating_sub(last_seen);
    match elapsed {
        0..60 => "Seen just now".to_owned(),
        60..3_600 => format!("Seen {} min ago", elapsed / 60),
        3_600..86_400 => format!("Seen {} h ago", elapsed / 3_600),
        _ => format!("Seen {} days ago", elapsed / 86_400),
    }
}

fn place(remote: bool) -> &'static str {
    if remote {
        "Through the tunnel"
    } else {
        "On this Mac"
    }
}

impl SettingsWindow {
    /// Rereads the server while this page shows, and once after it closes.
    pub(in crate::settings_window) fn watch_web_access(&mut self, cx: &mut Context<Self>) {
        self.web_access_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                let access = cx.background_executor().spawn(async { snapshot() }).await;
                let shown = this.update(cx, |this, cx| {
                    let error = this.web_access.error.take();
                    this.web_access = WebAccess { error, ..access };
                    cx.notify();
                    this.section == Section::WebAccess
                });
                if !matches!(shown, Ok(true)) {
                    break;
                }
                cx.background_executor().timer(REFRESH).await;
            }
        }));
    }

    /// Asks first, in the app's own dialog: a device that is forgotten must
    /// pair again, and a stray tap on a phone should not do that.
    fn forget_device(
        &mut self,
        id: String,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let detail = format!("{label} must pair again to use Herdr UI. Its open pages close.");
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Forget {label}?"),
            Some(&detail),
            &["Forget", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let error = cx
                .background_executor()
                .spawn(async move { forget(&id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.web_access.error = error;
                this.watch_web_access(cx);
            });
        })
        .detach();
    }

    pub(in crate::settings_window) fn render_web_access(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let access = &self.web_access;
        let page = div().flex().flex_col();
        if !access.served {
            return page.child(ui::section(theme, "Serving", None)).child(
                ui::row(theme, "web-not-served", "Not served").description(
                    "Run herdr-gpui serve to use this app in a browser, on this Mac or \
                     through a tunnel. Its pages and paired devices then show here.",
                ),
            );
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let mut page = page.child(ui::section(theme, "Serving", None)).child(
            ui::row(theme, "web-local", "On this Mac")
                .description("Any browser here opens it; no pairing is needed.")
                .control(div().child(access.local_address.clone())),
        );
        for (index, host) in access.public_hosts.iter().enumerate() {
            page = page.child(
                ui::row(theme, format!("web-public-{index}"), "Through the tunnel")
                    .description("New devices pair with a code, approved here or in the terminal.")
                    .control(div().child(format!("https://{host}"))),
            );
        }
        page = page.child(ui::section(theme, "Open now", None));
        if access.connections.is_empty() {
            page = page.child(ui::row(theme, "web-open-none", "No page is open"));
        }
        for (index, (label, remote)) in access.connections.iter().enumerate() {
            page = page.child(
                ui::row(theme, format!("web-open-{index}"), label.clone())
                    .description(place(*remote)),
            );
        }
        page = page.child(ui::section(theme, "Paired devices", None));
        if let Some(error) = &access.error {
            page = page.child(
                ui::row(theme, "web-forget-error", "Could not forget the device")
                    .description(error.clone()),
            );
        }
        if access.devices.is_empty() {
            page = page.child(
                ui::row(theme, "web-devices-none", "No device is paired")
                    .description("Devices that pair through the tunnel show here."),
            );
        }
        for device in &access.devices {
            let (id, label) = (device.id.clone(), device.label.clone());
            page = page.child(
                ui::row(
                    theme,
                    format!("web-device-{}", device.id),
                    device.label.clone(),
                )
                .description(format!(
                    "{} \u{b7} {}",
                    place(device.remote),
                    seen(device.last_seen, now)
                ))
                .control(
                    ui::button(
                        theme,
                        format!("web-device-forget-{}", device.id),
                        "Forget",
                        true,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.forget_device(id.clone(), label.clone(), window, cx);
                    })),
                ),
            );
        }
        page
    }
}
