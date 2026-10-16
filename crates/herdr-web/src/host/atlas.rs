//! A window's glyph and image tiles. GPUI rasterizes into it as into any
//! atlas; instead of packing a texture it numbers each tile and keeps its
//! pixels, so a page that connects late, or reconnects, gets them all again.
use crate::wire::{TileUpload, WireTile};
use gpui::{
    AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds, DevicePixels, PlatformAtlas,
    Point, Size, TileId,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

struct Stored {
    kind: WireTile,
    size: Size<DevicePixels>,
    bytes: Arc<[u8]>,
}

#[derive(Default)]
struct State {
    by_key: HashMap<AtlasKey, AtlasTile>,
    tiles: HashMap<u32, Stored>,
    next: u32,
    /// Tiles added or removed since the page last heard.
    added: Vec<u32>,
    removed: Vec<u32>,
}

#[derive(Default)]
pub(super) struct RemoteAtlas(Mutex<State>);

pub(super) fn wire_kind(kind: AtlasTextureKind) -> WireTile {
    match kind {
        AtlasTextureKind::Monochrome => WireTile::Mono,
        AtlasTextureKind::Subpixel => WireTile::Subpixel,
        AtlasTextureKind::Polychrome => WireTile::Poly,
    }
}

fn upload(id: u32, stored: &Stored) -> TileUpload {
    TileUpload {
        id,
        kind: stored.kind,
        width: stored.size.width.0,
        height: stored.size.height.0,
        bytes: stored.bytes.to_vec(),
    }
}

impl RemoteAtlas {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What changed since the last call: tiles to upload, then ids to drop.
    pub(super) fn take_changes(&self) -> (Vec<TileUpload>, Vec<u32>) {
        let mut state = self.state();
        let added = std::mem::take(&mut state.added);
        let removed = std::mem::take(&mut state.removed);
        let uploads = added
            .into_iter()
            .filter_map(|id| state.tiles.get(&id).map(|stored| upload(id, stored)))
            .collect();
        (uploads, removed)
    }

    /// Every tile, for a page that has none yet. The changes stay queued
    /// for the pages connected already; the new page may get some twice,
    /// which replaces a tile with itself.
    pub(super) fn everything(&self) -> Vec<TileUpload> {
        let state = self.state();
        state
            .tiles
            .iter()
            .map(|(id, stored)| upload(*id, stored))
            .collect()
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.state().tiles.len()
    }
}

impl PlatformAtlas for RemoteAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> anyhow::Result<Option<AtlasTile>> {
        let mut state = self.state();
        if let Some(tile) = state.by_key.get(&key) {
            return Ok(Some(*tile));
        }
        let Some((size, bytes)) = build()? else {
            return Ok(None);
        };
        let kind = key.texture_kind();
        let id = state.next;
        state.next = state.next.wrapping_add(1);
        let tile = AtlasTile {
            texture_id: AtlasTextureId { index: 0, kind },
            tile_id: TileId(id),
            padding: 0,
            bounds: Bounds {
                origin: Point::default(),
                size,
            },
        };
        state.tiles.insert(
            id,
            Stored {
                kind: wire_kind(kind),
                size,
                bytes: Arc::from(bytes.as_ref()),
            },
        );
        state.added.push(id);
        state.by_key.insert(key, tile);
        Ok(Some(tile))
    }

    fn remove(&self, key: &AtlasKey) {
        let mut state = self.state();
        let Some(tile) = state.by_key.remove(key) else {
            return;
        };
        let id = tile.tile_id.0;
        state.tiles.remove(&id);
        // A tile the page never heard of needs no removal either.
        if let Some(index) = state.added.iter().position(|added| *added == id) {
            state.added.remove(index);
        } else {
            state.removed.push(id);
        }
    }
}
