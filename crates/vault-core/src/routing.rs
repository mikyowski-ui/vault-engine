//! SPEC-007 §3.1 hex route search and §3.2 perception, blockers, and rays.
use crate::{ObjectRef, WorldState, world::WorldObject};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
};
use vault_formats::{
    Fid, HexGrid,
    map::ObjectState,
    pro::{BasicFlags, PrototypeData, SceneryData},
};

/// Frontier and expanded stores each have this many slots; reaching this
/// count is a failure, so at most one fewer entry is ever accepted.
pub const ROUTE_SLOTS: usize = 2000;
/// Backtracking stops after this many direction bytes.
pub const MAX_ROUTE: usize = 800;
/// A ray never returns more samples than this.
pub const MAX_RAY_SAMPLES: usize = 200;

/// The directional helper: one step in direction 0-5, refusing to step from
/// any outer-border hex (even inward), so a border hex returns itself.
pub fn step(hex: i32, direction: u8) -> i32 {
    HexGrid::step(hex, direction, 1).unwrap_or(hex)
}

/// `|DX| + |DY| - floor(min(|DX|, |DY|) / 2)` between projected reference
/// corners, in screen pixels.
pub fn heuristic(hex: i32, goal: i32) -> i64 {
    let grid = HexGrid::default();
    match (grid.hex_reference(hex), grid.hex_reference(goal)) {
        (Ok([ax, ay]), Ok([bx, by])) => {
            let (dx, dy) = ((ax - bx).abs(), (ay - by).abs());
            dx + dy - dx.min(dy) / 2
        }
        _ => i64::MAX / 4,
    }
}

/// The hexes of a map, 0-39999.
const HEXES: usize = 40000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RouteNode {
    hex: i32,
    /// The parent's index among the expanded nodes, or -1 for the source.
    parent: i32,
    /// Incoming direction; the source starts at 0, not the actor's facing.
    direction: u8,
    cost: i64,
}

/// A frontier node in its slot. The derived order is the selection order:
/// the lowest heuristic-plus-cost, then the lowest slot. Live slots differ,
/// so `node` never decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FrontierEntry {
    score: i64,
    slot: usize,
    node: RouteNode,
}

/// Heuristic best-first route search with fixed edge costs (not a fully
/// relaxed A*): hexes are marked seen on insertion and never reopened.
///
/// `blocker` is the movement blocker query; `auto_door` grants the actor's
/// permission to traverse a particular blocker. Returns forward direction
/// bytes; empty means no route, a rejected strict goal, or already at goal.
pub fn find_route<B>(
    source: i32,
    goal: i32,
    strict_goal: bool,
    combat: bool,
    mut blocker: impl FnMut(i32) -> Option<B>,
    mut auto_door: impl FnMut(&B) -> bool,
) -> Vec<u8> {
    // Invalid hexes are not validated by the reference before indexing.
    if !(0..40000).contains(&source) || !(0..40000).contains(&goal) {
        return Vec::new();
    }
    if strict_goal && blocker(goal).is_some() {
        return Vec::new();
    }
    let mut seen = vec![false; HEXES];
    seen[source as usize] = true;
    let mut frontier = BinaryHeap::new();
    frontier.push(Reverse(FrontierEntry {
        score: heuristic(source, goal),
        slot: 0,
        node: RouteNode {
            hex: source,
            parent: -1,
            direction: 0,
            cost: 0,
        },
    }));
    // A node takes the lowest vacant slot: the lowest one given back, or
    // else the first never used.
    let mut vacant = BinaryHeap::new();
    let mut unused = 1;
    let mut expanded: Vec<RouteNode> = Vec::new();
    loop {
        let Some(Reverse(FrontierEntry { slot, node, .. })) = frontier.pop() else {
            return Vec::new();
        };
        vacant.push(Reverse(slot));
        if node.hex == goal {
            return backtrack(node, &expanded);
        }
        let parent = expanded.len() as i32;
        expanded.push(node);
        if expanded.len() >= ROUTE_SLOTS {
            return Vec::new();
        }
        for direction in 0..6 {
            let neighbor = step(node.hex, direction);
            if seen[neighbor as usize] {
                continue;
            }
            if neighbor != goal
                && let Some(found) = blocker(neighbor)
                && !auto_door(&found)
            {
                continue;
            }
            if frontier.len() + 1 >= ROUTE_SLOTS {
                return Vec::new();
            }
            let slot = vacant.pop().map_or_else(
                || {
                    unused += 1;
                    unused - 1
                },
                |Reverse(slot)| slot,
            );
            seen[neighbor as usize] = true;
            let turn = if !combat && direction != node.direction {
                10
            } else {
                0
            };
            let cost = node.cost + 50 + turn;
            frontier.push(Reverse(FrontierEntry {
                score: heuristic(neighbor, goal) + cost,
                slot,
                node: RouteNode {
                    hex: neighbor,
                    parent,
                    direction,
                    cost,
                },
            }));
        }
    }
}

/// Collect incoming directions back toward the source, then reverse. Under
/// the SPEC-007 §3.1 safe rule a route longer than 800 steps keeps its first
/// 800 steps from the source (the reference keeps the last 800).
fn backtrack(goal: RouteNode, expanded: &[RouteNode]) -> Vec<u8> {
    let mut directions = Vec::new();
    let mut node = goal;
    // A parent was expanded before its child, so the indexes fall to -1.
    while node.parent != -1 {
        directions.push(node.direction);
        node = expanded[node.parent as usize];
    }
    directions.reverse();
    directions.truncate(MAX_ROUTE);
    directions
}

/// The first eligible movement blockers on a hex, in world order.
#[derive(Clone, Copy, Default)]
struct HexBlockers {
    any: Option<ObjectRef>,
    multihex: Option<ObjectRef>,
}

/// SPEC-007 movement blocker of a hex on the grid, from a table by hex:
/// [`WorldState::movement_blocker_in`] for a whole search.
fn table_blocker(table: &[HexBlockers], hex: i32) -> Option<ObjectRef> {
    table[hex as usize]
        .any
        .or_else(|| (0..6).find_map(|direction| table[step(hex, direction) as usize].multihex))
}

/// Hexes visited by a direction sequence, starting with the source.
pub fn route_hexes(source: i32, directions: &[u8]) -> Vec<i32> {
    let mut hexes = vec![source];
    for direction in directions {
        let next = step(*hexes.last().expect("source"), *direction);
        hexes.push(next);
    }
    hexes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaySample {
    pub hex: i32,
    pub elevation: u8,
    /// Pixel displacement from that hex's center.
    pub offset: [i64; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ray {
    /// The ray's return value: the sample count, or zero when the source
    /// hex was blocked or a 201st sample was attempted.
    pub count: usize,
    pub samples: Vec<RaySample>,
    /// The obstacle-output slot after traversal; cleared when the sample
    /// cap is reached, which ends a caller's cover/retry loop.
    pub obstacle: Option<ObjectRef>,
}

/// Ray parameters: sampling stride, whether an obstacle-output slot exists
/// (without one, no blocker query runs at all), and the initial ignored object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RayOptions {
    pub stride: usize,
    pub check_blockers: bool,
    pub ignore: Option<ObjectRef>,
}

/// Straight-ray traversal: pixel Bresenham between projected hex centers.
///
/// `blocker` is the movement blocker query; at stride 32 a blocker for which
/// `projectile_through` holds is skipped as well as the ignored object.
pub fn cast_ray(
    source: i32,
    target: i32,
    elevation: u8,
    options: RayOptions,
    mut blocker: impl FnMut(i32) -> Option<ObjectRef>,
    projectile_through: impl Fn(ObjectRef) -> bool,
) -> Ray {
    let RayOptions {
        stride,
        check_blockers,
        ignore,
    } = options;
    let grid = HexGrid::default();
    let (Ok(start), Ok(end)) = (grid.hex_center(source), grid.hex_center(target)) else {
        return Ray::default();
    };
    let stride = stride.max(1);
    let mut ray = Ray::default();
    let stops = |found: Option<ObjectRef>| {
        found.filter(|object| {
            Some(*object) != ignore && !(stride == 32 && projectile_through(*object))
        })
    };
    if check_blockers && let Some(object) = stops(blocker(source)) {
        ray.obstacle = Some(object);
        return ray;
    }
    let sample = |x: i64, y: i64| -> RaySample {
        let hex = pick(grid, x, y);
        let center = grid.hex_center(hex.max(0)).unwrap_or([x, y]);
        RaySample {
            hex,
            elevation,
            offset: [x - center[0], y - center[1]],
        }
    };
    let [mut x, mut y] = start;
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let (step_x, step_y) = (dx.signum(), dy.signum());
    let (ax, ay) = (dx.abs(), dy.abs());
    let y_major = ax <= ay;
    let (major, minor) = if y_major { (ay, ax) } else { (ax, ay) };
    let mut error = 2 * minor - major;
    let mut counter = 0;
    let mut previous = source;
    loop {
        let hex = pick(grid, x, y);
        counter += 1;
        if counter == stride {
            if ray.samples.len() == MAX_RAY_SAMPLES {
                // SPEC-007 §3.2 safe rule: the cap clears the obstacle output
                // (the reference leaves the caller's slot unchanged).
                ray.count = 0;
                ray.obstacle = None;
                return ray;
            }
            ray.samples.push(sample(x, y));
            counter = 0;
        }
        if (y_major && y == end[1]) || (!y_major && x == end[0]) {
            ray.obstacle = None;
            break;
        }
        if error >= 0 {
            if y_major {
                x += step_x;
            } else {
                y += step_y;
            }
            error -= 2 * major;
        }
        if y_major {
            y += step_y;
        } else {
            x += step_x;
        }
        error += 2 * minor;
        if hex != previous {
            if check_blockers && let Some(object) = stops(blocker(hex)) {
                ray.obstacle = Some(object);
                break;
            }
            previous = hex;
        }
    }
    if counter > 0 {
        if ray.samples.len() == MAX_RAY_SAMPLES {
            ray.count = 0;
            ray.obstacle = None;
            return ray;
        }
        ray.samples.push(sample(x, y));
    }
    ray.count = ray.samples.len();
    ray
}

fn pick(grid: HexGrid, x: i64, y: i64) -> i32 {
    match (i32::try_from(x), i32::try_from(y)) {
        (Ok(x), Ok(y)) => grid.pick_hex([x, y]).unwrap_or(-1),
        _ => -1,
    }
}

/// Line extension: pixel Bresenham from `from` toward `through`'s center,
/// continuing past it and recording each changed hex, until `count` hexes or
/// an outer-border hex (which is recorded). Not angular-neighbor stepping.
pub fn extend_line(from: i32, through: i32, count: usize) -> Vec<i32> {
    let grid = HexGrid::default();
    let (Ok(start), Ok(end)) = (grid.hex_center(from), grid.hex_center(through)) else {
        return Vec::new();
    };
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    if (dx, dy) == (0, 0) || count == 0 {
        return Vec::new();
    }
    let (step_x, step_y) = (dx.signum(), dy.signum());
    let (ax, ay) = (dx.abs(), dy.abs());
    let y_major = ax <= ay;
    let (major, minor) = if y_major { (ay, ax) } else { (ax, ay) };
    let mut error = 2 * minor - major;
    let [mut x, mut y] = start;
    let mut previous = from;
    let mut hexes = Vec::new();
    // A pixel walk crosses each 32-pixel hex in well under 64 steps.
    for _ in 0..count.saturating_mul(64).min(200_000) {
        if error >= 0 {
            if y_major {
                x += step_x;
            } else {
                y += step_y;
            }
            error -= 2 * major;
        }
        if y_major {
            y += step_y;
        } else {
            x += step_x;
        }
        error += 2 * minor;
        let hex = pick(grid, x, y);
        if hex < 0 {
            break;
        }
        if hex != previous {
            hexes.push(hex);
            previous = hex;
            let [column, row] = HexGrid::coordinates(hex).unwrap_or([0, 0]);
            if hexes.len() == count || matches!(column, 0 | 199) || matches!(row, 0 | 199) {
                break;
            }
        }
    }
    hexes
}

/// The six-direction quantization of the projected relative angle between
/// two hexes (SPEC-005 `rotation_to_tile`).
pub fn rotation(from: i32, to: i32) -> Option<u8> {
    let grid = HexGrid::default();
    let ([ax, ay], [bx, by]) = (grid.hex_reference(from).ok()?, grid.hex_reference(to).ok()?);
    let (dx, dy) = (bx - ax, by - ay);
    if dx == 0 {
        return Some(if dy < 0 { 0 } else { 2 });
    }
    let degrees = ((-dy as f64).atan2(dx as f64) * 180.0 * 0.318_309_886_285_112_2) as i64;
    let mut angle = 90 - degrees;
    if angle < 0 {
        angle += 360;
    }
    Some((angle / 60).min(5) as u8)
}

/// Shot query result: a hard (non-critter) obstruction and intervening bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shot {
    pub hard_obstruction: bool,
    pub cover: u32,
}

impl WorldState {
    fn art_category(&self, object: ObjectRef) -> Option<u8> {
        self.get(object).map(|o| Fid(o.raw.art_id).category())
    }
    pub(crate) fn multihex(&self, object: ObjectRef) -> bool {
        self.get(object)
            .is_some_and(|o| o.raw.flags.contains(BasicFlags::MULTIHEX))
    }
    /// Placed top-level objects on an elevation, by hex, in world order.
    pub fn occupancy(&self, elevation: u8) -> BTreeMap<i32, Vec<ObjectRef>> {
        let mut occupancy: BTreeMap<i32, Vec<ObjectRef>> = BTreeMap::new();
        for (reference, object) in self.iter() {
            if object.owner.is_none()
                && object.active
                && object.raw.elevation == elevation
                && let Some(hex) = object.raw.hex
            {
                occupancy.entry(hex).or_default().push(reference);
            }
        }
        occupancy
    }
    fn blocks_actor(object: ObjectRef, found: &WorldObject, actor: ObjectRef) -> bool {
        object != actor
            && found.raw.flags.0 & (BasicFlags::HIDDEN.0 | BasicFlags::NONBLOCKING.0) == 0
            && matches!(Fid(found.raw.art_id).category(), 1..=3)
    }
    fn eligible_blocker(&self, object: ObjectRef, actor: ObjectRef, neighbor: bool) -> bool {
        self.get(object).is_some_and(|found| {
            Self::blocks_actor(object, found, actor)
                && (!neighbor || found.raw.flags.contains(BasicFlags::MULTIHEX))
        })
    }
    /// SPEC-007 movement blocker: objects on the hex first, then multihex
    /// objects on neighbors in direction order 0-5 (expanding their footprint
    /// by one hex). Items and miscellaneous art never block movement.
    pub fn movement_blocker_in(
        &self,
        occupancy: &BTreeMap<i32, Vec<ObjectRef>>,
        hex: i32,
        actor: ObjectRef,
    ) -> Option<ObjectRef> {
        let at = |hex: i32, neighbor: bool| {
            occupancy.get(&hex).and_then(|objects| {
                objects
                    .iter()
                    .copied()
                    .find(|object| self.eligible_blocker(*object, actor, neighbor))
            })
        };
        at(hex, false).or_else(|| (0..6).find_map(|direction| at(step(hex, direction), true)))
    }
    pub fn movement_blocker(&self, hex: i32, elevation: u8, actor: ObjectRef) -> Option<ObjectRef> {
        self.movement_blocker_in(&self.occupancy(elevation), hex, actor)
    }
    /// A fingerprint of what `route` reads from the objects on an
    /// elevation for `actor`: each eligible blocker's identity, hex and
    /// multihex footprint, and whether the actor may open it. Equal
    /// fingerprints with equal endpoints and combat state give equal routes.
    pub fn route_fingerprint(&self, elevation: u8, actor: ObjectRef) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        for (reference, object) in self.iter() {
            if object.owner.is_none()
                && object.active
                && object.raw.elevation == elevation
                && let Some(hex) = object.raw.hex
                && self.eligible_blocker(reference, actor, false)
            {
                (
                    reference,
                    hex,
                    object.raw.flags.contains(BasicFlags::MULTIHEX),
                    self.may_auto_open(actor, reference),
                )
                    .hash(&mut hasher);
            }
        }
        hasher.finish()
    }
    /// Dedicated sight-blocker query: scenery/wall on the exact hex, ignoring
    /// hidden and light-through objects, with no multihex expansion.
    pub fn sight_blocker(&self, hex: i32, elevation: u8) -> Option<ObjectRef> {
        self.occupancy(elevation).get(&hex).and_then(|objects| {
            objects.iter().copied().find(|object| {
                self.get(*object).is_some_and(|found| {
                    found.raw.flags.0 & (BasicFlags::HIDDEN.0 | BasicFlags::LIGHT_THROUGH.0) == 0
                        && matches!(Fid(found.raw.art_id).category(), 2 | 3)
                })
            })
        })
    }
    /// Auto-door permission: a non-player actor with critter art and a biped
    /// or robotic body, and an unlocked scenery door with a prototype. The
    /// `enhanced` preset also lets the player open some doors outside
    /// combat ([`crate::doors`]).
    pub fn may_auto_open(&self, actor: ObjectRef, door: ObjectRef) -> bool {
        let (Some(mover), Some(blocker)) = (self.get(actor), self.get(door)) else {
            return false;
        };
        (actor != self.player || self.player_may_auto_open(door))
            && Fid(mover.raw.art_id).category() == 1
            && mover
                .character
                .as_ref()
                .is_some_and(|sheet| matches!(sheet.body, 0 | 2))
            && blocker.prototype.as_ref().is_some_and(|prototype| {
                matches!(
                    prototype.data,
                    PrototypeData::Scenery(vault_formats::pro::Scenery {
                        data: SceneryData::Door { .. },
                        ..
                    })
                )
            })
            && blocker
                .lock()
                .is_some_and(|lock| !lock.contains(vault_formats::pro::LockFlags::LOCKED))
    }
    /// The route search for an actor on its elevation, with this world's
    /// blocker query and auto-door permission; combat removes turn surcharges.
    pub fn route(&self, actor: ObjectRef, source: i32, goal: i32, strict_goal: bool) -> Vec<u8> {
        let elevation = self.get(actor).map_or(self.elevation, |o| o.raw.elevation);
        let blockers = self.blockers_by_hex(elevation, actor);
        find_route(
            source,
            goal,
            strict_goal,
            self.combat,
            |hex| table_blocker(&blockers, hex),
            |door| self.may_auto_open(actor, *door),
        )
    }
    /// The placed objects [`Self::occupancy`] lists that can block `actor`,
    /// for one search. A hex outside the grid is never asked about.
    fn blockers_by_hex(&self, elevation: u8, actor: ObjectRef) -> Vec<HexBlockers> {
        let mut table = vec![HexBlockers::default(); HEXES];
        for (reference, object) in self.iter() {
            if object.owner.is_none()
                && object.active
                && object.raw.elevation == elevation
                && let Some(entry) = object
                    .raw
                    .hex
                    .and_then(|hex| table.get_mut(usize::try_from(hex).ok()?))
                && Self::blocks_actor(reference, object, actor)
            {
                entry.any.get_or_insert(reference);
                if object.raw.flags.contains(BasicFlags::MULTIHEX) {
                    entry.multihex.get_or_insert(reference);
                }
            }
        }
        table
    }
    /// Hex distance reduced by one for each multihex participant, at least 0.
    pub fn object_distance(&self, a: ObjectRef, b: ObjectRef) -> i32 {
        let distance = self.distance(a, b);
        if distance >= 9999 {
            return distance;
        }
        (distance - i32::from(self.multihex(a)) - i32::from(self.multihex(b))).max(0)
    }
    /// A straight ray from `source`'s hex using the movement blocker for the
    /// casting actor.
    pub fn ray(
        &self,
        actor: ObjectRef,
        from: i32,
        to: i32,
        stride: usize,
        ignore: Option<ObjectRef>,
    ) -> Ray {
        let elevation = self.get(actor).map_or(self.elevation, |o| o.raw.elevation);
        let occupancy = self.occupancy(elevation);
        cast_ray(
            from,
            to,
            elevation,
            RayOptions {
                stride,
                check_blockers: true,
                ignore,
            },
            |hex| self.movement_blocker_in(&occupancy, hex, actor),
            |object| {
                self.get(object)
                    .is_some_and(|o| o.raw.flags.contains(BasicFlags::PROJECTILE_THROUGH))
            },
        )
    }
    /// SPEC-007 §3.2 forward perception sector: the target's direction is
    /// the observer's facing or either adjacent orientation.
    pub fn in_forward_sector(&self, observer: ObjectRef, target: ObjectRef) -> bool {
        let (Some(watcher), Some(seen)) = (self.get(observer), self.get(target)) else {
            return false;
        };
        let (Some(from), Some(to)) = (watcher.raw.hex, seen.raw.hex) else {
            return false;
        };
        let facing = watcher.raw.orientation;
        rotation(from, to).is_some_and(|direction| {
            direction == facing || direction == (facing + 1) % 6 || direction == (facing + 5) % 6
        })
    }
    /// SPEC-007 perception predicate (no obstacle ray).
    pub fn can_perceive(&self, observer: ObjectRef, target: ObjectRef) -> bool {
        let (Some(watcher), Some(seen)) = (self.get(observer), self.get(target)) else {
            return false;
        };
        if watcher.raw.hex.is_none() || seen.raw.hex.is_none() {
            return false;
        }
        let perception = watcher
            .character
            .as_ref()
            .map_or(0, |sheet| sheet.stat(1, self.context(observer)));
        let maximum = crate::ai::perception_limit(
            perception,
            self.in_forward_sector(observer, target),
            seen.raw.flags.contains(BasicFlags::GLASS),
            // SPEC-013 §3.7: sneak on and working.
            target == self.player
                && self.sneak_working
                && self
                    .player_sheet()
                    .is_some_and(|sheet| sheet.flags & 1 != 0),
            self.combat,
        );
        self.object_distance(observer, target) <= maximum
    }
    /// `obj_can_see_obj`: placed target, perception, and a stride-16 ray whose
    /// first reported movement blocker is the target itself. The ignored
    /// reference is explicitly null (the reference leaves it uninitialized).
    pub fn can_see(&self, observer: ObjectRef, target: ObjectRef) -> bool {
        self.can_perceive(observer, target) && self.clear_sight(observer, target)
    }
    /// `obj_can_see_obj`'s blocker ray alone.
    pub(crate) fn clear_sight(&self, observer: ObjectRef, target: ObjectRef) -> bool {
        let (Some(watcher), Some(seen)) = (self.get(observer), self.get(target)) else {
            return false;
        };
        let (Some(from), Some(to)) = (watcher.raw.hex, seen.raw.hex) else {
            return false;
        };
        self.ray(observer, from, to, 16, None).obstacle == Some(target)
    }
    /// Shooting/cover: repeated stride-32 casts. A non-critter is a hard
    /// obstruction; each other critter adds one cover body, and the next cast
    /// restarts at its center while ignoring it.
    pub fn shot(&self, shooter: ObjectRef, defender: ObjectRef) -> Shot {
        let mut result = Shot {
            hard_obstruction: false,
            cover: 0,
        };
        let (Some(mut from), Some(to)) = (
            self.get(shooter).and_then(|o| o.raw.hex),
            self.get(defender).and_then(|o| o.raw.hex),
        ) else {
            return result;
        };
        let mut ignore = shooter;
        for _ in 0..40000 {
            let Some(obstacle) = self.ray(shooter, from, to, 32, Some(ignore)).obstacle else {
                break;
            };
            if obstacle == defender {
                break;
            }
            if self.art_category(obstacle) != Some(1) {
                result.hard_obstruction = true;
                break;
            }
            result.cover += 1;
            match self.get(obstacle).and_then(|o| o.raw.hex) {
                Some(hex) => from = hex,
                None => break,
            }
            ignore = obstacle;
        }
        result
    }
    /// Current hit points of a critter object.
    pub fn hit_points(&self, object: ObjectRef) -> Option<i32> {
        match &self.get(object)?.raw.state {
            ObjectState::Critter(state) => Some(state.hp),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CharacterSheet, test_support::patched, world::object_record};
    use std::{collections::BTreeSet, sync::Arc};
    use vault_formats::{Prototype, map::ObjectDetail, pro::LockFlags};

    #[derive(Debug, Clone, Copy)]
    struct OldNode {
        hex: i32,
        parent: i32,
        direction: u8,
        cost: i64,
    }

    /// The search as it stood before the frontier heap: a full slot scan per
    /// expansion, with `stop` naming the way the search ended.
    fn old_find_route<B>(
        source: i32,
        goal: i32,
        strict_goal: bool,
        combat: bool,
        stop: &mut Stop,
        mut blocker: impl FnMut(i32) -> Option<B>,
        mut auto_door: impl FnMut(&B) -> bool,
    ) -> Vec<u8> {
        if !(0..40000).contains(&source) || !(0..40000).contains(&goal) {
            return Vec::new();
        }
        if strict_goal && blocker(goal).is_some() {
            return Vec::new();
        }
        let mut seen = vec![false; 40000];
        seen[source as usize] = true;
        let mut frontier: Vec<Option<OldNode>> = vec![None; ROUTE_SLOTS];
        frontier[0] = Some(OldNode {
            hex: source,
            parent: -1,
            direction: 0,
            cost: 0,
        });
        let mut live = 1;
        let mut expanded: Vec<OldNode> = Vec::new();
        loop {
            let mut best: Option<(usize, i64)> = None;
            for (slot, node) in frontier.iter().enumerate() {
                if let Some(node) = node {
                    let score = heuristic(node.hex, goal) + node.cost;
                    if best.is_none_or(|(_, lowest)| score < lowest) {
                        best = Some((slot, score));
                    }
                }
            }
            let Some((slot, _)) = best else {
                *stop = Stop::Exhausted;
                return Vec::new();
            };
            let node = frontier[slot].take().expect("selected slot");
            live -= 1;
            if node.hex == goal {
                *stop = Stop::Reached;
                return old_backtrack(node, &expanded);
            }
            expanded.push(node);
            if expanded.len() >= ROUTE_SLOTS {
                *stop = Stop::ExpandedCap;
                return Vec::new();
            }
            for direction in 0..6 {
                let neighbor = step(node.hex, direction);
                if seen[neighbor as usize] {
                    continue;
                }
                if neighbor != goal
                    && let Some(found) = blocker(neighbor)
                    && !auto_door(&found)
                {
                    continue;
                }
                live += 1;
                if live >= ROUTE_SLOTS {
                    *stop = Stop::FrontierCap;
                    return Vec::new();
                }
                let vacant = frontier
                    .iter()
                    .position(Option::is_none)
                    .expect("a live count below capacity leaves a vacant slot");
                seen[neighbor as usize] = true;
                let turn = if !combat && direction != node.direction {
                    10
                } else {
                    0
                };
                frontier[vacant] = Some(OldNode {
                    hex: neighbor,
                    parent: node.hex,
                    direction,
                    cost: node.cost + 50 + turn,
                });
            }
        }
    }

    fn old_backtrack(goal: OldNode, expanded: &[OldNode]) -> Vec<u8> {
        let mut directions = Vec::new();
        let mut node = goal;
        while node.parent != -1 && directions.len() <= expanded.len() {
            directions.push(node.direction);
            let Some(parent) = expanded.iter().find(|entry| entry.hex == node.parent) else {
                break;
            };
            node = *parent;
        }
        directions.reverse();
        directions.truncate(MAX_ROUTE);
        directions
    }

    /// How an old search ended.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
    enum Stop {
        #[default]
        Refused,
        Reached,
        Exhausted,
        ExpandedCap,
        FrontierCap,
    }

    /// The old `WorldState::route`: a tree of lists built for the query.
    fn old_route(
        world: &WorldState,
        actor: ObjectRef,
        source: i32,
        goal: i32,
        strict: bool,
    ) -> Vec<u8> {
        let elevation = world
            .get(actor)
            .map_or(world.elevation, |o| o.raw.elevation);
        let occupancy = world.occupancy(elevation);
        old_find_route(
            source,
            goal,
            strict,
            world.combat,
            &mut Stop::default(),
            |hex| world.movement_blocker_in(&occupancy, hex, actor),
            |door| world.may_auto_open(actor, *door),
        )
    }

    /// SplitMix64, so every layout follows from the seed.
    struct Seeded(u64);
    impl Seeded {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn below(&mut self, bound: u64) -> i32 {
            (self.next() % bound) as i32
        }
        fn hex_in(&mut self, column: i32, row: i32, span: i32) -> i32 {
            (column + self.below(span as u64)) + 200 * (row + self.below(span as u64))
        }
    }

    const WALL: u32 = 0x0300_0001;
    const DOOR: u32 = 0x0200_0001;
    const CRITTER: u32 = 0x0100_0001;
    const ITEM: u32 = 0x0000_0001;
    const MISC: u32 = 0x0500_0001;

    /// Every call an old and a new search make to their closures, in order.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Calls {
        blocker: Vec<i32>,
        door: Vec<i32>,
    }

    /// A search's route and the calls it made.
    type Logged = (Vec<u8>, Calls);

    /// One closure-driven layout: walls by hex, some of them doors.
    struct Layout {
        blocked: Vec<bool>,
        doors: BTreeSet<i32>,
    }
    impl Layout {
        fn random(random: &mut Seeded) -> (Self, [i32; 3]) {
            let mut layout = Self {
                blocked: vec![false; HEXES],
                doors: BTreeSet::new(),
            };
            let span = [12, 30, 60, 120][random.below(4) as usize];
            // Some patches lie against the border, where a step stays put.
            let (column, row) = match random.below(4) {
                0 => (0, 0),
                1 => (200 - span, 200 - span),
                _ => (random.below(70) + 5, random.below(70) + 5),
            };
            let density = [0, 3, 10, 25, 40, 55][random.below(6) as usize];
            for offset_row in 0..span {
                for offset_column in 0..span {
                    if random.below(100) < density {
                        let hex = column + offset_column + 200 * (row + offset_row);
                        layout.blocked[hex as usize] = true;
                        if random.below(8) == 0 {
                            layout.doors.insert(hex);
                        }
                    }
                }
            }
            let goal = random.hex_in(column, row, span);
            // A ring around the goal stops every search short of it, and
            // the search spends its slots on the way.
            if random.below(4) == 0 {
                for direction in 0..6 {
                    layout.blocked[step(goal, direction) as usize] = true;
                }
            }
            (layout, [column, row, span])
        }
        fn blocker(&self, hex: i32) -> Option<i32> {
            self.blocked[hex as usize].then_some(hex)
        }
    }

    /// Run one search with closures over `layout` that log every call.
    fn logged(
        layout: &Layout,
        search: impl FnOnce(&mut dyn FnMut(i32) -> Option<i32>, &mut dyn FnMut(&i32) -> bool) -> Vec<u8>,
    ) -> Logged {
        let calls = std::cell::RefCell::new(Calls::default());
        let route = search(
            &mut |hex| {
                calls.borrow_mut().blocker.push(hex);
                layout.blocker(hex)
            },
            &mut |door| {
                calls.borrow_mut().door.push(*door);
                layout.doors.contains(door)
            },
        );
        (route, calls.into_inner())
    }

    /// Both searches over one layout, and how the old one ended.
    fn both(
        layout: &Layout,
        source: i32,
        goal: i32,
        strict: bool,
        combat: bool,
    ) -> (Logged, Logged, Stop) {
        let mut stop = Stop::default();
        let old = logged(layout, |blocker, door| {
            old_find_route(source, goal, strict, combat, &mut stop, blocker, door)
        });
        let new = logged(layout, |blocker, door| {
            find_route(source, goal, strict, combat, blocker, door)
        });
        (old, new, stop)
    }

    #[test]
    fn spec007_routes_match_the_old_search_hex_for_hex() {
        let mut random = Seeded(0x5EED_0007);
        let mut stops = BTreeMap::<Stop, u32>::new();
        for _ in 0..300 {
            let (layout, [column, row, span]) = Layout::random(&mut random);
            for _ in 0..4 {
                let source = random.hex_in(column, row, span);
                let goal = random.hex_in(column, row, span);
                let (strict, combat) = (random.below(2) == 0, random.below(2) == 0);
                let (old, new, stop) = both(&layout, source, goal, strict, combat);
                assert_eq!(old, new, "{source} to {goal}, {strict}, {combat}");
                *stops.entry(stop).or_default() += 1;
            }
        }
        // Each way these layouts end a search was exercised. None fills the
        // frontier, whose check is the old one in `frontier.len() + 1` form.
        for stop in [
            Stop::Refused,
            Stop::Reached,
            Stop::Exhausted,
            Stop::ExpandedCap,
        ] {
            assert!(stops.get(&stop).is_some_and(|n| *n >= 5), "{stops:?}");
        }
    }

    #[test]
    fn spec007_routes_match_on_an_open_floor_where_scores_tie() {
        let layout = Layout {
            blocked: vec![false; HEXES],
            doors: BTreeSet::new(),
        };
        let mut random = Seeded(0x5EED_0008);
        for _ in 0..200 {
            let source = random.hex_in(70, 70, 60);
            // Near goals, where many frontier nodes share a score.
            let goal = random.hex_in(70, 70, 60);
            for combat in [false, true] {
                let (old, new, _) = both(&layout, source, goal, false, combat);
                assert_eq!(old, new, "{source} to {goal}, {combat}");
            }
        }
        // The edge cases of the entry checks.
        for (source, goal) in [(-1, 20100), (20100, -1), (40000, 20100), (20100, 40000)] {
            let (old, new, _) = both(&layout, source, goal, false, false);
            assert_eq!(old, new);
            assert!(new.0.is_empty());
        }
        for (source, goal) in [(20100, 20100), (0, 39999), (199, 39800), (0, 1), (20100, 0)] {
            let (old, new, _) = both(&layout, source, goal, false, false);
            assert_eq!(old, new, "{source} to {goal}");
        }
    }

    fn insert(
        world: &mut WorldState,
        pid: u32,
        hex: i32,
        flags: u32,
        prototype: Option<Arc<Prototype>>,
    ) -> ObjectRef {
        let id = world.next_object_id();
        world.set_next_object_id(id + 1);
        let mut raw = object_record(pid, id);
        raw.hex = Some(hex);
        raw.flags = BasicFlags(flags);
        world
            .insert(WorldObject {
                name: Vec::new(),
                description: Vec::new(),
                raw,
                prototype,
                character: None,
                owner: None,
                inventory: Vec::new(),
                money: None,
                active: true,
                opponent: 0,
            })
            .unwrap()
    }

    /// An object of a kind `route` must tell apart, on `hex`.
    fn scatter(world: &mut WorldState, random: &mut Seeded, hex: i32, door: &Arc<Prototype>) {
        let (hidden, nonblocking, multihex) = (
            BasicFlags::HIDDEN.0,
            BasicFlags::NONBLOCKING.0,
            BasicFlags::MULTIHEX.0,
        );
        let object = match random.below(16) {
            0..=5 => insert(world, WALL, hex, 0, None),
            6 => insert(world, WALL, hex, multihex, None),
            7 => insert(world, CRITTER, hex, multihex, None),
            8 => insert(world, CRITTER, hex, 0, None),
            9 => insert(world, WALL, hex, hidden, None),
            10 => insert(world, WALL, hex, nonblocking, None),
            11 => insert(world, ITEM, hex, 0, None),
            12 => insert(world, MISC, hex, 0, None),
            kind => {
                let object = insert(world, DOOR, hex, 0, Some(door.clone()));
                let lock = if kind == 13 { LockFlags::LOCKED.0 } else { 0 };
                world.get_mut(object).unwrap().raw.state = ObjectState::Other {
                    word: 0,
                    detail: ObjectDetail::Door(LockFlags(lock)),
                };
                object
            }
        };
        // Objects `occupancy` leaves out, and a hex past the grid.
        match random.below(24) {
            0 => world.get_mut(object).unwrap().raw.elevation = 1,
            1 => world.get_mut(object).unwrap().active = false,
            2 => world.get_mut(object).unwrap().owner = Some(world.player),
            3 => world.get_mut(object).unwrap().raw.hex = Some(41000),
            _ => {}
        }
    }

    /// A change a tick could make between two queries.
    fn change(world: &mut WorldState, random: &mut Seeded, objects: &[ObjectRef]) {
        let object = objects[random.below(objects.len() as u64) as usize];
        if world.get(object).is_none() {
            return;
        }
        let hex = random.hex_in(80, 80, 40);
        match random.below(7) {
            0..=2 => world.move_object(object, hex, 0, false).unwrap(),
            3 => world.get_mut(object).unwrap().raw.flags.0 ^= BasicFlags::HIDDEN.0,
            4 => {
                let active = world.get(object).unwrap().active;
                world.get_mut(object).unwrap().active = !active;
            }
            5 => {
                if let ObjectState::Other {
                    detail: ObjectDetail::Door(lock),
                    ..
                } = &mut world.get_mut(object).unwrap().raw.state
                {
                    lock.0 ^= LockFlags::LOCKED.0;
                }
            }
            _ => world.erase(object),
        }
    }

    #[test]
    fn spec007_world_routes_match_and_see_every_change_between_queries() {
        let mut random = Seeded(0x5EED_0009);
        let door = patched(2, 0, &[(0x18, 0x800)], &[(0x28, b'A')]);
        let (mut found, mut through_door) = (0, 0);
        for _ in 0..30 {
            let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
            let mover = world
                .spawn(patched(1, 0, &[(0x190, 0)], &[]), 20100, 0)
                .unwrap();
            world.get_mut(world.player).unwrap().raw.hex = Some(20500);
            for _ in 0..(200 + random.below(400)) {
                let hex = random.hex_in(80, 80, 40);
                scatter(&mut world, &mut random, hex, &door);
                // Stacks: the first blocker on a hex decides what a door
                // permission is asked about.
                if random.below(4) == 0 {
                    scatter(&mut world, &mut random, hex, &door);
                }
            }
            let objects: Vec<ObjectRef> = world.iter().map(|(reference, _)| reference).collect();
            let objects: Vec<ObjectRef> = objects
                .into_iter()
                .filter(|object| *object != mover && *object != world.player)
                .collect();
            for _ in 0..24 {
                let actor = if random.below(4) == 0 {
                    world.player
                } else {
                    mover
                };
                let source = random.hex_in(80, 80, 40);
                let goal = random.hex_in(80, 80, 40);
                let strict = random.below(2) == 0;
                world.combat = random.below(4) == 0;
                let new = world.route(actor, source, goal, strict);
                assert_eq!(old_route(&world, actor, source, goal, strict), new);
                found += usize::from(!new.is_empty());
                let doors = world.occupancy(0);
                through_door += usize::from(route_hexes(source, &new).iter().any(|hex| {
                    doors.get(hex).is_some_and(|list| {
                        list.iter()
                            .any(|object| world.may_auto_open(actor, *object))
                    })
                }));
                // No query may answer for the world before this change.
                change(&mut world, &mut random, &objects);
            }
        }
        assert!(found > 100, "{found} routes found");
        assert!(through_door > 5, "{through_door} routes through doors");
    }
}
