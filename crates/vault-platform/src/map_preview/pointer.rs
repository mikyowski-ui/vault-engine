//! SPEC-014 §3.26-3.29 pointer handling in the interactive world: the
//! cursor drawn into the frame (the system cursor is hidden), hover
//! feedback once the pointer rests 250 ms, map clicks on the left release,
//! the held-button action menu, and edge scrolling.
//!
//! Time is the shell clock passed to `tick`, so a replay sees the same
//! rests, holds and scroll steps as the desktop did.
use super::{MapPreview, Screen};
use crate::interface::Painter;
use vault_core::{
    ObjectAction, ObjectRef,
    critical::location,
    ui::{
        ColorKey,
        bar::{self, Action},
        cursor::{self, Entry, MenuTracker, Mode, MoveHover, Primary, Target},
    },
};
use vault_formats::Fid;
use vault_render::{IndexedImage, map::pick_object};

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
    /// The next held-button repeat event (every 250 ms after the first
    /// 250).
    next_repeat: u64,
    /// The press started on an edge pixel: it scrolls, never clicks.
    edge: bool,
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

/// Edge scrolling under way.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Edge {
    pub direction: usize,
    pub blocked: bool,
    last_step: u64,
}

#[derive(Debug, Default)]
pub(crate) struct PointerState {
    /// The pointer's frame pixel while it is over the window.
    pub at: Option<[i32; 2]>,
    /// When the pointer came to rest at `at`.
    rest_since: u64,
    /// The hover ran for this rest.
    rested: bool,
    pub feedback: Option<Feedback>,
    /// The object whose look-at text the monitor last received.
    looked: Option<ObjectRef>,
    pub held: Option<Held>,
    pub menu: Option<ActionMenu>,
    pub edge: Option<Edge>,
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

impl MapPreview {
    /// Hover, menu and edge input apply: the map is in front with no
    /// conversation or notice over it.
    fn world_pointer(&self) -> bool {
        matches!(self.screen, Screen::Map)
            && !self.host.world.dialogue.open
            && self.notice.is_none()
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
    /// The object drawn under a frame pixel.
    pub(crate) fn object_at(&self, point: [i32; 2]) -> Option<ObjectRef> {
        let map = self.host.world.render_snapshot();
        pick_object(&map, self.view, &self.art, point)
            .and_then(|id| self.host.world.reference_for_id(id))
    }
    /// The pointer moved to a frame pixel: the rest restarts, the hover art
    /// reverts, and an open menu tracks the vertical motion.
    pub(crate) fn pointer_moved(&mut self, point: [i32; 2]) {
        let previous = self.pointer.at;
        if previous == Some(point) {
            return;
        }
        self.pointer.at = Some(point);
        self.pointer.rest_since = self.clock;
        self.pointer.rested = false;
        self.pointer.feedback = None;
        if let Some(menu) = &mut self.pointer.menu {
            menu.tracker.moved(point[1]);
        }
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
        self.pointer.at = None;
        self.pointer.rested = false;
        self.pointer.feedback = None;
        self.pointer.edge = None;
    }
    /// A left press in the map area: the click waits for the release, and
    /// holding it in arrow mode opens the action menu.
    pub(crate) fn map_press(&mut self, point: [i32; 2]) {
        self.pointer.held = Some(Held {
            point,
            next_repeat: self.clock + cursor::MENU_HOLD_MS,
            edge: cursor::edge_direction(point[0], point[1]).is_some(),
        });
    }
    /// A left release: the action menu's entry, or the map click.
    pub(crate) fn map_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let held = self.pointer.held.take();
        if let Some(menu) = self.pointer.menu.take() {
            // The pointer returns to the menu's column at the last anchor.
            let at = [menu.origin[0], menu.tracker.anchor];
            self.pointer.at = Some(at);
            self.pointer.rest_since = self.clock;
            self.pointer.rested = false;
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
        let (Some(held), Some(point)) = (held, point) else {
            return false;
        };
        if held.edge
            || !self.world_pointer()
            || point[1] > map_bottom()
            || self.host.world.ui_disabled
        {
            return held.edge;
        }
        let object = self.object_at(point);
        self.map_click(point, object);
        // A crosshair attack refreshes the hover at once.
        if self.hud.mode == Mode::Crosshair && self.pointer.at == Some(point) {
            self.pointer.feedback = self.hover_feedback(point);
            self.pointer.rested = true;
        }
        true
    }
    /// One shell frame of pointer time: the held-button repeat that opens
    /// the menu, edge scrolling, and the hover after a rest. Returns
    /// whether the frame changed.
    pub(crate) fn pointer_tick(&mut self) -> bool {
        if !self.world_pointer() {
            return self.pointer.edge.take().is_some();
        }
        let mut changed = self.menu_repeat();
        if self.pointer.menu.is_some() {
            return changed;
        }
        changed |= self.edge_scroll();
        changed | self.hover_tick()
    }
    /// The held-button repeat events: in arrow mode, one over an object
    /// opens the action menu (`iaccuxx1`).
    fn menu_repeat(&mut self) -> bool {
        let Some(held) = &mut self.pointer.held else {
            return false;
        };
        if self.pointer.menu.is_some() || held.edge || self.clock <= held.next_repeat {
            return false;
        }
        held.next_repeat += cursor::MENU_HOLD_MS;
        let point = held.point;
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
        match entry {
            Entry::Use => self.interact(if matches!(target, Target::Item { .. }) {
                ObjectAction::Pickup
            } else {
                ObjectAction::Use
            }),
            Entry::Look => self.interact(ObjectAction::Description),
            Entry::Talk => self.interact(ObjectAction::Talk),
            Entry::Rotate => {
                self.game_action(Action::Rotate(true));
            }
            Entry::Inventory => self.open_inventory_on(Some(object)),
            Entry::Skill => self.open_skilldex_on(Some(object)),
            Entry::Cancel => {}
        }
    }
    /// §3.29 a pointer on an edge pixel scrolls the map 32 by 24 pixels at
    /// most every 33 ms; a step the map cannot take shows the blocked art.
    /// The map cannot scroll when the view's center would leave the hex
    /// grid (no approved spec gives finer limits).
    fn edge_scroll(&mut self) -> bool {
        let direction = self
            .pointer
            .at
            .and_then(|[x, y]| cursor::edge_direction(x, y));
        let Some(direction) = direction else {
            // Leaving the edge restores the previous cursor.
            return self.pointer.edge.take().is_some();
        };
        if self
            .pointer
            .edge
            .is_some_and(|edge| self.clock < edge.last_step + cursor::EDGE_PERIOD_MS)
        {
            return false;
        }
        let [dx, dy] = cursor::edge_step(direction);
        let mut view = self.view;
        view.pan(-dx, -dy);
        let center = view
            .grid
            .pick_hex([view.width as i32 / 2, view.height as i32 / 2]);
        let blocked = center.is_none();
        if let Some(hex) = center {
            self.view = view;
            self.host.world.camera_hex = hex;
            self.last_hover = None;
        }
        self.pointer.edge = Some(Edge {
            direction,
            blocked,
            last_step: self.clock,
        });
        true
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
                let path = self
                    .host
                    .plan_to_tile(player, hex, usize::MAX)
                    .map(|movement| movement.directions.len() as i32)
                    .filter(|length| *length > 0);
                let state = self.host.world.critter_state(player);
                let flags = state.map_or(0, |state| state.results.0);
                let legs = u8::from(flags & 0x04 != 0) + u8::from(flags & 0x08 != 0);
                let ap = state.map_or(0, |state| state.ap);
                match cursor::move_hover(
                    path,
                    self.host.world.combat,
                    legs,
                    self.host.world.free_movement,
                    ap,
                ) {
                    MoveHover::Nothing => None,
                    MoveHover::Cost(cost) => Some(Feedback::Cost(
                        cost.to_string().into_bytes(),
                        cursor::COST_COLOR,
                    )),
                    MoveHover::Blocked => {
                        Some(Feedback::Cost(b"X".to_vec(), cursor::BLOCKED_COLOR))
                    }
                }
            }
            Mode::Arrow => {
                let object = self.object_at(at)?;
                let target = self.target_kind(object)?;
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
                let target = self.object_at(at).filter(|object| {
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
    /// The uncalled torso hit chance of the player's attack request, or
    /// `None` when the preflight refuses it (SPEC-007 §3.2).
    fn hover_hit_chance(&self, target: ObjectRef) -> Option<i32> {
        let player = self.host.world.player;
        let world = &self.host.world;
        let attack = world
            .attack_profile(
                player,
                target,
                self.attack_mode(),
                location::UNCALLED,
                false,
            )
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
        painter.cursor(image, cursor::ARROW.art, at);
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
                // The 32-by-16 hex outline drawn as a flat object at the hex.
                if let Some(art) = painter.art(Mode::Movement.art())
                    && let Some(frame) = art.frame(0, 0)
                {
                    let [sx, sy] = art.shifts[0].map(i64::from);
                    image.blit(
                        frame,
                        [
                            center[0] + sx - i64::from(frame.width / 2),
                            center[1] + sy - i64::from(frame.height) + 1,
                        ],
                    );
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
}
