//! SPEC-010 world integration: effect names from live objects (unaliased
//! art stems, prototype sound bytes at item 0x38, weapon 0x79, and scenery
//! 0x28, target materials), the effect lookup chain over the asset
//! namespace, dialogue speech paths, relative effect volume, and the §3.12
//! talking-head speech and lip-sync with its head updater tick.
use crate::{
    ObjectRef, WorldEffect, WorldError, WorldState, assets::WorldAssets, dialogue::LipSync,
    host::WorldHost,
};
use vault_formats::{
    Fid,
    lip::{self, LipError, LipFile},
    pro::{Category, PrototypeData},
    sfx::{self, Opened, Owner, Target},
};

impl WorldState {
    /// What a hit sound knows about its target: items, scenery, and walls
    /// carry their prototype material; other targets have none.
    fn sound_target(&self, target: ObjectRef) -> Target {
        let Some(object) = self.get(target).filter(|_| target != 0) else {
            return Target::None;
        };
        match object.prototype.as_deref().map(|prototype| &prototype.data) {
            Some(PrototypeData::Item(item)) => Target::Material(item.material),
            Some(PrototypeData::Scenery(scenery)) => Target::Material(scenery.material),
            Some(PrototypeData::Wall { material }) => Target::Material(*material),
            _ => Target::Other,
        }
    }
    /// §3.11 relative volume of an effect tied to `object`: its outermost
    /// owner's hex distance to the player against the player's Perception
    /// (full when `on_screen`). No object, or a category other than item,
    /// critter, or scenery, gives full volume.
    pub fn effect_volume(&self, object: Option<ObjectRef>, on_screen: bool) -> i32 {
        let Some(mut object) = object.filter(|object| {
            self.get(*object).is_some_and(|found| {
                matches!(
                    found.category(),
                    Category::Item | Category::Critter | Category::Scenery
                )
            })
        }) else {
            return sfx::FULL_VOLUME;
        };
        for _ in 0..64 {
            match self.get(object).and_then(|found| found.owner) {
                Some(owner) => object = owner,
                None => break,
            }
        }
        sfx::relative_volume(
            on_screen,
            self.distance(object, self.player),
            self.player_stat(1),
        )
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// §3.8 open/close name: scenery uses its scenery sound byte (`A` when
    /// the prototype is unavailable); any other object is treated as an
    /// item with its item sound byte (0 for non-item prototypes).
    pub fn open_sound_name(&self, object: ObjectRef, action: u8) -> Option<Vec<u8>> {
        let found = self.world.get(object)?;
        let data = found.prototype.as_deref().map(|prototype| &prototype.data);
        let opened = if found.category() == Category::Scenery {
            Opened::Scenery(match data {
                Some(PrototypeData::Scenery(scenery)) => Some(scenery.stored_byte),
                _ => None,
            })
        } else {
            Opened::Item(match data {
                Some(PrototypeData::Item(item)) => item.stored_byte,
                _ => 0,
            })
        };
        sfx::open_name(opened, action)
    }
    /// §3.8 character name from the object's art: the unaliased list token
    /// at its art index, the animation suffix, and the extra-selector
    /// overrides. No name when the index is beyond the list or the suffix
    /// rule fails.
    pub fn character_sound_name(
        &self,
        object: ObjectRef,
        animation: u8,
        extra: u8,
    ) -> Option<Vec<u8>> {
        let art = Fid(self.world.get(object)?.raw.art_id);
        let stem = self.assets.art_token(art.category(), art.base())?;
        sfx::character_name(&stem, animation, art.weapon(), extra)
    }
    /// §3.8 weapon name: the weapon's sound byte and damage type (none for
    /// no weapon), the hit mode's variant, and the target's material.
    pub fn weapon_sound_name(
        &self,
        effect: u8,
        weapon: ObjectRef,
        hit_mode: u8,
        target: ObjectRef,
    ) -> Option<Vec<u8>> {
        let gun = self.world.weapon_data(weapon);
        let (sound, damage_type) = gun.map_or((0, 0), |gun| (gun.sound, gun.damage_type));
        let material = sfx::material_letter(effect, self.world.sound_target(target), damage_type);
        sfx::weapon_name(effect, sound, hit_mode, material)
    }
    /// Request a named effect owned by `owner`; no name requests nothing.
    pub(crate) fn emit_effect(&mut self, name: Option<Vec<u8>>, owner: ObjectRef) {
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            self.world.emit(WorldEffect::SoundEffect {
                name,
                owner: Some(owner),
            });
        }
    }
    /// Door and container cue: the open/close name for action 0 (open) or
    /// 1 (close), owned by the object.
    pub(crate) fn emit_open_sound(&mut self, object: ObjectRef, action: u8) {
        let name = self.open_sound_name(object, action);
        self.emit_effect(name, object);
    }
    /// Emit a weapon sound request with its effect name (ready and out of
    /// ammunition always use variant 1).
    pub(crate) fn emit_weapon_sound(&mut self, object: ObjectRef, weapon: ObjectRef, effect: u8) {
        let name = self
            .weapon_sound_name(effect, weapon, crate::critical::hit_mode::RIGHT_PRIMARY, 0)
            .unwrap_or_default();
        self.world.emit(WorldEffect::WeaponSound {
            object,
            weapon,
            effect,
            name,
        });
    }
    /// §3.9 effect lookup over the asset namespace. A critter owner (from an
    /// animation-registered effect) supplies the gender alias; direct
    /// playback passes none.
    pub fn effect_file(&self, name: &[u8], owner: Option<ObjectRef>) -> Option<String> {
        let owner = owner.and_then(|owner| {
            let found = self.world.get(owner)?;
            Some(Owner {
                critter: found.category() == Category::Critter,
                female: found
                    .character
                    .as_ref()
                    .is_some_and(|sheet| sheet.stat(34, self.world.context(owner)) == 1),
            })
        });
        let found = sfx::lookup(name, owner, |stem| {
            self.assets.available(sfx::effect_path(stem).as_bytes())
        });
        found.resolved.map(|stem| sfx::effect_path(&stem))
    }
    /// §3.10 dialogue speech: the active talking head's stem and a
    /// message's sound field. For the loaded speech of that field it is the
    /// audio stem its lip file stored (§3.12).
    pub fn speech_file(&self, sound: &[u8]) -> Option<String> {
        let dialogue = &self.world.dialogue;
        if let Some(lip_sync) = &dialogue.lip_sync
            && dialogue.speech.as_deref() == Some(sound)
        {
            return Some(lip_sync.audio.clone());
        }
        let head = dialogue.head.as_ref()?;
        sfx::speech_path(head.definition.stem.as_bytes(), sound)
    }
    /// §3.12 loading: a reply's speech request for sound field `sound`.
    /// Any loaded speech is stopped first, with no frame drawn. A missing
    /// `.LIP`, or one of another version, plays the audio named by the
    /// field as one phoneme 0 (safe rule); a short file, or zero counts,
    /// abandons the speech, and a speech state left from before then ends
    /// normally on the next tick. The presentation layer plays
    /// `WorldEffect::Speech` and reports its start (`speech_started`).
    pub fn request_speech(&mut self, sound: &[u8]) {
        let Some(head) = self.world.dialogue.head.as_ref() else {
            return;
        };
        let stem = head.definition.stem.clone().into_bytes();
        let (Some(lip_path), Some(named)) =
            (lip::lip_path(&stem, sound), sfx::speech_path(&stem, sound))
        else {
            self.world.diagnostic(format!(
                "speech: head stem {:?} contains a dot",
                String::from_utf8_lossy(&stem)
            ));
            return;
        };
        let loaded = self.world.dialogue.speech.is_some();
        if loaded {
            self.world.emit(WorldEffect::StopSpeech);
        }
        let lip = match self.assets.resource(&lip_path) {
            Err(_) => Ok(None),
            Ok(bytes) => match LipFile::parse(&bytes) {
                Ok(lip) => Ok(Some(lip)),
                Err(LipError::Version(version)) => {
                    self.world
                        .diagnostic(format!("{lip_path}: LIP version {version} is unsupported"));
                    Ok(None)
                }
                Err(error) => Err(error.to_string()),
            },
        };
        let lip = match lip {
            Ok(Some(lip)) if lip.phonemes.is_empty() || lip.markers.is_empty() => {
                Err("no phonemes or markers".to_owned())
            }
            other => other,
        };
        let (lip, audio) = match lip {
            Ok(Some(lip)) => {
                for warning in lip.check() {
                    self.world.diagnostic(format!("{lip_path}: {warning}"));
                }
                let audio = sfx::speech_path(&stem, &lip.stem).unwrap_or(named);
                (lip, audio)
            }
            Ok(None) => (LipFile::silent(), named),
            Err(error) => {
                self.world
                    .diagnostic(format!("{lip_path}: speech abandoned: {error}"));
                if let Some(old) = &mut self.world.dialogue.lip_sync {
                    old.audio_stopped();
                }
                return;
            }
        };
        self.world.dialogue.stop_speech();
        self.world.dialogue.speech = Some(sound.to_vec());
        self.world.dialogue.lip_sync = Some(LipSync::new(lip, audio, None));
        self.world.emit(WorldEffect::Speech(sound.to_vec()));
    }
    /// §3.12 start: the presentation layer started the speech audio, or
    /// could not (no file, no output); then lip-sync stops running and the
    /// next tick ends the speech normally.
    pub fn speech_started(&mut self, started: bool) {
        if let Some(lip_sync) = &mut self.world.dialogue.lip_sync
            && lip_sync.clock.is_none()
        {
            lip_sync.start(started);
        }
    }
    /// The speech audio's playback position in decoded PCM bytes, `None`
    /// once it no longer plays; read by the next head updater tick.
    pub fn speech_clock(&mut self, position: Option<u64>) {
        if let Some(lip_sync) = &mut self.world.dialogue.lip_sync {
            lip_sync.set_clock(position.unwrap_or(0), position.is_some());
        }
    }
    /// §3.12 stop without an ending (Barter, Review, Ask About): release
    /// the speech without drawing a frame.
    pub fn stop_speech(&mut self) {
        if self.world.dialogue.stop_speech() {
            self.world.emit(WorldEffect::StopSpeech);
        }
    }
    /// One head updater tick (SPEC-006 §3.11), from the choice loop's
    /// start: lip-sync while a speech state exists (unless a reaction
    /// transition plays), else the idle fidget. Missing head art keeps the
    /// current frame and is diagnosed once (safe rule); lip-sync runs
    /// whenever a speech state exists (§3.12 safe rule). Returns whether a
    /// frame was drawn.
    pub(crate) fn update_head(&mut self, now_ms: u64) -> Result<bool, WorldError> {
        let dialogue = &mut self.world.dialogue;
        let Some(head) = dialogue.head.as_mut().filter(|_| dialogue.updater) else {
            return Ok(false);
        };
        if dialogue.speech.is_some() && !head.transitioning() {
            let Some(lip_sync) = &mut dialogue.lip_sync else {
                return Ok(false);
            };
            let step = lip_sync.tick();
            let mut drawn = false;
            if let Some(phoneme) = step.draw {
                head.phoneme(usize::from(phoneme))?;
                drawn = true;
            }
            if step.ended {
                dialogue.lip_sync = None;
                dialogue.speech = None;
                head.speech_end();
                drawn = true;
            }
            if step.stop_audio {
                self.world.emit(WorldEffect::StopSpeech);
            }
            return Ok(drawn);
        }
        let art = head.path().and_then(|path| self.assets.frame(&path));
        let mut diagnostic = head.diagnostic.take();
        let mut drawn = false;
        match art {
            Ok(art) => {
                drawn = head.tick(now_ms, art.frame_count(), art.fps, &mut self.world.rng)?;
                diagnostic = diagnostic.or_else(|| head.diagnostic.take());
            }
            Err(error) if head.missing_art.is_none() => {
                head.missing_art = Some(error.to_string());
                diagnostic = Some(format!("talking-head art unavailable: {error}"));
            }
            Err(_) => {}
        }
        if let Some(message) = diagnostic {
            self.world.diagnostic(message);
        }
        Ok(drawn)
    }
}
