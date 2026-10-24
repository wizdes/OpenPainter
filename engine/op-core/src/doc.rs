//! Immutable document snapshots backed by copy-on-write tile pages.
pub use crate::selection::{MaskGrid, PolygonSet, SelectionSource, SelectionState};
use crate::selection::{RasterMode, Selection};
use crate::tiles::{
    TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileError, TileId, TileRef, TileStore,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DocError {
    #[error("invalid layer index")]
    LayerIndex,
    #[error("cannot remove the final layer")]
    LastLayer,
    #[error("invalid active layer")]
    ActiveLayer,
    #[error("tile is outside the canvas")]
    TileBounds,
    #[error("invalid canvas dimensions")]
    Dimensions,
    #[error("tile storage failed: {0}")]
    Tile(#[from] TileError),
}
pub type Result<T> = std::result::Result<T, DocError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerBlendMode {
    Normal,
    Multiply,
    Additive,
    ColorBurn,
    ColorDodge,
    Reflect,
    Glow,
    Overlay,
    Difference,
    Negation,
    Lighten,
    Darken,
    Screen,
    Xor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCompositeMode {
    Normal,
    Overwrite,
}

static RELEASE_FAILURES: AtomicU64 = AtomicU64::new(0);

/// Counts tile releases that failed where no caller could see the error, such as in `Drop`.
/// A failure means the ownership counts are wrong, so tests assert this stays 0.
pub fn release_failures() -> u64 {
    RELEASE_FAILURES.load(Ordering::Relaxed)
}

/// Releases one ownership whose failure no caller can handle. Debug builds panic; release
/// builds count it in `release_failures`.
fn release_owned(store: &TileStore, id: TileId) {
    let result = store.release(id);
    debug_assert!(
        result.is_ok(),
        "releasing owned tile {id:?} failed: {result:?}"
    );
    if result.is_err() {
        RELEASE_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
}

type PageCoord = (i32, i32);
struct Page {
    store: Arc<TileStore>,
    entries: BTreeMap<(u8, u8), TileId>,
}
impl Page {
    fn new(
        store: Arc<TileStore>,
        entries: BTreeMap<(u8, u8), TileId>,
    ) -> std::result::Result<Self, TileError> {
        let mut retained = Vec::new();
        for id in entries.values() {
            if let Err(e) = store.retain(*id) {
                for id in retained {
                    release_owned(&store, id);
                }
                return Err(e);
            }
            retained.push(*id);
        }
        Ok(Self { store, entries })
    }
}
impl Drop for Page {
    fn drop(&mut self) {
        for id in self.entries.values() {
            release_owned(&self.store, *id);
        }
    }
}

#[derive(Clone)]
pub struct LayerTiles {
    store: Arc<TileStore>,
    pages: BTreeMap<PageCoord, Arc<Page>>,
}
impl LayerTiles {
    pub fn new(store: Arc<TileStore>) -> Self {
        Self {
            store,
            pages: BTreeMap::new(),
        }
    }
    fn split(coord: TileCoord) -> (PageCoord, (u8, u8)) {
        (
            (coord.x.div_euclid(16), coord.y.div_euclid(16)),
            (coord.x.rem_euclid(16) as u8, coord.y.rem_euclid(16) as u8),
        )
    }
    pub fn get(&self, coord: TileCoord) -> Option<TileId> {
        let (page, local) = Self::split(coord);
        self.pages
            .get(&page)
            .and_then(|p| p.entries.get(&local))
            .copied()
    }
    pub fn iter(&self) -> impl Iterator<Item = (TileCoord, TileId)> + '_ {
        self.pages.iter().flat_map(|(&(px, py), page)| {
            page.entries.iter().map(move |(&(x, y), &id)| {
                (
                    TileCoord {
                        x: px * 16 + x as i32,
                        y: py * 16 + y as i32,
                    },
                    id,
                )
            })
        })
    }
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    pub fn shared_page_count(&self, other: &Self) -> usize {
        self.pages
            .iter()
            .filter(|(key, page)| {
                other
                    .pages
                    .get(key)
                    .is_some_and(|other| Arc::ptr_eq(page, other))
            })
            .count()
    }
    fn with_id(&self, coord: TileCoord, id: Option<TileId>) -> Result<Self> {
        let (key, local) = Self::split(coord);
        let mut entries = self
            .pages
            .get(&key)
            .map_or_else(BTreeMap::new, |p| p.entries.clone());
        match id {
            Some(id) => {
                entries.insert(local, id);
            }
            None => {
                entries.remove(&local);
            }
        }
        let mut result = self.clone();
        if entries.is_empty() {
            result.pages.remove(&key);
        } else {
            result
                .pages
                .insert(key, Arc::new(Page::new(self.store.clone(), entries)?));
        }
        Ok(result)
    }
    fn from_ids(
        store: Arc<TileStore>,
        entries: impl IntoIterator<Item = (TileCoord, TileId)>,
    ) -> Result<Self> {
        let mut groups: BTreeMap<PageCoord, BTreeMap<(u8, u8), TileId>> = BTreeMap::new();
        for (coord, id) in entries {
            let (page, local) = Self::split(coord);
            groups.entry(page).or_default().insert(local, id);
        }
        let mut pages = BTreeMap::new();
        for (key, entries) in groups {
            pages.insert(key, Arc::new(Page::new(store.clone(), entries)?));
        }
        Ok(Self { store, pages })
    }
}

#[derive(Clone)]
pub struct LayerState {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub blend: LayerBlendMode,
    pub tiles: LayerTiles,
}
impl LayerState {
    fn blank(id: LayerId, name: String, store: Arc<TileStore>) -> Self {
        Self {
            id,
            name,
            visible: true,
            opacity: 255,
            blend: LayerBlendMode::Normal,
            tiles: LayerTiles::new(store),
        }
    }
}

#[derive(Clone)]
pub struct DocState {
    pub width: u32,
    pub height: u32,
    pub dpi: f64,
    pub profile: Option<Arc<[u8]>>,
    pub layers: Vec<Arc<LayerState>>,
    pub active_layer: usize,
    pub selection: SelectionState,
    pub metadata: Arc<BTreeMap<String, String>>,
    pub revision: u64,
    pub selection_revision: u64,
    selection_cache: Selection,
    next_layer_id: Arc<AtomicU64>,
    store: Arc<TileStore>,
}
impl DocState {
    pub fn new(store: Arc<TileStore>, width: u32, height: u32, dpi: f64) -> Result<Self> {
        if width == 0 || height == 0 || !dpi.is_finite() || dpi <= 0.0 {
            return Err(DocError::Dimensions);
        }
        Ok(Self {
            width,
            height,
            dpi,
            profile: None,
            layers: vec![Arc::new(LayerState::blank(
                LayerId(1),
                "Background".into(),
                store.clone(),
            ))],
            active_layer: 0,
            selection: SelectionState::None,
            metadata: Arc::new(BTreeMap::new()),
            revision: 0,
            selection_revision: 0,
            selection_cache: Selection::none(),
            next_layer_id: Arc::new(AtomicU64::new(2)),
            store,
        })
    }
    fn advanced(&self) -> Self {
        let mut next = self.clone();
        next.revision = self.revision.saturating_add(1);
        next
    }
    fn layer(&self, index: usize) -> Result<&Arc<LayerState>> {
        self.layers.get(index).ok_or(DocError::LayerIndex)
    }
    fn update_layer(
        &self,
        index: usize,
        f: impl FnOnce(&mut LayerState) -> Result<()>,
    ) -> Result<Self> {
        let mut layer = (**self.layer(index)?).clone();
        f(&mut layer)?;
        let mut next = self.advanced();
        next.layers[index] = Arc::new(layer);
        Ok(next)
    }
    fn check_tile(&self, coord: TileCoord) -> Result<()> {
        if coord.x < 0
            || coord.y < 0
            || coord.x as u64 >= (self.width as u64).div_ceil(TILE_SIZE as u64)
            || coord.y as u64 >= (self.height as u64).div_ceil(TILE_SIZE as u64)
        {
            Err(DocError::TileBounds)
        } else {
            Ok(())
        }
    }
    pub fn write_tile(&self, index: usize, coord: TileCoord, mut data: TileData) -> Result<Self> {
        self.layer(index)?;
        self.check_tile(coord)?;
        let valid_width =
            (self.width as u64 - coord.x as u64 * TILE_SIZE as u64).min(TILE_SIZE as u64) as usize;
        let valid_height =
            (self.height as u64 - coord.y as u64 * TILE_SIZE as u64).min(TILE_SIZE as u64) as usize;
        if valid_width != TILE_SIZE || valid_height != TILE_SIZE {
            let mut bytes = data.as_bytes().to_vec();
            for y in 0..TILE_SIZE {
                let start = (y * TILE_SIZE + if y < valid_height { valid_width } else { 0 }) * 4;
                let end = (y + 1) * TILE_SIZE * 4;
                bytes[start..end].fill(0);
            }
            data = TileData::try_from(bytes)?;
        }
        if data.is_transparent_black() {
            return self.clear_tile(index, coord);
        }
        let id = self.store.insert(data)?;
        let result = self.update_layer(index, |layer| {
            layer.tiles = layer.tiles.with_id(coord, Some(id))?;
            Ok(())
        });
        self.store.release(id)?;
        result
    }
    pub fn clear_tile(&self, index: usize, coord: TileCoord) -> Result<Self> {
        self.layer(index)?;
        self.check_tile(coord)?;
        self.update_layer(index, |layer| {
            layer.tiles = layer.tiles.with_id(coord, None)?;
            Ok(())
        })
    }
    pub fn read_tile(&self, index: usize, coord: TileCoord) -> Result<Vec<u8>> {
        let layer = self.layer(index)?;
        self.check_tile(coord)?;
        match layer.tiles.get(coord) {
            Some(id) => Ok(self.store.get(id)?.to_vec()),
            None => Ok(vec![0; TILE_BYTES]),
        }
    }
    /// Makes sure later new layers get ids above `id`. Loaders call this for
    /// every layer id they restore, so a new layer never reuses a saved id.
    pub fn reserve_layer_id(&self, id: LayerId) {
        self.next_layer_id
            .fetch_max(id.0.saturating_add(1), Ordering::Relaxed);
    }
    pub fn add_layer(&self, index: usize, name: impl Into<String>) -> Result<Self> {
        if index > self.layers.len() {
            return Err(DocError::LayerIndex);
        }
        let mut next = self.advanced();
        let id = LayerId(
            self.next_layer_id
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| TileError::Overflow)?,
        );
        next.layers.insert(
            index,
            Arc::new(LayerState::blank(id, name.into(), self.store.clone())),
        );
        if index <= next.active_layer {
            next.active_layer += 1;
        }
        Ok(next)
    }
    pub fn delete_layer(&self, index: usize) -> Result<Self> {
        self.layer(index)?;
        if self.layers.len() == 1 {
            return Err(DocError::LastLayer);
        }
        let mut next = self.advanced();
        next.layers.remove(index);
        if next.active_layer > index {
            next.active_layer -= 1;
        } else if next.active_layer == index {
            next.active_layer = next.active_layer.min(next.layers.len() - 1);
        }
        Ok(next)
    }
    pub fn move_layer(&self, from: usize, to: usize) -> Result<Self> {
        self.layer(from)?;
        self.layer(to)?;
        let active_id = self.layers[self.active_layer].id;
        let mut next = self.advanced();
        let layer = next.layers.remove(from);
        next.layers.insert(to, layer);
        next.active_layer = next.layers.iter().position(|l| l.id == active_id).unwrap();
        Ok(next)
    }
    pub fn duplicate_layer(&self, index: usize) -> Result<Self> {
        let mut layer = (**self.layer(index)?).clone();
        let mut next = self.advanced();
        layer.id = LayerId(
            self.next_layer_id
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| TileError::Overflow)?,
        );
        layer.name.push_str(" copy");
        next.layers.insert(index + 1, Arc::new(layer));
        Ok(next)
    }
    pub fn set_layer_name(&self, index: usize, name: impl Into<String>) -> Result<Self> {
        self.update_layer(index, |l| {
            l.name = name.into();
            Ok(())
        })
    }
    pub fn set_layer_visible(&self, index: usize, visible: bool) -> Result<Self> {
        self.update_layer(index, |l| {
            l.visible = visible;
            Ok(())
        })
    }
    pub fn set_layer_opacity(&self, index: usize, opacity: u8) -> Result<Self> {
        self.update_layer(index, |l| {
            l.opacity = opacity;
            Ok(())
        })
    }
    pub fn set_layer_blend(&self, index: usize, blend: LayerBlendMode) -> Result<Self> {
        self.update_layer(index, |l| {
            l.blend = blend;
            Ok(())
        })
    }
    pub fn set_active_layer(&self, index: usize) -> Result<Self> {
        self.layer(index)?;
        let mut next = self.advanced();
        next.active_layer = index;
        Ok(next)
    }
    pub fn set_selection(&self, selection: SelectionState) -> Self {
        let mut next = self.advanced();
        next.selection_revision = self.selection_revision.saturating_add(1);
        next.selection_cache = Selection::from_revision(selection.clone(), next.selection_revision);
        next.selection = selection;
        next
    }
    pub fn rasterize_selection(&self, mode: RasterMode) -> Arc<MaskGrid> {
        self.selection_cache
            .rasterize(self.width, self.height, mode)
    }
    pub fn selection_contours(&self) -> Arc<PolygonSet> {
        self.selection_cache.contours(self.width, self.height)
    }
    pub fn set_profile(&self, profile: Option<Arc<[u8]>>) -> Self {
        let mut next = self.advanced();
        next.profile = profile;
        next
    }
    pub fn set_metadata(&self, key: String, value: String) -> Self {
        let mut next = self.advanced();
        Arc::make_mut(&mut next.metadata).insert(key, value);
        next
    }
    pub fn resize_canvas(
        &self,
        width: u32,
        height: u32,
        offset_x: i32,
        offset_y: i32,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(DocError::Dimensions);
        }
        let max_x = (width as u64).div_ceil(TILE_SIZE as u64);
        let max_y = (height as u64).div_ceil(TILE_SIZE as u64);
        let mut next = self.advanced();
        next.width = width;
        next.height = height;
        for (index, old) in self.layers.iter().enumerate() {
            let mut ids = Vec::new();
            let mut temporary = Vec::new();
            let build = (|| -> Result<LayerTiles> {
                for (coord, id) in old.tiles.iter() {
                    let x = coord.x as i64 + offset_x as i64;
                    let y = coord.y as i64 + offset_y as i64;
                    if x < 0 || y < 0 || x as u64 >= max_x || y as u64 >= max_y {
                        continue;
                    }
                    let new_coord = TileCoord {
                        x: x as i32,
                        y: y as i32,
                    };
                    let edge_x = (x as u64 + 1) * TILE_SIZE as u64 > width as u64;
                    let edge_y = (y as u64 + 1) * TILE_SIZE as u64 > height as u64;
                    if edge_x || edge_y {
                        let mut data = self.store.get(id)?.to_vec();
                        for py in 0..TILE_SIZE {
                            for px in 0..TILE_SIZE {
                                if x as u64 * TILE_SIZE as u64 + px as u64 >= width as u64
                                    || y as u64 * TILE_SIZE as u64 + py as u64 >= height as u64
                                {
                                    data[(py * TILE_SIZE + px) * 4..(py * TILE_SIZE + px) * 4 + 4]
                                        .fill(0);
                                }
                            }
                        }
                        if data.iter().any(|&b| b != 0) {
                            let fresh = self.store.insert(TileData::try_from(data)?)?;
                            temporary.push(fresh);
                            ids.push((new_coord, fresh));
                        }
                    } else {
                        ids.push((new_coord, id));
                    }
                }
                LayerTiles::from_ids(self.store.clone(), ids)
            })();
            // The pages built above own their tiles now; drop the insert ownership of each.
            let mut released = Ok(());
            for id in temporary {
                if let Err(error) = self.store.release(id)
                    && released.is_ok()
                {
                    released = Err(error);
                }
            }
            let tiles = build?;
            released?;
            let mut layer = (**old).clone();
            layer.tiles = tiles;
            next.layers[index] = Arc::new(layer);
        }
        Ok(next)
    }
    /// Pins and loads every tile of every layer that touches the rectangle. The tiles stay
    /// pinned, so the budget cannot evict them, until the returned view is dropped.
    /// On error nothing stays pinned.
    pub fn ensure_resident(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<ResidentView<'_>> {
        let mut view = ResidentView {
            state: self,
            tiles: Vec::new(),
        };
        if width == 0 || height == 0 || x >= self.width || y >= self.height {
            return Ok(view);
        }
        let x0 = x / TILE_SIZE as u32;
        let y0 = y / TILE_SIZE as u32;
        let x1 = (x as u64 + width as u64 - 1).min(self.width as u64 - 1) / TILE_SIZE as u64;
        let y1 = (y as u64 + height as u64 - 1).min(self.height as u64 - 1) / TILE_SIZE as u64;
        for (index, layer) in self.layers.iter().enumerate() {
            for ty in y0 as u64..=y1 {
                for tx in x0 as u64..=x1 {
                    let coord = TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    };
                    if let Some(id) = layer.tiles.get(coord) {
                        view.tiles.push((index, coord, id, self.store.get(id)?));
                    }
                }
            }
        }
        Ok(view)
    }
}

/// Keeps a viewport's tiles pinned and resident. Dropping it unpins them.
#[must_use = "dropping the view unpins its tiles at once"]
pub struct ResidentView<'a> {
    state: &'a DocState,
    tiles: Vec<(usize, TileCoord, TileId, TileRef<'a>)>,
}
impl<'a> ResidentView<'a> {
    /// The document state whose tiles this view holds.
    pub fn state(&self) -> &'a DocState {
        self.state
    }
    /// The number of pinned tiles. Transparent black tiles have no entry and are not counted.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
    /// The pixels of a pinned tile, or `None` if the tile is outside the view or empty.
    pub fn tile(&self, layer: usize, coord: TileCoord) -> Option<&[u8]> {
        self.tiles
            .iter()
            .find(|(index, at, _, _)| *index == layer && *at == coord)
            .map(|(_, _, _, tile)| &**tile)
    }
    /// The ids of the pinned tiles, in layer then row order.
    pub fn tile_ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.tiles.iter().map(|&(_, _, id, _)| id)
    }
}
