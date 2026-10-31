//! SPEC-024b §4.11: the words and functions the mod audit put first, which
//! Revision 3 promoted to tier P1. Everything here that scripts rebuild
//! ([`Promoted`]) is neither saved nor hashed (§5.9). The next unique ID and
//! the pending forced encounter are saved, so they live in the world
//! ([`crate::sfall::SfallWorld`]).
use crate::{
    assets::WorldAssets,
    critical::hit_mode,
    fallout2::Selected,
    host::WorldHost,
    host_sfall::SfallValue,
    progression::LEVEL_UP_FLAG,
    sfall::{UNIQUE_IDS, encounter_flag},
    skills,
    world::ObjectRef,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vault_formats::{
    msg::MsgCatalog,
    pro::{Category, ItemData, PrototypeData},
    vfs::VfsPath,
};
use vault_vm::{
    Vm, VmError, VmStatus,
    sfall::{self, SfallRow, Tier},
};

/// §4.11 the file of each `message_str_game` ID from 0.
const GAME_FILES: [&str; 21] = [
    "combat", "combatai", "scrname", "misc", "custom", "inventry", "item", "lsgame", "map",
    "options", "perk", "pipboy", "quests", "proto", "script", "skill", "skilldex", "stat", "trait",
    "worldmap", "editor",
];
/// §4.11 the file of each ID from `0x1000`.
const PROTO_FILES: [&str; 6] = [
    "pro_item", "pro_crit", "pro_scen", "pro_wall", "pro_tile", "pro_misc",
];
const FIRST_LISTED_FILE: i32 = 0x2000;
const FIRST_ADDED_FILE: i32 = 0x3000;
const LAST_ADDED_FILE: i32 = 0x3fff;
/// §4.11 `BoxBarCount` when `ddraw.ini` does not set it.
const CUSTOM_BOXES: i32 = 5;
const FIRST_CUSTOM_TAG: i32 = 5;
/// §4.11 the box tags, from SPEC-019 §3.5.2's boxes.
const SNEAK_TAG: i32 = 0;
const POISONED_TAG: i32 = 1;
const RADIATED_TAG: i32 = 2;
const LEVEL_TAG: i32 = 3;
const ADDICT_TAG: i32 = 4;
/// §4.11 both colors of a message box, when the script gives none.
const BOX_COLOR: i32 = 145;
/// §4.11 a message box's flags when they are absent or -1.
const BOX_FLAGS: i32 = 0x11;
/// The VM's wait predicate of a program a message box holds.
pub(crate) const BOX_WAIT: u32 = 3;
/// §4.11 `create_win`'s flags when it is called without them.
const WINDOW_FLAGS: i32 = 0x4;

/// One virtual file: its canonical path, bytes and position.
#[derive(Debug)]
struct VirtualFile {
    path: String,
    bytes: Vec<u8>,
    position: usize,
}

/// §4.11 a message box waiting for its closing key. The shell shows it, and
/// answers with [`WorldHost::sfall_close_message_box`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBox {
    program: u32,
    /// The lines the text's `\n` separates.
    pub lines: Vec<Vec<u8>>,
    pub flags: i32,
    /// The first line's palette index, and the other lines'.
    pub colors: [i32; 2],
}

/// The state of the promoted rows that scripts rebuild.
#[derive(Debug, Default)]
pub(crate) struct Promoted {
    /// §4.11 virtual files: handle `n` is entry `n - 1`.
    files: Vec<VirtualFile>,
    /// `add_extra_msg_file`'s table: lowercased name and catalog.
    added: Vec<(String, Arc<MsgCatalog>)>,
    /// `ExtraGameMsgFileList`, once `ddraw.ini` was read.
    listed: Option<BTreeMap<i32, String>>,
    /// The interface tags' script flags.
    tags: BTreeSet<i32>,
    hero_style: i32,
    message_box: Option<MessageBox>,
}

/// `name` with the extension of a message file.
fn message_file(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".msg") {
        name.to_owned()
    } else {
        format!("{name}.msg")
    }
}
/// §4.11 `ExtraGameMsgFileList`: consecutive IDs from `0x2000`, and an entry
/// `name:n` takes `0x2000 + n` with the entries after it following on.
fn listed_files(list: &str) -> BTreeMap<i32, String> {
    let mut files = BTreeMap::new();
    let mut next = FIRST_LISTED_FILE;
    for entry in list.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let (name, id) = match entry.split_once(':') {
            Some((name, number)) => (
                name.trim(),
                number
                    .trim()
                    .parse::<i32>()
                    .ok()
                    .and_then(|number| FIRST_LISTED_FILE.checked_add(number))
                    .unwrap_or(next),
            ),
            None => (entry, next),
        };
        files.insert(id, message_file(name));
        next = id.saturating_add(1);
    }
    files
}
fn canonical(path: &[u8]) -> Option<String> {
    let path = std::str::from_utf8(path).ok()?;
    Some(VfsPath::new(path).ok()?.as_str().to_owned())
}

impl<A: WorldAssets> WorldHost<A> {
    fn promoted(&mut self) -> Option<&mut Promoted> {
        self.sfall.as_deref_mut().map(|sfall| &mut sfall.promoted)
    }
    /// A new game or a load: the tables that clear (§4.11).
    pub(crate) fn sfall_clear_tables(&mut self) {
        let Some(promoted) = self.promoted() else {
            return;
        };
        let files = std::mem::take(&mut promoted.files);
        promoted.added.clear();
        promoted.tags.clear();
        promoted.message_box = None;
        for file in files {
            self.assets.set_virtual_file(&file.path, None);
        }
    }
    /// A word of §4.11, with its operands read by class.
    pub(crate) fn sfall_promoted(
        &mut self,
        vm: &mut Vm,
        row: &'static SfallRow,
        args: &[SfallValue],
    ) -> Result<Option<SfallValue>, VmError> {
        let int = |index: usize| args[index].int();
        Ok(match row.id {
            0x8162 => {
                self.tap_key(int(0));
                None
            }
            0x8171 => {
                self.force_encounter(int(0), 0);
                None
            }
            0x81b6 => {
                self.set_car_town(int(0));
                None
            }
            0x81dc | 0x81dd => {
                self.set_tag(int(0), row.id == 0x81dc);
                None
            }
            0x81de => Some(SfallValue::Int(i32::from(self.tag_lit(int(0))))),
            0x81f8 => {
                let (path, source) = (args[0].bytes(), args[1].bytes());
                Some(SfallValue::Int(self.fs_copy(path, source)))
            }
            0x81fb => {
                let Some(file) = self.virtual_file(int(0)) else {
                    return Ok(self.sfall_failed(row));
                };
                let at = file.position;
                let written = at
                    .checked_add(2)
                    .and_then(|end| file.bytes.get_mut(at..end))
                    .map(|bytes| bytes.copy_from_slice(&(int(1) as u16).to_be_bytes()));
                if written.is_none() {
                    self.short_diagnostic(row);
                    return Ok(None);
                }
                file.position += 2;
                let (path, bytes) = (file.path.clone(), file.bytes.clone());
                self.assets.set_virtual_file(&path, Some(bytes));
                None
            }
            0x8202 => {
                let Some(file) = self.virtual_file(int(0)) else {
                    return Ok(self.sfall_failed(row));
                };
                let position = int(1);
                match usize::try_from(position)
                    .ok()
                    .filter(|at| *at <= file.bytes.len())
                {
                    Some(position) => file.position = position,
                    None => self
                        .world
                        .diagnostic(format!("fs_seek: {position} is outside the file")),
                }
                None
            }
            0x820a => {
                let Some(file) = self.virtual_file(int(0)) else {
                    return Ok(self.sfall_failed(row));
                };
                let at = file.position;
                let read = at
                    .checked_add(2)
                    .and_then(|end| file.bytes.get(at..end))
                    .map(|bytes| i16::from_be_bytes([bytes[0], bytes[1]]));
                let Some(value) = read else {
                    self.short_diagnostic(row);
                    return Ok(Some(SfallValue::Int(0)));
                };
                file.position += 2;
                Some(SfallValue::Int(i32::from(value)))
            }
            0x8215 => {
                if let Some(promoted) = self.promoted() {
                    promoted.hero_style = int(0);
                }
                None
            }
            0x8220 => Some(SfallValue::Int(640)),
            0x8221 => Some(SfallValue::Int(480)),
            0x8227 => {
                let mode = if self.world.active_hand == 0 {
                    hit_mode::LEFT_PRIMARY
                } else {
                    hit_mode::RIGHT_PRIMARY
                };
                let active = self.world.wielded(self.world.player, mode);
                self.apply_figure_art(active);
                None
            }
            0x8229 => {
                self.force_encounter(int(0), int(1) as u32);
                None
            }
            0x8261 => Some(self.explosions(vm, int(0), int(1))),
            0x826b => Some(self.game_message(int(0), int(1))),
            0x8271 => {
                let members = self.party_entries(int(0));
                Some(SfallValue::Int(self.arrays().temp_list(members)))
            }
            _ => {
                self.stub_diagnostic(vm, row.name);
                sfall::stub_default(row)
            }
        })
    }
    /// A function of §4.11 by name, or `None` for another.
    pub(crate) fn sfall_promoted_function(
        &mut self,
        name: &str,
        args: &[SfallValue],
    ) -> Option<Option<SfallValue>> {
        Some(match name {
            "get_terrain_name" => {
                let point = (args.len() >= 2).then(|| [args[0].int(), args[1].int()]);
                Some(self.terrain_name(point))
            }
            "add_extra_msg_file" => match args.first() {
                Some(SfallValue::Text(file)) => Some(SfallValue::Int(self.add_message_file(file))),
                _ => {
                    self.world
                        .diagnostic("add_extra_msg_file: failed operand".to_owned());
                    None
                }
            },
            "create_win" => {
                self.create_window(args);
                None
            }
            "dialog_obj" => {
                let dialogue = &self.world.dialogue;
                Some(SfallValue::Int(if dialogue.open {
                    dialogue.target as i32
                } else {
                    0
                }))
            }
            "signal_close_game" => {
                self.signal_close_game();
                None
            }
            _ => return None,
        })
    }
    fn virtual_file(&mut self, handle: i32) -> Option<&mut VirtualFile> {
        let index = usize::try_from(handle.checked_sub(1)?).ok()?;
        self.promoted()?.files.get_mut(index)
    }
    fn short_diagnostic(&mut self, row: &SfallRow) {
        self.world
            .diagnostic(format!("{}: fewer than 2 bytes are left", row.name));
    }
    /// `fs_copy`: the source from a virtual file, else through the `Vfs`.
    /// An existing path keeps its handle, and returns to position 0.
    fn fs_copy(&mut self, path: &[u8], source: &[u8]) -> i32 {
        let (Some(path), Some(source)) = (canonical(path), canonical(source)) else {
            return -1;
        };
        let held = self.promoted().and_then(|promoted| {
            promoted
                .files
                .iter()
                .find(|file| file.path == source)
                .map(|file| file.bytes.clone())
        });
        let Some(bytes) = held.or_else(|| self.assets.resource(&source).ok()) else {
            return -1;
        };
        self.assets.set_virtual_file(&path, Some(bytes.clone()));
        let Some(promoted) = self.promoted() else {
            return -1;
        };
        let at = promoted.files.iter().position(|file| file.path == path);
        let index = at.unwrap_or_else(|| {
            promoted.files.push(VirtualFile {
                path,
                bytes: Vec::new(),
                position: 0,
            });
            promoted.files.len() - 1
        });
        let file = &mut promoted.files[index];
        file.bytes = bytes;
        file.position = 0;
        index as i32 + 1
    }
    /// `tap_key`: queued for the next Input phase ([`Self::sfall_take_keys`]).
    fn tap_key(&mut self, scan: i32) {
        if !(1..=255).contains(&scan) {
            self.world
                .diagnostic(format!("tap_key: {scan} is outside 1 to 255"));
            return;
        }
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.key_queue.push_back(scan);
        }
    }
    /// The keys `tap_key` queued: the shell presses and releases them as
    /// logical keys at the start of the Input phase, after that step's input
    /// records. They are not logged, because a replay queues them again.
    pub fn sfall_take_keys(&mut self) -> Vec<i32> {
        self.sfall
            .as_deref_mut()
            .map_or(Vec::new(), |sfall| sfall.key_queue.drain(..).collect())
    }
    /// `set_car_current_town`: with or without a car.
    fn set_car_town(&mut self, town: i32) {
        let Some(state) = self.world.fallout2.as_deref_mut() else {
            return;
        };
        if state.tables.city.get(town).is_some() {
            state.car.area = town;
        } else {
            self.world
                .diagnostic(format!("set_car_current_town: {town} is no area"));
        }
    }
    /// `force_encounter` and `force_encounter_with_flags`.
    fn force_encounter(&mut self, map: i32, flags: u32) {
        let known = self
            .world
            .fallout2
            .as_deref()
            .is_some_and(|state| state.tables.maps.get(map).is_some());
        if !known {
            self.world
                .diagnostic(format!("force_encounter: map {map} has no row"));
            return;
        }
        if let Some(state) = self.world.sfall.as_deref_mut() {
            let locked = state.forced_map != -1 && state.forced_flags & encounter_flag::LOCK != 0;
            if !locked {
                state.forced_map = map;
                state.forced_flags = flags;
            }
        }
    }
    /// The pending forced encounter, which replaces SPEC-018 §5.6's steps 1
    /// to 8 at this check and clears: no draw, no counter, no hook.
    pub(crate) fn take_forced_encounter(&mut self) -> Option<Selected> {
        let state = self.world.sfall.as_deref_mut()?;
        let map = u32::try_from(state.forced_map).ok()?;
        let flags = std::mem::take(&mut state.forced_flags);
        state.forced_map = -1;
        Some(Selected {
            table: 0,
            entry: 0,
            map,
            special: flags & encounter_flag::ICON_SP != 0,
            forced: Some(flags),
        })
    }
    fn terrain_name(&mut self, point: Option<[i32; 2]>) -> SfallValue {
        let index = self.world.fallout2.as_deref().and_then(|state| {
            let position = point.unwrap_or(state.position);
            let cell = state.tables.worldmap.cell_of(position)?;
            Some(state.subtile(cell).terrain as i32)
        });
        let text = index.and_then(|index| {
            let catalog = self.assets.game_messages("worldmap.msg").ok()?;
            catalog
                .get(1000 + index)
                .map(|message| message.text.clone())
        });
        SfallValue::Text(text.unwrap_or_default())
    }
    /// `metarule2_explosions`: sub-function 6 is P1, and the rest are stubs.
    fn explosions(&mut self, vm: &Vm, sub: i32, pid: i32) -> SfallValue {
        let row = sfall::SFALL_EXPLOSIONS
            .iter()
            .find(|row| i32::from(row.id) == sub);
        match row {
            Some(row) if row.tier == Tier::P1 => {
                let [low, high] = self.explosive_damage(pid as u32);
                let damage = vec![SfallValue::Int(low), SfallValue::Int(high)];
                SfallValue::Int(self.arrays().temp_list(damage))
            }
            Some(row) => {
                self.stub_diagnostic(vm, row.name);
                SfallValue::Int(0)
            }
            None => {
                self.world
                    .diagnostic(format!("metarule2_explosions: no sub-function {sub}"));
                SfallValue::Int(0)
            }
        }
    }
    fn explosive_damage(&mut self, pid: u32) -> [i32; 2] {
        match pid {
            skills::DYNAMITE | skills::DYNAMITE_ARMED => [30, 50],
            skills::PLASTIC | skills::PLASTIC_ARMED => [40, 80],
            _ => match self
                .assets
                .prototype(pid)
                .as_deref()
                .map(|proto| &proto.data)
            {
                Ok(PrototypeData::Item(item)) => match &item.data {
                    ItemData::Weapon(weapon) => weapon.damage,
                    _ => [0, 0],
                },
                _ => [0, 0],
            },
        }
    }
    /// `party_member_list`: 0 gives alive critters without the hidden flag,
    /// and any other value every entry.
    fn party_entries(&self, include: i32) -> Vec<SfallValue> {
        let shown = |member: &ObjectRef| {
            self.world.get(*member).is_some_and(|object| {
                object.category() == Category::Critter
                    && !self.world.is_dead(*member)
                    && object.raw.flags.0 & 0x01 == 0
            })
        };
        self.world
            .party_members()
            .into_iter()
            .filter(|member| include != 0 || shown(member))
            .map(|member| SfallValue::Int(member as i32))
            .collect()
    }
    /// `set_unique_id`: the object's ID afterwards.
    pub(crate) fn set_unique_id(&mut self, object: ObjectRef, flag: i32) -> i32 {
        let Some(current) = self.world.get(object).map(|object| object.raw.id) else {
            return 0;
        };
        let unique = UNIQUE_IDS.contains(&current);
        let new = if flag == -1 {
            if !unique {
                return current as i32;
            }
            match self.world.allocate_object_id() {
                Ok(id) => id,
                Err(error) => {
                    self.world.diagnostic(format!("set_unique_id: {error}"));
                    return current as i32;
                }
            }
        } else {
            if unique {
                return current as i32;
            }
            let Some(state) = self.world.sfall.as_deref_mut() else {
                return current as i32;
            };
            if !UNIQUE_IDS.contains(&state.next_unique) {
                self.world
                    .diagnostic("set_unique_id: the unique range is used up".to_owned());
                return current as i32;
            }
            let id = state.next_unique;
            state.next_unique += 1;
            id
        };
        self.world.set_object_id(object, new);
        new as i32
    }
    fn message_catalog(&mut self, file: i32) -> Option<Arc<MsgCatalog>> {
        let name = match file {
            0..=20 => format!("{}.msg", GAME_FILES[file as usize]),
            0x1000..=0x1005 => format!("{}.msg", PROTO_FILES[(file - 0x1000) as usize]),
            FIRST_LISTED_FILE..=0x2fff => self.listed_message_file(file)?,
            FIRST_ADDED_FILE..=LAST_ADDED_FILE => {
                let index = (file - FIRST_ADDED_FILE) as usize;
                let (_, catalog) = self.promoted()?.added.get(index)?;
                return Some(catalog.clone());
            }
            _ => return None,
        };
        self.assets.game_messages(&name).ok()
    }
    fn listed_message_file(&mut self, file: i32) -> Option<String> {
        if self.promoted()?.listed.is_none() {
            let list = self.ini_any_section("ddraw.ini", "ExtraGameMsgFileList");
            let files = list.as_deref().map(listed_files).unwrap_or_default();
            self.promoted()?.listed = Some(files);
        }
        self.promoted()?.listed.as_ref()?.get(&file).cloned()
    }
    /// `message_str_game`: `Error` for anything `message_str` refuses.
    fn game_message(&mut self, file: i32, number: i32) -> SfallValue {
        let text = (number > 0)
            .then(|| self.message_catalog(file))
            .flatten()
            .and_then(|catalog| catalog.get(number).map(|message| message.text.clone()));
        SfallValue::Text(text.unwrap_or_else(|| b"Error".to_vec()))
    }
    /// `add_extra_msg_file`: the current language's game folder, then
    /// English's. The same name, without case, keeps its ID.
    fn add_message_file(&mut self, name: &[u8]) -> i32 {
        let name = message_file(&String::from_utf8_lossy(name));
        let key = name.to_ascii_lowercase();
        let Some(promoted) = self.promoted() else {
            return -1;
        };
        let held = promoted.added.iter().position(|(file, _)| *file == key);
        if let Some(index) = held {
            return FIRST_ADDED_FILE + index as i32;
        }
        let id = FIRST_ADDED_FILE + promoted.added.len() as i32;
        if id > LAST_ADDED_FILE {
            return -1;
        }
        let english = || {
            let bytes = self.assets.resource(&format!("text/english/game/{key}"));
            Some(Arc::new(MsgCatalog::parse(&bytes.ok()?).ok()?))
        };
        let Some(catalog) = self.assets.game_messages(&name).ok().or_else(english) else {
            return -1;
        };
        if let Some(promoted) = self.promoted() {
            promoted.added.push((key, catalog));
        }
        id
    }
    fn last_tag(&mut self) -> i32 {
        let boxes = self.ini_any_section("ddraw.ini", "BoxBarCount");
        let boxes = boxes.map_or(CUSTOM_BOXES, |count| sfall::atoi(count.as_bytes()));
        ADDICT_TAG.saturating_add(boxes.max(0))
    }
    /// `show_iface_tag` and `hide_iface_tag`. Tags 1 and 2 are read-only.
    fn set_tag(&mut self, tag: i32, on: bool) {
        let last = self.last_tag();
        let settable = (0..=last).contains(&tag) && !matches!(tag, POISONED_TAG | RADIATED_TAG);
        if let Some(promoted) = self.promoted().filter(|_| settable) {
            if on {
                promoted.tags.insert(tag);
            } else {
                promoted.tags.remove(&tag);
            }
        }
    }
    /// `is_iface_tag_active`: SPEC-019 §3.5.2's condition or the script flag.
    fn tag_lit(&mut self, tag: i32) -> bool {
        if !(0..=self.last_tag()).contains(&tag) {
            return false;
        }
        let sheet = self.world.player_sheet();
        let flags = sheet.map_or(0, |sheet| sheet.flags);
        let engine = match tag {
            SNEAK_TAG => flags & crate::host_skills::SNEAK_FLAG != 0,
            POISONED_TAG => sheet.is_some_and(|sheet| sheet.poison > 0),
            RADIATED_TAG => sheet.is_some_and(|sheet| sheet.radiation > 0),
            LEVEL_TAG => flags & LEVEL_UP_FLAG != 0,
            ADDICT_TAG => flags & crate::progression::ADDICTED_FLAG != 0,
            _ => false,
        };
        engine || self.sfall_tag_flag(tag)
    }
    /// Whether a script set `tag`'s flag, for the indicator boxes.
    pub fn sfall_tag_flag(&self, tag: i32) -> bool {
        self.sfall
            .as_deref()
            .is_some_and(|sfall| sfall.promoted.tags.contains(&tag))
    }
    /// The custom tags a script set, ascending, for the indicator boxes.
    pub fn sfall_custom_tags(&self) -> Vec<i32> {
        self.sfall.as_deref().map_or(Vec::new(), |sfall| {
            let tags = &sfall.promoted.tags;
            tags.range(FIRST_CUSTOM_TAG..).copied().collect()
        })
    }
    /// `create_win` (name, x, y, width, height, optional flags): SPEC-005's
    /// `createwin`, in which a refused window is a diagnostic here (§4.1).
    fn create_window(&mut self, args: &[SfallValue]) {
        let numbers: Option<Vec<i32>> = args
            .iter()
            .skip(1)
            .map(|value| value.coerce(sfall::Class::I).map(|value| value.int()))
            .collect();
        let (Some(SfallValue::Text(name)), Some(numbers)) = (args.first(), numbers) else {
            self.world
                .diagnostic("create_win: failed operand".to_owned());
            return;
        };
        let [x, y, width, height] = numbers[..numbers.len().min(4)] else {
            self.world
                .diagnostic("create_win: failed operand".to_owned());
            return;
        };
        let flags = numbers.get(4).copied().unwrap_or(WINDOW_FLAGS);
        if let Err(error) = self
            .library
            .create_window(name, [x, y, width, height], flags)
        {
            self.world.diagnostic(format!("create_win: {error}"));
        }
    }
    /// `message_box`: the box opens and the program waits at the next
    /// instruction, or -1 where a program cannot wait. A second box cannot
    /// be created while one is open.
    pub(crate) fn sfall_message_box(
        &mut self,
        vm: &mut Vm,
        args: &[SfallValue],
    ) -> Option<SfallValue> {
        let Some(SfallValue::Text(text)) = args.first() else {
            self.world
                .diagnostic("message_box: failed operand".to_owned());
            return Some(SfallValue::Int(0));
        };
        let number = |index: usize, default: i32| {
            args.get(index)
                .and_then(|value| value.coerce(sfall::Class::I))
                .map_or(default, |value| value.int())
        };
        let flags = match number(1, -1) {
            -1 => BOX_FLAGS,
            flags => flags,
        };
        let colors = [number(2, BOX_COLOR), number(3, BOX_COLOR)];
        let lines = text
            .split(|byte| *byte == b'\n')
            .map(<[u8]>::to_vec)
            .collect();
        if self.sfall_cannot_wait() {
            return Some(SfallValue::Int(-1));
        }
        let promoted = self.promoted()?;
        if promoted.message_box.is_some() {
            return Some(SfallValue::Int(-1));
        }
        promoted.message_box = Some(MessageBox {
            program: vm.id,
            lines,
            flags,
            colors,
        });
        vm.wait_predicate = BOX_WAIT;
        vm.status.0 |= VmStatus::WAITING;
        None
    }
    /// The box the shell should show.
    pub fn sfall_open_message_box(&self) -> Option<&MessageBox> {
        self.sfall.as_deref()?.promoted.message_box.as_ref()
    }
    /// The box closed with `result` (1 Yes or Done, 0 No): the waiting
    /// program gets the result and goes on.
    pub fn sfall_close_message_box(&mut self, result: i32) -> Result<(), VmError> {
        let Some(open) = self
            .promoted()
            .and_then(|promoted| promoted.message_box.take())
        else {
            return Ok(());
        };
        let release = |vm: &mut Vm| {
            vm.status.0 &= !VmStatus::WAITING;
            vm.wait_predicate = 0;
            SfallValue::Int(result).push(vm)
        };
        if let Some(vm) = self.programs.get_mut(&open.program) {
            return release(vm);
        }
        self.sfall_resume_member(open.program, release)
    }
    /// `signal_close_game`: `metarule` rule 13, and the session ends after
    /// this step's Simulation phase.
    fn signal_close_game(&mut self) {
        self.world.emit(crate::WorldEffect::EndGame);
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.close_signalled = true;
        }
    }
    /// Whether `signal_close_game` has ended the session.
    pub fn sfall_session_closed(&self) -> bool {
        self.sfall.as_deref().is_some_and(|sfall| sfall.closed)
    }
}

#[cfg(test)]
impl<A: WorldAssets> WorldHost<A> {
    pub(crate) fn hero_style_for_test(&mut self) -> i32 {
        self.promoted().expect("sfall mode").hero_style
    }
    pub(crate) fn object_id_for_test(&self, object: ObjectRef) -> i32 {
        self.world.get(object).expect("a live object").raw.id as i32
    }
}
