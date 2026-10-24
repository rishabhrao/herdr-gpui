//! Removing a device whose provider can delete it (see `cloud::deletion`)
//! deletes its machine there by default, with everything it bills for; Remove
//! from Herdr only, beside it, just forgets the device, and the machine keeps
//! running until it is deleted elsewhere.
//!
//! Before that, the machine is searched for checkouts with work pushed
//! nowhere (see `cloud::unsynced`); when it has some, the dialog names them
//! and Delete becomes Delete anyway.
use super::{HostMenu, Saved};
use crate::{
    HerdrWindow,
    cloud::{Deletion, unsynced::Checkout},
};
use gpui::{prelude::*, *};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Checkouts the dialog names before summing up the rest.
const NAMED: usize = 4;

/// The search for work a machine holds that is pushed nowhere.
#[derive(Debug, Default)]
pub(super) enum Unsynced {
    #[default]
    NotChecked,
    /// Searching; closing the menu drops this, which stops the search.
    Checking {
        _cancel: Cancel,
    },
    Clean,
    Found(Vec<Checkout>),
    /// The machine could not be searched, often because it is stopped.
    Failed,
}

/// Stops a search when the menu that started it goes away.
#[derive(Debug)]
pub(super) struct Cancel(Arc<AtomicBool>);

impl Unsynced {
    fn is_search(&self, cancelled: &Arc<AtomicBool>) -> bool {
        matches!(self, Self::Checking { _cancel } if Arc::ptr_eq(&_cancel.0, cancelled))
    }
}

#[cfg(test)]
impl Cancel {
    pub(super) fn for_test() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}

impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl HostMenu {
    /// How removing this device deletes its machine, if its provider can.
    pub(super) fn deletion(&self) -> Option<Deletion> {
        match &self.saved {
            Saved::Cloud { provider, .. } => crate::cloud::deletion(*provider),
            Saved::Ssh { .. } => None,
        }
    }

    /// Whether Remove would delete the machine before the search for its
    /// unpushed work has answered; it waits rather than delete unseen work.
    pub(super) fn awaiting_check(&self) -> bool {
        self.destroy
            && self.deletion().is_some()
            && matches!(self.unsynced, Unsynced::Checking { .. })
    }

    fn machine_noun(&self) -> Option<&'static str> {
        match &self.saved {
            Saved::Cloud { id, .. } => Some((self.deletion()?.noun)(id)),
            Saved::Ssh { .. } => None,
        }
    }
}

/// The default button's text: Delete for a device its provider can delete,
/// Delete anyway once it is known to hold work pushed nowhere.
pub(super) fn remove_label(host: &HostMenu) -> String {
    match host.machine_noun() {
        Some(_) if host.awaiting_check() => "Checking…".to_owned(),
        Some(_) if matches!(host.unsynced, Unsynced::Found(_)) => "Delete anyway".to_owned(),
        Some(noun) => format!("Delete {noun}"),
        None => "Remove".to_owned(),
    }
}

/// What the search found, in a sentence or two for the dialog.
pub(super) fn unsynced_text(unsynced: &Unsynced, noun: &str) -> Option<String> {
    match unsynced {
        Unsynced::NotChecked | Unsynced::Clean => None,
        Unsynced::Checking { .. } => Some(format!("Checking the {noun} for work not pushed…")),
        Unsynced::Failed => Some(format!(
            "Could not check the {noun} for work not pushed. Anything on it is deleted with it."
        )),
        Unsynced::Found(checkouts) => {
            let mut text = format!("Work on this {noun} is pushed nowhere else:");
            for checkout in checkouts.iter().take(NAMED) {
                let mut counts = Vec::new();
                if checkout.changed > 0 {
                    counts.push(format!("{} changed files", checkout.changed));
                }
                if checkout.unpushed > 0 {
                    counts.push(format!("{} unpushed commits", checkout.unpushed));
                }
                text.push_str(&format!("\n{} ({})", checkout.path, counts.join(", ")));
            }
            if checkouts.len() > NAMED {
                text.push_str(&format!("\nand {} more", checkouts.len() - NAMED));
            }
            text.push_str("\nPush it, or Teleport it to another device, before deleting.");
            Some(text)
        }
    }
}

/// What deleting the device does, or `None` when it cannot be deleted.
pub(super) fn removal_note(host: &HostMenu) -> Option<String> {
    let Saved::Cloud { provider, .. } = &host.saved else {
        return None;
    };
    let deletion = host.deletion()?;
    let noun = host.machine_noun()?;
    Some(format!(
        "Delete the {noun} {} ({}) at {}{} and remove it from this computer? Its files are \
         not kept, and this cannot be undone.",
        host.label,
        host.target,
        crate::cloud::name(*provider),
        deletion.leftovers,
    ))
}

impl HerdrWindow {
    /// Search the machine for work pushed nowhere, if Remove would delete it.
    pub(super) fn check_unsynced(&mut self, cx: &mut Context<Self>) {
        let Some(host) = &mut self.menu.host else {
            return;
        };
        if host.deletion().is_none() || !matches!(host.unsynced, Unsynced::NotChecked) {
            return;
        }
        let Some(target) = self
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == host.id)
            .map(|endpoint| endpoint.connection.target.clone())
        else {
            return;
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        host.unsynced = Unsynced::Checking {
            _cancel: Cancel(cancelled.clone()),
        };
        let id = host.id.clone();
        let search_cancelled = cancelled.clone();
        let search = cx
            .background_executor()
            .spawn(async move { crate::cloud::unsynced::find(&target, &search_cancelled) });
        cx.spawn(async move |this, cx| {
            let found = search.await;
            let _ = this.update(cx, |this, cx| {
                let Some(host) = this
                    .menu
                    .host
                    .as_mut()
                    .filter(|host| host.id == id && host.unsynced.is_search(&cancelled))
                else {
                    return;
                };
                host.unsynced = match found {
                    Ok(checkouts) if checkouts.is_empty() => Unsynced::Clean,
                    Ok(checkouts) => Unsynced::Found(checkouts),
                    Err(error) => {
                        tracing::warn!(
                            category = "device_remove",
                            id,
                            error = %error,
                            "Could not check the machine for unsynced work"
                        );
                        Unsynced::Failed
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// The search's finding, under the removal note.
    pub(super) fn unsynced_note(&self, host: &HostMenu) -> Option<Div> {
        let text = unsynced_text(&host.unsynced, host.machine_noun()?)?;
        let warn = matches!(host.unsynced, Unsynced::Found(_) | Unsynced::Failed);
        Some(
            div()
                .debug_selector(|| "remove-device-unsynced".into())
                .whitespace_normal()
                .text_color(if warn {
                    crate::menu::danger(&self.theme)
                } else {
                    rgb(self.theme.subtext())
                })
                .children(text.lines().map(|line| div().child(line.to_owned()))),
        )
    }

    /// Remove from Herdr only, for a device its provider can delete: the
    /// secondary answer.
    pub(super) fn remove_only_button(
        &self,
        host: &HostMenu,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        host.deletion()?;
        Some(
            div()
                .id("remove-device-only")
                .debug_selector(|| "remove-device-only".into())
                .p(px(6.))
                .cursor_pointer()
                .child("Remove from Herdr only")
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(host) = &mut this.menu.host {
                        host.destroy = false;
                    }
                    this.confirm_remove_device(window, cx);
                })),
        )
    }
}

#[cfg(test)]
mod tests;
