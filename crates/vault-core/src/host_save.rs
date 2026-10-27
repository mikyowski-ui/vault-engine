//! SPEC-008 §2.4/§2.5/§3.3 save slots: `SAVEGAME/SLOTnn/SAVE.DAT` with its
//! companion `<map>.SAV` files and `AUTOMAP.SAV`, staged writes committed
//! only after every file succeeds, and the restore order (current map
//! before later components).
use crate::{
    ObjectRef, Perk, WorldError, WorldState,
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

/// The `enhanced` preset's quick-save slot: `SAVEGAME/QUICKSAV` below the
/// save root, beside the ten numbered slots. The SPEC-014 §3.14 screen
/// lists `SLOT01`-`SLOT10` only, so it never shows this slot and a quick
/// save never touches a numbered one. It holds the same files as any slot
/// (SPEC-008 §3.3), the enhanced sidecar included.
pub fn quick_slot_directory(root: &Path) -> PathBuf {
    root.join("SAVEGAME").join("QUICKSAV")
}

/// SPEC-014 §3.1 step 8 sets the clock to tick 264600, 07:21 on 5 December
/// 2161, so tick 0 is midnight on 5 December 2161, day 338 of the fixed
/// non-leap calendar (SPEC-005 §3.5); the SPEC-008 save fixture's tick
/// 864000 is 6 December 2161.
pub const CALENDAR_YEAR: u16 = 2161;
pub const CALENDAR_DAY: u64 = 338;
/// Month, day, year of a game tick on the fixed non-leap calendar.
pub fn game_date(ticks: u64) -> [u16; 3] {
    calendar_date(ticks, CALENDAR_YEAR, CALENDAR_DAY)
}
/// Month, day, year of a game tick on the fixed non-leap calendar whose
/// tick 0 is day `start_day` (from 0) of `start_year`.
pub fn calendar_date(ticks: u64, start_year: u16, start_day: u64) -> [u16; 3] {
    let days = ticks / 864000 + start_day;
    let mut day = days % 365;
    let year = (u64::from(start_year) + days / 365).min(u64::from(u16::MAX)) as u16;
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
/// SPEC-008 §3.10: a game load opens the `SAVE.DAT` field at `0x73`
/// uppercased again (`VAULTENT.sav` opens `VAULTENT.SAV`).
fn save_map_file(save: &SaveGame) -> String {
    String::from_utf8_lossy(text(&save.header.map_file)).to_ascii_uppercase()
}
fn backup_name(path: &Path) -> PathBuf {
    path.with_extension("BAK")
}
/// A companion name the slot accepts: exactly what
/// [`saved_map_name`](crate::persistence::saved_map_name) gives back for it,
/// so a plain uppercase stem of letters, digits, `_` or `-` plus `.SAV`,
/// with no `/` or `\` separator, no `..` component, and no `C:` drive
/// prefix. Manifest names are untrusted SAVE.DAT bytes; anything else would
/// let a crafted slot read or write files outside its directory.
pub fn valid_companion_name(name: &str) -> bool {
    let plain = name.strip_suffix(".SAV").is_some_and(|stem| {
        stem.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    });
    plain && crate::persistence::saved_map_name(name.as_bytes()).as_deref() == Some(name)
}
fn checked_companion(name: &str) -> Result<&str, WorldError> {
    if valid_companion_name(name) {
        Ok(name)
    } else {
        Err(WorldError::Invalid(format!(
            "save manifest name {name:?} is not a plain map name"
        )))
    }
}
/// Case-insensitive lookup of a slot file: the fixed central and automap
/// names, or a [`valid_companion_name`].
fn slot_file(dir: &Path, name: &str) -> Result<PathBuf, WorldError> {
    if name != CENTRAL_FILE && name != AUTOMAP_FILE {
        checked_companion(name)?;
    }
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
    /// party members) first joins the working saved-map set without being
    /// left: marked saved with the current time as its last visit, and
    /// written even when it is an excluded map (SPEC-014 §3.35).
    pub fn build_save(&mut self, info: &SaveInfo) -> Result<SaveGame, WorldError> {
        // A slot names its current map, which restore loads first
        // (SPEC-014 §3.35): with no map loaded the slot could never load. An
        // unnumbered map still has its working name (SPEC-008 §3.10).
        if self.world.unloaded || self.world.current_map_name().is_none() {
            return Err(WorldError::Invalid("saving requires a loaded map".into()));
        }
        self.world.map.header.flags.0 |= 1;
        self.world.map.header.last_visit = self.world.clock.ticks as u32;
        // SPEC-006 §3.5: the party records are written with the map's
        // other scripts, their marks cleared for the save and set again.
        self.world.set_party_marks(false);
        let persisted = self.world.persist_current_map(true);
        self.world.set_party_marks(true);
        if !persisted {
            return Err(WorldError::Invalid(
                "the current map could not be written to the working map set".into(),
            ));
        }
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
        // SPEC-008 §3.10: the working name's stem and a lower-case `.sav`.
        let map_file = world
            .current_map_name()
            .map(|name| crate::persistence::save_dat_map_file(&name))
            .unwrap_or_default();
        let header = SaveHeader {
            name: fixed(&sheet.name),
            description: fixed(&info.description),
            host_date: info.host_date,
            host_time: info.host_time,
            game_date: world.game_date(),
            game_ticks: u32::try_from(world.clock.ticks).map_err(|_| WorldError::Arithmetic)?,
            elevation: u16::from(world.elevation),
            // The number as its 16-bit truncation (-1 on an unnumbered map).
            map: world.map.header.map_number as u16,
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
                noncombatants: combat.list.len().saturating_sub(combat.active) as i32,
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
        // SPEC-011 §2.5: selected ranks only, by perk index.
        let perks = std::array::from_fn(|index| sheet.rank(Perk::ALL[index]) as i32);
        let mut preferences = carried.preferences;
        preferences[0] = world.difficulty as u32;
        if let Some(combat) = world.combat_difficulty {
            preferences[1] = combat as u32;
        }
        if let Some(violence) = world.violence {
            preferences[2] = violence as u32;
        }
        preferences[4] = world.running_burning as u32;
        if let Some(taunts) = world.ai.combat_taunts {
            preferences[6] = taunts as u32;
        }
        if let Some(filter) = world.ai.language_filter {
            preferences[7] = filter as u32;
        }
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
        let fallout2 = world.fallout2.as_deref().map(|state| {
            // SPEC-018 §7.5: each member's control record (one created as
            // joining creates it when none is stored), and the retained
            // records.
            let records = world
                .party
                .iter()
                .filter(|member| **member != world.player)
                .filter_map(|member| world.get(*member))
                .map(|object| {
                    state
                        .control
                        .get(&object.raw.id)
                        .copied()
                        .unwrap_or_else(|| state.joining_record(object.raw.pid))
                })
                .collect();
            Box::new(vault_formats::savegame::Fallout2Payload {
                kills: state.kills,
                world_map: state.save(),
                movies: state.movies,
                records,
                retained: state
                    .retained
                    .iter()
                    .map(|(id, record)| (*id as i32, *record))
                    .collect(),
            })
        });
        let mut header = header;
        if fallout2.is_some() {
            // SPEC-018 §7.2: version 1.2, the layout tag, G and the gated
            // mark.
            header.set_fallout2(campaign.len() as u32, self.scripts_gated());
        }
        let world = &self.world;
        let carried = &world.persistence;
        let campaign_status = if fallout2.is_some() {
            0
        } else {
            carried.campaign_status
        };
        Ok(SaveGame {
            header,
            player_combat_id: player.raw.combat_id,
            campaign: campaign.clone(),
            campaign_status,
            maps: carried
                .maps
                .keys()
                .map(|name| name.as_bytes().to_vec())
                .collect(),
            automap_length: carried.automap.bytes().len() as i32,
            campaign_b: campaign,
            campaign_b_status: campaign_status,
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
            pending_perk: u8::from(sheet.pending_perk),
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
            fallout2,
        })
    }
    /// Write a slot under the SPEC-008 §3.3 safe rule: every file is first
    /// written to a temporary `<name>.TMP`, so a failure leaves the old slot
    /// untouched. Only then is the slot committed: the old central and
    /// companion files are renamed to `.BAK`, the staged files are renamed
    /// into place (restoring the backups if that fails), and the backups are
    /// removed.
    pub fn save_game(&mut self, root: &Path, slot: u8, info: &SaveInfo) -> Result<(), WorldError> {
        self.save_game_in(&slot_directory(root, slot)?, info)
    }
    /// [`Self::save_game`] into the slot directory `dir` (a numbered slot's,
    /// or [`quick_slot_directory`]).
    pub fn save_game_in(&mut self, dir: &Path, info: &SaveInfo) -> Result<(), WorldError> {
        let dir = dir.to_path_buf();
        let save = self.build_save(info)?;
        let mut files = vec![(CENTRAL_FILE.to_string(), save.write()?)];
        for (name, bytes) in &self.world.persistence.maps {
            files.push((checked_companion(name)?.to_string(), bytes.clone()));
        }
        files.push((
            AUTOMAP_FILE.to_string(),
            self.world.persistence.automap.bytes().to_vec(),
        ));
        // The `enhanced` state sits beside the slot's files, never in them
        // (`SAVE.DAT` keeps SPEC-008's layout). Only an enhanced save with
        // settings to keep writes it; any other save removes an old one, so
        // an overwritten slot never carries stale settings.
        let sidecar = self.preset() == crate::EnginePreset::Enhanced
            && !(self.world.companion_distance.is_empty() && self.world.enhanced.is_empty());
        if sidecar {
            let text = crate::companion::write_sidecar(&self.world.companion_distance)
                + &crate::doors::write_table(&self.world.enhanced.opened_doors);
            files.push((crate::companion::SIDECAR.to_string(), text.into_bytes()));
        }
        fs::create_dir_all(&dir).map_err(|error| io(&dir, error))?;
        let staged = |name: &str| dir.join(format!("{name}.TMP"));
        let staging = files.iter().try_for_each(|(name, bytes)| {
            let path = staged(name);
            fs::write(&path, bytes).map_err(|error| io(&path, error))
        });
        if let Err(error) = staging {
            for (name, _) in &files {
                let _ = fs::remove_file(staged(name));
            }
            return Err(error);
        }
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
        let mut committed = Vec::new();
        let result = files.iter().try_for_each(|(name, _)| {
            let path = dir.join(name);
            fs::rename(staged(name), &path).map_err(|error| io(&path, error))?;
            committed.push(path);
            Ok(())
        });
        match result {
            Ok(()) => {
                for path in backups {
                    let _ = fs::remove_file(backup_name(&path));
                }
                if !sidecar {
                    let path = dir.join(crate::companion::SIDECAR);
                    match fs::remove_file(&path) {
                        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                            return Err(io(&path, error));
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            Err(error) => {
                for path in committed {
                    let _ = fs::remove_file(path);
                }
                for (name, _) in &files {
                    let _ = fs::remove_file(staged(name));
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
        self.load_game_from(&slot_directory(root, slot)?)
    }
    /// [`Self::load_game`] from the slot directory `dir` (a numbered
    /// slot's, or [`quick_slot_directory`]).
    pub fn load_game_from(&mut self, dir: &Path) -> Result<(), WorldError> {
        let dir = dir.to_path_buf();
        let central_path = slot_file(&dir, CENTRAL_FILE)?;
        let central = fs::read(&central_path).map_err(|error| io(&central_path, error))?;
        // SPEC-018 §2.3, §7.2: slots belong to one game; a Fallout 2 game
        // reads only the Fallout 2 layout.
        let fallout2 = self.world.fallout2.is_some();
        let header = SaveHeader::parse_for(
            &central,
            if fallout2 {
                crate::GameProfile::Fallout2
            } else {
                crate::GameProfile::Fallout1
            },
        )?;
        // G comes from the installed campaign initializer.
        let initial = self
            .assets
            .gam("data/vault13.gam", vault_formats::GamSection::Campaign)?
            .ok_or_else(|| WorldError::Invalid("campaign initializer did not open".into()))?;
        let lookup = |pid| self.prototype_info(pid);
        // SPEC-018 §3.5, §7.8: the tables are parsed again for the load, and
        // the whole central stream is validated against them before any
        // working file changes.
        let tables = if fallout2 {
            header.check_fallout2(initial.values.len(), self.scripts_gated())?;
            Some(
                self.load_fallout2_tables()
                    .map_err(|error| WorldError::Invalid(error.to_string()))?,
            )
        } else {
            None
        };
        let save = match &tables {
            Some(tables) => SaveGame::parse_fallout2(
                &central,
                initial.values.len(),
                &lookup,
                &crate::fallout2::Fallout2World::shape(tables),
            )?,
            None => SaveGame::parse(&central, initial.values.len(), &lookup)?,
        };
        let mut maps = BTreeMap::new();
        for name in &save.maps {
            let name = std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("save manifest name is not ASCII".into()))?;
            // Validated before any path is formed, so a crafted manifest
            // (`../../X.SAV`, `C:X.SAV`, ...) never reads outside the slot
            // and never enters the working saved-map set.
            let name = checked_companion(name)?.to_string();
            let path = slot_file(&dir, &name)?;
            let bytes = fs::read(&path).map_err(|error| io(&path, error))?;
            // SPEC-008 §3.3 safe rule: every companion map is validated
            // before any working state changes.
            vault_formats::MapFile::parse_for(&bytes, &lookup, self.assets.game_profile())?;
            maps.insert(name, bytes);
        }
        let automap_path = slot_file(&dir, AUTOMAP_FILE)?;
        let automap = fs::read(&automap_path).map_err(|error| io(&automap_path, error))?;
        if i64::try_from(automap.len()).ok() != Some(i64::from(save.automap_length)) {
            return Err(WorldError::Invalid(
                "AUTOMAP.SAV length differs from the recorded length".into(),
            ));
        }
        let automap = if tables.is_some() {
            AutomapDb::parse_fallout2(automap)?
        } else {
            AutomapDb::parse(automap)?
        };
        // Restore loads the slot's current map first (SPEC-014 §3.35): its
        // name must be usable and its stream present before any state is
        // reset. SPEC-008 §3.10: the name is uppercased and opened like any
        // map name, a `.MAP` name through its saved copy first.
        let map_file = save_map_file(&save);
        let probe = crate::persistence::saved_probe(&map_file).unwrap_or_else(|| map_file.clone());
        let present = maps.contains_key(&probe)
            || self.assets.available(format!("maps/{map_file}").as_bytes());
        if map_file.is_empty() || map_file.contains(['/', '\\']) || !present {
            return Err(WorldError::Invalid(format!(
                "save names no loadable map ({map_file:?})"
            )));
        }
        // The `enhanced` sidecar, read (and validated) under Enhanced only;
        // a slot without one has no companion settings or opened doors.
        let (distances, enhanced) = if self.preset() == crate::EnginePreset::Enhanced {
            let path = dir.join(crate::companion::SIDECAR);
            match fs::read_to_string(&path) {
                Ok(text) => (
                    crate::companion::parse_sidecar(&text).map_err(WorldError::Invalid)?,
                    crate::doors::EnhancedState {
                        opened_doors: crate::doors::parse_table(&text)
                            .map_err(WorldError::Invalid)?,
                    },
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
                Err(error) => return Err(io(&path, error)),
            }
        } else {
            Default::default()
        };
        match tables {
            Some(tables) => self.apply_fallout2_save(save, maps, automap, tables)?,
            None => self.apply_save(save, maps, automap)?,
        }
        self.world.companion_distance = distances;
        self.world.enhanced = enhanced;
        Ok(())
    }
    /// SPEC-018 §7: restore a validated Fallout 2 slot: its world state is
    /// installed before the current map loads (whose number and flags it
    /// drives), then the saved position, the kill types 15-18 and the party
    /// records; the gate notice follows a successful load.
    pub fn apply_fallout2_save(
        &mut self,
        mut save: SaveGame,
        maps: BTreeMap<String, Vec<u8>>,
        automap: AutomapDb,
        tables: std::sync::Arc<crate::fallout2::Fallout2Tables>,
    ) -> Result<(), WorldError> {
        let payload = save
            .fallout2
            .take()
            .ok_or_else(|| WorldError::Invalid("not a Fallout 2 save".into()))?;
        let mut notes = Vec::new();
        let state = crate::fallout2::Fallout2World::restore(
            tables,
            &payload.world_map,
            payload.movies,
            &mut notes,
        );
        let position = state.position;
        // The member records follow the saved identifiers, in order.
        let party: Vec<u32> = save.party.iter().map(|id| *id as u32).collect();
        self.install_fallout2 = Some(Box::new(state));
        let result = self.apply_save(save, maps, automap);
        self.install_fallout2 = None;
        result?;
        let state = self
            .world
            .fallout2
            .as_deref_mut()
            .ok_or_else(|| WorldError::Invalid("Fallout 2 state was not installed".into()))?;
        state.position = position;
        state.kills = payload.kills;
        state.control.clear();
        for (id, record) in party.iter().zip(&payload.records) {
            let record = crate::fallout2::Fallout2World::sanitize(*record, &mut notes);
            if record.record != -1 {
                state.control.insert(*id, record);
            }
        }
        state.retained = payload
            .retained
            .iter()
            .map(|(id, record)| {
                (
                    *id as u32,
                    crate::fallout2::Fallout2World::sanitize(*record, &mut notes),
                )
            })
            .collect();
        for note in notes {
            self.world.diagnostic(note);
        }
        self.fallout2_notice();
        Ok(())
    }
    /// Reset like a new game, keeping rules and generator state (the random
    /// component has no bytes and is not restored, so the fresh world's
    /// startup diagnostic draws are skipped).
    fn reset_game(&mut self) -> Result<(), WorldError> {
        for program in self.programs.keys().copied().collect::<Vec<_>>() {
            self.registry.purge(program)?;
            self.library.cleanup(program);
        }
        // SPEC-005 §2.2: a full game reset clears the whole exported-variable
        // registry, all owners.
        self.registry.clear();
        self.programs.clear();
        self.clear_program_bindings();
        self.motions.clear();
        self.building_animation = None;
        self.pending_dialogue = None;
        let sheet = self.world.player_sheet().cloned().unwrap_or_default();
        let mut world = WorldState::fresh(sheet, 1)?;
        world.rules = std::mem::take(&mut self.world.rules);
        world.rng = self.world.rng.clone();
        // The head updater's idle state lasts the whole program run.
        world.dialogue.idle = self.world.dialogue.idle;
        // The SPEC-009 §3.1 packet table and taunt catalog come from game
        // initialization, not from the game, so they outlive a reset (the
        // catalog with its language-filter state).
        let ai = &self.world.ai;
        world.ai.packets = ai.packets.clone();
        world.ai.messages = ai.messages.clone();
        world.ai.filtered = ai.filtered;
        world.ai.filter_state = ai.filter_state;
        // The `fallout.cfg` preferences the world reads belong to the
        // program run as well: a load restores only the save's own words.
        world.combat_speed = self.world.combat_speed;
        world.player_speedup = self.world.player_speedup;
        world.target_highlight = self.world.target_highlight;
        world.verbose_combat_messages = self.world.verbose_combat_messages;
        // SPEC-018 §7: a Fallout 2 load installs its restored state here,
        // before the current map loads.
        world.fallout2 = self.install_fallout2.take();
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
        let map_file = save_map_file(&save);
        self.load_map(&format!("maps/{map_file}"))?;
        self.restore_player(&save)?;
        // SPEC-018 §6.5 (Revision 3): a Fallout 2 player record's locals.
        let player_locals = self
            .world
            .fallout2
            .is_some()
            .then(|| self.assets.local_count(crate::world::PLAYER_SCRIPT_FILE));
        let world = &mut self.world;
        // SPEC-014 §3.44 / §3.36: the restored player record is a placement
        // on its hex: it is reinserted there, and the roof tracking moves
        // from the entrance square to the player's square.
        let player = world.player;
        world.reinsert(player);
        // SPEC-006 §3.5: the player record sets the last attacker again.
        world.resolve_last_attacker(player);
        if let Some(hex) = save.player.hex {
            world.place_player_roof(hex, save.player.elevation);
        }
        world.set_view_center(save.viewport);
        world.elevation = save.player.elevation;
        world.sneak_working = save.sneak_working != 0;
        world.kills = save.kills;
        world.persistence.automap_flags = save.automap_flags;
        world.persistence.preferences = save.preferences;
        world.difficulty = save.preferences[0] as i32;
        world.combat_difficulty = Some(save.preferences[1] as i32);
        world.violence = Some(save.preferences[2] as i32);
        world.running_burning = save.preferences[4] as i32;
        world.ai.combat_taunts = Some(save.preferences[6] as i32);
        world.ai.language_filter = Some(save.preferences[7] as i32);
        world.world_map = WorldMap::from_saved(save.world_map.clone());
        world.persistence.movies = save.movies;
        world.persistence.skill_uses = save.skill_uses;
        world.persistence.interface = save.interface;
        world.ui_disabled = save.interface[0] == 0;
        world.active_hand = save.interface[2];
        world.set_next_object_id(save.next_item_id.max(1) as u32);
        world.queue = Default::default();
        // SPEC-008 §2.6: an identifier that matches no object leaves the
        // event without an owner.
        for record in &save.queue {
            let kind = event_kind(record)?;
            let object = match record.owner {
                -2 => 0,
                id => world.reference_for_id(id as u32).unwrap_or_else(|| {
                    world.diagnostic(format!(
                        "timed event owner {id} is not on the restored map; restored without an owner"
                    ));
                    0
                }),
            };
            world.queue.insert(u64::from(record.due), 0, object, kind)?;
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
        // SPEC-006 §3.5: the saved player record (with its action code) is
        // not restored; the player gets a newly created one, and the saved
        // party list's records get both marks again.
        let saved_script = save.player.script_id;
        world.replace_player_script(saved_script, player_locals)?;
        world.set_party_marks(true);
        // No AI state refers to objects from before the load.
        self.world.ai.snapshot.clear();
        self.world.ai.callbacks.clear();
        self.world.ai.requested = None;
        if save.combat.state & state::ACTIVE != 0 {
            self.resume_combat(&save.combat);
        }
        if let Err(error) = self.reevaluate_language_filter() {
            self.world.diagnostic(format!("language filter: {error}"));
        }
        if let Some(sheet) = self.world.player_sheet_mut() {
            sheet.name = text(&save.header.name).to_vec();
        }
        let name = text(&save.header.name).to_vec();
        if let Some(player) = self.world.get_mut(self.world.player) {
            player.name = name;
        }
        self.world.rebuild_lighting();
        // SPEC-005 §3.8: the gender update ends the load.
        self.update_player_gender();
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
        for (perk, rank) in Perk::ALL.into_iter().zip(save.perks) {
            if rank > 0 {
                sheet.perks.insert(perk, rank as u32);
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
        sheet.pending_perk = save.pending_perk != 0;
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
            highlight: world.target_highlight,
            ..CombatState::default()
        };
        // SPEC-009 §3.2 (improved): rebuild the snapshot from the restored
        // list, in its restored order.
        world.ai.snapshot = world.combat_state.list.clone();
        world.combat = true;
        world.free_movement = saved.free_movement;
        world.combat_experience = saved.experience;
    }
}
