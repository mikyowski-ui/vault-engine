//! The gallery's animated demonstrations of play: the tactical overlay in
//! combat, the widescreen windows and quality-of-life buttons, the F9
//! debugger and the touch UI's action menu. Each is a session over the
//! showcase map on the fixed 30 frames-a-second clock, its input routed as
//! the desktop routes it (`enhanced_pointer`, `enhanced_button`, `step`),
//! composed as the desktop composes its layers.
use super::{
    MOVIE, STAND, clock, delay, enhanced_picture, indexed, map, surface_point, widen, world,
};
use crate::{
    AppResult,
    enhanced::{PointerAt, PointerButton},
    preview::AssetPreview,
    replay::{enhanced_layout, pointer_at},
};
use std::{path::Path, sync::Arc};
use vault_core::{EnginePreset, ObjectRef, WorldEffect, camera::CameraCommand};
use vault_formats::{HexGrid, pro::Category};
use winit::keyboard::KeyCode;

/// A demonstration's frames and their delays.
pub(super) type Frames = Vec<(super::encode::Indexed, u16)>;

/// A session being filmed: its preview, the frame count, the pointer (a
/// surface point), the touch gestures and the finger's place (drawn as a
/// ring), and the frames so far.
pub(super) struct Film {
    pub preview: AssetPreview,
    frame: u64,
    pointer: Option<[f64; 2]>,
    touch: crate::touch::TouchGestures,
    finger: Option<[f64; 2]>,
    pub frames: Frames,
}

impl Film {
    pub fn new(preview: AssetPreview) -> Self {
        Self {
            preview,
            frame: 0,
            pointer: None,
            touch: crate::touch::TouchGestures::default(),
            finger: None,
            frames: Vec::new(),
        }
    }
    /// The shell clock at the next frame.
    fn now(&self) -> u64 {
        clock(64, self.frame)
    }
    /// Run the world to the next frame's time and record it.
    pub fn shoot(&mut self) -> AppResult<()> {
        // A held finger's press comes due on the frame, as the shell polls.
        let due = self.touch.poll(self.now());
        self.apply(due)?;
        self.preview.tick(self.now())?;
        let mut image = enhanced_picture(&mut self.preview, MOVIE, self.pointer)?;
        if let Some(finger) = self.finger {
            ring(&mut image, finger, &self.preview);
        }
        self.frames.push((indexed(&image), delay(self.frame)));
        self.frame += 1;
        Ok(())
    }
    /// `count` frames with nothing new done.
    pub fn hold(&mut self, count: u64) -> AppResult<()> {
        for _ in 0..count {
            self.shoot()?;
        }
        Ok(())
    }
    /// The pointer's place in both layers, as the shell routes it.
    fn at(&mut self) -> AppResult<PointerAt> {
        let layout = enhanced_layout(&mut self.preview, MOVIE).ok_or("no unlocked viewport")?;
        Ok(pointer_at(&layout, self.pointer))
    }
    /// The pointer to interface pixel `ui`.
    pub fn point_ui(&mut self, ui: [i32; 2]) -> AppResult<()> {
        let layout = enhanced_layout(&mut self.preview, MOVIE).ok_or("no unlocked viewport")?;
        let scale = f64::from(layout.ui_scale);
        self.pointer = Some(
            [0, 1]
                .map(|axis| f64::from(layout.ui_rect[axis]) + (f64::from(ui[axis]) + 0.5) * scale),
        );
        Ok(())
    }
    /// The pointer to `lift` pixels above `hex`'s center on the canvas.
    pub fn point_hex(&mut self, hex: i32, lift: f64) -> AppResult<()> {
        let origin = map(&mut self.preview)
            .enhanced_view()
            .ok_or("no unlocked viewport")?
            .grid
            .origin;
        let [x, y] = HexGrid::default().hex_center(hex)?;
        let pixel = [(origin[0] + x) as f64, (origin[1] + y) as f64 - lift];
        self.pointer = Some(surface_point(&mut self.preview, pixel)?);
        Ok(())
    }
    /// A glide of the pointer to interface pixel `ui` over `count` frames.
    pub fn glide_ui(&mut self, ui: [i32; 2], count: u64) -> AppResult<()> {
        let from = self.pointer;
        self.point_ui(ui)?;
        let to = self.pointer.expect("just set");
        let from = from.unwrap_or(to);
        for step in 1..=count {
            let t = step as f64 / count as f64;
            self.pointer = Some([0, 1].map(|axis| from[axis] + (to[axis] - from[axis]) * t));
            self.shoot()?;
        }
        Ok(())
    }
    /// A left click where the pointer is: the press shows for two frames.
    pub fn click(&mut self) -> AppResult<()> {
        let at = self.at()?;
        self.preview.enhanced_pointer(at);
        self.preview.enhanced_button(at, PointerButton::Left, true);
        self.hold(2)?;
        let at = self.at()?;
        self.preview.enhanced_button(at, PointerButton::Left, false);
        Ok(())
    }
    /// One touch event of the single finger at surface point `at`, through
    /// the touch UI's gestures; the finger shows as a ring while down.
    pub fn finger(&mut self, phase: winit::event::TouchPhase, at: [f64; 2]) -> AppResult<()> {
        use winit::event::TouchPhase;
        let layout = enhanced_layout(&mut self.preview, MOVIE).ok_or("no unlocked viewport")?;
        let button = phase == TouchPhase::Started
            && layout
                .ui_pixel(at)
                .is_some_and(|pixel| self.preview.touch_button_at(pixel));
        let context = crate::touch::TouchContext {
            camera: true,
            button,
            menu: self.preview.held_menu_open(),
        };
        let actions = self.touch.feed(phase, 0, at, self.now(), context);
        self.finger = matches!(phase, TouchPhase::Started | TouchPhase::Moved).then_some(at);
        self.apply(actions)
    }
    /// The surface point of interface pixel `ui`.
    pub fn ui_point(&mut self, ui: [i32; 2]) -> AppResult<[f64; 2]> {
        let layout = enhanced_layout(&mut self.preview, MOVIE).ok_or("no unlocked viewport")?;
        let scale = f64::from(layout.ui_scale);
        Ok(
            [0, 1]
                .map(|axis| f64::from(layout.ui_rect[axis]) + (f64::from(ui[axis]) + 0.5) * scale),
        )
    }
    /// Gesture actions as the shell applies them: pointer moves, buttons
    /// and wheel lines through the enhanced routing.
    fn apply(&mut self, actions: Vec<crate::touch::TouchAction>) -> AppResult<()> {
        use crate::touch::{TouchAction, TouchButton};
        for action in actions {
            match action {
                TouchAction::Pointer(point) => {
                    self.pointer = Some(point);
                    let at = self.at()?;
                    self.preview.enhanced_pointer(at);
                }
                TouchAction::Leave => {
                    self.pointer = None;
                    let at = self.at()?;
                    self.preview.enhanced_pointer(at);
                }
                TouchAction::Button { button, pressed } => {
                    let button = match button {
                        TouchButton::Left => PointerButton::Left,
                        TouchButton::Right => PointerButton::Right,
                        TouchButton::Middle => PointerButton::Middle,
                    };
                    let at = self.at()?;
                    self.preview.enhanced_button(at, button, pressed);
                }
                TouchAction::Zoom { lines } => {
                    let at = self.at()?;
                    self.preview.enhanced_wheel(at, lines);
                }
            }
        }
        Ok(())
    }
    /// A key press and release, with `modifiers` (shift, control, alt) held
    /// for it.
    pub fn key(&mut self, key: KeyCode, modifiers: [bool; 3]) {
        self.preview.set_modifiers(modifiers);
        self.preview.step(key);
        self.preview.key_up(key);
        self.preview.set_modifiers([false; 3]);
    }
}

/// A ring marking a finger at surface point `at`: 14 pixels across, in
/// the palette's nearest white, drawn over the frame (the one mark the
/// gallery adds, so a touch shows).
fn ring(image: &mut vault_render::IndexedImage, at: [f64; 2], preview: &AssetPreview) {
    let colors = super::display(preview.palette(), preview.brightness());
    let ink = super::nearest(&colors, [240, 240, 240]);
    let [cx, cy] = at.map(|value| value.round() as i32);
    let (width, height) = (image.width() as i32, image.height() as i32);
    for y in cy - 15..=cy + 15 {
        for x in cx - 15..=cx + 15 {
            let d2 = (x - cx).pow(2) + (y - cy).pow(2);
            if (144..=196).contains(&d2) && (0..width).contains(&x) && (0..height).contains(&y) {
                image.pixels_mut()[(y * width + x) as usize] = ink;
            }
        }
    }
}

/// The showcase's critter (inside the building).
fn npc(preview: &mut AssetPreview) -> AppResult<ObjectRef> {
    let world = &map(preview).host.world;
    let player = world.player;
    Ok(world
        .iter()
        .find(|(reference, object)| object.category() == Category::Critter && *reference != player)
        .ok_or("the showcase map has no NPC")?
        .0)
}

/// The showcase's critter and, cloned from its prototype, a second one
/// placed at `hex`.
fn critters(preview: &mut AssetPreview, hex: i32) -> AppResult<(ObjectRef, ObjectRef)> {
    let npc = npc(preview)?;
    let world = &mut map(preview).host.world;
    let prototype = world
        .get(npc)
        .and_then(|object| object.prototype.clone())
        .ok_or("the NPC has no prototype")?;
    let other = world.spawn(Arc::clone(&prototype), hex, 0)?;
    Ok((npc, other))
}

/// `combat-overlay.gif`: a companion's HUD pill cycled through its
/// distance modes (DEF, CLS, HLD, CHG), then the player's attack on an
/// injured critter starts turn-based combat under the tactical overlay
/// (Alt+H): the HP bars over hurt critters and the crosshair's to-hit
/// percentage at once, the hit lowering the target's bar.
pub(super) fn combat_overlay(corpus: &Path) -> AppResult<Frames> {
    let mut preview = world(corpus, EnginePreset::Enhanced)?;
    widen(&mut preview, false)?;
    // A punch reaches one hex: the foe stands next to the player.
    let foe_hex = HexGrid::step(STAND, 2, 1)?;
    let (companion, foe) = critters(&mut preview, foe_hex)?;
    {
        let world = &mut map(&mut preview).host.world;
        // The companion beside the player, in the party.
        world.get_mut(companion).ok_or("no companion")?.raw.id = 18000;
        world.party.insert(companion);
        let beside = HexGrid::step(STAND, 4, 1)?;
        world.move_object(companion, beside, 0, false)?;
        // The foe, already hurt.
        let max = {
            let context = world.context(foe);
            world
                .get(foe)
                .and_then(|object| object.character.as_ref())
                .map(|character| character.stat(7, context))
                .ok_or("the foe has no stats")?
        };
        if let Some(state) = world.critter_state_mut(foe) {
            state.hp = (max * 2 / 3).max(1);
        }
        // The camera on the player at 2x, so the marks read.
        for command in [
            CameraCommand::PanTo {
                tile: STAND,
                duration_ms: 0,
            },
            CameraCommand::SetZoom {
                percent: 200,
                duration_ms: 0,
            },
        ] {
            world.effects.push_back(WorldEffect::Camera(command));
        }
    }
    let mut film = Film::new(preview);
    film.point_ui([320, 200])?;
    film.hold(12)?;
    // The pill's mode region, clicked three times: DEF, CLS, HLD, CHG.
    let [_, mode] = crate::map_preview::party::regions(0);
    film.glide_ui([mode.x + mode.w / 2, mode.y + mode.h / 2], 10)?;
    for _ in 0..3 {
        film.hold(8)?;
        film.click()?;
    }
    film.hold(14)?;
    // The overlay's toggle, off and on again (its HUD message shows).
    film.key(KeyCode::KeyH, [false, false, true]);
    film.hold(3)?;
    film.key(KeyCode::KeyH, [false, false, true]);
    // The crosshair onto the foe, and the attack.
    map(&mut film.preview).hud.mode = vault_core::ui::cursor::Mode::Crosshair;
    film.point_hex(foe_hex, 20.0)?;
    film.hold(10)?;
    map(&mut film.preview).attack_request(foe);
    film.hold(70)?;
    // The player's next turn: the crosshair's to-hit at once.
    film.point_hex(foe_hex, 22.0)?;
    film.hold(40)?;
    Ok(film.frames)
}

/// An item of `pid` (the showcase's prototypes: 1 the box, 2 the knife,
/// 3 the spear) at `hex`, or held by `owner`.
fn item(preview: &mut AssetPreview, pid: u32, place: Place) -> AppResult<ObjectRef> {
    use vault_core::assets::WorldAssets;
    let host = &mut map(preview).host;
    let prototype = host.assets.prototype(pid)?;
    let hex = match place {
        Place::Ground(hex) => hex,
        Place::Held(_) => 0,
    };
    // Its name, as a loaded map's objects take theirs (`pro_item.msg`).
    let name = host.assets.prototype_text(&prototype, false);
    let world = &mut host.world;
    let item = world.spawn(prototype, hex, 0)?;
    if let (Some(name), Some(object)) = (name, world.get_mut(item)) {
        object.name = name;
    }
    if let Place::Held(owner) = place {
        world.add_item(owner, item, 1, true)?;
    }
    Ok(item)
}
#[derive(Clone, Copy)]
enum Place {
    Ground(i32),
    Held(ObjectRef),
}

/// The NPC as a companion (party ID 18000) on the player's other side,
/// carrying the knife and the spear.
fn companion(preview: &mut AssetPreview, npc: ObjectRef) -> AppResult<()> {
    {
        let world = &mut map(preview).host.world;
        world.get_mut(npc).ok_or("no companion")?.raw.id = 18000;
        world.party.insert(npc);
        world.move_object(npc, HexGrid::step(STAND, 4, 1)?, 0, false)?;
    }
    for pid in [2, 3] {
        item(preview, pid, Place::Held(npc))?;
    }
    Ok(())
}

/// The camera on the player at `percent` zoom.
fn frame_player(preview: &mut AssetPreview, percent: u32) {
    let world = &mut map(preview).host.world;
    for command in [
        CameraCommand::PanTo {
            tile: STAND,
            duration_ms: 0,
        },
        CameraCommand::SetZoom {
            percent,
            duration_ms: 0,
        },
    ] {
        world.effects.push_back(WorldEffect::Camera(command));
    }
}

/// `widescreen-modals-qol.gif`: Shift outlines the items lying on the
/// ground; a crate's loot window opens over the live widescreen world,
/// dimmed through its vignette, and TAKE ALL empties it; then the
/// companion's HUD pill opens its Gear window, whose WIELD button takes
/// the knife, then the spear, in hand.
pub(super) fn widescreen_modals(corpus: &Path) -> AppResult<Frames> {
    use vault_core::LootRequest;
    let mut preview = world(corpus, EnginePreset::Enhanced)?;
    widen(&mut preview, false)?;
    frame_player(&mut preview, 150);
    let npc = npc(&mut preview)?;
    companion(&mut preview, npc)?;
    // Crates on the ground, one holding the knife and the spear.
    let mut crates = Vec::new();
    for (direction, count) in [(1, 2), (2, 3), (0, 3)] {
        let hex = HexGrid::step(STAND, direction, count)?;
        crates.push(item(&mut preview, 1, Place::Ground(hex))?);
    }
    for pid in [2, 3] {
        item(&mut preview, pid, Place::Held(crates[0]))?;
    }
    let mut film = Film::new(preview);
    film.point_hex(HexGrid::step(STAND, 1, 4)?, 0.0)?;
    film.hold(15)?;
    // Shift held: the ground items' outline.
    film.preview.set_modifiers([true, false, false]);
    film.hold(36)?;
    film.preview.set_modifiers([false; 3]);
    film.hold(6)?;
    // The crate's loot window, over the dimmed live world.
    {
        let world = &mut map(&mut film.preview).host.world;
        let player = world.player;
        world.pending_loot = Some(LootRequest {
            looter: player,
            target: crates[0],
            steal: false,
        });
    }
    film.hold(20)?;
    let [ox, oy] = vault_core::skills::LOOT_ORIGIN;
    let take_all = crate::map_preview::loot::TAKE_ALL;
    film.glide_ui(
        [
            ox + take_all.x + take_all.w / 2,
            oy + take_all.y + take_all.h / 2,
        ],
        14,
    )?;
    film.hold(6)?;
    film.click()?;
    film.hold(26)?;
    film.key(KeyCode::Escape, [false; 3]);
    film.hold(10)?;
    // The companion's pill opens its Gear window; WIELD twice.
    let [window, _] = crate::map_preview::party::regions(0);
    film.glide_ui([window.x + window.w / 2, window.y + window.h / 2], 12)?;
    film.click()?;
    film.hold(16)?;
    let wield = crate::map_preview::loot::WIELD;
    film.glide_ui([ox + wield.x + wield.w / 2, oy + wield.y + wield.h / 2], 14)?;
    for _ in 0..2 {
        film.hold(12)?;
        film.click()?;
    }
    film.hold(30)?;
    Ok(film.frames)
}

/// `f9-live-debugger.gif`: F9 opens the vault-debug overlay over the live
/// world; F8 steps its pages (the watch view, the object inspector with a
/// critter clicked, the GVAR and MVAR editors, the map jump, the item
/// spawner) to the camera page, whose Export Current Setup as Mod button
/// writes the running setup to `export` as a mod folder.
pub(super) fn debugger(corpus: &Path, export: &Path) -> AppResult<Frames> {
    let mut preview = world(corpus, EnginePreset::Enhanced)?;
    widen(&mut preview, false)?;
    frame_player(&mut preview, 150);
    // The NPC out of its building, west of the player (the overlay's
    // panel covers the view's right side), so the inspector's click finds
    // it.
    let npc = npc(&mut preview)?;
    let beside = HexGrid::step(STAND, 4, 3)?;
    map(&mut preview)
        .host
        .world
        .move_object(npc, beside, 0, false)?;
    map(&mut preview).mod_export = Some(export.to_path_buf());
    let mut film = Film::new(preview);
    film.point_hex(HexGrid::step(STAND, 3, 3)?, 0.0)?;
    film.hold(12)?;
    // F9: the watch view.
    film.key(KeyCode::F9, [false; 3]);
    film.hold(30)?;
    // F8: the object inspector; a click selects the critter.
    film.key(KeyCode::F8, [false; 3]);
    film.hold(8)?;
    film.point_hex(beside, 20.0)?;
    film.hold(4)?;
    film.click()?;
    film.hold(30)?;
    // The GVAR editor, its cursor moved down a few rows.
    film.key(KeyCode::F8, [false; 3]);
    film.hold(10)?;
    for _ in 0..4 {
        film.key(KeyCode::ArrowDown, [false; 3]);
        film.hold(4)?;
    }
    film.hold(10)?;
    // The MVAR editor, the map jump (down two maps), the item spawner.
    for downs in [0, 2, 0] {
        film.key(KeyCode::F8, [false; 3]);
        film.hold(8)?;
        for _ in 0..downs {
            film.key(KeyCode::ArrowDown, [false; 3]);
            film.hold(4)?;
        }
        film.hold(10)?;
    }
    // The camera page and its export button.
    film.key(KeyCode::F8, [false; 3]);
    film.hold(12)?;
    let [x, y, w, h] = crate::map_preview::debug::EXPORT_BUTTON;
    film.glide_ui([x + w / 2, y + h / 2], 14)?;
    film.hold(6)?;
    film.click()?;
    film.hold(45)?;
    Ok(film.frames)
}

/// `touch-ui-menu.gif`: the touch UI (`--touch-ui`) over the widescreen
/// world: its pill bar at the top right (MODE, PEEK, TURN, INV, PIP, ESC);
/// a tap on MODE (a right click) turns the cursor to the arrow; then a
/// finger held on the companion presses after 250 ms, the game's own held-
/// button pulse opens the action menu at 500 ms, the finger slides down
/// through its entries, and lifting on Gear opens the companion's Gear
/// window.
pub(super) fn touch_menu(corpus: &Path) -> AppResult<Frames> {
    use winit::event::TouchPhase::{Ended, Moved, Started};
    let mut preview = world(corpus, EnginePreset::Enhanced)?;
    widen(&mut preview, false)?;
    frame_player(&mut preview, 150);
    preview.set_touch_ui(true);
    let npc = npc(&mut preview)?;
    companion(&mut preview, npc)?;
    let mut film = Film::new(preview);
    film.hold(15)?;
    // A tap on MODE.
    let mode = map(&mut film.preview)
        .touch_bar()
        .into_iter()
        .find(|(button, _)| *button == crate::map_preview::touchbar::BarButton::Mode)
        .map(|(_, rect)| rect)
        .ok_or("no MODE button")?;
    let tap = film.ui_point([mode.x + mode.w / 2, mode.y + mode.h / 2])?;
    film.finger(Started, tap)?;
    film.hold(4)?;
    film.finger(Ended, tap)?;
    film.hold(14)?;
    // A finger held on the companion.
    film.point_hex(HexGrid::step(STAND, 4, 1)?, 18.0)?;
    let on = film.pointer.ok_or("no pointer")?;
    film.finger(Started, on)?;
    film.hold(22)?;
    // The slide down the entries: each move of more than 10 interface
    // pixels moves the highlight one entry (SPEC-014 §3.28). The companion's
    // menu is Talk, Look, Inventory, Skill, Push, Gear, Cancel: five steps
    // reach Gear.
    let scale =
        enhanced_layout(&mut film.preview, MOVIE).map_or(1.0, |layout| f64::from(layout.ui_scale));
    let mut at = on;
    for _ in 0..5 {
        for _ in 0..3 {
            at[1] += 12.0 * scale / 3.0;
            film.finger(Moved, at)?;
            film.shoot()?;
        }
        film.hold(8)?;
    }
    film.hold(8)?;
    film.finger(Ended, at)?;
    film.hold(40)?;
    Ok(film.frames)
}
