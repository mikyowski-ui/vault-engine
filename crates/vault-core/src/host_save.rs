//! SPEC-008 §2.4/§2.5/§3.3 save slots: `SAVEGAME/SLOTnn/SAVE.DAT` with its
//! companion `<map>.SAV` files and `AUTOMAP.SAV`, best-effort `.BAK`
//! backups, and the restore order (current map before later components).
use crate::{
    ObjectRef, WorldError, WorldState,
    assets::WorldAssets,
    combat_state::CombatState,
    critical::state,
    host::WorldHost,
    queue::{EventKind, TimedEvent},
    worldmap::WorldMap,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use vault_formats::{
    AutomapDb, ParseError,
    map::{MapObject, ObjectState},
    savegame::{
        CombatSave, CritterStats, QueuePayload, QueueRecord, SaveGame, SaveHeader, THUMBNAIL_LEN,
    },
};
use vault_vm::VmHost;

pub const CENTRAL_FILE: &str = "SAVE.DAT";
pub const AUTOMAP_FILE: &str = "AUTOMAP.SAV";

/// Presentation fields supplied by the platform (host clock and capture).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveInfo {
    /// At most 30 bytes are stored.
    pub description: Vec<u8>,
    /// Day, month, year, in the writer's raw order.
    pub host_date: [u16; 3],
    /// Hour plus minute (not HHMM), as the recovered writer stores it.
    pub host_time: u32,
    /// 224 x 133 palette indices scaled from the game view; zeros if absent.
    pub thumbnail: Option<Vec<u8>>,
}

/// `SAVEGAME/SLOTnn` under the writable patch root, for slots 01-10.
pub fn slot_directory(root: &Path, slot: u8) -> Result<PathBuf, WorldError> {
    if !(1..=10).contains(&slot) {
        return Err(WorldError::Invalid(format!(
            "save slot {slot} outside 1-10"
        )));
    }
    Ok(root.join("SAVEGAME").join(format!("SLOT{slot:02}")))
}

/// Month, day, year on the opcodes' fixed non-leap calendar.
pub fn game_date(ticks: u64, base_year: u16) -> [u16; 3] {
    let mut day = (ticks / 864000) % 365;
    let year = (u64::from(base_year) + ticks / (864000 * 365)).min(u64::from(u16::MAX)) as u16;
    let mut month = 1;
    for days in [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] {
        if day < days {
            break;
        }
        day -= days;
        month += 1;
    }
    [month, day as u16 + 1, year]
}

fn io(path: &Path, error: std::io::Error) -> WorldError {
    WorldError::Invalid(format!("{}: {error}", path.display()))
}
fn fixed<const N: usize>(text: &[u8]) -> [u8; N] {
    let mut out = [0; N];
    let count = text.len().min(N);
    out[..count].copy_from_slice(&text[..count]);
    out
}
fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())]
}
fn backup_name(path: &Path) -> PathBuf {
    path.with_extension("BAK")
}
/// Case-insensitive lookup of a slot file.
fn slot_file(dir: &Path, name: &str) -> Result<PathBuf, WorldError> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Ok(exact);
    }
    let entries = fs::read_dir(dir).map_err(|error| io(dir, error))?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| WorldError::Invalid(format!("{}: missing save companion", exact.display())))
}

fn queue_record(world: &WorldState, event: &TimedEvent) -> Result<QueueRecord, WorldError> {
    let payload = match event.kind {
        EventKind::Drug { stats, modifiers } => QueuePayload::Drug { stats, modifiers },
        EventKind::Withdrawal(values) => QueuePayload::Withdrawal(values),
        EventKind::Script { script, parameter } => QueuePayload::Script {
            script: script as i32,
            parameter,
        },
        EventKind::Radiation { level, healing } => QueuePayload::Radiation { level, healing },
        EventKind::Knockout | EventKind::Midnight | EventKind::Poison | EventKind::MapUpdate => {
            QueuePayload::None
        }
        EventKind::Other(kind) if (7..=11).contains(&kind) => QueuePayload::None,
        EventKind::Other(kind) => {
            return Err(WorldError::Invalid(format!(
                "timed event type {kind} has no save representation"
            )));
        }
    };
    let owner = match event.object {
        0 => -2,
        object => world.get(object).map_or(-2, |o| o.raw.id as i32),
    };
    Ok(QueueRecord {
        due: u32::try_from(event.due).map_err(|_| WorldError::Arithmetic)?,
        kind: event.kind.type_number(),
        owner,
        payload,
    })
}
fn event_kind(record: &QueueRecord) -> Result<EventKind, WorldError> {
    Ok(match (record.kind, record.payload) {
        (0, QueuePayload::Drug { stats, modifiers }) => EventKind::Drug { stats, modifiers },
        (1, QueuePayload::None) => EventKind::Knockout,
        (2, QueuePayload::Withdrawal(values)) => EventKind::Withdrawal(values),
        (3, QueuePayload::Script { script, parameter }) => EventKind::Script {
            script: script as u32,
            parameter,
        },
        (4, QueuePayload::None) => EventKind::Midnight,
        (5, QueuePayload::None) => EventKind::Poison,
        (6, QueuePayload::Radiation { level, healing }) => EventKind::Radiation { level, healing },
        (kind @ 7..=11, QueuePayload::None) => EventKind::Other(kind as u32),
        (12, QueuePayload::None) => EventKind::MapUpdate,
        (kind, _) => {
            return Err(WorldError::Invalid(format!(
                "timed event type {kind} has a mismatched payload"
            )));
        }
    })
}

impl<A: WorldAssets> WorldHost<A> {
    fn prototype_info(&self, pid: u32) -> Result<vault_formats::pro::PrototypeInfo, ParseError> {
        self.assets
            .prototype(pid)
            .map(|prototype| prototype.info())
            .map_err(|error| ParseError {
                format: "PRO",
                offset: 0,
                message: error.to_string(),
            })
    }
    /// The 27 ordered components from live state. The current map (with its
    /// party members) first joins the working saved-map set.
    pub fn build_save(&mut self, info: &SaveInfo) -> Result<SaveGame, WorldError> {
        self.world.persist_current_map(true);
        let world = &self.world;
        let player = world
            .get(world.player)
            .ok_or_else(|| WorldError::Invalid("save requires a player object".into()))?;
        let sheet = player
            .character
            .as_ref()
            .ok_or_else(|| WorldError::Invalid("save requires a player sheet".into()))?;
        let thumbnail = info
            .thumbnail
            .clone()
            .unwrap_or_else(|| vec![0; THUMBNAIL_LEN]);
        if thumbnail.len() != THUMBNAIL_LEN {
            return Err(WorldError::Invalid(
                "thumbnail must be 224 x 133 bytes".into(),
            ));
        }
        let map_file = world.current_map_name().unwrap_or_default();
        let header = SaveHeader {
            name: fixed(&sheet.name),
            description: fixed(&info.description),
            host_date: info.host_date,
            host_time: info.host_time,
            game_date: game_date(world.clock.ticks, world.rules.calendar_year),
            game_ticks: u32::try_from(world.clock.ticks).map_err(|_| WorldError::Arithmetic)?,
            elevation: u16::from(world.elevation),
            map: world.map.header.map_number.clamp(0, i32::from(u16::MAX)) as u16,
            map_file: fixed(map_file.as_bytes()),
            thumbnail,
            ..SaveHeader::default()
        };
        let combat_id =
            |reference: &ObjectRef| world.get(*reference).map_or(-1, |o| o.raw.combat_id);
        let combat = &world.combat_state;
        let combat = if world.combat && combat.flags & state::ACTIVE != 0 {
            CombatSave {
                state: combat.flags,
                turn_running: i32::from(combat.player_turn),
                free_movement: world.free_movement,
                experience: world.combat_experience,
                combatants: combat.active as i32,
                noncombatants: (combat.list.len() - combat.active) as i32,
                player_id: combat_id(&world.player),
                list: combat.list.iter().map(combat_id).collect(),
            }
        } else {
            CombatSave {
                state: combat.flags & !state::ACTIVE,
                turn_running: 0,
                free_movement: 0,
                experience: 0,
                combatants: 0,
                noncombatants: 0,
                player_id: -1,
                list: Vec::new(),
            }
        };
        let carried = &world.persistence;
        let perks = std::array::from_fn(|index| {
            world
                .rules
                .perk_ids
                .get(&(index as i32))
                .map_or(carried.perk_ranks[index], |perk| sheet.rank(*perk) as i32)
        });
        let mut preferences = carried.preferences;
        preferences[0] = world.difficulty as u32;
        preferences[1] = world.combat_difficulty as u32;
        if let Some(violence) = world.violence {
            preferences[2] = violence as u32;
        }
        preferences[4] = world.running_burning as u32;
        let queue = world
            .queue
            .events()
            .map(|event| queue_record(world, event))
            .collect::<Result<_, _>>()?;
        let party: Vec<i32> = world
            .party
            .iter()
            .filter(|member| **member != world.player)
            .filter_map(|member| world.get(*member))
            .map(|object| object.raw.id as i32)
            .collect();
        let campaign = world.gvars.clone().unwrap_or_default();
        Ok(SaveGame {
            header,
            player_combat_id: player.raw.combat_id,
            campaign: campaign.clone(),
            campaign_status: carried.campaign_status,
            maps: carried
                .maps
                .keys()
                .map(|name| name.as_bytes().to_vec())
                .collect(),
            automap_length: carried.automap.bytes().len() as i32,
            campaign_b: campaign,
            campaign_b_status: carried.campaign_status,
            player: world
                .object_tree(world.player)
                .ok_or_else(|| WorldError::Invalid("player tree exceeds depth limit".into()))?,
            viewport: world.camera_hex,
            sneak_working: i32::from(world.sneak_working),
            stats: CritterStats {
                flags: sheet.flags,
                base: sheet.base,
                bonus: sheet.bonus,
                skills: sheet.invested,
                body: sheet.body,
                experience: sheet.experience_reward,
                kill_category: sheet.kill_category,
            },
            kills: world.kills,
            tags: sheet.tags,
            perks,
            combat,
            progression: [
                sheet.unspent_skills,
                sheet.level,
                sheet.experience,
                sheet.reputation,
                sheet.karma,
            ],
            queue,
            traits: sheet.traits,
            automap_flags: carried.automap_flags,
            preferences,
            reconciled_level: sheet.reconciled_level,
            pending_perk: carried.pending_perk,
            world_map: world.world_map.saved.clone(),
            movies: carried.movies,
            skill_uses: carried.skill_uses,
            party_count: party.len() as i32 + 1,
            next_item_id: world.next_object_id() as i32,
            party,
            interface: [
                i32::from(!world.ui_disabled),
                carried.interface[1],
                world.active_hand,
                carried.interface[3],
            ],
        })
    }
    /// Write a slot: existing central/companion files are first renamed to
    /// `.BAK` (best effort), restored if writing fails, and removed after
    /// success. This is not an atomic multi-file transaction.
    pub fn save_game(&mut self, root: &Path, slot: u8, info: &SaveInfo) -> Result<(), WorldError> {
        let dir = slot_directory(root, slot)?;
        let save = self.build_save(info)?;
        let mut files = vec![(CENTRAL_FILE.to_string(), save.write()?)];
        for (name, bytes) in &self.world.persistence.maps {
            files.push((name.clone(), bytes.clone()));
        }
        files.push((
            AUTOMAP_FILE.to_string(),
            self.world.persistence.automap.bytes().to_vec(),
        ));
        fs::create_dir_all(&dir).map_err(|error| io(&dir, error))?;
        let mut backups = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|error| io(&dir, error))? {
            let Ok(path) = entry.map(|entry| entry.path()) else {
                continue;
            };
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let central = path
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| f.eq_ignore_ascii_case(CENTRAL_FILE));
            if (central || extension.eq_ignore_ascii_case("SAV"))
                && fs::rename(&path, backup_name(&path)).is_ok()
            {
                backups.push(path);
            }
        }
        let mut written = Vec::new();
        let result = files.iter().try_for_each(|(name, bytes)| {
            let path = dir.join(name);
            written.push(path.clone());
            fs::write(&path, bytes).map_err(|error| io(&path, error))
        });
        match result {
            Ok(()) => {
                for path in backups {
                    let _ = fs::remove_file(backup_name(&path));
                }
                Ok(())
            }
            Err(error) => {
                for path in written {
                    let _ = fs::remove_file(path);
                }
                for path in backups {
                    let _ = fs::rename(backup_name(&path), &path);
                }
                Err(error)
            }
        }
    }
    /// Read and validate a whole slot (header, every payload, the listed
    /// companions, and the automap byte length) before changing any state,
    /// then restore it.
    pub fn load_game(&mut self, root: &Path, slot: u8) -> Result<(), WorldError> {
        let dir = slot_directory(root, slot)?;
        let central_path = slot_file(&dir, CENTRAL_FILE)?;
        let central = fs::read(&central_path).map_err(|error| io(&central_path, error))?;
        SaveHeader::parse(&central)?;
        // G comes from the installed campaign initializer.
        let initial = self
            .assets
            .gam("data/vault13.gam", vault_formats::GamSection::Campaign)?
            .ok_or_else(|| WorldError::Invalid("campaign initializer did not open".into()))?;
        let lookup = |pid| self.prototype_info(pid);
        let save = SaveGame::parse(&central, initial.values.len(), &lookup)?;
        let mut maps = BTreeMap::new();
        for name in &save.maps {
            let name = std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("save manifest name is not ASCII".into()))?
                .to_ascii_uppercase();
            let path = slot_file(&dir, &name)?;
            maps.insert(name, fs::read(&path).map_err(|error| io(&path, error))?);
        }
        let automap_path = slot_file(&dir, AUTOMAP_FILE)?;
        let automap = fs::read(&automap_path).map_err(|error| io(&automap_path, error))?;
        if i64::try_from(automap.len()).ok() != Some(i64::from(save.automap_length)) {
            return Err(WorldError::Invalid(
                "AUTOMAP.SAV length differs from the recorded length".into(),
            ));
        }
        let automap = AutomapDb::parse(automap)?;
        self.apply_save(save, maps, automap)
    }
    /// Reset like a new game, keeping rules and generator state (the random
    /// component has no bytes and is not restored).
    fn reset_game(&mut self) -> Result<(), WorldError> {
        for program in self.programs.keys().copied().collect::<Vec<_>>() {
            self.registry.purge(program)?;
            self.library.cleanup(program);
        }
        self.programs.clear();
        self.clear_program_bindings();
        self.animations.clear();
        self.building_animation = None;
        let sheet = self.world.player_sheet().cloned().unwrap_or_default();
        let mut world = WorldState::new(sheet, 1)?;
        world.rules = std::mem::take(&mut self.world.rules);
        world.rng = self.world.rng.clone();
        self.world = world;
        Ok(())
    }
    /// Restore in the reference order: reset and install the game tick,
    /// campaign snapshot A, the companion working set, then the current area
    /// map **before** the player, queue, world-map, and later components.
    /// Snapshot B is consumed but ignored. Finalization restores the header's
    /// player name and requests combat continuation when it was active.
    pub fn apply_save(
        &mut self,
        save: SaveGame,
        maps: BTreeMap<String, Vec<u8>>,
        automap: AutomapDb,
    ) -> Result<(), WorldError> {
        self.reset_game()?;
        self.world.clock.set(u64::from(save.header.game_ticks));
        self.world.gvars = Some(save.campaign.clone());
        self.world.persistence.campaign_status = save.campaign_status;
        self.world.persistence.maps = maps;
        self.world.persistence.automap = automap;
        // Installed before the map load as well, so temporary objects created
        // during map entry cannot take saved inventory identities.
        self.world
            .set_next_object_id(save.next_item_id.max(1) as u32);
        let map_file = String::from_utf8_lossy(text(&save.header.map_file)).to_ascii_lowercase();
        let stem = map_file
            .rsplit_once('.')
            .map_or(map_file.as_str(), |(stem, _)| stem);
        self.load_map(&format!("maps/{stem}.map"))?;
        self.restore_player(&save)?;
        let world = &mut self.world;
        world.camera_hex = save.viewport;
        world.elevation = save.player.elevation;
        world.sneak_working = save.sneak_working != 0;
        world.kills = save.kills;
        world.persistence.perk_ranks = save.perks;
        world.persistence.automap_flags = save.automap_flags;
        world.persistence.preferences = save.preferences;
        world.difficulty = save.preferences[0] as i32;
        world.combat_difficulty = save.preferences[1] as i32;
        world.violence = Some(save.preferences[2] as i32);
        world.running_burning = save.preferences[4] as i32;
        world.persistence.pending_perk = save.pending_perk;
        world.world_map = WorldMap::from_saved(save.world_map.clone());
        world.persistence.movies = save.movies;
        world.persistence.skill_uses = save.skill_uses;
        world.persistence.interface = save.interface;
        world.ui_disabled = save.interface[0] == 0;
        world.active_hand = save.interface[2];
        world.set_next_object_id(save.next_item_id.max(1) as u32);
        world.queue = Default::default();
        for record in &save.queue {
            let object = match record.owner {
                -2 => Some(0),
                id => world.reference_for_id(id as u32),
            };
            match (object, event_kind(record)) {
                (Some(object), Ok(kind)) => {
                    world.queue.insert(u64::from(record.due), 0, object, kind)?;
                }
                (None, _) => world.diagnostic(format!(
                    "timed event owner {} is not on the restored map",
                    record.owner
                )),
                (_, Err(error)) => return Err(error),
            }
        }
        world.party.clear();
        for id in &save.party {
            match world.reference_for_id(*id as u32) {
                Some(member) => {
                    world.party.insert(member);
                }
                None => world.diagnostic(format!("party member {id} is not on the restored map")),
            }
        }
        if save.combat.state & state::ACTIVE != 0 {
            self.resume_combat(&save.combat);
        }
        if let Some(sheet) = self.world.player_sheet_mut() {
            sheet.name = text(&save.header.name).to_vec();
        }
        let name = text(&save.header.name).to_vec();
        if let Some(player) = self.world.get_mut(self.world.player) {
            player.name = name;
        }
        self.world.rebuild_lighting();
        Ok(())
    }
    /// Components 1, 5, 6, 8, 10, 13, 16, 19: the player object tree and
    /// inventory, combat identity, statistics, and progression.
    fn restore_player(&mut self, save: &SaveGame) -> Result<(), WorldError> {
        let player = self.world.player;
        let held: Vec<_> = self
            .world
            .get(player)
            .map(|o| o.inventory.iter().map(|(child, _)| *child).collect())
            .unwrap_or_default();
        for child in held {
            self.world.erase(child);
        }
        let mut tree = save.player.clone();
        let inventory = std::mem::take(&mut tree.inventory);
        let (hp, poison, radiation) = match &tree.state {
            ObjectState::Critter(state) => (state.hp, state.poison, state.radiation),
            ObjectState::Other { .. } => (0, 0, 0),
        };
        let rules = self.world.rules.perk_ids.clone();
        let object = self
            .world
            .get_mut(player)
            .ok_or_else(|| WorldError::Invalid("restore requires a player object".into()))?;
        object.raw = MapObject {
            combat_id: save.player_combat_id,
            owner: None,
            ..tree
        };
        object.inventory.clear();
        let sheet = object
            .character
            .as_mut()
            .ok_or_else(|| WorldError::Invalid("restore requires a player sheet".into()))?;
        let stats = &save.stats;
        sheet.flags = stats.flags;
        sheet.base = stats.base;
        sheet.bonus = stats.bonus;
        sheet.invested = stats.skills;
        sheet.body = stats.body;
        sheet.experience_reward = stats.experience;
        sheet.kill_category = stats.kill_category;
        sheet.tags = save.tags;
        sheet.traits = save.traits;
        // Ranks only: stored bonus words already include perk effects.
        sheet.perks.clear();
        for (index, perk) in rules {
            if let Some(rank) = usize::try_from(index).ok().and_then(|i| save.perks.get(i))
                && *rank > 0
            {
                sheet.perks.insert(perk, *rank as u32);
            }
        }
        [
            sheet.unspent_skills,
            sheet.level,
            sheet.experience,
            sheet.reputation,
            sheet.karma,
        ] = save.progression;
        sheet.reconciled_level = save.reconciled_level;
        sheet.current_hp = hp;
        sheet.poison = poison;
        sheet.radiation = radiation;
        for entry in inventory {
            let child = self
                .world
                .import_object(entry.object, Some(player), &self.assets)?;
            if let Some(object) = self.world.get_mut(player) {
                object.inventory.push((child, entry.quantity as i32));
            }
        }
        Ok(())
    }
    /// Finalization's combat continuation: the saved list resolves through
    /// combat identities on the restored map, and the player's turn resumes
    /// without an AP refill.
    fn resume_combat(&mut self, saved: &CombatSave) {
        let world = &mut self.world;
        let mut list = Vec::new();
        for id in &saved.list {
            let found = world
                .iter()
                .find(|(_, o)| o.owner.is_none() && o.raw.hex.is_some() && o.raw.combat_id == *id)
                .map(|(reference, _)| reference);
            match found {
                Some(reference) => list.push(reference),
                None => world.diagnostic(format!("combatant {id} is not on the restored map")),
            }
        }
        let active = usize::try_from(saved.combatants)
            .unwrap_or(0)
            .min(list.len());
        let cursor = list.iter().position(|r| *r == world.player).unwrap_or(0);
        let elevation = world
            .get(world.player)
            .map_or(world.elevation, |o| o.raw.elevation);
        world.combat_state = CombatState {
            flags: saved.state,
            list,
            active,
            elevation,
            round: 1,
            cursor,
            resumed: true,
            ..CombatState::default()
        };
        world.combat = true;
        world.free_movement = saved.free_movement;
        world.combat_experience = saved.experience;
    }
}
