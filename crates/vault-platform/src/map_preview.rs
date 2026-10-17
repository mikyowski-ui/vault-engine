use crate::AppResult;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vault_formats::{
    FrmArchive, LocalDirProvider, LocalPrototypeCatalog, LstKind, LstTable, MAX_ASSET_BYTES,
    MapFile, Palette, Vfs, VfsPath,
    pro::{BasicFlags, Category},
};
use vault_render::{
    IndexedImage,
    map::{MapArt, MapHover, MapView, render_map},
};
use winit::keyboard::KeyCode;

const MAX_ART_BYTES: u64 = 256 * 1024 * 1024;
pub struct MapPreview {
    map: MapFile,
    pub palette: Palette,
    art: MapSprites,
    pub view: MapView,
    path: String,
    last_hover: Option<MapHover>,
}
struct MapSprites {
    bindings: BTreeMap<(Category, u32, u8), Arc<FrmArchive>>,
}
impl MapArt for MapSprites {
    fn frame(
        &self,
        category: Category,
        art_id: u32,
        orientation: u8,
        frame: usize,
    ) -> Option<(&vault_formats::frm::FrmFrame, [i16; 2])> {
        let archive = self.bindings.get(&(category, art_id, orientation))?;
        Some((
            archive
                .frame(orientation as usize, frame)
                .or_else(|| archive.frame(orientation as usize, 0))?,
            *archive.shifts.get(orientation as usize)?,
        ))
    }
}
impl MapPreview {
    pub fn load(vfs: &LocalDirProvider, path: &str) -> AppResult<Self> {
        let path = VfsPath::new(path)?;
        let palette = Palette::parse(&vfs.read_bounded("color.pal", MAX_ASSET_BYTES)?)?;
        let map = MapFile::parse(
            &vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?,
            &LocalPrototypeCatalog::new(vfs),
        )?;
        let mut needed = BTreeSet::new();
        for tiles in map.tiles.iter().flatten() {
            for tile in tiles {
                for (index, hidden) in [
                    (tile.floor(), tile.floor_hidden()),
                    (tile.roof(), tile.roof_hidden()),
                ] {
                    if index != 1 && !hidden {
                        needed.insert((Category::Tile, u32::from(index), 0_u8));
                    }
                }
            }
        }
        for object in map.objects.elevations.iter().flatten() {
            if !object.flags.contains(BasicFlags::HIDDEN) {
                needed.insert((
                    Category::from_pid(object.pid)?,
                    object.art_id,
                    object.orientation,
                ));
            }
        }
        let mut lists = BTreeMap::new();
        let mut archives: BTreeMap<String, Arc<FrmArchive>> = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        let mut bytes_loaded = 0_u64;
        for (category, fid, orientation) in needed {
            let folder = category.directory();
            if let std::collections::btree_map::Entry::Vacant(entry) = lists.entry(category) {
                let bytes =
                    vfs.read_bounded(&format!("art/{folder}/{folder}.lst"), MAX_ASSET_BYTES)?;
                entry.insert(LstTable::parse(&bytes, LstKind::Art)?);
            }
            let name = lists[&category]
                .art_id(fid)
                .and_then(|entry| entry.name.as_deref())
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    format!("art/{folder}/{folder}.lst has no art index {}", fid & 0xfff)
                })?;
            let name = std::str::from_utf8(name)?;
            let base = VfsPath::new(&format!("art/{folder}/{name}"))?.to_string();
            let resolved = if base.ends_with(".frm")
                || matches!(
                    VfsPath::new(&base)?.extension(),
                    Some("fr0" | "fr1" | "fr2" | "fr3" | "fr4" | "fr5")
                ) {
                base
            } else {
                // SPEC-001/003 do not define the critter animation suffix codec.
                // Choose a deterministic available representation, and report it.
                let split = format!("fr{orientation}");
                let path = vfs
                    .files()
                    .find(|path| {
                        path.as_str().starts_with(&base) && path.extension() == Some("frm")
                    })
                    .or_else(|| {
                        vfs.files().find(|path| {
                            path.as_str().starts_with(&base)
                                && path.extension() == Some(split.as_str())
                        })
                    })
                    .ok_or_else(|| format!("no sprite for art stem {base}"))?
                    .to_string();
                tracing::warn!(art_id=fid,stem=%base,sprite=%path,"animation filename encoding is unspecified; using available stem art");
                path
            };
            let archive = if let Some(archive) = archives.get(&resolved) {
                archive.clone()
            } else {
                let bytes = vfs.read_bounded(&resolved, MAX_ASSET_BYTES)?;
                bytes_loaded += bytes.len() as u64;
                if bytes_loaded > MAX_ART_BYTES {
                    return Err("map preview art exceeds 256 MiB limit".into());
                }
                let archive = Arc::new(
                    FrmArchive::parse(&bytes).map_err(|error| format!("{resolved}: {error}"))?,
                );
                archives.insert(resolved, archive.clone());
                archive
            };
            bindings.insert((category, fid, orientation), archive);
        }
        let art = MapSprites { bindings };
        for object in map.objects.elevations.iter().flatten() {
            let category = Category::from_pid(object.pid)?;
            if let Some(archive) = art
                .bindings
                .get(&(category, object.art_id, object.orientation))
                && object.frame >= archive.frame_count()
            {
                tracing::warn!(
                    object = object.id,
                    frame = object.frame,
                    "frame unavailable in preview representation; showing frame zero"
                );
            }
        }
        let view = MapView::new(&map, 640, 380)?;
        tracing::info!(map=%path,objects=map.objects.total,sprites=archives.len(),"map preview loaded");
        Ok(Self {
            map,
            palette,
            art,
            view,
            path: path.to_string(),
            last_hover: None,
        })
    }
    pub fn image(&self) -> AppResult<IndexedImage> {
        Ok(render_map(&self.map, self.view, &self.art)?)
    }
    pub fn title(&self) -> String {
        format!(
            "Vault — {} — elevation {} — roofs {} — arrows: pan, R: roofs, 0–2: elevation",
            self.path,
            self.view.elevation,
            if self.view.roofs { "on" } else { "off" }
        )
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::ArrowLeft => self.view.pan(32, 0),
            KeyCode::ArrowRight => self.view.pan(-32, 0),
            KeyCode::ArrowUp => self.view.pan(0, 24),
            KeyCode::ArrowDown => self.view.pan(0, -24),
            KeyCode::KeyR => self.view.roofs = !self.view.roofs,
            KeyCode::Digit0 | KeyCode::Numpad0 => self.view.elevation = 0,
            KeyCode::Digit1 | KeyCode::Numpad1 => self.view.elevation = 1,
            KeyCode::Digit2 | KeyCode::Numpad2 => self.view.elevation = 2,
            _ => return false,
        }
        self.last_hover = None;
        true
    }
    pub fn hover(&mut self, point: [i32; 2]) -> Option<MapHover> {
        let hover = self.view.hover(point);
        if self.last_hover == Some(hover) {
            return None;
        }
        self.last_hover = Some(hover);
        Some(hover)
    }
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_preview_loads_all_categories_and_handles_camera_roofs_elevations() {
        let dir = tempfile::tempdir().unwrap();
        fixture::write_corpus(dir.path());
        let vfs = LocalDirProvider::mount(dir.path()).unwrap();
        let mut preview = MapPreview::load(&vfs, r"MAPS\SYNTHETIC.MAP").unwrap();
        assert_eq!(preview.map.objects.total, 5);
        let image = preview.image().unwrap();
        for color in [7, 15, 16, 17, 18] {
            assert!(
                image.pixels().contains(&color),
                "missing category color {color}"
            );
        }
        assert_eq!(preview.hover([320, 190]).unwrap().hex, Some(20100));
        assert!(preview.hover([320, 190]).is_none());
        preview.step(KeyCode::KeyR);
        assert!(!preview.view.roofs);
        preview.step(KeyCode::ArrowRight);
        assert_eq!(preview.hover([288, 190]).unwrap().hex, Some(20100));
        for key in [KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2] {
            assert!(preview.step(key));
            preview.image().unwrap();
        }
        assert!(!preview.step(KeyCode::Space));
    }
}
