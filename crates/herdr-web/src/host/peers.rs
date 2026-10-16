//! The pages connected at once, one per browser tab on any device. The
//! app has one size per window, so it lays out for the page used last, the
//! driver; the others show the same picture scaled to fit.
use crate::wire::WindowId;
use std::collections::HashMap;

/// A window's viewport as a page reported it: width, height, pixel ratio.
pub(super) type Viewport = (f32, f32, f32);

#[derive(Clone, Debug, Default)]
pub(super) struct Peer {
    id: u64,
    remote: bool,
    visible: bool,
    pub(super) dark: Option<bool>,
    pub(super) viewports: HashMap<WindowId, Viewport>,
}

/// What a page leaving leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Left {
    /// It was not connected.
    Unknown,
    /// No page is left.
    Last,
    /// It drove; this page, used before it, drives next.
    Driver(u64),
    /// Another page drives, as before.
    Other,
}

/// Connected pages, oldest first.
#[derive(Debug, Default)]
pub(super) struct Peers {
    peers: Vec<Peer>,
    driver: Option<u64>,
}

impl Peers {
    /// A new page. The first one drives until another is used.
    pub(super) fn add(&mut self, id: u64, remote: bool) {
        self.peers.push(Peer {
            id,
            remote,
            visible: true,
            ..Peer::default()
        });
        self.driver.get_or_insert(id);
    }

    /// Forgets page `id`, and says what that leaves.
    pub(super) fn remove(&mut self, id: u64) -> Left {
        let Some(index) = self.peers.iter().position(|peer| peer.id == id) else {
            return Left::Unknown;
        };
        self.peers.remove(index);
        if self.peers.is_empty() {
            self.driver = None;
            return Left::Last;
        }
        if self.driver != Some(id) {
            return Left::Other;
        }
        // The newest page left is the likeliest one in use.
        self.driver = None;
        match self.peers.last() {
            Some(peer) => Left::Driver(peer.id),
            None => Left::Last,
        }
    }

    pub(super) fn contains(&self, id: u64) -> bool {
        self.peers.iter().any(|peer| peer.id == id)
    }

    pub(super) fn driver(&self) -> Option<u64> {
        self.driver
    }

    pub(super) fn driver_is_remote(&self) -> bool {
        self.driver
            .and_then(|driver| self.peers.iter().find(|peer| peer.id == driver))
            .is_some_and(|peer| peer.remote)
    }

    /// Makes page `id` the driver. Returns its state when it was not the
    /// driver already, for the windows to follow.
    pub(super) fn drive(&mut self, id: u64) -> Option<Peer> {
        if self.driver == Some(id) {
            return None;
        }
        let peer = self.peers.iter().find(|peer| peer.id == id)?.clone();
        self.driver = Some(id);
        Some(peer)
    }

    fn peer(&mut self, id: u64) -> Option<&mut Peer> {
        self.peers.iter_mut().find(|peer| peer.id == id)
    }

    pub(super) fn set_viewport(&mut self, id: u64, window: WindowId, viewport: Viewport) {
        if let Some(peer) = self.peer(id) {
            peer.viewports.insert(window, viewport);
        }
    }

    pub(super) fn set_visible(&mut self, id: u64, visible: bool) {
        if let Some(peer) = self.peer(id) {
            peer.visible = visible;
        }
    }

    pub(super) fn set_dark(&mut self, id: u64, dark: bool) {
        if let Some(peer) = self.peer(id) {
            peer.dark = Some(dark);
        }
    }

    pub(super) fn any_visible(&self) -> bool {
        self.peers.iter().any(|peer| peer.visible)
    }
}
