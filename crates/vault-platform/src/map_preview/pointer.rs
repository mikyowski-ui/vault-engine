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
use std::sync::Arc;
use vault_core::host_sfall::Preview;
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
use vault_formats::{Fid, FrmArchive, HexGrid, map::outline, pro::Category};
use vault_render::{
    IndexedImage,
    map::{MapView, pick_candidates_backdrop, pick_candidates_cut_backdrop},
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
    /// The pointer's frame pixel while it is over the window (clamped to
    /// the frame when it is in the letterbox).
    pub at: Option<[i32; 2]>,
    /// `at` is a pixel of the `enhanced` world canvas (of any size, all of
    /// it map area, never the bar) rather than of the 640-by-480 frame.
    /// Always false with the Vanilla preset.
    pub world: bool,
    /// H3: the pointer is outside the frame, in the window's letterbox. Its
    /// clamped pixel still hovers and clicks, but it is never an edge for
    /// edge scrolling (the in-game map here, the world map's edges in
    /// `world.rs`).
    pub outside: bool,
    /// When the pointer came to rest at `at`.
    rest_since: u64,
    /// The hover ran for this rest.
    pub rested: bool,
    pub feedback: Option<Feedback>,
    /// The object whose look-at text the monitor last received.
    pub(crate) looked: Option<ObjectRef>,
    pub held: Option<Held>,
    pub menu: Option<ActionMenu>,
    pub edge: Option<Edge>,
    /// The last scroll attempt (edge or arrow key), for the 33 ms gate.
    last_scroll: Option<u64>,
    /// The cursor mode, player turn and interface state the hover last saw.
    seen: Option<(Mode, bool, bool)>,
    /// The last movement hover's inputs and feedback.
    route: Option<(RouteKey, Option<Feedback>)>,
    /// The item the arrow hover outlined (type 16 in its outline word),
    /// until the pointer moves.
    pub outlined: Option<ObjectRef>,
    /// The hover outline was removed: the scene is drawn again.
    pub redraw: bool,
    /// Paths planned for movement hovers.
    #[cfg(test)]
    pub routes_planned: u32,
}

/// SPEC-014 §3.27 the in-world cursor's objects for one refresh: the
/// cursor's art layer (every mode's art, a flat object), movement mode's
/// hex outline frame and its corner, and the AP-text layer (an upright
/// object), all at the hex under the pointer. Each layer is an image of its
/// own origin in the view, with index 0 transparent.
pub(crate) struct CursorObjects {
    pub hex: i32,
    pub art: IndexedImage,
    /// `art`'s top-left corner in the view.
    pub art_origin: [i64; 2],
    pub ring: Option<(Arc<FrmArchive>, [i64; 2])>,
    pub text: Option<IndexedImage>,
    /// `text`'s top-left corner in the view.
    pub text_origin: [i64; 2],
}

impl CursorObjects {
    /// The same objects with view-sized layers at the view's corner.
    #[cfg(test)]
    pub(crate) fn view_sized(self, [width, height]: [u32; 2]) -> Option<Self> {
        let widen = |layer: &IndexedImage, origin: [i64; 2]| {
            let mut wide = IndexedImage::new(width, height).ok()?;
            wide.copy_image(layer, origin.map(|value| value as i32));
            Some(wide)
        };
        Some(Self {
            hex: self.hex,
            art: widen(&self.art, self.art_origin)?,
            art_origin: [0, 0],
            ring: self.ring,
            text: match &self.text {
                Some(text) => Some(widen(text, self.text_origin)?),
                None => None,
            },
            text_origin: [0, 0],
        })
    }
}

/// The font and width limit of the cursor's text.
const CURSOR_FONT: i32 = 101;
const CURSOR_TEXT_LIMIT: i32 = 80;

/// One draw of a cursor layer, in view pixels.
enum Stroke<'a> {
    /// Frame 0 of the art, from its top-left corner.
    Art(Arc<FrmArchive>, [i64; 2]),
    /// Text with a one-pixel black outline.
    Text(ColorKey, &'a [u8], [i32; 2]),
}

impl Stroke<'_> {
    /// Frame 0 of the interface art `name` at `origin`.
    fn art(painter: &Painter, name: &str, origin: [i32; 2]) -> Option<Self> {
        Some(Self::Art(painter.art(name)?, origin.map(i64::from)))
    }
    /// The cursor art `name` with its hotspot ([`Painter::cursor`]) at
    /// `point`.
    fn cursor(painter: &Painter, name: &str, point: [i32; 2]) -> Option<Self> {
        let art = painter.art(name)?;
        let frame = art.frame(0, 0)?;
        let [sx, sy] = art.shifts[0].map(i64::from);
        let origin = [
            i64::from(point[0]) - i64::from(frame.width / 2) + sx,
            i64::from(point[1]) - i64::from(frame.height) + 1 + sy,
        ];
        Some(Self::Art(art, origin))
    }
    /// `[left, top, right, bottom]` around every view pixel it can write,
    /// `None` when it writes none.
    fn bounds(&self, painter: &Painter) -> Option<[i64; 4]> {
        match self {
            Self::Art(art, [left, top]) => {
                let frame = art.frame(0, 0)?;
                Some([
                    *left,
                    *top,
                    left + i64::from(frame.width),
                    top + i64::from(frame.height),
                ])
            }
            Self::Text(_, text, [left, top]) => {
                let font = painter.font(CURSOR_FONT)?;
                let glyphs = font.layout(text, CURSOR_TEXT_LIMIT as u32, false).glyphs;
                let (right, bottom) = glyphs
                    .iter()
                    .filter_map(|at| Some((at, font.glyph(at.code)?)))
                    .map(|(at, glyph)| {
                        (
                            at.x + i64::from(glyph.width),
                            i64::from(at.y + glyph.height),
                        )
                    })
                    .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))?;
                let [left, top] = [left, top].map(|value| i64::from(*value));
                Some([left - 1, top - 1, left + right + 1, top + bottom + 1])
            }
        }
    }
    /// Draw it on a layer whose corner is view pixel `[left, top]`.
    fn draw(&self, painter: &Painter, layer: &mut IndexedImage, [left, top]: [i64; 2]) {
        match self {
            Self::Art(art, origin) => {
                if let Some(frame) = art.frame(0, 0) {
                    layer.blit(frame, [origin[0] - left, origin[1] - top]);
                }
            }
            Self::Text(key, text, origin) => {
                let origin = [origin[0] - left as i32, origin[1] - top as i32];
                outlined(painter, layer, *key, text, origin);
            }
        }
    }
}

/// `strokes` drawn in order on a transparent view of `size`, as an image of
/// just the pixels they can write and its corner in the view: one
/// transparent pixel when they write none.
fn cropped_layer(
    painter: &Painter,
    strokes: &[Stroke],
    size: [u32; 2],
) -> Option<(IndexedImage, [i64; 2])> {
    let crop = strokes
        .iter()
        .filter_map(|stroke| stroke.bounds(painter))
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
        .map(|[left, top, right, bottom]| {
            [
                left.max(0),
                top.max(0),
                right.min(i64::from(size[0])),
                bottom.min(i64::from(size[1])),
            ]
        })
        .filter(|[left, top, right, bottom]| left < right && top < bottom);
    let Some([left, top, right, bottom]) = crop else {
        return Some((IndexedImage::new(1, 1).ok()?, [0, 0]));
    };
    let mut layer = IndexedImage::new((right - left) as u32, (bottom - top) as u32).ok()?;
    for stroke in strokes {
        stroke.draw(painter, &mut layer, [left, top]);
    }
    Some((layer, [left, top]))
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
pub(crate) fn map_bottom() -> i32 {
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
                    CURSOR_FONT,
                    ColorKey::BLACK,
                    text,
                    [origin[0] + dx, origin[1] + dy],
                    CURSOR_TEXT_LIMIT,
                );
            }
        }
    }
    painter.text(image, CURSOR_FONT, key, text, origin, CURSOR_TEXT_LIMIT);
}

impl MapPreview {
    /// Hover, menu and edge input apply: the map is in front with no
    /// conversation or notice over it.
    pub(crate) fn world_pointer(&self) -> bool {
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
        // A point of the `enhanced` world canvas picks in the canvas tile
        // that draws it, through the same roof cutaway.
        let picked = self.enhanced_pick(point);
        let view = picked.map_or(self.view, |(view, ..)| view);
        // The backdrop the refresh of that view draws: a sprite pixel a
        // painted wall covers is no hit, and a suppressed object never is
        // (brief §3.8.5-6).
        let prepared = {
            let mut backdrops = self.backdrops.borrow_mut();
            backdrops.sync(&self.host);
            backdrops.prepare(&*self.host.assets.vfs, &view)
        };
        let tiles = prepared
            .as_ref()
            .map(super::backdrop::Prepared::tiles)
            .unwrap_or_default();
        let backdrop = prepared.as_ref().map(|prepared| prepared.view(&tiles));
        let candidates = match picked {
            Some((view, point, cutaway)) => pick_candidates_cut_backdrop(
                map,
                &objects,
                view,
                &self.art,
                point,
                cutout.as_ref(),
                cutaway,
                backdrop.as_ref(),
            ),
            None => pick_candidates_backdrop(
                map,
                &objects,
                self.view,
                &self.art,
                point,
                cutout.as_ref(),
                backdrop.as_ref(),
            ),
        };
        let hits: Vec<_> = candidates?
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
    pub(crate) fn restart_rest(&mut self, back_date: bool) {
        self.pointer.rest_since = if back_date {
            self.clock.saturating_sub(cursor::HOVER_REST_MS)
        } else {
            self.clock
        };
        self.pointer.rested = false;
        self.pointer.feedback = None;
        self.clear_item_outline();
    }
    /// SPEC-014 §3.27 and §3.44: the world sees the `target_highlight`
    /// setting and the cursor mode; entering crosshair mode runs the
    /// target-outline enable step and leaving it the disable step. Returns
    /// whether the mode changed.
    pub(crate) fn sync_crosshair(&mut self) -> bool {
        let crosshair = self.hud.mode == Mode::Crosshair;
        let world = &mut self.host.world;
        world.target_highlight = self.preferences.target_highlight;
        if world.crosshair == crosshair {
            return false;
        }
        world.crosshair = crosshair;
        if crosshair {
            world.enable_target_outlines();
        } else {
            world.disable_target_outlines();
        }
        true
    }
    /// SPEC-014 §3.27 and §3.44: the arrow hover's item outline is removed
    /// (its outline word set to 0) and the scene is drawn again.
    pub(crate) fn clear_item_outline(&mut self) {
        if let Some(item) = self.pointer.outlined.take() {
            self.host.world.remove_outline(item);
            self.pointer.redraw = true;
        }
    }
    /// The pointer moved to a frame pixel: the rest restarts, the hover art
    /// reverts, and an open menu tracks the vertical motion.
    #[cfg(test)]
    pub(crate) fn pointer_moved(&mut self, point: [i32; 2]) {
        self.pointer_moved_in(point, false);
    }
    /// The pointer moved to `point`: a frame pixel, or with `world` a pixel
    /// of the `enhanced` world canvas.
    pub(crate) fn pointer_moved_in(&mut self, point: [i32; 2], world: bool) {
        let previous = self.pointer.at;
        if previous == Some(point) && self.pointer.world == world {
            return;
        }
        let was_map = previous.is_some_and(|previous| self.map_area(previous));
        self.pointer.at = Some(point);
        self.pointer.world = world;
        let in_map = self.map_area(point);
        // Entering the map window turns the in-world cursor back on.
        let entered_map = in_map && !was_map;
        self.restart_rest(entered_map);
        // M-P4: an open menu follows the pointer once per poll
        // (`pointer_tick`), not once per OS pointer event.
        self.window_pointer(point);
        // Leaving the map window out of combat drops the crosshair, use and
        // skill modes back to movement.
        let left_map = was_map && !in_map;
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
        self.pointer.outside = false;
        self.pointer.rested = false;
        self.pointer.feedback = None;
        self.clear_item_outline();
        self.pointer.edge = None;
    }
    /// H3: whether the pointer is outside the frame (in the letterbox),
    /// where no screen edge-scrolls.
    pub(crate) fn pointer_outside(&self) -> bool {
        self.pointer.outside
    }
    /// The shell reports whether the pointer's clamped pixel lies outside
    /// the frame, before the move reaches `hover`.
    pub(crate) fn set_pointer_outside(&mut self, outside: bool) {
        self.pointer.outside = outside;
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
            // On the `enhanced` world canvas the menu's place is in
            // interface pixels; the pointer returns to the canvas pixel
            // there, and the shell's warp is the canvas point.
            match self.enhanced_menu_return(at) {
                Some(world) => self.pointer.at = Some(world),
                None => {
                    self.pointer.at = Some(at);
                    self.hud.warp = Some(at);
                }
            }
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
            || !self.map_area(point)
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
        // held press is dropped, and no menu or hover runs. M-P2: the combat
        // lock leaves edge scrolling on (§3.27, §3.29), its scroll art
        // replacing the watch for a moment; a hand swap's wait out of
        // combat turns it off (§3.23).
        if watch {
            self.pointer.held = None;
            let scrolling = if self.combat_locked() {
                self.edge_scroll()
            } else {
                self.pointer.edge.take().is_some()
            };
            return scrolling | self.pointer.feedback.take().is_some();
        }
        // §3.21: a counter roll or the panel animation reads no button, so
        // no hold pulse opens the menu meanwhile.
        let mut changed = !self.bar_blocks() && self.menu_repeat();
        let menu_at = self.menu_point();
        if let Some(menu) = &mut self.pointer.menu {
            // §3.28 Must-match: each poll moves the highlight at most one
            // entry, however far the pointer moved.
            if let Some([_, y]) = menu_at {
                let highlight = menu.tracker.highlight;
                menu.tracker.moved(y);
                changed |= menu.tracker.highlight != highlight;
            }
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
        let at = self.menu_point().unwrap_or(point);
        let mut entries = cursor::menu_entries(target, self.host.world.combat);
        // SPEC-019 §3.6: Fallout 2's Push; it takes the enhanced entries'
        // place under the Fallout 2 profile (§3.18).
        if matches!(target, Target::Critter { .. }) && self.host.world.fallout2.is_some() {
            entries = cursor::fallout2_entries(entries, self.host.fallout2_can_push(object));
        } else if matches!(target, Target::Critter { .. }) {
            // The `enhanced` critter entries (the host refuses them in Vanilla).
            entries = cursor::enhanced_entries(
                entries,
                self.host.can_push(object, self.debug_session),
                self.host.is_companion(object),
            );
        }
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
            // SPEC-019 §3.6: the talk approach, `push_p_proc`, then the
            // step; E-push-blocked reaches the monitor as a message.
            Entry::Push if self.host.world.fallout2.is_some() => {
                if let Err(error) = self.host.fallout2_push(object) {
                    self.status = error.to_string().into_bytes();
                }
            }
            Entry::Push => match self.host.push_critter(object, self.debug_session) {
                Ok(Some(vault_core::companion::Push::Blocked)) => {
                    self.status = b"There is no room to push.".to_vec();
                }
                Ok(_) => {}
                Err(error) => self.status = error.to_string().into_bytes(),
            },
            Entry::Gear => self.open_companion_gear(object),
            Entry::Cancel => {}
        }
    }
    /// §3.29 a pointer on an edge pixel scrolls the map one step at most
    /// every 33 ms; a step the map cannot take shows the blocked art.
    /// Leaving the edge restores the previous cursor and turns the
    /// in-world cursor back on.
    fn edge_scroll(&mut self) -> bool {
        // The `enhanced` world scrolls from its own canvas edges.
        if let Some(changed) = self.enhanced_edge_scroll() {
            return changed;
        }
        // H3: a pointer in the letterbox is not on an edge pixel. §3.29:
        // scrolling is off while the combat panel animates and while the
        // world turned it off (dialogue).
        let direction = self
            .pointer
            .at
            .filter(|_| {
                !self.pointer_outside()
                    && self.hud.panel.animation.is_none()
                    && self.host.world.scrolling_enabled
            })
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
        // The `enhanced` camera follows the player again instead.
        if self.enhanced_follow_player() {
            return;
        }
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
        if !self.map_area(at) || self.host.world.ui_disabled || self.pointer.edge.is_some() {
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
                let hex = self.pointer_view().grid.pick_hex(at)?;
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
                let hooked = |hexes: i32, cost: i32| {
                    let inputs = vec![hexes, cost];
                    self.host
                        .sfall_preview(Preview::MoveCost, inputs, |host| {
                            host.hook_move_cost(player, hexes, cost)
                        })
                        .unwrap_or(cost)
                };
                let hover = cursor::priced_move_hover(
                    path,
                    key.combat,
                    key.legs,
                    key.free_movement,
                    key.ap,
                    hooked,
                );
                let feedback = match hover {
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
                // With item highlight on, an item gets outline type 16
                // (opaque) unless it has a type already or carries the
                // no-highlight flag (SPEC-014 §3.44 "set a type").
                if matches!(target, Target::Item { .. })
                    && self.preferences.item_highlight != 0
                    && self.pointer.outlined != Some(object)
                    && self.host.world.set_outline(object, outline::ITEM)
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
    pub(crate) fn hover_hit_chance(&self, target: ObjectRef) -> Option<i32> {
        let player = self.host.world.player;
        let world = &self.host.world;
        let (mode, aimed) = self.slot_attack()?;
        let mut attack = world
            .attack_profile(player, target, mode, location::UNCALLED, aimed)
            .ok()?;
        attack.hooked_cost = self
            .host
            .sfall_previewed(Preview::SlotCost, &[i32::from(mode), i32::from(aimed)]);
        world.preflight(player, target, &attack).ok()?;
        let inputs = [target as i32, i32::from(mode), i32::from(aimed)];
        if let Some(chance) = self.host.sfall_previewed(Preview::HoverChance, &inputs) {
            return Some(chance);
        }
        attack.hit_chance(true).ok()
    }
    /// SPEC-024b §6.6: this step's previews, in the Simulation phase. Drawing
    /// reads what they store, and never runs a script.
    pub(crate) fn sfall_previews(&mut self) {
        if !self.host.hooks_on() {
            return;
        }
        self.sfall_figure_preview();
        if let Some(item) = self.active_item()
            && self.slot_weapon(Some(item))
        {
            self.host
                .sfall_slot_usable(item, self.slot_hit_mode(self.slot_mode()));
        }
        let player = self.host.world.player;
        let Some((mode, aimed)) = self.slot_attack() else {
            return;
        };
        let profile = |world: &vault_core::WorldState, target| {
            world
                .attack_profile(player, target, mode, location::UNCALLED, aimed)
                .ok()
        };
        if let Some(attack) = profile(&self.host.world, player) {
            let inputs = vec![i32::from(mode), i32::from(aimed)];
            self.host.sfall_preview(Preview::SlotCost, inputs, |host| {
                host.hook_ap_cost(player, &attack)
            });
        }
        if let Some(target) = self.pointer.looked
            && let Some(mut attack) = profile(&self.host.world, target)
            && {
                attack.hooked_cost = self
                    .host
                    .sfall_previewed(Preview::SlotCost, &[i32::from(mode), i32::from(aimed)]);
                self.host.world.preflight(player, target, &attack).is_ok()
            }
        {
            let inputs = vec![target as i32, i32::from(mode), i32::from(aimed)];
            self.host
                .sfall_preview(Preview::HoverChance, inputs, |host| {
                    host.hook_hit_chance(player, target, &attack, true, None)
                        .unwrap_or(0)
                });
        }
    }
    /// §6.19 `HOOK_ADJUSTFID` on the inventory or barter screen's figure.
    /// The query takes the figure facing the viewer, so turning it is no
    /// new input.
    fn sfall_figure_preview(&mut self) {
        if !matches!(self.screen, Screen::Inventory(_) | Screen::Barter(_)) {
            return;
        }
        let player = self.host.world.player;
        if let Some(fid) = self.host.figure_art(player, self.active_item(), 3) {
            self.host.sfall_figure(fid);
        }
    }
    /// The figure to draw for `fid`: what the preview stored, when a chain
    /// changed it.
    pub(crate) fn shown_figure(&self, fid: Option<u32>) -> Option<u32> {
        let player = self.host.world.player;
        let front = self.host.figure_art(player, self.active_item(), 3)?;
        let shown = self.host.sfall_figure_shown(front);
        if shown == front { fid } else { Some(shown) }
    }
    /// Draw the cursor at the pointer: a dragged item, the action menu, the
    /// edge-scroll art, the in-world cursor over the map, else the arrow.
    pub(crate) fn draw_cursor(&self, image: &mut IndexedImage) {
        self.draw_cursor_shifted(image, [0, 0]);
    }
    /// [`Self::draw_cursor`] with the art moved by `shift` (the `enhanced`
    /// frame draws the cursor apart, into its own image); the pointer's
    /// place still decides which art shows.
    pub(crate) fn draw_cursor_shifted(&self, image: &mut IndexedImage, shift: [i32; 2]) {
        let Some(at) = self.pointer.at else {
            return;
        };
        if matches!(self.screen, Screen::Movie(_)) {
            return;
        }
        let pointer = at;
        let at = [at[0] + shift[0], at[1] + shift[1]];
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
            // On the `enhanced` world canvas the menu sits in interface
            // pixels relative to the pointer's.
            let shift = match self.menu_point() {
                Some(menu_at) if self.pointer.world => [at[0] - menu_at[0], at[1] - menu_at[1]],
                _ => shift,
            };
            self.draw_menu(&painter, image, menu, shift);
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
                    self.window_entry_under(pointer),
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
                    self.window_entry_under(pointer),
                );
                painter.cursor(image, art, at);
                return;
            }
            // SPEC-014 §3.39-§3.40: the world map's scroll art, else the
            // arrow.
            Screen::World(_) | Screen::Town(_) => {
                painter.cursor(image, self.world_cursor(pointer), at);
                return;
            }
            Screen::World2(_) | Screen::Town2(_) => {
                painter.cursor(image, self.world2_cursor(pointer), at);
                return;
            }
            _ => {}
        }
        // SPEC-014 §3.41.6: the animated watch while the Pip-Boy rests.
        if let Some(frame) = self.pipboy_watch() {
            painter.cursor_frame(image, cursor::ANIMATED_WATCH.art, frame, at);
            return;
        }
        // §3.29: the scroll art, which under the combat lock temporarily
        // replaces the animated watch.
        if let Some(edge) = self.pointer.edge {
            let art = if edge.blocked {
                cursor::SCROLL_BLOCKED
            } else {
                cursor::SCROLL
            }[edge.direction];
            painter.cursor(image, art.art, at);
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
        // §3.27: the in-world cursor is a flat map object, drawn by the
        // map refresh (`cursor_objects`), not over the frame.
        if self.world_cursor_at().is_some() {
            return;
        }
        // SPEC-014 §3.20: over the message monitor's scroll areas, while
        // the bar is enabled, `suparrow.frm` or `sdnarrow.frm`; leaving
        // them restores the arrow.
        if self.world_pointer()
            && !self.pointer.world
            && let Some(older) = self.monitor_area(pointer)
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
    /// SPEC-014 §3.27 where the in-world cursor shows: over the map window
    /// (the bar starts below row 379) while the map takes the pointer and
    /// the interface is enabled, unless the action menu, the scroll art or
    /// the animated watch replaces it. Elsewhere the system cursor or a
    /// window's cursor shows instead (`draw_cursor`).
    pub(crate) fn world_cursor_at(&self) -> Option<[i32; 2]> {
        let at = self.pointer.at?;
        (self.world_pointer()
            && self.pointer.menu.is_none()
            && self.pointer.edge.is_none()
            && self.hud.watch.is_none()
            && self.map_area(at)
            && !self.host.world.ui_disabled)
            .then_some(at)
    }
    /// SPEC-014 §3.27 the in-world cursor's objects over `view`.
    pub(crate) fn cursor_objects(&self, view: MapView) -> Option<CursorObjects> {
        self.cropped_cursor_objects(view)
    }
    /// SPEC-014 §3.27 the in-world cursor's objects over `view`, at the hex
    /// under the pointer: none while it does not show, or off the hex grid.
    /// Movement mode snaps the `msef003.frm` hex outline to that hex, with
    /// its AP text at the hex's cell; the other modes follow the pointer by
    /// pixel. The layers hold only what they draw, so a wide canvas costs
    /// no more than a small one and they always fit the instanced path's
    /// atlas.
    pub(crate) fn cropped_cursor_objects(&self, view: MapView) -> Option<CursorObjects> {
        let [width, height] = [view.width, view.height];
        let at = self.world_cursor_at()?;
        let hex = view.grid.pick_hex(at)?;
        if ![width, height].iter().all(|side| (1..=4096).contains(side)) {
            return None;
        }
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let mut art = Vec::new();
        let mut ring = None;
        let mut text = None;
        let feedback = self.pointer.feedback.as_ref();
        match self.hud.mode {
            Mode::Movement => {
                let center = view.grid.hex_center(hex).ok()?;
                // The 32-by-16 hex outline at the hex, with outline type 2
                // and the translucent bit (movement mode only).
                if let Some(frames) = painter.art(Mode::Movement.art())
                    && let Some(frame) = frames.frame(0, 0)
                {
                    let [sx, sy] = frames.shifts[0].map(i64::from);
                    let origin = [
                        center[0] + sx - i64::from(frame.width / 2),
                        center[1] + sy - i64::from(frame.height) + 1,
                    ];
                    art.push(Stroke::Art(frames.clone(), origin));
                    ring = Some((frames.clone(), origin));
                }
                if let Some(Feedback::Cost(cost, key)) = feedback {
                    let cell = [center[0] as i32 - 16, center[1] as i32 - 8];
                    let width = painter.width(CURSOR_FONT, cost);
                    let origin = [cell[0] + (32 - width) / 2, cell[1] + 3];
                    text = Some(Stroke::Text(*key, cost, origin));
                }
            }
            Mode::Arrow => match feedback {
                Some(Feedback::Pick(icon)) => {
                    // The mirror and shift rules apply at the view's right
                    // and bottom edges (640 by 380 in Vanilla).
                    let edge = [width as i32 - 640, height as i32 - 380];
                    let (name, shift) = cursor::pick_art(at[0] - edge[0], at[1] - edge[1]);
                    let top = at[1] - shift;
                    if name == "actarrom.frm" {
                        art.extend(Stroke::art(&painter, name, [at[0] - 28, top]));
                        art.extend(Stroke::art(&painter, icon, [at[0] - 68, top]));
                    } else {
                        art.extend(Stroke::art(&painter, "actarrow.frm", [at[0], top]));
                        art.extend(Stroke::art(&painter, icon, [at[0] + 29, top]));
                    }
                }
                _ => art.extend(Stroke::cursor(&painter, Mode::Arrow.art(), at)),
            },
            Mode::Crosshair => match feedback {
                Some(Feedback::Hit(hit, key)) => {
                    art.extend(Stroke::cursor(&painter, "acttohit.frm", at));
                    art.push(Stroke::Text(*key, hit, [at[0] - 10 + 22, at[1] - 10 + 1]));
                }
                _ => art.extend(Stroke::cursor(&painter, Mode::Crosshair.art(), at)),
            },
            mode @ (Mode::UseCrosshair | Mode::Skill(_)) => {
                art.extend(Stroke::cursor(&painter, mode.art(), at));
            }
        }
        let (art, art_origin) = cropped_layer(&painter, &art, [width, height])?;
        let (text, text_origin) = match text {
            Some(stroke) => {
                let (layer, origin) = cropped_layer(&painter, &[stroke], [width, height])?;
                (Some(layer), origin)
            }
            None => (None, [0, 0]),
        };
        Some(CursorObjects {
            hex,
            art,
            art_origin,
            ring,
            text,
            text_origin,
        })
    }
    /// The arrow and the stacked 40-by-40 icons, the highlighted one in its
    /// `h` art; mirrored at the right edge and shifted up at the bottom.
    fn draw_menu(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        menu: &ActionMenu,
        shift: [i32; 2],
    ) {
        let [x, y] = menu.origin;
        // The mirror and shift rules apply at the map area's right and
        // bottom edges (640 and 380 in Vanilla, the `enhanced` world's
        // edges in interface pixels).
        let edge = self.menu_edges();
        let top = y - cursor::menu_shift(y - (edge[1] - 380), menu.entries.len());
        let mirrored = x + 68 >= edge[0];
        let (x, top) = (x + shift[0], top + shift[1]);
        let (arrow, icons_x) = if mirrored {
            (("actarrom.frm", x - 28), x - 68)
        } else {
            (("actarrow.frm", x), x + 29)
        };
        painter.blit(image, arrow.0, 0, [arrow.1, top]);
        for (index, entry) in menu.entries.iter().enumerate() {
            let lit = index == menu.tracker.highlight;
            let at = [icons_x, top + 40 * index as i32];
            // SPEC-019 §2.2: Fallout 2's push icon pair, else the engine's.
            let fallout2 = (*entry == Entry::Push && self.host.world.fallout2.is_some())
                .then(|| painter.role(&vault_core::ui::fallout2::PUSH_ICONS[usize::from(lit)]))
                .flatten();
            match (fallout2, entry.icons()) {
                (Some((name, _)), _) => {
                    painter.blit(image, name, 0, at);
                }
                (None, Some((normal, highlighted))) => {
                    painter.blit(image, if lit { highlighted } else { normal }, 0, at);
                }
                (None, None) => draw_engine_icon(painter, image, *entry, lit, at),
            }
        }
    }
}

/// An `enhanced` menu entry's engine-drawn icon (Fallout 1 has no art for
/// it): a 40-pixel plate in the menu's place with its label, its colors
/// swapped while highlighted.
fn draw_engine_icon(
    painter: &Painter,
    image: &mut IndexedImage,
    entry: Entry,
    lit: bool,
    [x, y]: [i32; 2],
) {
    let label: &[u8] = match entry {
        Entry::Push => b"PUSH",
        Entry::Gear => b"GEAR",
        _ => return,
    };
    let (fill, text) = if lit {
        (ColorKey::AMBER, ColorKey::BLACK)
    } else {
        (ColorKey::BLACK, ColorKey::AMBER)
    };
    image.fill_rect([x + 2, y + 2, 36, 36], painter.color(ColorKey::AMBER));
    image.fill_rect([x + 3, y + 3, 34, 34], painter.color(fill));
    painter.centered(image, 101, text, label, [x + 3, y + 16], 34);
}

impl MapPreview {
    /// Whether a held-button menu is open: the map's action menu, or the
    /// inventory, loot or barter window's item menu (the touch UI's hold
    /// slides through it).
    pub(crate) fn held_menu_open(&self) -> bool {
        self.pointer.menu.is_some()
            || match &self.screen {
                Screen::Inventory(screen) => screen.menu.is_some(),
                Screen::Loot(screen) => screen.menu.is_some(),
                Screen::Barter(screen) => screen.menu.is_some(),
                _ => false,
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

#[cfg(test)]
mod tests {
    use super::super::test_support::{interactive, synthetic_corpus};
    use super::*;
    use vault_core::ui::cursor::Mode;
    use vault_fixtures::world::frm;
    use vault_formats::HexGrid;

    /// The art the cursor draws: patterned with holes, some with shifts, so
    /// a wrong hotspot or a lost pixel shows.
    const ART: [(&str, u16, u16, [i16; 2]); 7] = [
        ("msef003.frm", 32, 16, [2, -1]),
        ("actarrow.frm", 24, 28, [0, 0]),
        ("actarrom.frm", 24, 28, [0, 0]),
        ("testicon.frm", 40, 40, [0, 0]),
        ("acrshair.frm", 21, 21, [3, 5]),
        ("acttohit.frm", 21, 21, [-3, 4]),
        ("crossuse.frm", 19, 23, [-2, -6]),
    ];

    fn world() -> (tempfile::TempDir, MapPreview) {
        let dir = synthetic_corpus();
        let root = dir.path();
        std::fs::write(
            root.join("font1.aaf"),
            super::super::state_fixture::preview_aaf(),
        )
        .unwrap();
        for (name, width, height, [sx, sy]) in ART {
            let mut bytes = frm(width, height, 0);
            bytes[10..12].copy_from_slice(&sx.to_be_bytes());
            bytes[22..24].copy_from_slice(&sy.to_be_bytes());
            let start = bytes.len() - usize::from(width) * usize::from(height);
            for (at, pixel) in bytes[start..].iter_mut().enumerate() {
                *pixel = if at % 7 == 0 { 0 } else { 40 + (at % 90) as u8 };
            }
            std::fs::write(root.join("art/intrface").join(name), bytes).unwrap();
        }
        let mut preview = interactive(root);
        preview.tick(16).unwrap();
        preview.image().unwrap();
        (dir, preview)
    }

    fn view_of(preview: &MapPreview, width: u32, height: u32) -> MapView {
        MapView {
            grid: HexGrid::centered(20100, width, height).unwrap(),
            width,
            height,
            ..preview.view
        }
    }

    /// Every cursor state: each mode, with and without its feedback.
    fn states() -> Vec<(Mode, Option<Feedback>)> {
        vec![
            (Mode::Movement, None),
            (
                Mode::Movement,
                Some(Feedback::Cost(b"12".to_vec(), ColorKey::AMBER)),
            ),
            (Mode::Arrow, None),
            (Mode::Arrow, Some(Feedback::Pick("testicon.frm"))),
            (Mode::Crosshair, None),
            (
                Mode::Crosshair,
                Some(Feedback::Hit(b"95%".to_vec(), ColorKey::AMBER)),
            ),
            (Mode::UseCrosshair, None),
        ]
    }

    /// A view-sized draw: the hex, art layer, ring origin and text layer.
    type ViewSized = (i32, IndexedImage, Option<[i64; 2]>, Option<IndexedImage>);

    /// The layers as `cursor_objects` drew them before they were cropped:
    /// view-sized, through the painter.
    fn drawn_view_sized(preview: &MapPreview, view: MapView) -> Option<ViewSized> {
        let [width, height] = [view.width, view.height];
        let at = preview.world_cursor_at()?;
        let hex = view.grid.pick_hex(at)?;
        let painter = Painter {
            assets: &preview.host.assets,
            palette: &preview.palette,
        };
        let mut art = IndexedImage::new(width, height).ok()?;
        let mut ring = None;
        let mut text = None;
        let feedback = preview.pointer.feedback.as_ref();
        match preview.hud.mode {
            Mode::Movement => {
                let center = view.grid.hex_center(hex).ok()?;
                if let Some(frames) = painter.art(Mode::Movement.art())
                    && let Some(frame) = frames.frame(0, 0)
                {
                    let [sx, sy] = frames.shifts[0].map(i64::from);
                    let origin = [
                        center[0] + sx - i64::from(frame.width / 2),
                        center[1] + sy - i64::from(frame.height) + 1,
                    ];
                    art.blit(frame, origin);
                    ring = Some(origin);
                }
                if let Some(Feedback::Cost(cost, key)) = feedback {
                    let mut layer = IndexedImage::new(width, height).ok()?;
                    let cell = [center[0] as i32 - 16, center[1] as i32 - 8];
                    let width = painter.width(101, cost);
                    let origin = [cell[0] + (32 - width) / 2, cell[1] + 3];
                    outlined(&painter, &mut layer, *key, cost, origin);
                    text = Some(layer);
                }
            }
            Mode::Arrow => match feedback {
                Some(Feedback::Pick(icon)) => {
                    let edge = [width as i32 - 640, height as i32 - 380];
                    let (name, shift) = cursor::pick_art(at[0] - edge[0], at[1] - edge[1]);
                    let top = at[1] - shift;
                    if name == "actarrom.frm" {
                        painter.blit(&mut art, name, 0, [at[0] - 28, top]);
                        painter.blit(&mut art, icon, 0, [at[0] - 68, top]);
                    } else {
                        painter.blit(&mut art, "actarrow.frm", 0, [at[0], top]);
                        painter.blit(&mut art, icon, 0, [at[0] + 29, top]);
                    }
                }
                _ => {
                    painter.cursor(&mut art, Mode::Arrow.art(), at);
                }
            },
            Mode::Crosshair => match feedback {
                Some(Feedback::Hit(hit, key)) => {
                    painter.cursor(&mut art, "acttohit.frm", at);
                    let origin = [at[0] - 10 + 22, at[1] - 10 + 1];
                    outlined(&painter, &mut art, *key, hit, origin);
                }
                _ => {
                    painter.cursor(&mut art, Mode::Crosshair.art(), at);
                }
            },
            mode @ (Mode::UseCrosshair | Mode::Skill(_)) => {
                painter.cursor(&mut art, mode.art(), at);
            }
        }
        Some((hex, art, ring, text))
    }

    /// In every state at `at`: the cropped layers hold exactly the pixels
    /// of the view-sized ones, in less than `limit` pixels a side. Returns
    /// the opaque pixels the view shows, `None` off the hex grid.
    fn check(preview: &mut MapPreview, view: MapView, at: [i32; 2], limit: u32) -> Option<usize> {
        check_states(preview, view, at, limit, states())
    }

    fn check_states(
        preview: &mut MapPreview,
        view: MapView,
        at: [i32; 2],
        limit: u32,
        states: Vec<(Mode, Option<Feedback>)>,
    ) -> Option<usize> {
        preview.pointer.at = Some(at);
        preview.pointer.world = true;
        let size = [view.width, view.height];
        let mut shown = None;
        for (mode, feedback) in states {
            let label = format!("{at:?} {mode:?} {feedback:?}");
            preview.hud.mode = mode;
            preview.pointer.feedback = feedback;
            let want = drawn_view_sized(preview, view);
            let got = preview.cropped_cursor_objects(view);
            assert_eq!(got.is_some(), want.is_some(), "{label}");
            let (Some(got), Some((hex, art, ring, text))) = (got, want) else {
                continue;
            };
            for layer in [Some(&got.art), got.text.as_ref()].into_iter().flatten() {
                assert!(
                    layer.width() <= limit && layer.height() <= limit,
                    "{label}: {} by {}",
                    layer.width(),
                    layer.height()
                );
            }
            let opaque = |image: &IndexedImage| image.pixels().iter().filter(|p| **p != 0).count();
            let cropped_opaque = opaque(&got.art);
            let wide = got.view_sized(size).unwrap();
            assert_eq!(wide.hex, hex, "{label}");
            assert_eq!(wide.ring.map(|(_, origin)| origin), ring, "{label}");
            assert!(wide.art.pixels() == art.pixels(), "{label}: art");
            assert_eq!(cropped_opaque, opaque(&art), "{label}: art");
            match (&wide.text, &text) {
                (Some(got), Some(want)) => {
                    assert!(got.pixels() == want.pixels(), "{label}: text");
                }
                (None, None) => {}
                _ => panic!("{label}: text layer"),
            }
            *shown.get_or_insert(0) += cropped_opaque + text.as_ref().map_or(0, opaque);
            // What the refresh is given is the same picture.
            let live = preview.cursor_objects(view).unwrap().view_sized(size);
            assert!(live.unwrap().art.pixels() == art.pixels(), "{label}");
        }
        shown
    }

    #[test]
    fn cursor_layers_keep_their_pixels_near_every_canvas_edge() {
        let (_dir, mut preview) = world();
        let view = view_of(&preview, 640, 380);
        for at in [
            [320, 190],
            [0, 190],
            [2, 190],
            [639, 190],
            [637, 190],
            [320, 0],
            [320, 2],
            [320, 379],
            [320, 377],
            [0, 0],
            [639, 0],
            [0, 379],
            [639, 379],
        ] {
            let shown = check(&mut preview, view, at, 200);
            assert!(shown.is_some_and(|shown| shown > 0), "{at:?}");
        }
    }

    #[test]
    fn cursor_layers_stay_small_on_a_4096_pixel_canvas() {
        let (_dir, mut preview) = world();
        let view = view_of(&preview, 4096, 380);
        for at in [
            [2048, 190],
            [0, 190],
            [4095, 190],
            [4095, 0],
            [4095, 379],
            [4060, 190],
        ] {
            let shown = check(&mut preview, view, at, 200);
            assert!(shown.is_some_and(|shown| shown > 0), "{at:?}");
        }
        // The hex grid is a rhombus: a tall canvas's corners are off it, so
        // they have no cursor, and its edges' middles do.
        let view = view_of(&preview, 4096, 2160);
        assert_eq!(check(&mut preview, view, [4095, 0], 200), None);
        for at in [[2048, 1080], [2048, 0], [2048, 2159], [0, 1080]] {
            let shown = check(&mut preview, view, at, 200);
            assert!(shown.is_some_and(|shown| shown > 0), "{at:?}");
        }
    }

    /// Past the canvas the art and text clip away. The objects stay, as
    /// they did, with a one-pixel transparent layer in place of an empty
    /// image.
    #[test]
    fn an_off_canvas_cursor_keeps_its_objects_with_transparent_layers() {
        let (_dir, mut preview) = world();
        let view = view_of(&preview, 640, 380);
        for at in [[-200, 190], [840, 190], [320, -200], [-200, -200]] {
            assert_eq!(check(&mut preview, view, at, 1), Some(0), "{at:?}");
        }
        // Below the canvas the pick art's shift rule brings it back up, as
        // it always did; everything else clips away.
        let (pick, others): (Vec<_>, Vec<_>) = states()
            .into_iter()
            .partition(|(_, feedback)| matches!(feedback, Some(Feedback::Pick(_))));
        for at in [[320, 580], [840, 580]] {
            let clipped = check_states(&mut preview, view, at, 1, others.clone());
            assert_eq!(clipped, Some(0), "{at:?}");
        }
        assert!(check_states(&mut preview, view, [320, 580], 200, pick).is_some_and(|n| n > 0));
        // Partly off the canvas, the cropped layer keeps what still shows.
        for at in [[-5, 190], [645, 190], [320, -5], [320, 385]] {
            assert!(check(&mut preview, view, at, 200).is_some(), "{at:?}");
        }
    }

    /// A view no image can be made for has no cursor.
    #[test]
    fn a_view_past_the_image_limit_has_no_cursor_objects() {
        let (_dir, mut preview) = world();
        preview.pointer.at = Some([320, 190]);
        preview.pointer.world = true;
        let mut view = view_of(&preview, 640, 380);
        assert!(preview.cursor_objects(view).is_some());
        view.width = 4097;
        assert!(preview.cursor_objects(view).is_none());
        view.width = 0;
        assert!(preview.cursor_objects(view).is_none());
    }
}
