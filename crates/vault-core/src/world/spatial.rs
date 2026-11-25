use super::{ObjectRef, ScriptEvent, Transition, WorldEffect, WorldState};
use crate::{Perk, WorldError, camera::ViewFollow};
use std::collections::{BTreeSet, VecDeque};
use vault_formats::{
    HexGrid,
    map::ObjectState,
    pro::{BasicFlags, Light},
};

/// The most holders [`WorldState::seat_held`] follows below the root, as
/// the inventory code's owner-chain walks do.
const HELD_DEPTH: usize = 64;

/// The margin rule of `docs/camera-follow-toggle.md` §3.2: whether the
/// center of `hex`, in the pixels of a view of `view` (width, height)
/// centered on the center of hex `camera` (the default grid's geometry, as
/// the map window draws it), lies less than `margin` pixels from an edge of
/// that view, or outside it. A margin of 0 never fires; a margin above half
/// the width or height is clamped, on that axis, to that half minus one
/// pixel. A hex the geometry cannot place counts as outside.
fn outside_margin(camera: i32, hex: i32, margin: u16, view: [u16; 2]) -> bool {
    if margin == 0 {
        return false;
    }
    let [width, height] = view.map(i64::from);
    let Ok([x, y]) = HexGrid::centered(camera, u32::from(view[0]), u32::from(view[1]))
        .and_then(|grid| grid.hex_center(hex))
    else {
        return true;
    };
    let clamp = |size: i64| i64::from(margin).min((size / 2 - 1).max(0));
    let (mx, my) = (clamp(width), clamp(height));
    x < mx || x > width - mx || y < my || y > height - my
}

impl WorldState {
    /// SPEC-014 §3.29: a view center outside the map border is refused and
    /// leaves the view where it was. Returns whether the view moved there.
    pub fn set_view_center(&mut self, hex: i32) -> bool {
        let inside = crate::ui::cursor::inside_border(hex);
        if inside {
            self.camera_hex = hex;
        }
        inside
    }
    /// The view-center hex that script words (`tile_is_visible`,
    /// `obj_on_screen`) and the idle fidgets measure against, so their
    /// results and random draws do not depend on the follow setting
    /// (`docs/camera-follow-toggle.md` §5, question 3): under
    /// [`ViewFollow::Off`] the player's hex, as [`ViewFollow::Step`] would
    /// have centered it, while the player stands on a hex; otherwise
    /// [`Self::camera_hex`].
    pub fn script_view_hex(&self) -> i32 {
        match self.view_follow {
            ViewFollow::Step => self.camera_hex,
            ViewFollow::Off { .. } => self
                .get(self.player)
                .and_then(|player| player.raw.hex)
                .unwrap_or(self.camera_hex),
        }
    }
    /// Whether a placement of the player on `hex` recenters the view
    /// (`docs/camera-follow-toggle.md` §3.1-3.2). Under [`ViewFollow::Step`]
    /// every placement does. Under [`ViewFollow::Off`] a placement that is
    /// not a walk step (`animated` false: a transition, an elevator,
    /// `move_to`, `override_map_start`) or that changes the elevation does,
    /// and a walk step does when the margin rule fires.
    fn placement_recenters(&self, hex: i32, animated: bool, changed_elevation: bool) -> bool {
        match self.view_follow {
            ViewFollow::Step => true,
            ViewFollow::Off { .. } if !animated || changed_elevation => true,
            ViewFollow::Off { margin, view } => outside_margin(self.camera_hex, hex, margin, view),
        }
    }
    /// SPEC-003 §2.10 `[Must-match]`: every object `holder` holds,
    /// recursively, has the elevation of the object at the root of the
    /// tree, as the map and save readers seat them. Writes `elevation` over
    /// `holder`'s inventory tree (not `holder` itself) by walking the
    /// inventory lists; no other field changes, and the held objects get no
    /// hex. `[Safe rule]` a tree that loops (an object listed again under
    /// itself) or runs deeper than 64 holders is not followed past that
    /// point, and is reported once per walk.
    pub(crate) fn seat_held(&mut self, holder: ObjectRef, elevation: u8) {
        if self
            .get(holder)
            .is_none_or(|object| object.inventory.is_empty())
        {
            return;
        }
        let mut visited = BTreeSet::from([holder]);
        let mut pending = vec![(holder, 0_usize)];
        let mut looped = false;
        while let Some((parent, depth)) = pending.pop() {
            let children: Vec<ObjectRef> = self
                .get(parent)
                .map(|object| object.inventory.iter().map(|(child, _)| *child).collect())
                .unwrap_or_default();
            for child in children {
                if depth >= HELD_DEPTH || !visited.insert(child) {
                    looped = true;
                    continue;
                }
                if let Some(object) = self.get_mut(child) {
                    object.raw.elevation = elevation;
                    pending.push((child, depth + 1));
                }
            }
        }
        if looped {
            self.diagnostic(format!(
                "the inventory of object {holder} loops or is deeper than {HELD_DEPTH}; \
                 its held objects' elevations are left as they are"
            ));
        }
    }
    pub fn move_object(
        &mut self,
        reference: ObjectRef,
        hex: i32,
        elevation: u8,
        animated: bool,
    ) -> Result<(), WorldError> {
        if !(0..40000).contains(&hex) || elevation > 2 {
            return Err(WorldError::Invalid("invalid movement destination".into()));
        }
        let object = self
            .get_mut(reference)
            .ok_or_else(|| WorldError::Invalid("invalid object reference".into()))?;
        let previous = object.raw.hex;
        let previous_elevation = object.raw.elevation;
        let changed_elevation = previous_elevation != elevation;
        if changed_elevation {
            object.opponent = 0;
            if let ObjectState::Critter(state) = &mut object.raw.state {
                state.maneuvers = 0;
            }
        }
        object.raw.hex = Some(hex);
        object.raw.elevation = elevation;
        // SPEC-014 §3.44: a move to a hex always reinserts the object, even
        // onto the hex it is on, and resets its pixel offset.
        object.raw.displacement = [0; 2];
        self.reinsert(reference);
        // SPEC-003 §2.10: what the object holds, recursively, is on its
        // elevation, as a save and load of the world would seat it. Done
        // before anything below can run a script.
        self.seat_held(reference, elevation);
        // SPEC-005 0x80B6/0x80FF: changing a critter's elevation removes its
        // combat membership.
        if changed_elevation {
            self.remove_from_combat(reference);
        }
        if reference == self.player {
            if self.placement_recenters(hex, animated, changed_elevation) {
                self.set_view_center(hex);
            }
            self.set_current_elevation(elevation);
            self.mark_visited(hex);
            self.check_exit_grids(hex, elevation);
            // SPEC-014 §3.36: every placement of the player runs the roof
            // rule after that hex's exit grids are checked.
            self.place_player_roof(hex, elevation);
        }
        // P-02: the light map is rebuilt only when this move can change it.
        if self.move_changes_lighting(reference, previous, previous_elevation, hex, elevation) {
            self.rebuild_lighting();
        }
        if animated && previous != Some(hex) {
            self.check_spatials(reference);
        }
        // SPEC-013 §3.22: the player's elevation change, by placement or
        // by script (`move_to`, `override_map_start`), re-syncs the party.
        if reference == self.player && changed_elevation {
            self.place_party()?;
        }
        Ok(())
    }
    /// SPEC-013 §3.22 party following, by the placement rule with starting
    /// radius 2: each visible party member other than the player, placed
    /// on the map, fills the next free neighbor of the player's hex in NE
    /// through NW order. Run on an elevation change and after the final
    /// player placement of a map change (SPEC-006 §3.5 step 7).
    pub(crate) fn place_party(&mut self) -> Result<(), WorldError> {
        let player = self.player;
        let Some((hex, elevation)) = self
            .get(player)
            .and_then(|o| Some((o.raw.hex?, o.raw.elevation)))
        else {
            return Ok(());
        };
        let members: Vec<_> = self
            .party
            .iter()
            .copied()
            .filter(|member| *member != player)
            .filter(|member| {
                self.get(*member)
                    .is_some_and(|o| o.owner.is_none() && !o.raw.flags.contains(BasicFlags::HIDDEN))
            })
            .collect();
        // SPEC-019 §3.14: under Fallout 2 a hidden entry (the car trunk)
        // moves to the player's hex and elevation without the placement
        // rule, keeping its hidden flag.
        if self.fallout2.is_some() {
            let hidden: Vec<_> = self
                .party
                .iter()
                .copied()
                .filter(|member| *member != player)
                .filter(|member| {
                    self.get(*member).is_some_and(|o| {
                        o.owner.is_none() && o.raw.flags.contains(BasicFlags::HIDDEN)
                    })
                })
                .collect();
            for member in hidden {
                self.move_object(member, hex, elevation, false)?;
            }
        }
        let mut neighbors =
            (0..6).filter_map(|direction| vault_formats::HexGrid::step(hex, direction, 1).ok());
        for member in members {
            let Some(spot) = neighbors
                .by_ref()
                .find(|spot| self.movement_blocker(*spot, elevation, member).is_none())
            else {
                break;
            };
            self.move_object(member, spot, elevation, false)?;
        }
        Ok(())
    }
    /// SPEC-013 §3.22: the player moved onto `hex`: every exit grid there
    /// requests a transition, the last one winning; a request in combat
    /// ends combat.
    pub(crate) fn check_exit_grids(&mut self, hex: i32, elevation: u8) {
        let grid = self
            .iter()
            .filter(|(_, object)| {
                crate::skills::is_exit_grid(object.raw.pid)
                    && object.raw.hex == Some(hex)
                    && object.raw.elevation == elevation
            })
            .filter_map(|(_, object)| match object.raw.state {
                ObjectState::Other {
                    detail:
                        vault_formats::map::ObjectDetail::ExitGrid {
                            map,
                            hex,
                            elevation,
                            orientation,
                        },
                    ..
                } => Some(Transition {
                    map,
                    elevation,
                    hex,
                    facing: orientation,
                }),
                _ => None,
            })
            .last();
        if let Some(transition) = grid {
            if self.combat {
                self.combat_state.flags |= crate::critical::state::END_REQUESTED;
            }
            self.pending_transition = Some(transition);
        }
    }
    /// SPEC-006 §3.5: a later change of the current elevation requests a map
    /// update when a current map script exists. (Map loading assigns the entry
    /// elevation directly, before any map script, and so takes no update path.)
    pub fn set_current_elevation(&mut self, elevation: u8) {
        let changed = self.elevation != elevation;
        self.elevation = elevation;
        // SPEC-014 §3.46 safe rule: an elevation change removes every
        // floating text.
        if changed {
            self.floating.clear();
        }
        if changed && self.map_script.is_some() {
            self.broadcast(ScriptEvent::MapUpdate, 0);
        }
    }
    /// The ambient light `set_light_level` asks for.
    pub(crate) fn ambient_request(percent: i32) -> i64 {
        if percent == 50 {
            40960
        } else {
            (if percent > 50 { 40960 } else { 16384 }) + i64::from(percent) * 24576 / 100
        }
    }
    pub fn ambient_percent(&mut self, percent: i32) {
        self.set_ambient(Self::ambient_request(percent));
    }
    pub fn set_ambient(&mut self, value: i64) {
        self.ambient = self.ambient_of(value);
    }
    /// The ambient light a requested `value` becomes.
    pub(crate) fn ambient_of(&self, value: i64) -> i32 {
        (value + 6553 * i64::from(self.perk(Perk::NightVision))).clamp(16384, 65536) as i32
    }
    pub fn object_light(
        &mut self,
        reference: ObjectRef,
        percent: i32,
        radius: i32,
    ) -> Result<(), WorldError> {
        self.store_object_light(reference, Self::light_request(percent)?, radius)
    }
    /// The intensity `obj_set_light_level` computes from `percent`.
    pub(crate) fn light_request(percent: i32) -> Result<i32, WorldError> {
        let request = if percent == 0 {
            0
        } else {
            i64::from(percent) * 65636 / 100
        };
        i32::try_from(request).map_err(|_| WorldError::Arithmetic)
    }
    pub(crate) fn store_object_light(
        &mut self,
        reference: ObjectRef,
        requested: i32,
        radius: i32,
    ) -> Result<(), WorldError> {
        self.emit(WorldEffect::ObjectLight {
            object: reference,
            requested,
            radius,
        });
        if radius < 0 || requested < 0 {
            return Err(WorldError::Invalid("negative object light".into()));
        }
        let object = self
            .get_mut(reference)
            .ok_or_else(|| WorldError::Invalid("missing light owner".into()))?;
        object.raw.light = Light {
            radius: radius.min(8),
            intensity: requested.min(65536),
        };
        self.rebuild_lighting();
        Ok(())
    }
    /// SPEC-007 §3.9 setting an object's light: the radius is clamped to
    /// at most 8 at the given intensity, and an intensity of 0 or less sets
    /// both to 0.
    pub fn set_light(&mut self, reference: ObjectRef, radius: i32, intensity: i32) {
        if let Some(object) = self.get_mut(reference) {
            object.raw.light = if intensity > 0 {
                Light {
                    radius: radius.min(8),
                    intensity,
                }
            } else {
                Light {
                    radius: 0,
                    intensity: 0,
                }
            };
        }
        self.rebuild_lighting();
    }
    /// The lattice distance beyond a light's radius within which a line
    /// from the source can still pass: `hex_line` rounds each point to the
    /// nearest hex, so an interior tile can lie one hex beyond the distance
    /// of the line's end (`lighting_tests` checks this bound).
    pub(crate) const LIGHT_LINE_SLACK: i32 = 1;
    /// P-02: whether moving `reference` from `from` on `from_elevation` to
    /// `hex` on `elevation` can change the light map, so `move_object`
    /// rebuilds it only then. `rebuild_lighting` reads only the sources
    /// (active, not hidden, lit) and the blockers (not hidden, not
    /// light-through): a hidden mover, or a light-through one without
    /// light, changes neither; a source always does; a blocker changes the
    /// map only where it vacates a hex no other blocker holds, or enters a
    /// hex none held, within reach of a source on that elevation.
    pub(crate) fn move_changes_lighting(
        &self,
        reference: ObjectRef,
        from: Option<i32>,
        from_elevation: u8,
        hex: i32,
        elevation: u8,
    ) -> bool {
        if from == Some(hex) && from_elevation == elevation {
            return false;
        }
        let Some(object) = self.get(reference) else {
            return true;
        };
        if object.raw.flags.contains(BasicFlags::HIDDEN) {
            return false;
        }
        if object.active && object.raw.light.intensity > 0 {
            return true;
        }
        if object.raw.flags.contains(BasicFlags::LIGHT_THROUGH) {
            return false;
        }
        let mut changed = [
            from.map(|from| (from_elevation, from)),
            Some((elevation, hex)),
        ];
        for (other, object) in self.iter() {
            if other == reference
                || object.raw.flags.contains(BasicFlags::HIDDEN)
                || object.raw.flags.contains(BasicFlags::LIGHT_THROUGH)
            {
                continue;
            }
            let Some(held) = object.raw.hex else {
                continue;
            };
            for slot in &mut changed {
                if *slot == Some((object.raw.elevation, held)) {
                    *slot = None;
                }
            }
        }
        let changed: Vec<_> = changed.into_iter().flatten().collect();
        !changed.is_empty()
            && self.iter().any(|(_, object)| {
                object.active
                    && !object.raw.flags.contains(BasicFlags::HIDDEN)
                    && object.raw.light.intensity > 0
                    && object.raw.elevation < 3
                    && object.raw.hex.is_some_and(|center| {
                        let reach = object.raw.light.radius.clamp(0, 8) + Self::LIGHT_LINE_SLACK;
                        changed.iter().any(|(layer, hex)| {
                            *layer == object.raw.elevation
                                && vault_formats::HexGrid::lattice_distance(center, *hex)
                                    .is_ok_and(|distance| distance <= reach)
                        })
                    })
            })
    }
    pub fn rebuild_lighting(&mut self) {
        for layer in &mut self.tile_light {
            layer.fill(655);
        }
        let blockers: BTreeSet<_> = self
            .iter()
            .filter_map(|(_, object)| {
                (!object.raw.flags.contains(BasicFlags::HIDDEN)
                    && !object.raw.flags.contains(BasicFlags::LIGHT_THROUGH))
                .then_some((object.raw.elevation, object.raw.hex?))
            })
            .collect();
        let sources: Vec<_> = self
            .iter()
            .filter_map(|(_, object)| {
                (object.active
                    && !object.raw.flags.contains(BasicFlags::HIDDEN)
                    && object.raw.light.intensity > 0
                    && object.raw.elevation < 3)
                    .then_some((object.raw.hex?, object.raw.elevation, object.raw.light))
            })
            .collect();
        for (center, layer, light) in sources {
            let radius = light.radius.clamp(0, 8);
            let intensity = light.intensity.clamp(0, 65536);
            let mut visited = BTreeSet::new();
            let mut todo = VecDeque::from([(center, 0)]);
            while let Some((hex, distance)) = todo.pop_front() {
                if !visited.insert(hex) || !(0..40000).contains(&hex) {
                    continue;
                }
                let line = hex_line(center, hex);
                if line
                    .iter()
                    .skip(1)
                    .take(line.len().saturating_sub(2))
                    .all(|tile| !blockers.contains(&(layer, *tile)))
                {
                    let value =
                        i64::from(intensity - distance * ((intensity - 655) / (radius + 1)));
                    self.tile_light[layer as usize][hex as usize] += value;
                }
                if distance < radius {
                    for direction in 0..6 {
                        if let Ok(Some(next)) = vault_formats::HexGrid::neighbor(hex, direction) {
                            todo.push_back((next, distance + 1));
                        }
                    }
                }
            }
        }
    }
    pub fn visible_light(&self, reference: ObjectRef) -> i32 {
        let Some(object) = self.get(reference) else {
            return self.ambient;
        };
        let mut local = object
            .raw
            .hex
            .and_then(|hex| {
                self.tile_light
                    .get(object.raw.elevation as usize)?
                    .get(hex as usize)
            })
            .copied()
            .unwrap_or(0);
        if reference == self.player {
            local -= i64::from(object.raw.light.intensity);
        }
        local.max(i64::from(self.ambient)).clamp(0, 65536) as i32
    }
}

/// Deterministic integer cube interpolation used by the host's line/blocker policy.
pub fn hex_line(from: i32, to: i32) -> Vec<i32> {
    let (Ok([ac, ar]), Ok([bc, br])) = (
        vault_formats::HexGrid::coordinates(from),
        vault_formats::HexGrid::coordinates(to),
    ) else {
        return Vec::new();
    };
    let (aq, aqr) = (i64::from(ac), i64::from(ar - (ac + 1) / 2));
    let (bq, bqr) = (i64::from(bc), i64::from(br - (bc + 1) / 2));
    let n = i64::from(vault_formats::HexGrid::lattice_distance(from, to).unwrap_or(0));
    if n == 0 {
        return vec![from];
    }
    let mut result = Vec::new();
    for step in 0..=n {
        let numerators = [
            aq * (n - step) + bq * step,
            aqr * (n - step) + bqr * step,
            (-aq - aqr) * (n - step) + (-bq - bqr) * step,
        ];
        let mut rounded = numerators.map(|value| (value + n / 2).div_euclid(n));
        let errors = std::array::from_fn::<_, 3, _>(|i| (rounded[i] * n - numerators[i]).abs());
        if errors[0] >= errors[1] && errors[0] >= errors[2] {
            rounded[0] = -rounded[1] - rounded[2];
        } else if errors[1] >= errors[2] {
            rounded[1] = -rounded[0] - rounded[2];
        }
        let c = rounded[0];
        let r = rounded[1] + (c + 1).div_euclid(2);
        if (0..200).contains(&c) && (0..200).contains(&r) {
            result.push((c + 200 * r) as i32);
        }
    }
    result
}
