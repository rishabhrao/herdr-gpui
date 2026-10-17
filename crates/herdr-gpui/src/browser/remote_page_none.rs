//! Windows cannot serve the app (`herdr-gpui serve` refuses there), so a
//! window never has served pages.
use super::{
    Location, Tab, TabId,
    native::{Event, Source},
};
use gpui::{AnyElement, Window};
use herdr_web::wire::PageOp;
use std::sync::mpsc::SyncSender;

#[derive(Default)]
pub(crate) struct Remote;

impl Remote {
    pub(crate) fn contains(&self, _: TabId) -> bool {
        false
    }

    pub(crate) fn ids(&self) -> impl Iterator<Item = TabId> + '_ {
        std::iter::empty()
    }

    pub(crate) fn view(&self, _: TabId) -> Option<AnyElement> {
        None
    }

    pub(crate) fn set_theme(&self, _: &str) {}

    pub(crate) fn ensure(
        &mut self,
        _: &Tab,
        _: &Location,
        _: &SyncSender<Event<Source>>,
        _: &Window,
    ) -> crate::Result<()> {
        Ok(())
    }

    pub(crate) fn show(&self, _: TabId, _: bool) {}

    pub(crate) fn close(&mut self, _: TabId) {}

    pub(crate) fn load(&self, _: TabId, _: &Location) {}

    pub(crate) fn send(&self, _: TabId, _: impl FnOnce(u64) -> PageOp) {}
}
