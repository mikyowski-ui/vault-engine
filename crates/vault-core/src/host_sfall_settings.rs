//! SPEC-024b §4.12: the words and functions Revision 4 promoted. Most set a
//! session setting ([`crate::sfall::SessionSettings`]), which starts at its
//! default on a new game and on each load, and is neither saved nor hashed
//! (Decision D-48).
use crate::{
    assets::WorldAssets,
    critical::CriticalEffect,
    host::WorldHost,
    host_sfall::SfallValue,
    sfall::{NO_STAT_MAX, SessionSettings},
    stats::Perk,
    tables::CampaignTables,
    world::ObjectRef,
};
use std::{collections::BTreeSet, sync::Arc};
use vault_formats::pro::BasicFlags;
use vault_vm::{
    Vm,
    sfall::{Class, SfallRow},
};

/// `get_tile_fid`'s FID form for a floor or roof index.
const TILE_FID: i32 = 0x0400_0000;
/// One map elevation's tile squares per row and the grid they cover.
const SQUARES_PER_ROW: i32 = 100;
/// An entry's seven values, in a value type's order.
const CRITICAL_VALUES: i32 = 7;

/// A critical table's changed entries, and the tables they replaced.
#[derive(Debug)]
pub(crate) struct CriticalOverlay {
    base: Option<Arc<CampaignTables>>,
    changed: BTreeSet<[i32; 4]>,
}

fn value_of(entry: &CriticalEffect, kind: i32) -> i32 {
    match kind {
        0 => entry.multiplier,
        1 => entry.flags as i32,
        2 => entry.save_stat,
        3 => entry.save_adjustment,
        4 => entry.failed_flags as i32,
        5 => entry.message,
        _ => entry.failed_message,
    }
}
fn set_value(entry: &mut CriticalEffect, kind: i32, value: i32) {
    match kind {
        0 => entry.multiplier = value,
        1 => entry.flags = value as u32,
        2 => entry.save_stat = value,
        3 => entry.save_adjustment = value,
        4 => entry.failed_flags = value as u32,
        5 => entry.message = value,
        _ => entry.failed_message = value,
    }
}
/// `[critter type, body part, level, value type]`, in range.
fn critical_index(index: [i32; 4]) -> Option<[usize; 4]> {
    let [matrix, part, level, kind] = index;
    let fits = (0..16).contains(&matrix)
        && (0..9).contains(&part)
        && (0..6).contains(&level)
        && (0..CRITICAL_VALUES).contains(&kind);
    fits.then_some([
        matrix as usize,
        part as usize,
        level as usize,
        kind as usize,
    ])
}

impl<A: WorldAssets> WorldHost<A> {
    /// A new game or a load: every setting returns to its default.
    pub(crate) fn sfall_reset_settings(&mut self) {
        self.world.session = SessionSettings::sfall();
        let player = self.world.player;
        if let Some(sheet) = self
            .world
            .get_mut(player)
            .and_then(|p| p.character.as_mut())
        {
            sheet.perk_levels.clear();
        }
        let overlay = self
            .promoted()
            .and_then(|promoted| promoted.critical.take());
        if let Some(overlay) = overlay {
            self.world.rules.tables = overlay.base;
        }
        self.sfall_reset_names();
    }
    /// How many critical entries a script changed.
    pub fn sfall_changed_criticals(&mut self) -> usize {
        self.promoted()
            .and_then(|promoted| promoted.critical.as_ref())
            .map_or(0, |overlay| overlay.changed.len())
    }
    fn critical_value(&self, index: [i32; 4]) -> Option<i32> {
        let [matrix, part, level, kind] = critical_index(index)?;
        let entry = self
            .world
            .rules
            .tables()
            .success_effect(matrix, part as u8, level)?;
        Some(value_of(&entry, kind as i32))
    }
    /// The world's tables with one value of one entry replaced. A copy is
    /// taken once, so the attack lookups read the change as they stand.
    fn write_critical(&mut self, index: [i32; 4], value: i32) -> bool {
        let Some([matrix, part, level, kind]) = critical_index(index) else {
            self.world.diagnostic(format!(
                "set_critical_table: {index:?} is outside the table"
            ));
            return false;
        };
        let base = self.world.rules.tables.clone();
        let tables = self
            .world
            .rules
            .tables
            .get_or_insert_with(|| Arc::new(CampaignTables::compiled().clone()));
        set_value(
            &mut Arc::make_mut(tables).critical_success[matrix][part][level],
            kind as i32,
            value,
        );
        if let Some(promoted) = self.promoted() {
            let overlay = promoted.critical.get_or_insert(CriticalOverlay {
                base,
                changed: BTreeSet::new(),
            });
            overlay.changed.insert(index);
        }
        true
    }
    fn reset_critical(&mut self, index: [i32; 4]) {
        let Some(promoted) = self.promoted() else {
            return;
        };
        let Some(overlay) = promoted.critical.as_mut() else {
            return;
        };
        if !overlay.changed.remove(&index) {
            return;
        }
        let base = overlay.base.clone();
        let [matrix, part, level, kind] = critical_index(index).expect("a changed entry fits");
        let original = base
            .as_deref()
            .unwrap_or_else(|| CampaignTables::compiled())
            .success_effect(matrix, part as u8, level)
            .map(|entry| value_of(&entry, kind as i32));
        if let (Some(value), Some(tables)) = (original, self.world.rules.tables.as_mut()) {
            set_value(
                &mut Arc::make_mut(tables).critical_success[matrix][part][level],
                kind as i32,
                value,
            );
        }
    }
    /// `get_tile_fid`: the data packs hex, elevation and mode.
    fn tile_fid(&self, data: i32) -> i32 {
        let data = data as u32;
        let hex = (data & 0x00ff_ffff) as i32;
        let elevation = ((data >> 24) & 0xf) as usize;
        let mode = data >> 28;
        if !(0..40000).contains(&hex) || elevation > 2 {
            return 0;
        }
        let square = (hex % 200) / 2 + SQUARES_PER_ROW * ((hex / 200) / 2);
        let Some(pair) = self.world.map.tile(elevation, square as usize) else {
            return 0;
        };
        match mode {
            0 => TILE_FID + i32::from(pair.floor()),
            1 => TILE_FID + i32::from(pair.roof()),
            2 => pair.word as i32,
            _ => 0,
        }
    }
    /// A word of §4.12, with its operands read by class, or `None` for
    /// another word.
    pub(crate) fn sfall_settings_word(
        &mut self,
        vm: &mut Vm,
        row: &'static SfallRow,
        args: &[SfallValue],
    ) -> Option<Option<SfallValue>> {
        let int = |index: usize| args[index].int();
        let index = || [int(0), int(1), int(2), int(3)];
        let session = &mut self.world.session;
        Some(match row.id {
            0x81a2 => {
                session.skill_cap = int(0).clamp(0, 300);
                None
            }
            0x81b7 => {
                if let Some(stat) = usize::try_from(int(0)).ok().filter(|stat| *stat < 35) {
                    session.stat_max.get_or_insert([NO_STAT_MAX; 35])[stat] = int(1);
                }
                None
            }
            0x822a => {
                let multiplier = args[0].number();
                if multiplier >= 0.0 {
                    session.map_time = multiplier;
                } else {
                    self.world
                        .diagnostic(format!("set_map_time_multi: {multiplier} is negative"));
                }
                None
            }
            0x8246 => {
                session.level_points = int(0).clamp(-100, 100);
                None
            }
            0x8247 => {
                session.perk_interval = int(0).max(0);
                None
            }
            0x817a => {
                if let Some(perk) = Perk::from_index(int(0)) {
                    let player = self.world.player;
                    let object = self.world.get_mut(player);
                    if let Some(sheet) = object.and_then(|object| object.character.as_mut()) {
                        sheet.perk_levels.insert(perk, int(1));
                    }
                }
                None
            }
            0x823a => Some(SfallValue::Int(self.tile_fid(int(0)))),
            0x81e2 => {
                let value = self.critical_value(index());
                if value.is_none() {
                    self.world
                        .diagnostic(format!("get_critical_table: {:?} is outside", index()));
                }
                Some(SfallValue::Int(value.unwrap_or(0)))
            }
            0x81e1 => {
                self.write_critical(index(), int(4));
                None
            }
            0x81e3 => {
                if critical_index(index()).is_none() {
                    self.world
                        .diagnostic(format!("reset_critical_table: {:?} is outside", index()));
                }
                self.reset_critical(index());
                None
            }
            0x8274 => Some(SfallValue::Int(i32::from(
                int(0) >= 0 && self.assets.art_exists(int(0) as u32),
            ))),
            0x827e => {
                self.sequence_step(vm, int(0));
                None
            }
            0x8273 => {
                let [script, tile, elevation, radius] = index();
                Some(SfallValue::Int(
                    self.create_spatial(script, tile, elevation, radius),
                ))
            }
            0x81f5 => Some(SfallValue::Int(self.script_line(int(0) as ObjectRef))),
            0x8172 => {
                self.set_world_position([int(0), int(1)]);
                None
            }
            0x8270 => {
                let (hex, elevation) = (int(0), int(1));
                let on_map = (0..40000).contains(&hex) && (0..=2).contains(&elevation);
                let objects = if on_map {
                    self.world.objects_on_hex(hex, elevation as u8)
                } else {
                    Vec::new()
                };
                let objects = objects
                    .into_iter()
                    .map(|object| SfallValue::Int(object as i32))
                    .collect();
                Some(SfallValue::Int(self.arrays().temp_list(objects)))
            }
            _ => return None,
        })
    }
    /// `reg_anim_callback`: a step of the sequence `vm` has open.
    fn sequence_step(&mut self, vm: &Vm, procedure: i32) {
        let object = self.self_object(vm);
        let open = self.building_program == Some(vm.id);
        let Some(sequence) = self.building_animation.as_mut().filter(|_| open) else {
            self.world
                .diagnostic("reg_anim_callback: no sequence is open".to_owned());
            return;
        };
        // A step belongs to the sequence's last object, or else the script's.
        let owner = sequence
            .commands
            .last()
            .and_then(|(_, values)| values.last().copied())
            .unwrap_or(object as i32);
        sequence
            .commands
            .push((0x827e, vec![vm.id as i32, procedure, owner]));
    }
    /// A `reg_anim_callback` step came up.
    pub(crate) fn sequence_callback(&mut self, program: u32, procedure: u32) {
        if let Err(error) = self.callback(program, procedure) {
            self.world.diagnostic(format!("reg_anim_callback: {error}"));
        }
    }
    /// `create_spatial`: the record's ID, as spatial records have no object
    /// here; 0 for an unknown script.
    fn create_spatial(&mut self, line: i32, tile: i32, elevation: i32, radius: i32) -> i32 {
        let file = line - 1;
        if self.assets.program(file).is_err()
            || !(0..40000).contains(&tile)
            || !(0..=2).contains(&elevation)
        {
            return 0;
        }
        match self
            .world
            .add_spatial_script(file, tile, elevation as u8, radius)
        {
            Ok(id) => id as i32,
            Err(error) => {
                self.world.diagnostic(format!("create_spatial: {error}"));
                0
            }
        }
    }
    /// `get_script`: one-based, 0 unscripted, -1 null (Decision D-54).
    fn script_line(&self, object: ObjectRef) -> i32 {
        if object == 0 {
            return -1;
        }
        self.world
            .script_for(object)
            .and_then(|sid| self.world.scripts.get(&sid))
            .map_or(0, |script| script.file + 1)
    }
    /// `set_world_map_pos`: no travel, time or encounter check.
    fn set_world_position(&mut self, point: [i32; 2]) {
        if let Some(state) = self.world.fallout2.as_deref_mut()
            && state.tables.worldmap.cell_of(point).is_some()
        {
            state.position = point;
        }
    }
    /// `set_scr_name` on `object`.
    pub(crate) fn set_script_name(&mut self, object: ObjectRef, name: &[u8]) {
        if object == 0 {
            return;
        }
        if name.is_empty() {
            let own = self.promoted().and_then(|p| p.names.remove(&object));
            if let (Some(own), Some(found)) = (own, self.world.get_mut(object)) {
                found.name = own;
            }
            return;
        }
        let Some(own) = self.world.get(object).map(|found| found.name.clone()) else {
            return;
        };
        let Some(promoted) = self.promoted() else {
            return;
        };
        if promoted.names.contains_key(&object) {
            return;
        }
        promoted.names.insert(object, own);
        if let Some(found) = self.world.get_mut(object) {
            found.name = name.to_vec();
        }
    }
    /// Leaving the map or loading a slot ends every `set_scr_name`.
    pub(crate) fn sfall_reset_names(&mut self) {
        let names = self
            .promoted()
            .map(|promoted| std::mem::take(&mut promoted.names));
        for (object, own) in names.into_iter().flatten() {
            if let Some(found) = self.world.get_mut(object) {
                found.name = own;
            }
        }
    }
    /// `unwield_slot`. Slot 0 is armor, 1 the right hand, 2 the left hand.
    fn unwield_slot(&mut self, critter: ObjectRef, slot: i32) {
        let flag = match slot {
            0 => BasicFlags::WORN,
            1 => BasicFlags::RIGHT_HAND,
            2 => BasicFlags::LEFT_HAND,
            _ => {
                self.world
                    .diagnostic(format!("unwield_slot: {slot} is no slot"));
                return;
            }
        };
        if self.world.get(critter).is_none() {
            self.world
                .diagnostic("unwield_slot: a null critter".to_owned());
            return;
        }
        let item = self.world.get(critter).and_then(|object| {
            object.inventory.iter().map(|(item, _)| *item).find(|item| {
                self.world
                    .get(*item)
                    .is_some_and(|held| held.raw.flags.contains(flag))
            })
        });
        let Some(item) = item else {
            return;
        };
        if let Err(error) = self.world.unequip(critter, item) {
            self.world.diagnostic(format!("unwield_slot: {error}"));
            return;
        }
        if slot == 0 {
            self.unworn_art(critter);
        }
    }
    /// `get_text_width`: the sum of the advances in the current font.
    fn text_width(&self, text: &[u8]) -> i32 {
        self.assets
            .font_number(self.library.font)
            .map_or(0, |font| font.text_width(text, false) as i32)
    }
    /// A function of §4.12 by name, or `None` for another. A function with no
    /// documented result gives `Some(None)`, which pushes int 0.
    pub(crate) fn sfall_settings_function(
        &mut self,
        name: &str,
        args: &[SfallValue],
    ) -> Option<Option<SfallValue>> {
        let int = |index: usize| {
            args.get(index)
                .and_then(|value| value.coerce(Class::I))
                .map(|value| value.int())
        };
        let text = |index: usize| match args.get(index) {
            Some(SfallValue::Text(text)) => Some(text.clone()),
            _ => None,
        };
        let toggle = int(0);
        let session = &mut self.world.session;
        Some(match name {
            "encounter_detection"
            | "npc_engine_level_up"
            | "remove_wm_town_names"
            | "set_fo1_hit_chance"
            | "set_rest_mode"
            | "set_car_intface_art"
            | "set_cursor_mode"
                if toggle.is_none() =>
            {
                self.world.diagnostic(format!("{name}: failed operand"));
                None
            }
            "encounter_detection" => {
                session.encounter_detection = toggle != Some(0);
                None
            }
            "npc_engine_level_up" => {
                session.npc_level_up = toggle != Some(0);
                None
            }
            "remove_wm_town_names" => {
                session.town_names = toggle == Some(0);
                None
            }
            "set_fo1_hit_chance" => {
                session.fo1_hit_chance = toggle != Some(0);
                None
            }
            "set_rest_mode" => {
                session.rest_mode = toggle.unwrap_or(0);
                None
            }
            "set_car_intface_art" => {
                if let Some(index) = toggle.filter(|index| *index >= 0) {
                    session.car_art = index;
                }
                None
            }
            "set_cursor_mode" => {
                if let Some(mode) = toggle.filter(|mode| (0..=2).contains(mode)) {
                    session.cursor_mode = Some(mode);
                }
                None
            }
            "set_reaction_thresholds" => {
                self.reaction_thresholds(int(0), int(1));
                None
            }
            "set_terrain_name" => {
                self.terrain_override(int(0), int(1), text(2));
                None
            }
            "car_gas_amount" => {
                let fuel = self.world.fallout2.as_deref().map_or(0, |s| s.car.fuel);
                Some(SfallValue::Int(fuel))
            }
            "get_text_width" => match text(0) {
                Some(text) => Some(SfallValue::Int(self.text_width(&text))),
                None => {
                    self.world
                        .diagnostic("get_text_width: failed operand".to_owned());
                    None
                }
            },
            "intface_redraw" | "tile_refresh_display" => None,
            "real_dude_obj" => Some(SfallValue::Int(self.world.player as i32)),
            "unwield_slot" => {
                match (int(0), int(1)) {
                    (Some(critter), Some(slot)) => self.unwield_slot(critter as ObjectRef, slot),
                    _ => self
                        .world
                        .diagnostic("unwield_slot: failed operand".to_owned()),
                }
                None
            }
            _ => return None,
        })
    }
    /// `set_reaction_thresholds`: each clamped, and good below neutral is
    /// ignored.
    fn reaction_thresholds(&mut self, neutral: Option<i32>, good: Option<i32>) {
        let (Some(neutral), Some(good)) = (neutral, good) else {
            self.world
                .diagnostic("set_reaction_thresholds: failed operand".to_owned());
            return;
        };
        let (neutral, good) = (neutral.clamp(-125, 125), good.clamp(-125, 125));
        if good < neutral {
            self.world
                .diagnostic("set_reaction_thresholds: good is below neutral".to_owned());
        } else {
            self.world.session.reaction = [neutral, good];
        }
    }
    /// `set_terrain_name`: x and y are cell coordinates.
    fn terrain_override(&mut self, x: Option<i32>, y: Option<i32>, name: Option<Vec<u8>>) {
        let (Some(x), Some(y), Some(name)) = (x, y, name) else {
            self.world
                .diagnostic("set_terrain_name: failed operand".to_owned());
            return;
        };
        let grid = self.world.fallout2.as_deref().map(|state| state.grid());
        let cell = usize::try_from(x).ok().zip(usize::try_from(y).ok());
        if let (Some([columns, rows]), Some((x, y))) = (grid, cell)
            && x < columns
            && y < rows
        {
            self.world.session.terrain_names.insert([x, y], name);
        }
    }
    /// `dialog_message`'s text as the reply: the shell's call, outside barter.
    pub fn show_script_reply(&mut self, text: Vec<u8>) -> Result<(), crate::WorldError> {
        self.world.dialogue.reply = Some(crate::dialogue::DialogueText::Literal(text));
        self.realize_dialogue()
    }
    /// `dialog_message`: the reply window, or the barter response area.
    pub(crate) fn dialog_message(&mut self, text: &[u8]) {
        if self.world.dialogue.open {
            self.world.dialogue.script_message = Some(text.to_vec());
        } else {
            self.world
                .diagnostic("dialog_message: no dialogue or barter is open".to_owned());
        }
    }
}
