//! SPEC-010 world integration: effect names from live objects (unaliased
//! art stems, prototype sound bytes at item 0x38, weapon 0x79, and scenery
//! 0x28, target materials), the effect lookup chain over the asset
//! namespace, dialogue speech paths, and relative effect volume.
use crate::{ObjectRef, WorldEffect, WorldState, assets::WorldAssets, host::WorldHost};
use vault_formats::{
    Fid,
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
    /// Movement cue: the mover's walk (1) or run (19) character effect.
    pub(crate) fn emit_movement_sound(&mut self, object: ObjectRef, run: bool) {
        let name = self.character_sound_name(object, if run { 19 } else { 1 }, 0);
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
    /// message's sound field.
    pub fn speech_file(&self, sound: &[u8]) -> Option<String> {
        let head = self.world.dialogue.head.as_ref()?;
        sfx::speech_path(head.definition.stem.as_bytes(), sound)
    }
}
