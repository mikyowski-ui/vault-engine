//! SPEC-014 §3.26-3.29 pointer handling in the interactive world: the
//! cursor drawn into the frame (the system cursor is hidden), hover
//! feedback once the pointer rests 250 ms, map clicks on the left release,
//! the held-button action menu, and edge and arrow-key scrolling.
//!
//! Time is the shell clock passed to `tick`, and the desktop logs each
//! frame's clock, so a replay of its input log ticks at the same times and
//! sees the same rests, holds and scroll steps (D-11).
use super::{MapPreview, Screen};
use crate::interface::Painter;
use vault_core::{
    ObjectAction, ObjectRef,
    assets::WorldAssets,
    critical::location,
    ui::{
        ColorKey,
        bar::{self, Action},
        cursor::{self, Entry, MenuTracker, Mode, MoveHover, Primary, Target},
    },
};
use vault_formats::{
    Fid, HexGrid,
    pro::{BasicFlags, Category},
};
use vault_render::{
    IndexedImage,
    map::{object_sprite, pick_candidates},
};

/// The scroll blocker (misc PID `0x0500000C`).
const SCROLL_BLOCKER: u32 = 0x0500_000c;

/// What a movement hover depends on (§3.27): the hovered hex, the player's
/// hex and elevation, combat, crippled legs, free movement, AP and the route
/// blockers. A rest with the same inputs reuses the last result instead of
/// planning the path again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteKey {
    hex: i32,
    player: Option<(i32, u8)>,
    combat: bool,
    legs: u8,
    free_movement: i32,
    ap: i32,
    blockers: u64,
}

/// What the hover computed for the current rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Feedback {
    /// Movement: the AP cost in white, or a red `X`.
    Cost(Vec<u8>, ColorKey),
    /// Arrow: the primary action's icon beside the arrow.
    Pick(&'static str),
    /// Crosshair over a critter: `NN%` in white, or ` X ` in red.
    Hit(Vec<u8>, ColorKey),
}

/// A left press in the map area, waiting for its release.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Held {
    pub point: [i32; 2],
    /// The next combined pressed-and-held pulse: strictly more than 250 ms
    /// after the press, then after each pulse.
    next_pulse: u64,
}

/// The open action menu.
#[derive(Debug, Clone)]
pub(crate) struct ActionMenu {
    pub object: ObjectRef,
    pub target: Target,
    pub entries: Vec<Entry>,
    pub tracker: MenuTracker,
    /// The pointer where the menu opened.
    pub origin: [i32; 2],
}

/// Edge scrolling under way: the direction and whether its last step was
/// blocked.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Edge {
    pub direction: usize,
    pub blocked: bool,
}

#[derive(Debug, Default)]
pub(crate) struct PointerState {
    /// The pointer's frame pixel while it is over the window.
    pub at: Option<[i32; 2]>,
    /// When the pointer came to rest at `at`.
    rest_since: u64,
    /// The hover ran for this rest.
    pub rested: bool,
    pub feedback: Option<Feedback>,
    /// The object whose look-at text the monitor last received.
    looked: Option<ObjectRef>,
    pub held: Option<Held>,
    pub menu: Option<ActionMenu>,
    pub edge: Option<Edge>,
    /// The last scroll attempt (edge or arrow key), for the 33 ms gate.
    last_scroll: Option<u64>,
    /// The cursor mode, player turn and interface state the hover last saw.
    seen: Option<(Mode, bool, bool)>,
    /// The last movement hover's inputs and feedback.
    route: Option<(RouteKey, Option<Feedback>)>,
    /// The item the arrow hover outlines, until the pointer moves.
    pub outlined: Option<ObjectRef>,
    /// Paths planned for movement hovers.
    #[cfg(test)]
    pub routes_planned: u32,
}

/// The loot and barter windows' cursor art.
fn window_cursor(counter: bool, arrow: bool, over_entry: bool) -> &'static str {
    use vault_core::ui::inventory::{ARROW_CURSOR, HAND_CURSOR};
    match (counter, arrow, over_entry) {
        (true, ..) => ARROW_CURSOR,
        (false, true, true) => "actpick.frm",
        (false, true, false) => ARROW_CURSOR,
        (false, false, _) => HAND_CURSOR,
    }
}

/// The map area's last row (the bar window starts below it).
fn map_bottom() -> i32 {
    bar::BAR_ORIGIN[1]
}

/// Text with a one-pixel black outline.
fn outlined(
    painter: &Painter,
    image: &mut IndexedImage,
    key: ColorKey,
    text: &[u8],
    origin: [i32; 2],
) {
    for dy in -1..=1 {
        for dx in -1..=1 {
            if (dx, dy) != (0, 0) {
                painter.text(
                    image,
                    101,
                    ColorKey::BLACK,
                    text,
                    [origin[0] + dx, origin[1] + dy],
                    80,
                );
            }
        }
    }
    painter.text(image, 101, key, text, origin, 80);
}

/// SPEC-014 §3.27 the movement cursor's translucent red outline (G-31): one
/// pixel around the hex outline's opaque pixels, above the bar window's
/// top row, each pixel an even mix of red (key 31744) and the pixel
/// beneath. The specification names the outline but not its geometry or
/// translucency, so this rule is provisional.
fn red_outline(
    palette: &vault_formats::Palette,
    image: &mut IndexedImage,
    frame: &vault_formats::frm::FrmFrame,
    origin: [i64; 2],
) {
    let (width, height) = (i64::from(frame.width), i64::from(frame.height));
    let opaque = |x: i64, y: i64| {
        (0..width).contains(&x)
            && (0..height).contains(&y)
            && frame.pixels()[(y * width + x) as usize] != 0
    };
    let stride = i64::from(image.width());
    let [red, ..] = ColorKey::RED.rgb5();
    for y in -1..=height {
        for x in -1..=width {
            if opaque(x, y) || !(-1..=1).any(|dy| (-1..=1).any(|dx| opaque(x + dx, y + dy))) {
                continue;
            }
            let (px, py) = (origin[0] + x, origin[1] + y);
            if !(0..stride).contains(&px) || !(0..i64::from(map_bottom())).contains(&py) {
                continue;
            }
            let pixel = &mut image.pixels_mut()[(py * stride + px) as usize];
            let [r, g, b] = palette.rgb6(*pixel).map(|level| level / 2);
            if let Some(mixed) = palette.lookup_rgb5([(r + red) / 2, g / 2, b / 2]) {
                *pixel = mixed;
            }
        }
    }
}

impl MapPreview {
    /// Hover, menu and edge input apply: the map is in front with no
    /// conversation or notice over it.
    fn world_pointer(&self) -> bool {
        matches!(self.screen, Screen::Map)
            && !self.host.world.dialogue.open
            && self.dialog.is_none()
            && self.hud.timer.is_none()
            && self.hud.called.is_none()
    }
    /// The item a window's drag holds, drawn as the cursor.
    fn dragged_item(&self) -> Option<ObjectRef> {
        match &self.screen {
            Screen::Loot(screen) if screen.counter.is_none() => {
                screen.held.map(|(_, item, _)| item)
            }
            Screen::Inventory(screen) => screen.dragged(),
            Screen::Barter(screen) => screen.dragged(),
            _ => None,
        }
    }
    /// §3.27 the object under a frame pixel: none under a visible roof
    /// outside the player's cutout mask; otherwise the topmost solid one
    /// that is not a dead or knocked-out critter, falling back to the
    /// bottom-most one hit.
    pub(crate) fn object_at(&self, point: [i32; 2]) -> Option<ObjectRef> {
        // P-02: the world's map by reference; no map clone per pick.
        let objects = self.host.world.render_objects();
        let map = &self.host.world.map;
        let egg = self.egg.clone().flatten();
        let extended = |pid: u32| {
            self.host
                .assets
                .prototype(pid)
                .map_or(0, |prototype| prototype.extended.0)
        };
        let cutout = self.cutout(egg.as_deref(), &extended);
        let hits: Vec<_> =
            pick_candidates(map, &objects, self.view, &self.art, point, cutout.as_ref())?
                .into_iter()
                .filter_map(|(id, solid)| Some((self.host.world.reference_for_id(id)?, solid)))
                .collect();
        let world = &self.host.world;
        let down = |object: ObjectRef| {
            world
                .get(object)
                .is_some_and(|o| o.category() == Category::Critter)
                && (world.is_dead(object)
                    || world
                        .critter_state(object)
                        .is_some_and(|s| s.results.0 & 0x01 != 0))
        };
        hits.iter()
            .find(|(object, solid)| *solid && !down(*object))
            .or_else(|| hits.last())
            .map(|(object, _)| *object)
    }
    /// Restart the hover's rest now, or back-date it 250 ms so the next
    /// idle poll computes it (the in-world cursor turned back on).
    fn restart_rest(&mut self, back_date: bool) {
        self.pointer.rest_since = if back_date {
            self.clock.saturating_sub(cursor::HOVER_REST_MS)
        } else {
            self.clock
        };
        self.pointer.rested = false;
        self.pointer.feedback = None;
        self.pointer.outlined = None;
    }
    /// The pointer moved to a frame pixel: the rest restarts, the hover art
    /// reverts, and an open menu tracks the vertical motion.
    pub(crate) fn pointer_moved(&mut self, point: [i32; 2]) {
        let previous = self.pointer.at;
        if previous == Some(point) {
            return;
        }
        self.pointer.at = Some(point);
        // Entering the map window turns the in-world cursor back on.
        let entered_map =
            point[1] <= map_bottom() && previous.is_none_or(|[_, y]| y > map_bottom());
        self.restart_rest(entered_map);
        if let Some(menu) = &mut self.pointer.menu {
            menu.tracker.moved(point[1]);
        }
        self.window_pointer(point);
        // Leaving the map window out of combat drops the crosshair, use and
        // skill modes back to movement.
        let left_map = previous.is_some_and(|[_, y]| y <= map_bottom()) && point[1] > map_bottom();
        if left_map
            && self.world_pointer()
            && !self.host.world.combat
            && matches!(
                self.hud.mode,
                Mode::Crosshair | Mode::UseCrosshair | Mode::Skill(_)
            )
        {
            self.hud.mode = Mode::Movement;
        }
    }
    /// The pointer left the window: no cursor, hover or scrolling.
    pub fn pointer_left(&mut self) {
        // SPEC-014 §3.2: not polled during a game fade; applied after it.
        if self.fade_blocks() {
            self.fade_pointer = Some(None);
            return;
        }
        self.pointer.at = None;
        self.pointer.rested = false;
        self.pointer.feedback = None;
        self.pointer.outlined = None;
        self.pointer.edge = None;
    }
    /// A left press in the map area: the click waits for the release, and
    /// holding it in arrow mode opens the action menu.
    pub(crate) fn map_press(&mut self, point: [i32; 2]) {
        self.pointer.held = Some(Held {
            point,
            next_pulse: self.clock + cursor::MENU_HOLD_MS,
        });
    }
    /// A left release: the action menu's entry, or the map click.
    pub(crate) fn map_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let held = self.pointer.held.take();
        if let Some(menu) = self.pointer.menu.take() {
            // The pointer returns to the menu's column at the last anchor
            // (G-31: the system pointer is warped there too), and the rest
            // restarts.
            let at = [menu.origin[0], menu.tracker.anchor];
            self.pointer.at = Some(at);
            self.hud.warp = Some(at);
            self.restart_rest(false);
            let entry = if self.quit_requested {
                Entry::Cancel
            } else {
                menu.entries
                    .get(menu.tracker.highlight)
                    .copied()
                    .unwrap_or(Entry::Cancel)
            };
            self.menu_entry(entry, menu.object, menu.target);
            return true;
        }
        // Click-to-scroll is never on in R1, so an edge press still clicks.
        let (Some(_), Some(point)) = (held, point) else {
            return false;
        };
        // §3.26: map clicks are ignored while the animated watch shows.
        if !self.world_pointer()
            || point[1] > map_bottom()
            || self.host.world.ui_disabled
            || self.watch_shown()
        {
            return false;
        }
        let object = self.object_at(point);
        let mode = self.hud.mode;
        self.map_click(point, object);
        // A crosshair attack marks the hover done at the click position: the
        // to-hit text stays as it was until the pointer moves.
        if mode == Mode::Crosshair && self.pointer.at == Some(point) {
            self.pointer.rested = true;
        }
        true
    }
    /// G-22: a press in the map belongs to the map. Once a window, a
    /// conversation or a dialog box is in front, its release no longer
    /// reaches the map, so the press (and any action menu) is dropped;
    /// kept, it would open the menu later with the button up.
    pub(crate) fn drop_map_press(&mut self) {
        if !self.world_pointer() {
            self.pointer.held = None;
            self.pointer.menu = None;
        }
    }
    /// Keys are dropped while the action menu is open or edge scrolling is
    /// under way.
    pub(crate) fn pointer_swallows_keys(&self) -> bool {
        self.world_pointer() && (self.pointer.menu.is_some() || self.pointer.edge.is_some())
    }
    /// One shell frame of pointer time: the held-button repeat that opens
    /// the menu, edge scrolling, and the hover after a rest. Returns
    /// whether the frame changed.
    pub(crate) fn pointer_tick(&mut self) -> bool {
        if !self.world_pointer() {
            self.drop_map_press();
            return self.pointer.edge.take().is_some();
        }
        // A mode change restarts the rest; the start of a player turn, and
        // the interface coming back on, force the hover on the next poll.
        // The watch going away turns the in-world cursor back on too.
        let watch = self.watch_shown();
        let seen = (
            self.hud.mode,
            self.host.world.combat && self.host.world.combat_state.player_turn,
            self.host.world.ui_disabled || watch,
        );
        if let Some((mode, turn, disabled)) = self.pointer.seen.replace(seen) {
            if mode != seen.0 {
                self.restart_rest(false);
            }
            if (!turn && seen.1) || (disabled && !seen.2) {
                self.restart_rest(true);
            }
        }
        // §3.26-§3.27: while the watch shows the game mouse is disabled: a
        // held press is dropped, and no menu, edge scroll or hover runs.
        if watch {
            self.pointer.held = None;
            let scrolling = self.pointer.edge.take().is_some();
            return scrolling | self.pointer.feedback.take().is_some();
        }
        let mut changed = self.menu_repeat();
        if self.pointer.menu.is_some() {
            return changed;
        }
        changed |= self.edge_scroll();
        changed | self.hover_tick()
    }
    /// §3.28 the combined pressed-and-held pulses: in arrow mode, the
    /// first pulse with an object under the pointer opens the action menu
    /// (`iaccuxx1`). In other modes a held button does nothing.
    fn menu_repeat(&mut self) -> bool {
        let Some(held) = &mut self.pointer.held else {
            return false;
        };
        if self.pointer.menu.is_some() || self.clock <= held.next_pulse {
            return false;
        }
        held.next_pulse = self.clock + cursor::MENU_HOLD_MS;
        let point = self.pointer.at.unwrap_or(held.point);
        if self.hud.mode != Mode::Arrow || self.host.world.ui_disabled {
            return false;
        }
        let Some(object) = self.object_at(point) else {
            return false;
        };
        let Some(target) = self.target_kind(object) else {
            return false;
        };
        let at = self.pointer.at.unwrap_or(point);
        let entries = cursor::menu_entries(target, self.host.world.combat);
        self.pointer.menu = Some(ActionMenu {
            object,
            target,
            tracker: MenuTracker::new(at[1], entries.len()),
            entries,
            origin: at,
        });
        self.pointer.feedback = None;
        self.play_effect(cursor::MENU_OPEN_SOUND.as_bytes(), None);
        true
    }
    /// §3.28 the chosen entry.
    fn menu_entry(&mut self, entry: Entry, object: ObjectRef, target: Target) {
        self.selected = Some(object);
        // §3.27: an entry that starts a sequence follows the click policy.
        let act = |preview: &mut Self, action: ObjectAction| {
            if preview.action_allowed(object, action) {
                preview.interact(action);
            }
        };
        match entry {
            Entry::Use => act(
                self,
                if matches!(target, Target::Item { .. }) {
                    ObjectAction::Pickup
                } else {
                    ObjectAction::Use
                },
            ),
            Entry::Look => self.interact(ObjectAction::Description),
            Entry::Talk => act(self, ObjectAction::Talk),
            Entry::Rotate => {
                self.game_action(Action::Rotate(true));
            }
            Entry::Inventory => self.open_inventory_on(Some(object)),
            Entry::Skill => self.open_skilldex_on(Some(object)),
            Entry::Cancel => {}
        }
    }
    /// §3.29 a pointer on an edge pixel scrolls the map one step at most
    /// every 33 ms; a step the map cannot take shows the blocked art.
    /// Leaving the edge restores the previous cursor and turns the
    /// in-world cursor back on.
    fn edge_scroll(&mut self) -> bool {
        let direction = self
            .pointer
            .at
            .and_then(|[x, y]| cursor::edge_direction(x, y));
        let Some(direction) = direction else {
            let left = self.pointer.edge.take().is_some();
            if left {
                self.restart_rest(true);
            }
            return left;
        };
        let Some(blocked) = self.scroll_step(direction) else {
            return false;
        };
        self.pointer.edge = Some(Edge { direction, blocked });
        true
    }
    /// One scroll step (an edge or an arrow key) through the shared 33 ms
    /// gate: `None` when throttled, else whether the step was blocked. A
    /// diagonal is one step, blocked as a whole, and a blocked attempt also
    /// restarts the gate. The new center is the hex at the current center
    /// plus 32 pixels across and 24 down per step; it is blocked when there
    /// is no such hex or `center_refused` refuses it.
    pub(crate) fn scroll_step(&mut self, direction: usize) -> Option<bool> {
        if self
            .pointer
            .last_scroll
            .is_some_and(|last| self.clock < last + cursor::EDGE_PERIOD_MS)
        {
            return None;
        }
        self.pointer.last_scroll = Some(self.clock);
        let target = self.scroll_target(direction);
        if let Some(hex) = target {
            self.host.world.set_view_center(hex);
            if let Ok(grid) = HexGrid::centered(hex, self.view.width, self.view.height) {
                self.view.grid = grid;
            }
            self.last_hover = None;
        }
        Some(target.is_none())
    }
    fn scroll_target(&self, direction: usize) -> Option<i32> {
        let grid = HexGrid::default();
        let world = &self.host.world;
        let center = grid.hex_center(world.camera_hex).ok()?;
        let [dx, dy] = cursor::edge_step(direction).map(i64::from);
        let point = [center[0] + dx, center[1] + dy];
        let hex = grid.pick_hex([i32::try_from(point[0]).ok()?, i32::try_from(point[1]).ok()?])?;
        if !(0..40000).contains(&hex) {
            return None;
        }
        (!self.center_refused(hex)).then_some(hex)
    }
    /// §3.29 the tests a scrolled view center must pass (edge and arrow
    /// scrolling and the Home walk): scroll limiting's player-distance test,
    /// in the current view's pixels; no scroll blocker on the hex on the
    /// current elevation (safe rule: hex 0 too); and the map border.
    fn center_refused(&self, hex: i32) -> bool {
        let world = &self.host.world;
        let view = HexGrid::centered(world.camera_hex, self.view.width, self.view.height);
        let player = world.get(world.player).and_then(|object| object.raw.hex);
        let limited = match (view, player) {
            (Ok(view), Some(player)) => {
                match (view.hex_reference(hex), view.hex_reference(player)) {
                    (Ok(target), Ok(player)) => {
                        cursor::distance_refused(target, player, world.camera_hex)
                    }
                    _ => true,
                }
            }
            _ => false,
        };
        let elevation = world.elevation;
        let blocker = world.iter().any(|(_, object)| {
            object.raw.pid == SCROLL_BLOCKER
                && object.raw.hex == Some(hex)
                && object.raw.elevation == elevation
                && object.owner.is_none()
        });
        limited || blocker || !cursor::inside_border(hex)
    }
    /// §3.29 Home: the view walks hex by hex toward the player under the
    /// scroll tests; the first refused step ends the walk.
    pub(crate) fn center_on_player(&mut self) {
        let world = &self.host.world;
        let Some(player) = world.get(world.player).and_then(|object| object.raw.hex) else {
            return;
        };
        for _ in 0..400 {
            let center = self.host.world.camera_hex;
            let next = vault_core::routing::rotation(center, player)
                .and_then(|direction| HexGrid::step(center, direction, 1).ok());
            let Some(next) = next.filter(|_| center != player) else {
                break;
            };
            if self.center_refused(next) || !self.host.world.set_view_center(next) {
                break;
            }
        }
        if let Ok(grid) = HexGrid::centered(
            self.host.world.camera_hex,
            self.view.width,
            self.view.height,
        ) {
            self.view.grid = grid;
        }
        self.last_hover = None;
    }
    /// The hover once the pointer has rested for 250 ms, and only again
    /// after it moves.
    fn hover_tick(&mut self) -> bool {
        let Some(at) = self.pointer.at else {
            return false;
        };
        if self.pointer.rested
            || self.clock.saturating_sub(self.pointer.rest_since) < cursor::HOVER_REST_MS
        {
            return false;
        }
        self.pointer.rested = true;
        if at[1] > map_bottom() || self.host.world.ui_disabled || self.pointer.edge.is_some() {
            return false;
        }
        self.pointer.feedback = self.hover_feedback(at);
        self.pointer.feedback.is_some()
    }
    /// §3.27 hover feedback by cursor mode.
    pub(crate) fn hover_feedback(&mut self, at: [i32; 2]) -> Option<Feedback> {
        let player = self.host.world.player;
        match self.hud.mode {
            Mode::Movement => {
                let hex = self.view.grid.pick_hex(at)?;
                let world = &self.host.world;
                let state = world.critter_state(player);
                let flags = state.map_or(0, |state| state.results.0);
                let place = world
                    .get(player)
                    .and_then(|object| Some((object.raw.hex?, object.raw.elevation)));
                let key = RouteKey {
                    hex,
                    player: place,
                    combat: world.combat,
                    legs: u8::from(flags & 0x04 != 0) + u8::from(flags & 0x08 != 0),
                    free_movement: world.free_movement,
                    ap: state.map_or(0, |state| state.ap),
                    blockers: place.map_or(0, |(_, elevation)| {
                        world.route_fingerprint(elevation, player)
                    }),
                };
                if let Some((seen, feedback)) = &self.pointer.route
                    && *seen == key
                {
                    return feedback.clone();
                }
                #[cfg(test)]
                {
                    self.pointer.routes_planned += 1;
                }
                let path = self
                    .host
                    .plan_to_tile(player, hex, usize::MAX)
                    .map(|movement| movement.directions.len() as i32)
                    .filter(|length| *length > 0);
                let feedback =
                    match cursor::move_hover(path, key.combat, key.legs, key.free_movement, key.ap)
                    {
                        MoveHover::Nothing => None,
                        MoveHover::Cost(cost) => Some(Feedback::Cost(
                            cost.to_string().into_bytes(),
                            cursor::COST_COLOR,
                        )),
                        MoveHover::Blocked => {
                            Some(Feedback::Cost(b"X".to_vec(), cursor::BLOCKED_COLOR))
                        }
                    };
                self.pointer.route = Some((key, feedback.clone()));
                feedback
            }
            Mode::Arrow => {
                let object = self.object_at(at)?;
                let target = self.target_kind(object)?;
                // With item highlight on, an item gets the outline unless
                // it has one already or carries the no-highlight flag.
                if matches!(target, Target::Item { .. })
                    && self.preferences.item_highlight != 0
                    && self.host.world.get(object).is_some_and(|item| {
                        item.raw.outline == 0 && !item.raw.flags.contains(BasicFlags::NO_HIGHLIGHT)
                    })
                {
                    self.pointer.outlined = Some(object);
                }
                // A newly hovered object prints its look-at text.
                if self.pointer.looked != Some(object) {
                    self.pointer.looked = Some(object);
                    if let Err(error) = self.host.action(player, object, ObjectAction::LookAt) {
                        self.status = error.to_string().into_bytes();
                    }
                    self.drain_effects();
                }
                Some(Feedback::Pick(
                    match cursor::primary_action(target, self.host.world.combat) {
                        Primary::Use => "usegetn.frm",
                        Primary::Rotate => "rotaten.frm",
                        Primary::Talk => "talkn.frm",
                        Primary::Look => "lookn.frm",
                    },
                ))
            }
            Mode::Crosshair => {
                // Crosshair hovers record the object too, so switching to
                // the arrow over the same critter prints nothing. One pick
                // (P-02: one world snapshot) serves both.
                let hit = self.object_at(at);
                if let Some(object) = hit {
                    self.pointer.looked = Some(object);
                }
                let target = hit.filter(|object| {
                    *object != player
                        && self
                            .host
                            .world
                            .get(*object)
                            .is_some_and(|o| Fid(o.raw.art_id).category() == 1)
                })?;
                let chance = self.hover_hit_chance(target);
                Some(Feedback::Hit(
                    cursor::hit_text(chance),
                    if chance.is_some() {
                        ColorKey::WHITE
                    } else {
                        ColorKey::RED
                    },
                ))
            }
            Mode::UseCrosshair | Mode::Skill(_) => None,
        }
    }
    /// The uncalled torso hit chance of the player's attack request, even
    /// in an aimed mode, or `None` when the preflight refuses it (SPEC-007
    /// §3.2) or the slot reports no attack, as in reload mode (§3.23). The
    /// item slot's aiming still counts in the AP the attack needs.
    fn hover_hit_chance(&self, target: ObjectRef) -> Option<i32> {
        let player = self.host.world.player;
        let world = &self.host.world;
        let (mode, aimed) = self.slot_attack()?;
        let attack = world
            .attack_profile(player, target, mode, location::UNCALLED, aimed)
            .ok()?;
        world.preflight(player, target, &attack).ok()?;
        attack.hit_chance(true).ok()
    }
    /// Draw the cursor at the pointer: a dragged item, the action menu, the
    /// edge-scroll art, the in-world cursor over the map, else the arrow.
    pub(crate) fn draw_cursor(&self, image: &mut IndexedImage) {
        let Some(at) = self.pointer.at else {
            return;
        };
        if matches!(self.screen, Screen::Movie(_)) {
            return;
        }
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        // The Set Timer window shows the arrow over any window, and the
        // called-shot window over the map (SPEC-014 §3.38).
        if self.hud.timer.is_some() || self.hud.called.is_some() {
            painter.cursor(image, cursor::ARROW.art, at);
            return;
        }
        self.draw_outline(&painter, image);
        if let Some(item) = self.dragged_item() {
            if let Some(icon) = self.item_icon(item)
                && let Some(frame) = icon.frame(0, 0)
            {
                image.blit(
                    frame,
                    [
                        i64::from(at[0]) - i64::from(frame.width / 2),
                        i64::from(at[1]) - i64::from(frame.height / 2),
                    ],
                );
            }
            return;
        }
        if let Some(menu) = &self.pointer.menu {
            self.draw_menu(&painter, image, menu);
            return;
        }
        // The inventory-type windows: the hand cursor, or the arrow over a
        // counter (the inventory's item menu hides the pointer).
        match &self.screen {
            Screen::Inventory(screen) => {
                if let Some(art) = screen.cursor() {
                    painter.cursor(image, art, at);
                }
                return;
            }
            // SPEC-013 §3.16 the loot and barter cursors: none while the
            // item menu is open, the arrow over a counter, the pick cursor
            // over a filled entry in the arrow cursor, else the mode's arrow
            // or hand.
            Screen::Loot(screen) => {
                if screen.menu.is_some() {
                    return;
                }
                let art = window_cursor(
                    screen.counter.is_some(),
                    screen.arrow,
                    self.window_entry_under(at),
                );
                painter.cursor(image, art, at);
                return;
            }
            Screen::Barter(screen) => {
                if screen.menu.is_some() {
                    return;
                }
                let art = window_cursor(
                    screen.dragged().is_none() && screen.counter_open(),
                    screen.arrow,
                    self.window_entry_under(at),
                );
                painter.cursor(image, art, at);
                return;
            }
            // SPEC-014 §3.39-§3.40: the world map's scroll art, else the
            // arrow.
            Screen::World(_) | Screen::Town(_) => {
                painter.cursor(image, self.world_cursor(at), at);
                return;
            }
            _ => {}
        }
        // SPEC-014 §3.41.6: the animated watch while the Pip-Boy rests.
        if let Some(frame) = self.pipboy_watch() {
            painter.cursor_frame(image, cursor::ANIMATED_WATCH.art, frame, at);
            return;
        }
        // §3.26: the animated watch replaces the in-world cursor while the
        // combat lock or a hand swap holds.
        if self.world_pointer()
            && let Some((_, frame)) = self.hud.watch
        {
            let art = cursor::ANIMATED_WATCH.art;
            painter.cursor_frame(image, art, frame as usize, at);
            return;
        }
        if let Some(edge) = self.pointer.edge {
            let art = if edge.blocked {
                cursor::SCROLL_BLOCKED
            } else {
                cursor::SCROLL
            }[edge.direction];
            painter.cursor(image, art.art, at);
            return;
        }
        if self.world_pointer() && at[1] <= map_bottom() && !self.host.world.ui_disabled {
            self.draw_world_cursor(&painter, image, at);
            return;
        }
        // SPEC-014 §3.20: over the message monitor's scroll areas, while
        // the bar is enabled, `suparrow.frm` or `sdnarrow.frm`; leaving
        // them restores the arrow.
        if self.world_pointer()
            && let Some(older) = self.monitor_area(at)
        {
            let art = if older {
                cursor::SMALL_UP
            } else {
                cursor::SMALL_DOWN
            };
            painter.cursor(image, art.art, at);
            return;
        }
        painter.cursor(image, cursor::ARROW.art, at);
    }
    /// §3.27 the hovered item's outline in key 30632, drawn with the cursor
    /// so that a move removes it at once. No approved spec gives its
    /// geometry: it is one pixel around the item's opaque pixels, over the
    /// composed map view (provisional).
    fn draw_outline(&self, painter: &Painter, image: &mut IndexedImage) {
        let Some(item) = self.pointer.outlined else {
            return;
        };
        if !self.world_pointer() {
            return;
        }
        let Some(object) = self.host.world.get(item) else {
            return;
        };
        if object.owner.is_some() || object.raw.elevation != self.view.elevation {
            return;
        }
        if let Some((frame, origin)) = object_sprite(self.view, &self.art, &object.raw) {
            let color = painter.color(cursor::ITEM_OUTLINE);
            // The bar window covers row 379 down.
            image.outline(frame, origin, color, [0, i64::from(map_bottom())]);
        }
    }
    fn draw_world_cursor(&self, painter: &Painter, image: &mut IndexedImage, at: [i32; 2]) {
        let feedback = self.pointer.feedback.as_ref();
        match self.hud.mode {
            Mode::Movement => {
                let Some(hex) = self.view.grid.pick_hex(at) else {
                    return;
                };
                let Ok(center) = self.view.grid.hex_center(hex) else {
                    return;
                };
                // The 32-by-16 hex outline drawn as a flat object at the hex,
                // with its translucent red outline (movement mode only).
                if let Some(art) = painter.art(Mode::Movement.art())
                    && let Some(frame) = art.frame(0, 0)
                {
                    let [sx, sy] = art.shifts[0].map(i64::from);
                    let origin = [
                        center[0] + sx - i64::from(frame.width / 2),
                        center[1] + sy - i64::from(frame.height) + 1,
                    ];
                    red_outline(painter.palette, image, frame, origin);
                    image.blit(frame, origin);
                }
                if let Some(Feedback::Cost(text, key)) = feedback {
                    let cell = [center[0] as i32 - 16, center[1] as i32 - 8];
                    let width = painter.width(101, text);
                    outlined(
                        painter,
                        image,
                        *key,
                        text,
                        [cell[0] + (32 - width) / 2, cell[1] + 3],
                    );
                }
            }
            Mode::Arrow => match feedback {
                Some(Feedback::Pick(icon)) => {
                    let (art, shift) = cursor::pick_art(at[0], at[1]);
                    let top = at[1] - shift;
                    if art == "actarrom.frm" {
                        painter.blit(image, art, 0, [at[0] - 28, top]);
                        painter.blit(image, icon, 0, [at[0] - 68, top]);
                    } else {
                        painter.blit(image, "actarrow.frm", 0, [at[0], top]);
                        painter.blit(image, icon, 0, [at[0] + 29, top]);
                    }
                }
                _ => {
                    painter.cursor(image, Mode::Arrow.art(), at);
                }
            },
            Mode::Crosshair => match feedback {
                Some(Feedback::Hit(text, key)) => {
                    painter.cursor(image, "acttohit.frm", at);
                    outlined(
                        painter,
                        image,
                        *key,
                        text,
                        [at[0] - 10 + 22, at[1] - 10 + 1],
                    );
                }
                _ => {
                    painter.cursor(image, Mode::Crosshair.art(), at);
                }
            },
            mode @ (Mode::UseCrosshair | Mode::Skill(_)) => {
                painter.cursor(image, mode.art(), at);
            }
        }
    }
    /// The arrow and the stacked 40-by-40 icons, the highlighted one in its
    /// `h` art; mirrored at the right edge and shifted up at the bottom.
    fn draw_menu(&self, painter: &Painter, image: &mut IndexedImage, menu: &ActionMenu) {
        let [x, y] = menu.origin;
        let top = y - cursor::menu_shift(y, menu.entries.len());
        let mirrored = x + 68 >= 640;
        let (arrow, icons_x) = if mirrored {
            (("actarrom.frm", x - 28), x - 68)
        } else {
            (("actarrow.frm", x), x + 29)
        };
        painter.blit(image, arrow.0, 0, [arrow.1, top]);
        for (index, entry) in menu.entries.iter().enumerate() {
            let (normal, highlighted) = entry.icons();
            let art = if index == menu.tracker.highlight {
                highlighted
            } else {
                normal
            };
            painter.blit(image, art, 0, [icons_x, top + 40 * index as i32]);
        }
    }
}

#[cfg(test)]
impl MapPreview {
    /// A left press and release at one point (a click on the map acts on
    /// the release).
    pub(crate) fn tap(&mut self, point: [i32; 2]) -> bool {
        let pressed = self.click(point, false);
        self.release(Some(point), false) || pressed
    }
    /// Whether the action menu is open.
    pub(crate) fn menu_open(&self) -> bool {
        self.pointer.menu.is_some()
    }
}
