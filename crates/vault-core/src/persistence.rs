//! SPEC-008 working save state: the saved-map set (`MAPS/*.SAV`), the
//! automap database, the live-world area-map `.SAV` export (§2.9), and the
//! save components this engine carries without interpreting them.
use crate::{ObjectRef, WorldState, world::ScriptInstance};
use std::collections::BTreeMap;
use vault_formats::{
    AutomapDb, automap,
    map::{
        InventoryEntry, MapFile, MapObject, MapScript, ObjectState, ScriptBlock, ScriptCategory,
        ScriptPrefix,
    },
    pro::{BasicFlags, MapFlags, PackedLocation, ScriptFlags},
};

/// SPEC-014 §3.33 the visited set: 40000 hexes, 8 per byte.
pub const VISITED_BYTES: usize = 5000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Persistence {
    /// Working `MAPS/<NAME>.SAV` images keyed by uppercase file name.
    pub maps: BTreeMap<String, Vec<u8>>,
    /// The loaded map's working `.SAV` name, from its requested file name.
    pub current_map: Option<String>,
    /// Working `MAPS/AUTOMAP.DB`.
    pub automap: AutomapDb,
    /// Save payload 17 display flags.
    pub automap_flags: u32,
    /// Payload 2's water-warning/movie-status byte.
    pub campaign_status: u8,
    /// Preference words not modeled by world state.
    pub preferences: [u32; 20],
    pub movies: [u8; 14],
    pub skill_uses: [[i32; 3]; 18],
    pub interface: [i32; 4],
}
/// A map resource name's working `.SAV` file name: the uppercase stem of
/// the final path component plus `.SAV`.
pub fn saved_map_name(name: &[u8]) -> Option<String> {
    let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
    let file = std::str::from_utf8(name).ok()?.rsplit(['/', '\\']).next()?;
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    (!stem.is_empty() && stem.is_ascii()).then(|| format!("{}.SAV", stem.to_ascii_uppercase()))
}

impl WorldState {
    /// The next object identity the allocator will try (save payload 24).
    pub fn next_object_id(&self) -> u32 {
        self.next_uid
    }
    pub fn set_next_object_id(&mut self, id: u32) {
        self.next_uid = id.max(1);
    }
    /// One object's record with live HP/poison/radiation and money synced
    /// back into the serialized state.
    fn live_record(&self, reference: ObjectRef) -> Option<MapObject> {
        let object = self.get(reference)?;
        let mut raw = object.raw.clone();
        match &mut raw.state {
            ObjectState::Critter(state) => {
                if let Some(sheet) = &object.character {
                    state.hp = sheet.current_hp;
                    state.poison = sheet.poison;
                    state.radiation = sheet.radiation;
                }
            }
            ObjectState::Other { word, .. } => {
                if let Some(money) = object.money {
                    *word = money as u32;
                }
            }
        }
        Some(raw)
    }
    /// The live object tree rooted at `reference`, inventories nested
    /// depth-first in inventory order.
    pub fn object_tree(&self, reference: ObjectRef) -> Option<MapObject> {
        self.object_tree_at(reference, 0)
    }
    fn object_tree_at(&self, reference: ObjectRef, depth: usize) -> Option<MapObject> {
        if depth > vault_formats::map::MAX_INVENTORY_DEPTH {
            return None;
        }
        let mut raw = self.live_record(reference)?;
        raw.inventory = self
            .get(reference)?
            .inventory
            .iter()
            .filter_map(|(child, quantity)| {
                Some(InventoryEntry {
                    quantity: (*quantity).max(0) as u32,
                    object: self.object_tree_at(*child, depth + 1)?,
                })
            })
            .collect();
        raw.inventory_capacity = raw.inventory_capacity.max(raw.inventory.len());
        Some(raw)
    }
    fn script_record(&self, instance: &ScriptInstance, template: Option<&MapScript>) -> MapScript {
        let mut record = template.cloned().unwrap_or(MapScript {
            id: instance.id,
            link: -1,
            prefix: ScriptPrefix::None,
            stored_flags: ScriptFlags(0),
            flags: ScriptFlags(0),
            file_index: instance.file,
            object_id: -1,
            local_base: -1,
            stored_local_count: 0,
            local_count: 0,
            result: 0,
            event: 0,
            parameter: 0,
            action: 0,
            override_default: 0,
            execution_state: 0,
            quantity: 0,
            annotations: 0,
            file_offset: 0,
        });
        record.flags = ScriptFlags(instance.flags);
        record.stored_flags = record.flags;
        record.file_index = instance.file;
        record.object_id = self.get(instance.owner).map_or(-1, |o| o.raw.id as i32);
        record.local_base = instance.local_base;
        record.local_count = instance.local_count.min(i32::MAX as usize) as i32;
        record.stored_local_count = record.local_count;
        record.result = instance.result;
        record.event = instance.event;
        record.parameter = instance.fixed;
        record.action = instance.action;
        record.override_default = i32::from(instance.overridden);
        if let Some((hex, elevation, radius)) = instance.spatial {
            let unchanged = matches!(&record.prefix, ScriptPrefix::Spatial { location, .. }
                if location.hex() as i32 == hex && location.elevation() == elevation);
            if !unchanged {
                record.prefix = ScriptPrefix::Spatial {
                    location: PackedLocation(
                        (u32::from(elevation) << 29) | (hex as u32 & 0x03ff_ffff),
                    ),
                    radius,
                };
            } else if let ScriptPrefix::Spatial { radius: stored, .. } = &mut record.prefix {
                *stored = radius;
            }
        }
        record
    }
    /// SPEC-008 §2.9 area-map `.SAV` image of the current map: the saved
    /// bit set, live globals/locals, every script instance except the
    /// per-load map script, and placed top-level objects with nested
    /// inventories. The player (saved centrally) is excluded, as are party
    /// members unless `include_party` (they travel with the player between
    /// maps but are saved on the current map in a save slot).
    pub fn export_map(&self, include_party: bool) -> MapFile {
        let mut map = self.map.clone();
        map.header.flags = MapFlags(map.header.flags.0 | MapFlags::SAVED.0);
        map.mvars = self.mvars.clone();
        map.lvars = self.lvars.clone();
        let templates: BTreeMap<u32, &MapScript> = self
            .map
            .scripts
            .iter()
            .flat_map(ScriptCategory::active)
            .map(|script| (script.id, script))
            .collect();
        let mut records: [Vec<MapScript>; 5] = Default::default();
        for sid in &self.script_order {
            let Some(instance) = self.scripts.get(sid) else {
                continue;
            };
            let category = (*sid >> 24) as usize;
            if Some(*sid) == self.map_script || category >= 5 {
                continue;
            }
            records[category].push(self.script_record(instance, templates.get(sid).copied()));
        }
        map.scripts = records.map(|slots| ScriptCategory {
            declared_count: slots.len(),
            blocks: vec![ScriptBlock {
                active: slots.len(),
                slots,
            }],
        });
        map.objects.elevations = std::array::from_fn(|_| Vec::new());
        for (reference, object) in self.iter() {
            if reference == self.player
                || object.owner.is_some()
                || object.raw.hex.is_none()
                || object.raw.elevation > 2
                || (!include_party && self.party.contains(&reference))
            {
                continue;
            }
            if let Some(tree) = self.object_tree(reference) {
                map.objects.elevations[usize::from(object.raw.elevation)].push(tree);
            }
        }
        map.objects.total = map.objects.elevations.iter().map(Vec::len).sum();
        map
    }
    /// SPEC-014 §3.33: the player entered `hex`.
    pub fn mark_visited(&mut self, hex: i32) {
        if let Ok(hex) = usize::try_from(hex)
            && let Some(byte) = self.visited.get_mut(hex / 8)
        {
            *byte |= 1 << (hex % 8);
        }
    }
    /// SPEC-014 §3.33 seen-state processing (when the automap opens, before
    /// an automap-database write, and when the map's objects are saved):
    /// around every nonzero visited byte b, the bytes `b + 25k + d` for k
    /// -16 through 15 and d -2 through 2, within bytes 0 through 5000, are
    /// marked; every object on a hex of a marked byte and on the player's
    /// current elevation gets the seen flag for good. The visited set is
    /// then cleared. Bytes run on across map rows, so d near a row's start
    /// or end reaches the neighboring row.
    pub fn process_seen(&mut self) {
        if self.visited.iter().all(|byte| *byte == 0) {
            return;
        }
        let mut marked = vec![false; VISITED_BYTES + 1];
        for (byte, _) in self.visited.iter().enumerate().filter(|(_, b)| **b != 0) {
            for k in -16..=15_i64 {
                for d in -2..=2_i64 {
                    let at = byte as i64 + 25 * k + d;
                    if let Ok(at) = usize::try_from(at)
                        && let Some(slot) = marked.get_mut(at)
                    {
                        *slot = true;
                    }
                }
            }
        }
        let elevation = self
            .get(self.player)
            .map_or(self.elevation, |player| player.raw.elevation);
        let seen: Vec<_> = self
            .iter()
            .filter(|(_, object)| {
                object.owner.is_none()
                    && object.raw.elevation == elevation
                    && object
                        .raw
                        .hex
                        .and_then(|hex| usize::try_from(hex).ok())
                        .is_some_and(|hex| marked.get(hex / 8).copied().unwrap_or(false))
            })
            .map(|(reference, _)| reference)
            .collect();
        for reference in seen {
            if let Some(object) = self.get_mut(reference) {
                object.raw.flags.0 |= BasicFlags::SEEN.0;
            }
        }
        self.visited.fill(0);
    }
    /// The placed objects that automap extraction reads.
    fn automap_objects(&self) -> Vec<MapObject> {
        self.iter()
            .filter(|(_, object)| object.owner.is_none() && object.raw.hex.is_some())
            .map(|(_, object)| object.raw.clone())
            .collect()
    }
    /// The current map's live SPEC-008 §2.8 raster for `elevation`, as a
    /// capture would store it.
    pub fn automap_raster(&self, elevation: u8) -> Vec<u8> {
        automap::raster_from_objects(&self.automap_objects(), elevation)
    }
    /// Capture the current map's automap rasters into the working database
    /// for each elevation with tile data. Seen-object bookkeeping is taken
    /// from the objects' existing seen flags.
    pub fn capture_automap(&mut self) {
        for elevation in 0..3_u8 {
            self.capture_automap_at(elevation);
        }
    }
    /// Capture one elevation's automap raster, when it has tile data.
    pub fn capture_automap_at(&mut self, elevation: u8) {
        self.process_seen();
        let number = self.map.header.map_number;
        if !(0..automap::MAPS as i32).contains(&number)
            || self
                .map
                .tiles
                .get(usize::from(elevation))
                .is_none_or(Option::is_none)
        {
            return;
        }
        let raster = automap::raster_from_objects(&self.automap_objects(), elevation);
        if let Err(error) =
            self.persistence
                .automap
                .store(number as usize, usize::from(elevation), &raster)
        {
            self.diagnostic(format!("automap capture failed: {error}"));
        }
    }
    /// The current map's working `.SAV` name: its requested file name, else
    /// its header resource name.
    pub fn current_map_name(&self) -> Option<String> {
        self.persistence
            .current_map
            .clone()
            .or_else(|| saved_map_name(&self.map.header.name))
    }
    /// Write the current map into the working saved-map set and capture its
    /// automap for the player's elevation (SPEC-014 §3.35); a map without a
    /// usable resource name is not persisted. Returns false when the write
    /// failed.
    pub fn persist_current_map(&mut self, include_party: bool) -> bool {
        // The seen flags are saved with the objects.
        self.process_seen();
        let Some(name) = self.current_map_name() else {
            return true;
        };
        let written = match self.export_map(include_party).write() {
            Ok(bytes) => {
                self.persistence.maps.insert(name, bytes);
                true
            }
            Err(error) => {
                self.diagnostic(format!("saved map {name} not written: {error}"));
                false
            }
        };
        let elevation = self
            .get(self.player)
            .map_or(self.elevation, |player| player.raw.elevation);
        self.capture_automap_at(elevation);
        written
    }
    /// SPEC-014 §3.35 the save on leaving a map: a map whose number is in
    /// the SPEC-008 §2.8 excluded set (the desert and encounter maps) has
    /// its working copy deleted and writes nothing, so it is rebuilt from
    /// its `.MAP` on every visit; any other map is persisted. Returns false
    /// when the write failed.
    pub fn save_left_map(&mut self) -> bool {
        let excluded = usize::try_from(self.map.header.map_number)
            .is_ok_and(|number| automap::EXCLUDED.contains(&number));
        if excluded {
            if let Some(name) = self.current_map_name() {
                self.persistence.maps.remove(&name);
            }
            return true;
        }
        self.persist_current_map(false)
    }
}
