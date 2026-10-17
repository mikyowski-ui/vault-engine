use crate::AppResult;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vault_core::{
    CharacterSheet, ObjectAction, Perk, WorldEffect, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
};
use vault_formats::{
    Font, FrmArchive, HexGrid, LocalDirProvider, LstKind, LstTable, MAX_ASSET_BYTES, MapFile,
    Palette, Vfs, VfsPath,
    pro::{BasicFlags, Category},
};
use vault_render::{
    IndexedImage, TextStyle,
    dialogue::{self, ChoiceHit},
    map::{MapArt, MapHover, MapView, pick_object, render_map},
};
use winit::keyboard::KeyCode;

const MAX_ART_BYTES: u64 = 256 * 1024 * 1024;
pub struct MapPreview {
    pub host: WorldHost<VfsWorldAssets>,
    pub palette: Palette,
    art: MapSprites,
    pub view: MapView,
    path: String,
    last_hover: Option<MapHover>,
    font: Option<Arc<Font>>,
    pub selected: Option<u32>,
    status: Vec<u8>,
    choices: Vec<ChoiceHit>,
    option_scroll: usize,
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
impl MapSprites {
    fn load(vfs: &LocalDirProvider, map: &MapFile) -> AppResult<Self> {
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
        Ok(art)
    }
}
impl MapPreview {
    pub fn load(
        vfs: Arc<LocalDirProvider>,
        path: &str,
        language: &str,
        character: Option<&str>,
        seed: i32,
    ) -> AppResult<Self> {
        let path = VfsPath::new(path)?;
        let palette = Palette::parse(&vfs.read_bounded("color.pal", MAX_ASSET_BYTES)?)?;
        let assets = VfsWorldAssets::new(vfs, language)?;
        let character = if let Some(path) = character {
            CharacterSheet::from_gcd(&assets.gcd(path)?, Default::default())
        } else {
            CharacterSheet::default()
        };
        let font = assets.font().ok();
        let mut host = WorldHost::new(WorldState::new(character, seed)?, assets);
        host.interactive_dialogue = true;
        host.load_campaign()?;
        host.load_map(path.as_str())?;
        let map = host.world.render_snapshot();
        let art = MapSprites::load(&host.assets.vfs, &map)?;
        let mut view = MapView::new(&map, 640, 380)?;
        view.grid = HexGrid::centered(host.world.camera_hex, 640, 380)?;
        view.elevation = host.world.elevation;
        tracing::info!(map=%path,objects=map.objects.total,"interactive map preview loaded");
        Ok(Self {
            host,
            palette,
            art,
            view,
            path: path.to_string(),
            last_hover: None,
            font,
            selected: None,
            status: Vec::new(),
            choices: Vec::new(),
            option_scroll: 0,
        })
    }
    fn drain_effects(&mut self) {
        while let Some(effect) = self.host.world.effects.pop_front() {
            match effect {
                WorldEffect::Message(text) => self.status = text,
                WorldEffect::Diagnostic(error) => {
                    tracing::warn!(%error,"world diagnostic");
                    self.status = error.into_bytes();
                }
                WorldEffect::DefaultAction { action, target } => {
                    let name = self
                        .host
                        .world
                        .get(target)
                        .map_or(b"object".as_slice(), |object| object.name.as_slice());
                    self.status = [format!("{action:?}: ").as_bytes(), name].concat();
                }
                other => tracing::debug!(?other, "host presentation request"),
            }
        }
    }
    pub fn image(&mut self) -> AppResult<IndexedImage> {
        self.drain_effects();
        let map = self.host.world.render_snapshot();
        if map.objects.elevations.iter().flatten().any(|object| {
            !object.flags.contains(BasicFlags::HIDDEN)
                && Category::from_pid(object.pid).is_ok_and(|category| {
                    !self
                        .art
                        .bindings
                        .contains_key(&(category, object.art_id, object.orientation))
                })
        }) {
            self.art = MapSprites::load(&self.host.assets.vfs, &map)?;
        }
        let mut image = render_map(&map, self.view, &self.art)?;
        for y in 0..image.height() {
            for x in 0..image.width() {
                let light = self.view.grid.pick_hex([x as i32, y as i32]).map_or(
                    self.host.world.ambient,
                    |hex| {
                        self.host.world.tile_light[self.view.elevation as usize][hex as usize]
                            .max(i64::from(self.host.world.ambient))
                            .min(65536) as i32
                    },
                );
                let index = (y * image.width() + x) as usize;
                let color = image.pixels()[index];
                if color != 0 {
                    image.pixels_mut()[index] = self
                        .palette
                        .intensity(color, ((light as u32 * 128) / 65536) as u8);
                }
            }
        }
        if self.host.world.dialogue.open {
            let font = self
                .font
                .as_ref()
                .ok_or("dialogue requires a loaded FON/AAF font")?;
            let layout = dialogue::compose(
                &image,
                &self.host.world.dialogue,
                font,
                &self.palette,
                &self.host.assets,
                self.host.world.perk(Perk::Empathy) > 0,
                self.option_scroll,
            )?;
            self.host.world.dialogue.page_offsets = layout.page_offsets;
            self.choices = layout.choices;
            return Ok(layout.image);
        }
        self.choices.clear();
        if let Some(font) = &self.font {
            let green = self.palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
            image.fill_rect([0, 350, 640, 30], 0);
            for (index, label) in [
                b"[L] Look".as_slice(),
                b"[D] Describe",
                b"[U] Use",
                b"[P] Pickup",
                b"[T] Talk",
            ]
            .into_iter()
            .enumerate()
            {
                let x = 8 + index as i32 * 126;
                image.fill_rect(
                    [x, 350, 118, 14],
                    self.palette.lookup_rgb5([0, 5, 0]).unwrap_or(0),
                );
                image.draw_text(
                    font,
                    &self.palette,
                    label,
                    [x + 3, 353],
                    112,
                    TextStyle {
                        foreground: green,
                        ..Default::default()
                    },
                );
            }
            image.draw_text(
                font,
                &self.palette,
                &self.status,
                [8, 365],
                624,
                TextStyle {
                    foreground: green,
                    ..Default::default()
                },
            );
        }
        Ok(image)
    }
    pub fn title(&self) -> String {
        format!(
            "Vault — {} — elevation {} — roofs {} — click objects; L/D/U/P/T actions",
            self.path,
            self.view.elevation,
            if self.view.roofs { "on" } else { "off" }
        )
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        if self.host.world.dialogue.open {
            let choice = match key {
                KeyCode::Digit1 => Some(Some(0)),
                KeyCode::Digit2 => Some(Some(1)),
                KeyCode::Digit3 => Some(Some(2)),
                KeyCode::Digit4 => Some(Some(3)),
                KeyCode::Digit5 => Some(Some(4)),
                KeyCode::Digit6 => Some(Some(5)),
                KeyCode::Digit7 => Some(Some(6)),
                KeyCode::Digit8 => Some(Some(7)),
                KeyCode::Digit9 => Some(Some(8)),
                KeyCode::Digit0 => Some(None),
                _ => None,
            };
            if let Some(index) = choice {
                if let Err(error) = self.host.choose_dialogue(index) {
                    self.status = error.to_string().into_bytes();
                }
                self.option_scroll = 0;
                return true;
            }
            match key {
                KeyCode::Escape => {
                    if let Err(error) = self.host.close_dialogue() {
                        self.status = error.to_string().into_bytes();
                    }
                    return true;
                }
                KeyCode::Space | KeyCode::ArrowDown => {
                    let dialog = &mut self.host.world.dialogue;
                    dialog.page =
                        (dialog.page + 1).min(dialog.page_offsets.len().saturating_sub(1));
                    dialog.page_ms = self.host.world.elapsed_ms;
                    return true;
                }
                KeyCode::ArrowUp => {
                    self.host.world.dialogue.page = self.host.world.dialogue.page.saturating_sub(1);
                    return true;
                }
                _ => return false,
            }
        }
        if self.host.world.ui_disabled {
            return false;
        }
        let key_number = match key {
            KeyCode::KeyL => Some(b'L'),
            KeyCode::KeyD => Some(b'D'),
            KeyCode::KeyU => Some(b'U'),
            KeyCode::KeyP => Some(b'P'),
            KeyCode::KeyT => Some(b'T'),
            _ => None,
        };
        if let Some(key) = key_number
            && self.host.key_callback(i32::from(key))
        {
            return true;
        }
        let action = match key {
            KeyCode::KeyL => Some(ObjectAction::LookAt),
            KeyCode::KeyD => Some(ObjectAction::Description),
            KeyCode::KeyU => Some(ObjectAction::Use),
            KeyCode::KeyP => Some(ObjectAction::Pickup),
            KeyCode::KeyT => Some(ObjectAction::Talk),
            _ => None,
        };
        if let Some(action) = action {
            self.interact(action);
            return true;
        }
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
        self.host.world.camera_hex = self
            .view
            .grid
            .pick_hex([320, 190])
            .unwrap_or(self.host.world.camera_hex);
        if self.host.world.elevation != self.view.elevation {
            self.host.world.elevation = self.view.elevation;
            if self.host.world.map_script.is_some()
                && let Err(error) = self.host.map_update()
            {
                self.status = error.to_string().into_bytes();
            }
        }
        true
    }
    fn interact(&mut self, action: ObjectAction) {
        if let Some(target) = self.selected {
            if let Err(error) = self.host.action(self.host.world.player, target, action) {
                self.status = error.to_string().into_bytes();
            }
        } else {
            self.status = b"Select an object first".to_vec();
        }
        self.drain_effects();
    }
    pub fn click(&mut self, point: [i32; 2], right: bool) -> bool {
        if self.host.world.dialogue.open {
            if let Some(choice) = self.choices.iter().find(|choice| choice.contains(point)) {
                if let Err(error) = self.host.choose_dialogue(Some(choice.index)) {
                    self.status = error.to_string().into_bytes();
                }
                self.option_scroll = 0;
                return true;
            }
            return false;
        }
        if self.host.world.ui_disabled {
            return false;
        }
        if (350..364).contains(&point[1]) {
            for (index, action) in [
                ObjectAction::LookAt,
                ObjectAction::Description,
                ObjectAction::Use,
                ObjectAction::Pickup,
                ObjectAction::Talk,
            ]
            .into_iter()
            .enumerate()
            {
                let x = 8 + index as i32 * 126;
                if (x..x + 118).contains(&point[0]) {
                    self.interact(action);
                    return true;
                }
            }
        }
        let map = self.host.world.render_snapshot();
        self.selected = pick_object(&map, self.view, &self.art, point)
            .and_then(|id| self.host.world.reference_for_id(id));
        if self.selected.is_some() {
            self.interact(if right {
                ObjectAction::Talk
            } else {
                ObjectAction::LookAt
            });
        }
        true
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        if !self.host.world.dialogue.open {
            return false;
        }
        if delta < 0.0 {
            self.option_scroll = (self.option_scroll + 1)
                .min(self.host.world.dialogue.options.len().saturating_sub(1));
        } else {
            self.option_scroll = self.option_scroll.saturating_sub(1);
        }
        true
    }
    pub fn tick(&mut self, now: u64) -> AppResult<bool> {
        let mut changed = match self.host.tick(now) {
            Ok(changed) => changed,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                tracing::warn!(%error,"world update failed");
                true
            }
        };
        if self.view.grid.pick_hex([320, 190]) != Some(self.host.world.camera_hex) {
            self.view.grid = HexGrid::centered(self.host.world.camera_hex, 640, 380)?;
            changed = true;
        }
        if self.view.elevation != self.host.world.elevation {
            self.view.elevation = self.host.world.elevation;
            changed = true;
        }
        if !self.host.world.effects.is_empty() {
            self.drain_effects();
            changed = true;
        }
        Ok(changed)
    }
    pub fn hover(&mut self, point: [i32; 2]) -> Option<MapHover> {
        if self.host.world.dialogue.open {
            return None;
        }
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
pub(crate) mod fixture;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/state/support.rs"]
mod state_fixture;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_preview_loads_all_categories_and_handles_camera_roofs_elevations() {
        let dir = tempfile::tempdir().unwrap();
        fixture::write_corpus(dir.path());
        let vfs = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let mut preview = MapPreview::load(vfs, r"MAPS\SYNTHETIC.MAP", "english", None, 1).unwrap();
        assert_eq!(preview.host.world.map.objects.total, 5);
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
    #[test]
    fn clicking_npc_renders_choices_executes_callbacks_and_returns_to_map() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        let vfs = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let mut preview = MapPreview::load(
            vfs,
            "maps/synthetic.map",
            "english",
            Some("premade/synthetic.gcd"),
            1,
        )
        .unwrap();
        preview.image().unwrap();
        assert!(preview.click([336, 170], false));
        let selected = preview.selected.unwrap();
        assert_eq!(
            preview.host.world.get(selected).unwrap().category(),
            Category::Critter
        );
        assert!(preview.host.library.palette_cycling);
        assert!(preview.click([336, 170], true));
        assert!(preview.host.world.dialogue.open && preview.host.world.dialogue.waiting);
        // The shell's cycling poll follows this flag (SPEC-001/SPEC-006).
        assert!(!preview.host.library.palette_cycling);
        let image = preview.image().unwrap();
        assert_eq!((image.width(), image.height()), (640, 480));
        assert_eq!(preview.choices.len(), 1);
        assert_eq!(preview.choices[0].rectangle[0..2], [127, 335]);
        assert!(preview.click([200, 338], false));
        assert_eq!(preview.host.world.dialogue.options[0].procedure, 0);
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Digit1));
        assert!(!preview.host.world.dialogue.open);
        assert!(preview.host.library.palette_cycling);
        assert_eq!(preview.image().unwrap().height(), 380);
        // The clickable action strip routes a real pickup into authoritative inventory.
        preview.click([320, 185], false);
        let item = preview.selected.unwrap();
        assert_eq!(
            preview.host.world.get(item).unwrap().category(),
            Category::Item
        );
        preview.click([400, 355], false);
        assert_eq!(
            preview.host.world.get(item).unwrap().owner,
            Some(preview.host.world.player)
        );
    }
}
