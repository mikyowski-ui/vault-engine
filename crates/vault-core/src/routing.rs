//! SPEC-007 §3.1 hex route search and §3.2 perception, blockers, and rays.
use crate::{ObjectRef, WorldState};
use std::collections::BTreeMap;
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

#[derive(Debug, Clone, Copy)]
struct RouteNode {
    hex: i32,
    /// Parent hex, or -1 for the source.
    parent: i32,
    /// Incoming direction; the source starts at 0, not the actor's facing.
    direction: u8,
    cost: i64,
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
    let mut seen = vec![false; 40000];
    seen[source as usize] = true;
    let mut frontier: Vec<Option<RouteNode>> = vec![None; ROUTE_SLOTS];
    frontier[0] = Some(RouteNode {
        hex: source,
        parent: -1,
        direction: 0,
        cost: 0,
    });
    let mut live = 1;
    let mut expanded: Vec<RouteNode> = Vec::new();
    loop {
        // Lowest heuristic-plus-cost; strict comparison keeps the lowest slot.
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
            return Vec::new();
        };
        let node = frontier[slot].take().expect("selected slot");
        live -= 1;
        if node.hex == goal {
            return backtrack(node, &expanded);
        }
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
            live += 1;
            if live >= ROUTE_SLOTS {
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
            frontier[vacant] = Some(RouteNode {
                hex: neighbor,
                parent: node.hex,
                direction,
                cost: node.cost + 50 + turn,
            });
        }
    }
}

/// Collect incoming directions back toward the source, then reverse. Under
/// the SPEC-007 §3.1 safe rule a route longer than 800 steps keeps its first
/// 800 steps from the source (the reference keeps the last 800).
fn backtrack(goal: RouteNode, expanded: &[RouteNode]) -> Vec<u8> {
    let mut directions = Vec::new();
    let mut node = goal;
    // Each expanded node is visited at most once on the parent chain.
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
    fn multihex(&self, object: ObjectRef) -> bool {
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
    fn eligible_blocker(&self, object: ObjectRef, actor: ObjectRef, neighbor: bool) -> bool {
        let Some(found) = self.get(object) else {
            return false;
        };
        object != actor
            && found.raw.flags.0 & (BasicFlags::HIDDEN.0 | BasicFlags::NONBLOCKING.0) == 0
            && matches!(Fid(found.raw.art_id).category(), 1..=3)
            && (!neighbor || found.raw.flags.contains(BasicFlags::MULTIHEX))
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
    /// or robotic body, and an unlocked scenery door with a prototype.
    pub fn may_auto_open(&self, actor: ObjectRef, door: ObjectRef) -> bool {
        let (Some(mover), Some(blocker)) = (self.get(actor), self.get(door)) else {
            return false;
        };
        actor != self.player
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
        let occupancy = self.occupancy(elevation);
        find_route(
            source,
            goal,
            strict_goal,
            self.combat,
            |hex| self.movement_blocker_in(&occupancy, hex, actor),
            |door| self.may_auto_open(actor, *door),
        )
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
        let (Some(watcher), Some(seen)) = (self.get(observer), self.get(target)) else {
            return false;
        };
        let (Some(from), Some(to)) = (watcher.raw.hex, seen.raw.hex) else {
            return false;
        };
        self.can_perceive(observer, target)
            && self.ray(observer, from, to, 16, None).obstacle == Some(target)
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
