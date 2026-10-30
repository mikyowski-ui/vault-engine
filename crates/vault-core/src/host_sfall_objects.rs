//! SPEC-024b §4.8: the prototype and object fields, by the offsets scripts
//! name. Offsets address fields by meaning, not by the `.pro` layout.
use crate::{assets::WorldAssets, host::WorldHost, world::ObjectRef};
use std::sync::Arc;
use vault_formats::{
    map::{ObjectDetail, ObjectState},
    pro::{ExtendedFlags, ItemData, Light, Prototype, PrototypeData},
};

/// One prototype field, read and written as a 32-bit value.
enum Field<'a> {
    Int(&'a mut i32),
    Word(&'a mut u32),
    Byte(&'a mut u8),
    /// A value the record shape fixes: read only.
    Fixed(i32),
}
impl Field<'_> {
    fn get(&self) -> i32 {
        match self {
            Self::Int(value) => **value,
            Self::Word(value) => **value as i32,
            Self::Byte(value) => i32::from(**value),
            Self::Fixed(value) => *value,
        }
    }
    fn set(self, new: i32) -> bool {
        match self {
            Self::Int(value) => *value = new,
            Self::Word(value) => *value = new as u32,
            Self::Byte(value) => *value = new as u8,
            Self::Fixed(_) => return false,
        }
        true
    }
}
/// `array[(offset - first) / 4]`, for an offset on the array's 4-byte grid.
fn element(array: &mut [i32], offset: i32, first: i32) -> Option<Field<'_>> {
    let index = offset.checked_sub(first)?;
    if index < 0 || index % 4 != 0 {
        return None;
    }
    array.get_mut((index / 4) as usize).map(Field::Int)
}
fn item_type(data: &ItemData) -> i32 {
    match data {
        ItemData::Armor(_) => 0,
        ItemData::Container { .. } => 1,
        ItemData::Drug(_) => 2,
        ItemData::Weapon(_) => 3,
        ItemData::Ammunition { .. } => 4,
        ItemData::Misc { .. } => 5,
        ItemData::Key { .. } => 6,
    }
}
fn item_field(item: &mut vault_formats::pro::Item, offset: i32) -> Option<Field<'_>> {
    match offset {
        32 => return Some(Field::Fixed(item_type(&item.data))),
        108 => return Some(Field::Int(&mut item.material)),
        112 => return Some(Field::Int(&mut item.size)),
        116 => return Some(Field::Int(&mut item.weight)),
        120 => return Some(Field::Int(&mut item.cost)),
        124 => return Some(Field::Word(&mut item.inventory_art)),
        128 => return Some(Field::Byte(&mut item.stored_byte)),
        _ => {}
    }
    match &mut item.data {
        ItemData::Weapon(weapon) => Some(match offset {
            36 => Field::Int(&mut weapon.animation),
            40 | 44 => return element(&mut weapon.damage, offset, 40),
            48 => Field::Int(&mut weapon.damage_type),
            52 | 56 => return element(&mut weapon.ranges, offset, 52),
            60 => Field::Int(&mut weapon.projectile_pid),
            64 => Field::Int(&mut weapon.strength),
            68 | 72 => return element(&mut weapon.ap_cost, offset, 68),
            76 => Field::Int(&mut weapon.critical_failure),
            80 => Field::Int(&mut weapon.perk),
            84 => Field::Int(&mut weapon.burst),
            88 => Field::Int(&mut weapon.caliber),
            92 => Field::Int(&mut weapon.ammo_pid),
            96 => Field::Int(&mut weapon.capacity),
            100 => Field::Byte(&mut weapon.sound),
            _ => return None,
        }),
        ItemData::Armor(armor) => match offset {
            36 => Some(Field::Int(&mut armor.armor_class)),
            40..=64 => element(&mut armor.resistances, offset, 40),
            68..=92 => element(&mut armor.thresholds, offset, 68),
            96 => Some(Field::Int(&mut armor.perk)),
            100 => Some(Field::Int(&mut armor.male_art)),
            104 => Some(Field::Int(&mut armor.female_art)),
            _ => None,
        },
        ItemData::Container { capacity, flags } => match offset {
            36 => Some(Field::Int(capacity)),
            40 => Some(Field::Word(&mut flags.0)),
            _ => None,
        },
        ItemData::Ammunition {
            caliber,
            quantity,
            armor_modifier,
            resistance_modifier,
            multiplier,
            divisor,
        } => Some(Field::Int(match offset {
            36 => caliber,
            40 => quantity,
            44 => armor_modifier,
            48 => resistance_modifier,
            52 => multiplier,
            56 => divisor,
            _ => return None,
        })),
        ItemData::Misc {
            power_pid,
            power_type,
            ..
        } => Some(Field::Int(match offset {
            36 => power_pid,
            40 => power_type,
            _ => return None,
        })),
        _ => None,
    }
}
fn critter_field(critter: &mut vault_formats::pro::Critter, offset: i32) -> Option<Field<'_>> {
    match offset {
        32 => Some(Field::Word(&mut critter.flags.0)),
        36..=172 => element(&mut critter.base_stats, offset, 36),
        176..=312 => element(&mut critter.bonus_stats, offset, 176),
        316..=384 => element(&mut critter.skills, offset, 316),
        388 => Some(Field::Int(&mut critter.body)),
        392 => Some(Field::Int(&mut critter.experience)),
        396 => Some(Field::Int(&mut critter.kill_category)),
        400 => Some(Field::Fixed(critter.damage_type() as i32)),
        404 => Some(Field::Word(&mut critter.head_art)),
        408 => Some(Field::Int(&mut critter.ai)),
        412 => Some(Field::Int(&mut critter.team)),
        _ => None,
    }
}
fn prototype_field(prototype: &mut Prototype, offset: i32) -> Option<Field<'_>> {
    match offset {
        0 => return Some(Field::Word(&mut prototype.pid)),
        4 => return Some(Field::Int(&mut prototype.message_base)),
        8 => return Some(Field::Word(&mut prototype.art_id)),
        12 | 16 => {
            let light = prototype.light.get_or_insert(Light {
                radius: 0,
                intensity: 0,
            });
            let field = if offset == 12 {
                &mut light.radius
            } else {
                &mut light.intensity
            };
            return Some(Field::Int(field));
        }
        20 => return Some(Field::Word(&mut prototype.flags.0)),
        24 => {
            let ExtendedFlags(word) = &mut prototype.extended;
            return Some(Field::Word(word));
        }
        28 => return Some(Field::Int(prototype.script.get_or_insert(-1))),
        _ => {}
    }
    match &mut prototype.data {
        PrototypeData::Item(item) => item_field(item, offset),
        PrototypeData::Critter(critter) => critter_field(critter, offset),
        _ => None,
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// `get_proto_data`: the field, -1 for an invalid PID, and 0 with a
    /// diagnostic for an offset not listed.
    pub(crate) fn sfall_proto_get(&mut self, pid: i32, offset: i32) -> i32 {
        let Ok(prototype) = self.assets.prototype(pid as u32) else {
            return -1;
        };
        let mut copy = (*prototype).clone();
        match prototype_field(&mut copy, offset) {
            Some(field) => field.get(),
            None => {
                self.world.diagnostic(format!(
                    "get_proto_data: offset {offset} of {pid:#x} is not a listed field"
                ));
                0
            }
        }
    }
    /// `set_proto_data`: changes the in-memory prototype, here and on every
    /// object holding it. It lasts until the prototype reloads and is not
    /// saved.
    pub(crate) fn sfall_proto_set(&mut self, pid: i32, offset: i32, value: i32) {
        let Ok(prototype) = self.assets.prototype(pid as u32) else {
            self.world
                .diagnostic(format!("set_proto_data: no prototype {pid:#x}"));
            return;
        };
        let mut copy = (*prototype).clone();
        let done =
            offset != 0 && prototype_field(&mut copy, offset).is_some_and(|field| field.set(value));
        if !done {
            self.world.diagnostic(format!(
                "set_proto_data: offset {offset} of {pid:#x} is not a writable field"
            ));
            return;
        }
        let copy = Arc::new(copy);
        self.assets.replace_prototype(copy.clone());
        let holders: Vec<ObjectRef> = self
            .world
            .iter()
            .filter(|(_, object)| object.raw.pid == pid as u32)
            .map(|(reference, _)| reference)
            .collect();
        for holder in holders {
            if let Some(object) = self.world.get_mut(holder) {
                object.prototype = Some(copy.clone());
            }
        }
    }
    /// `get_object_data`: the listed field, or 0 with a diagnostic.
    pub(crate) fn sfall_object_get(&mut self, object: ObjectRef, offset: i32) -> i32 {
        let Some(found) = self.world.get(object) else {
            return 0;
        };
        let raw = &found.raw;
        let critter = match &raw.state {
            ObjectState::Critter(state) => Some(state),
            ObjectState::Other { .. } => None,
        };
        let value = match (offset, critter, &raw.state) {
            (0x00, ..) => Some(raw.id as i32),
            (0x04, ..) => Some(raw.hex.unwrap_or(-1)),
            (0x18, ..) => Some(raw.frame as i32),
            (0x1c, ..) => Some(i32::from(raw.orientation)),
            (0x20, ..) => Some(raw.art_id as i32),
            (0x24, ..) => Some(raw.flags.0 as i32),
            (0x28, ..) => Some(i32::from(raw.elevation)),
            (0x38, _, ObjectState::Other { word, .. }) => Some(*word as i32),
            (0x38, Some(_), _) => Some(0),
            (0x64, ..) => Some(raw.pid as i32),
            (0x68, ..) => Some(raw.combat_id),
            (0x6c, ..) => Some(raw.light.radius),
            (0x70, ..) => Some(raw.light.intensity),
            (0x78, ..) => Some(raw.script_id),
            (0x80, ..) => Some(raw.cached_script_file),
            (
                0x3c,
                None,
                ObjectState::Other {
                    detail: ObjectDetail::Charges(charges) | ObjectDetail::Key(charges),
                    ..
                },
            ) => Some(*charges),
            (0x3c, Some(state), _) => Some(state.maneuvers as i32),
            (0x40, Some(state), _) => Some(state.ap),
            (0x44, Some(state), _) => Some(state.results.0 as i32),
            (0x48, Some(state), _) => Some(state.damage),
            (0x54, Some(state), _) => Some(state.attacker),
            (0x58, Some(state), _) => Some(
                found
                    .character
                    .as_ref()
                    .map_or(state.hp, |sheet| sheet.current_hp),
            ),
            _ => None,
        };
        value.unwrap_or_else(|| {
            self.world.diagnostic(format!(
                "get_object_data: offset {offset:#x} of {object:#x} is not a listed field"
            ));
            0
        })
    }
    /// `set_object_data`: the listed field, through the ordinary state, so
    /// the hash sees what it covers.
    pub(crate) fn sfall_object_set(&mut self, object: ObjectRef, offset: i32, value: i32) {
        let Some(found) = self.world.get_mut(object) else {
            return;
        };
        let raw = &mut found.raw;
        let done = match offset {
            0x00 => {
                raw.id = value as u32;
                true
            }
            0x04 => {
                raw.hex = (value >= 0).then_some(value);
                true
            }
            0x18 => {
                raw.frame = value.max(0) as usize;
                true
            }
            0x1c => {
                raw.orientation = (value.rem_euclid(6)) as u8;
                true
            }
            0x20 => {
                raw.art_id = value as u32;
                true
            }
            0x24 => {
                raw.flags.0 = value as u32;
                true
            }
            0x28 => {
                raw.elevation = value.clamp(0, 2) as u8;
                true
            }
            0x64 => {
                raw.pid = value as u32;
                true
            }
            0x68 => {
                raw.combat_id = value;
                true
            }
            0x6c => {
                raw.light.radius = value;
                true
            }
            0x70 => {
                raw.light.intensity = value;
                true
            }
            0x78 => {
                raw.script_id = value;
                true
            }
            0x80 => {
                raw.cached_script_file = value;
                true
            }
            _ => match (offset, &mut raw.state) {
                (0x38, ObjectState::Other { word, .. }) => {
                    *word = value as u32;
                    true
                }
                (
                    0x3c,
                    ObjectState::Other {
                        detail: ObjectDetail::Charges(charges) | ObjectDetail::Key(charges),
                        ..
                    },
                ) => {
                    *charges = value;
                    true
                }
                (0x3c, ObjectState::Critter(state)) => {
                    state.maneuvers = value as u32;
                    true
                }
                (0x40, ObjectState::Critter(state)) => {
                    state.ap = value;
                    true
                }
                (0x44, ObjectState::Critter(state)) => {
                    state.results.0 = value as u32;
                    true
                }
                (0x48, ObjectState::Critter(state)) => {
                    state.damage = value;
                    true
                }
                (0x54, ObjectState::Critter(state)) => {
                    state.attacker = value;
                    true
                }
                (0x58, ObjectState::Critter(state)) => {
                    state.hp = value;
                    if let Some(sheet) = found.character.as_mut() {
                        sheet.current_hp = value;
                    }
                    true
                }
                _ => false,
            },
        };
        if !done {
            self.world.diagnostic(format!(
                "set_object_data: offset {offset:#x} of {object:#x} is not a listed field"
            ));
        }
    }
    /// The weapon detail of `item`: loaded rounds and ammunition PID.
    pub(crate) fn sfall_weapon(&mut self, item: ObjectRef) -> Option<(&mut i32, &mut i32)> {
        match &mut self.world.get_mut(item)?.raw.state {
            ObjectState::Other {
                detail: ObjectDetail::Weapon { rounds, ammo_pid },
                ..
            } => Some((rounds, ammo_pid)),
            _ => None,
        }
    }
}
