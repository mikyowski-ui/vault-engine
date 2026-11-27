//! Pre-rendered map backdrops in the shell (owner brief
//! `docs/prerendered-backdrop-cutaway.md`): the bound map's `.bkd` index,
//! read through the session's VFS when the map changes, and a cache of
//! decoded `.bkc` chunks that outlives single refreshes. The drawing and
//! the picking are `vault_render::map`'s; this module only hands each
//! refresh a [`BackdropView`] of the chunks under it.
//!
//! Nothing here touches the world: the index, the cache and their
//! diagnostics (`tracing` only, never a world diagnostic) change no
//! `state_hash`, save, RNG draw or checkpoint (brief §5.1), and the cache
//! never changes a canvas byte (§3.10.2).
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
    sync::Arc,
};
use vault_core::{
    WorldHost,
    assets::{VfsWorldAssets, WorldAssets},
};
use vault_formats::{
    MAX_ASSET_BYTES, Vfs, VfsError,
    backdrop::{self, BackdropChunk, BackdropIndex},
    pro::{PrototypeData, Scenery, SceneryData},
};
use vault_render::map::{BackdropTile, BackdropView, MapView, backdrop_chunks, backdrop_rect};

/// `backdrop_cache_mib`'s default: 64 MiB of decoded chunks (brief §3.10.2,
/// question 8).
pub(crate) const DEFAULT_CACHE_MIB: u32 = 64;
/// `backdrop_cache_mib`'s whole numbers.
pub(crate) const CACHE_MIB_RANGE: RangeInclusive<u32> = 1..=1024;

/// The bytes of `mib` mebibytes.
pub(crate) fn cache_bytes(mib: u32) -> usize {
    (mib as usize).saturating_mul(1024 * 1024)
}

/// The map a loaded index belongs to: the header's name and number (the
/// identity `MapSprites::bind` keys on) and the working file stem the
/// backdrop's paths are named after.
type MapKey = ([u8; 16], i32, Option<String>);
/// A cached chunk: (elevation, row, column), so the map's order is the
/// eviction tie order.
pub(crate) type ChunkKey = (u8, u16, u16);

/// The current map's index, with each elevation's suppressed PIDs sorted,
/// without door PIDs (brief question 6).
struct MapBackdrop {
    stem: String,
    index: BackdropIndex,
    pids: [Vec<u32>; 3],
}

/// One cached chunk and the refresh that last drew it.
struct Entry {
    chunk: Arc<BackdropChunk>,
    drawn: u64,
    bytes: usize,
}

/// Decoded chunks up to a byte budget, evicted least-recently-drawn, ties
/// by ascending (elevation, row, column). A chunk the current refresh uses
/// is never evicted during it, even past the budget.
#[derive(Default)]
struct ChunkCache {
    entries: BTreeMap<ChunkKey, Entry>,
    bytes: usize,
    /// The current refresh's number; each refresh takes a new one.
    stamp: u64,
}

impl ChunkCache {
    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
    /// The cached chunk at `key`, now drawn by the current refresh.
    fn touch(&mut self, key: ChunkKey) -> Option<Arc<BackdropChunk>> {
        let entry = self.entries.get_mut(&key)?;
        entry.drawn = self.stamp;
        Some(entry.chunk.clone())
    }
    /// Keep `chunk` at `key` as drawn by the current refresh, then evict
    /// down to `budget`.
    fn insert(&mut self, key: ChunkKey, chunk: Arc<BackdropChunk>, budget: usize) {
        let bytes = chunk.colour.len()
            + chunk.depth.as_ref().map_or(0, |depth| 2 * depth.len())
            + chunk.fade.as_ref().map_or(0, Vec::len);
        let entry = Entry {
            chunk,
            drawn: self.stamp,
            bytes,
        };
        if let Some(old) = self.entries.insert(key, entry) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
        self.evict(budget);
    }
    /// Evict least-recently-drawn chunks, ties by ascending key, until the
    /// cache fits `budget` or only the current refresh's chunks are left.
    fn evict(&mut self, budget: usize) {
        while self.bytes > budget {
            let stamp = self.stamp;
            let Some(victim) = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.drawn < stamp)
                .min_by_key(|(key, entry)| (entry.drawn, **key))
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&victim) {
                self.bytes -= entry.bytes;
            }
        }
    }
}

/// The session's backdrop state for one world: the bound map's index, the
/// chunk cache, and what was already reported.
pub(crate) struct Backdrops {
    /// The SPEC-021 §2.1 hash (FNV-1a 64) of the raw `color.pal` bytes the
    /// world read at setup; an index made for another palette is refused.
    palette_hash: Option<u64>,
    /// The cache's byte budget.
    budget: usize,
    key: Option<MapKey>,
    current: Option<Arc<MapBackdrop>>,
    cache: ChunkCache,
    /// Maps whose index was refused or whose door PIDs were dropped: each
    /// is reported once per session.
    refused: BTreeSet<String>,
    doors: BTreeSet<String>,
    /// Elevations a missing or malformed chunk disabled for the session
    /// (brief §4.9), by map stem.
    disabled: BTreeSet<(String, u8)>,
}

impl Default for Backdrops {
    fn default() -> Self {
        Self {
            palette_hash: None,
            budget: cache_bytes(DEFAULT_CACHE_MIB),
            key: None,
            current: None,
            cache: ChunkCache::default(),
            refused: BTreeSet::new(),
            doors: BTreeSet::new(),
            disabled: BTreeSet::new(),
        }
    }
}

/// The backdrop under one refresh: the index's placement for the view's
/// elevation and the decoded chunks the refresh draws, held so no eviction
/// can drop them while it draws.
pub(crate) struct Prepared {
    elevation: u8,
    origin_hex: i32,
    origin_px: [i32; 2],
    size: [u32; 2],
    walls: bool,
    map: Arc<MapBackdrop>,
    chunks: Vec<Arc<BackdropChunk>>,
}

impl Prepared {
    /// The chunks as the renderer takes them.
    pub(crate) fn tiles(&self) -> Vec<BackdropTile<'_>> {
        self.chunks
            .iter()
            .map(|chunk| BackdropTile {
                origin: chunk.origin,
                size: chunk.size,
                colour: &chunk.colour,
                depth: chunk.depth.as_deref(),
                fade: chunk.fade.as_deref(),
            })
            .collect()
    }
    /// The refresh's view over `tiles` ([`Self::tiles`]).
    pub(crate) fn view<'a>(&'a self, tiles: &'a [BackdropTile<'a>]) -> BackdropView<'a> {
        BackdropView {
            elevation: self.elevation,
            origin_hex: self.origin_hex,
            origin_px: self.origin_px,
            size: self.size,
            chunks: tiles,
            suppress_walls: self.walls,
            suppress_pids: self
                .map
                .pids
                .get(usize::from(self.elevation))
                .map_or(&[], Vec::as_slice),
        }
    }
}

/// Whether prototype `pid` is a door (scenery of the door subtype).
fn is_door(assets: &VfsWorldAssets, pid: u32) -> bool {
    assets.prototype(pid).is_ok_and(|prototype| {
        matches!(
            prototype.data,
            PrototypeData::Scenery(Scenery {
                data: SceneryData::Door { .. },
                ..
            })
        )
    })
}

impl Backdrops {
    /// A world's backdrops for the palette file read at setup.
    pub(crate) fn new(palette_bytes: &[u8]) -> Self {
        Self {
            palette_hash: Some(vault_core::state_hash::fnv1a64(palette_bytes)),
            ..Self::default()
        }
    }
    /// The cache's byte budget from now on.
    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
        // No refresh is drawing now, so no chunk is held.
        self.cache.stamp += 1;
        self.cache.evict(bytes);
    }
    /// The cached chunks' keys, ascending (tests).
    #[cfg(test)]
    pub(crate) fn cached(&self) -> Vec<ChunkKey> {
        self.cache.entries.keys().copied().collect()
    }
    /// Whether the bound map has a usable index (tests).
    #[cfg(test)]
    pub(crate) fn loaded(&self) -> bool {
        self.current.is_some()
    }
    /// Follow the world's map: a map not seen last time drops the cache and
    /// reads its index (absent: no backdrop, silently).
    pub(crate) fn sync(&mut self, host: &WorldHost<VfsWorldAssets>) {
        let world = &host.world;
        let stem = world.current_map_name().map(|name| {
            name.split_once('.')
                .map_or(name.as_str(), |(stem, _)| stem)
                .to_ascii_lowercase()
        });
        let key = (world.map.header.name, world.map.header.map_number, stem);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.cache.clear();
        self.current = key
            .2
            .clone()
            .filter(|stem| !stem.is_empty())
            .and_then(|stem| self.load(&host.assets, &stem))
            .map(Arc::new);
        self.key = Some(key);
    }
    /// One diagnostic per map, and the map draws without a backdrop
    /// (brief §4.8).
    fn refuse(&mut self, stem: &str, reason: String) {
        if self.refused.insert(stem.to_owned()) {
            tracing::warn!(map = stem, %reason, "map backdrop refused; drawing without it");
        }
    }
    /// The index of map `stem`, when it has one that is valid and made for
    /// the loaded palette.
    fn load(&mut self, assets: &VfsWorldAssets, stem: &str) -> Option<MapBackdrop> {
        let path = backdrop::index_path(stem);
        let bytes = match assets.vfs.read_bounded(&path, MAX_ASSET_BYTES) {
            Ok(bytes) => bytes,
            Err(VfsError::NotFound(_)) => return None,
            Err(error) => {
                self.refuse(stem, format!("{path}: {error}"));
                return None;
            }
        };
        let index = match BackdropIndex::parse(&bytes) {
            Ok(index) => index,
            Err(error) => {
                self.refuse(stem, format!("{path}: {error}"));
                return None;
            }
        };
        match self.palette_hash {
            Some(hash) if hash == index.palette_hash => {}
            Some(hash) => {
                self.refuse(
                    stem,
                    format!(
                        "{path}: palette hash {:016x} is not the loaded palette's {hash:016x}",
                        index.palette_hash
                    ),
                );
                return None;
            }
            None => {
                self.refuse(stem, format!("{path}: no palette to check it against"));
                return None;
            }
        }
        let mut pids: [Vec<u32>; 3] = Default::default();
        let mut doors = Vec::new();
        for record in &index.elevations {
            let Some(slot) = pids.get_mut(usize::from(record.elevation)) else {
                continue;
            };
            for &pid in record.suppress.pids() {
                if is_door(assets, pid) {
                    doors.push(pid);
                } else {
                    slot.push(pid);
                }
            }
            slot.sort_unstable();
            slot.dedup();
        }
        if !doors.is_empty() && self.doors.insert(stem.to_owned()) {
            doors.sort_unstable();
            doors.dedup();
            let doors: Vec<String> = doors.iter().map(|pid| format!("{pid:#010x}")).collect();
            tracing::warn!(
                map = stem,
                doors = %doors.join(", "),
                "map backdrop lists door PIDs to suppress; the doors still draw"
            );
        }
        Some(MapBackdrop {
            stem: stem.to_owned(),
            index,
            pids,
        })
    }
    /// The backdrop under one refresh of `view`: every present chunk under
    /// it, decoded now if it is not cached (never skipped, brief §5.4).
    /// `None` without a backdrop for the view's elevation, or when a
    /// present chunk is missing or malformed: that disables the elevation
    /// for the session, with one diagnostic, and it then draws without
    /// backdrop and without suppression (brief §4.9).
    pub(crate) fn prepare(&mut self, vfs: &dyn Vfs, view: &MapView) -> Option<Prepared> {
        let map = self.current.clone()?;
        let elevation = view.elevation;
        if self.disabled.contains(&(map.stem.clone(), elevation)) {
            return None;
        }
        let record = map.index.elevation(elevation)?;
        let chunk_size = map.index.chunk_size;
        self.cache.stamp += 1;
        let mut chunks = Vec::new();
        let rect = backdrop_rect(view, record.origin_hex, record.origin_px, record.size);
        for [column, row] in rect
            .into_iter()
            .flat_map(|rect| backdrop_chunks(rect, u32::from(chunk_size)))
        {
            let (Ok(column), Ok(row)) = (u16::try_from(column), u16::try_from(row)) else {
                continue;
            };
            if !record.is_present(column, row) {
                continue;
            }
            let key = (elevation, row, column);
            if let Some(chunk) = self.cache.touch(key) {
                chunks.push(chunk);
                continue;
            }
            let path = backdrop::chunk_path(&map.stem, elevation, column, row);
            let decoded = vfs
                .read_bounded(&path, MAX_ASSET_BYTES)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    BackdropChunk::parse(&bytes, &map.index, elevation)
                        .map_err(|error| error.to_string())
                })
                .and_then(|chunk| {
                    // A file that names another chunk would draw elsewhere.
                    let expected = record.chunk_rect(chunk_size, column, row);
                    if expected == Some((chunk.origin, chunk.size)) {
                        Ok(chunk)
                    } else {
                        Err(format!("the file holds another chunk than {column}_{row}"))
                    }
                });
            match decoded {
                Ok(chunk) => {
                    let chunk = Arc::new(chunk);
                    self.cache.insert(key, chunk.clone(), self.budget);
                    chunks.push(chunk);
                }
                Err(error) => {
                    self.disabled.insert((map.stem.clone(), elevation));
                    tracing::warn!(
                        map = %map.stem,
                        elevation,
                        chunk = %path,
                        %error,
                        "map backdrop chunk unusable; the elevation draws without its backdrop"
                    );
                    return None;
                }
            }
        }
        Some(Prepared {
            elevation,
            origin_hex: record.origin_hex,
            origin_px: record.origin_px,
            size: record.size,
            walls: record.suppress.walls(),
            map,
            chunks,
        })
    }
}

#[cfg(test)]
#[path = "backdrop_tests.rs"]
mod tests;
