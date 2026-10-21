//! SPEC-014 §3.33 the automap window: opened by Tab (or the bar's Map
//! button, which sends Tab) with the scanner off, or by using a Motion
//! Sensor with the scanner on. `automap.frm` covers screen (75, 0), 519 by
//! 480, over the paused world; the Scanner and Cancel red buttons act on
//! their release, and the detail switch toggles on its press. The window
//! shows the player's elevation: seen walls, the exit-grid marker, the
//! player and (in high detail) seen scenery, plus every living, visible
//! critter while the scanner is on. The detail setting is the saved
//! automap display flag (SPEC-008 payload 17, mask `0x02`), low by default;
//! the scanner is cleared at every opening.
use super::{MapPreview, Screen, dialog::Dialog};
use crate::{AppResult, interface::Painter};
use vault_core::{
    ObjectRef,
    ui::{
        BUTTON_PRESS, BUTTON_RELEASE, ColorKey, ERROR_SOUND, Rect,
        automap::{self, Mark},
        saveload,
    },
};
use vault_formats::{
    automap::DISPLAY_HIGH_DETAIL,
    pro::{BasicFlags, Category},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct AutomapScreen {
    pub scanner: bool,
    /// A red button under a press, acting on its release over it.
    pressed: Option<Rect>,
}

/// The message with its `%s` replaced by `name`.
fn substitute(template: &[u8], name: &[u8]) -> Vec<u8> {
    match template.windows(2).position(|pair| pair == b"%s") {
        Some(at) => [&template[..at], name, &template[at + 2..]].concat(),
        None => template.to_vec(),
    }
}

/// A window rectangle on screen.
fn on_screen(rect: Rect) -> Rect {
    let [wx, wy] = automap::WINDOW;
    Rect::new(wx + rect.x, wy + rect.y, rect.w, rect.h)
}

impl MapPreview {
    fn high_detail(&self) -> bool {
        self.host.world.persistence.automap_flags & DISPLAY_HIGH_DETAIL != 0
    }
    /// Open over the map: the seen state is processed first.
    pub(crate) fn open_automap(&mut self, scanner: bool) {
        if !matches!(self.screen, Screen::Map) {
            return;
        }
        self.host.world.process_seen();
        self.screen = Screen::Automap(AutomapScreen {
            scanner,
            pressed: None,
        });
    }
    /// A Motion Sensor used from the inventory or the item slot: one charge
    /// opens the automap with the scanner on; an empty sensor prints
    /// `item.msg` 5 and opens nothing.
    pub(crate) fn use_motion_sensor(&mut self, sensor: ObjectRef) {
        if self.host.world.spend_charge(sensor) {
            // G-31: the inventory closes as it always does (the player
            // takes the figure's art, SPEC-014 §3.34) before the automap.
            if matches!(self.screen, Screen::Inventory(_)) {
                self.close_inventory();
            } else {
                self.return_to_map();
            }
            self.open_automap(true);
            return;
        }
        let name = self
            .host
            .world
            .get(sensor)
            .map(|object| object.name.clone())
            .unwrap_or_default();
        let text = self.message("item.msg", automap::NO_CHARGES);
        self.notify(substitute(&text, &name));
    }
    pub(crate) fn close_automap(&mut self) {
        if matches!(self.screen, Screen::Automap(_)) {
            self.screen = Screen::Map;
        }
    }
    fn set_detail(&mut self, high: bool) {
        let flags = &mut self.host.world.persistence.automap_flags;
        if high {
            *flags |= DISPLAY_HIGH_DETAIL;
        } else {
            *flags &= !DISPLAY_HIGH_DETAIL;
        }
    }
    /// `s`: back to the player's elevation (always shown here); with the
    /// scanner off, a Motion Sensor in the left hand, then the right, with
    /// charges spends one and turns it on. Otherwise `iisxxxx1` and the
    /// refusal box: `misc.msg` 17 without a sensor, 18 with an empty one.
    fn automap_scanner(&mut self) {
        let Screen::Automap(screen) = &self.screen else {
            return;
        };
        if screen.scanner {
            return;
        }
        let player = self.host.world.player;
        let held = [BasicFlags::LEFT_HAND, BasicFlags::RIGHT_HAND]
            .into_iter()
            .find_map(|hand| {
                self.host
                    .world
                    .get(player)?
                    .inventory
                    .iter()
                    .find_map(|(item, _)| {
                        let object = self.host.world.get(*item)?;
                        (object.raw.pid == automap::MOTION_SENSOR
                            && object.raw.flags.contains(hand))
                        .then_some(*item)
                    })
            });
        match held {
            Some(sensor) if self.host.world.spend_charge(sensor) => {
                if let Screen::Automap(screen) = &mut self.screen {
                    screen.scanner = true;
                }
            }
            held => {
                self.play_effect(ERROR_SOUND.as_bytes(), None);
                let title = self.message("misc.msg", if held.is_some() { 18 } else { 17 });
                self.show_dialog(Dialog::done(
                    automap::SCANNER_DIALOG,
                    title,
                    Vec::new(),
                    ColorKey::ERROR,
                ));
            }
        }
    }
    /// Tab, Esc and `a` close; `h` and `l` set the detail (the switch
    /// follows, under the safe rule); `s` the scanner; Ctrl+Q, Alt+X and
    /// F10 the quit confirmation, whose Yes closes the window. Other keys do
    /// nothing while the window is open.
    pub(crate) fn automap_key(&mut self, key: KeyCode) -> bool {
        let [_, control, alt] = self.hud.modifiers;
        match key {
            KeyCode::Tab | KeyCode::Escape | KeyCode::KeyA => self.close_automap(),
            KeyCode::KeyH => self.set_detail(true),
            KeyCode::KeyL => self.set_detail(false),
            KeyCode::KeyS => self.automap_scanner(),
            KeyCode::KeyQ if control => self.automap_quit(),
            KeyCode::KeyX if alt => self.automap_quit(),
            KeyCode::F10 => self.automap_quit(),
            // §3.33 F12 takes a screenshot.
            KeyCode::F12 => self.screenshot(),
            _ => {}
        }
        true
    }
    fn automap_quit(&mut self) {
        let title = self.message("misc.msg", 0);
        self.show_dialog(Dialog::quit_confirmation(
            [169, 117],
            title,
            ColorKey::ERROR,
        ));
    }
    /// A press: a red button waits for its release; the detail switch
    /// toggles at once (`toggle`), sending `l` switched down, `h` up.
    pub(crate) fn automap_click(&mut self, point: [i32; 2]) -> bool {
        if on_screen(automap::SWITCH).contains(point) {
            self.play_effect(automap::SWITCH_SOUND.as_bytes(), None);
            let high = !self.high_detail();
            self.set_detail(high);
            self.pressed = Some(automap::SWITCH_SOUND.as_bytes());
            return true;
        }
        let button = [automap::SCANNER, automap::CANCEL]
            .into_iter()
            .find(|rect| on_screen(*rect).contains(point));
        if let Some(rect) = button {
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
            if let Screen::Automap(screen) = &mut self.screen {
                screen.pressed = Some(rect);
            }
        }
        true
    }
    /// The release over the pressed red button sends its key.
    pub(crate) fn automap_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::Automap(screen) = &mut self.screen else {
            return false;
        };
        let Some(rect) = screen.pressed.take() else {
            return false;
        };
        self.play_effect(BUTTON_RELEASE.as_bytes(), None);
        if point.is_some_and(|point| on_screen(rect).contains(point)) {
            if rect == automap::SCANNER {
                self.automap_scanner();
            } else {
                self.close_automap();
            }
        }
        true
    }
    pub(crate) fn automap_image(&mut self) -> AppResult<IndexedImage> {
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let Screen::Automap(screen) = &self.screen else {
            return Ok(image);
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let [wx, wy] = automap::WINDOW;
        painter.blit_opaque(&mut image, automap::BACKGROUND, 0, [wx, wy]);
        for rect in [automap::SCANNER, automap::CANCEL] {
            let art = if screen.pressed == Some(rect) {
                "lilreddn.frm"
            } else {
                "lilredup.frm"
            };
            painter.blit(&mut image, art, 0, [wx + rect.x, wy + rect.y]);
        }
        let high = self.high_detail();
        let switch = if high {
            automap::SWITCH_UP
        } else {
            automap::SWITCH_DOWN
        };
        painter.blit(
            &mut image,
            switch,
            0,
            [wx + automap::SWITCH.x, wy + automap::SWITCH.y],
        );
        // The objects of the player's elevation, in object-list order.
        let world = &self.host.world;
        let player = world.player;
        let elevation = world
            .get(player)
            .map_or(world.elevation, |object| object.raw.elevation);
        let bright_green = painter.color(automap::BRIGHT_GREEN);
        let dark_green = painter.color(automap::DARK_GREEN);
        let paint = |image: &mut IndexedImage, [x, y]: [i32; 2], color: u8| {
            let (x, y) = (wx + x, wy + y);
            if !(0..640).contains(&x) || !(0..480).contains(&y) {
                return;
            }
            let index = (y * 640 + x) as usize;
            let pixels = image.pixels_mut();
            // Dark green never covers bright green; walls cover scenery.
            if color == dark_green && pixels[index] == bright_green {
                return;
            }
            pixels[index] = color;
        };
        for (reference, object) in world.iter() {
            if object.owner.is_some() || object.raw.elevation != elevation {
                continue;
            }
            let Some(at) = object.raw.hex.and_then(automap::pixel) else {
                continue;
            };
            let mark = match object.category() {
                _ if reference == player => Mark::Player,
                Category::Critter => Mark::Critter {
                    hidden: object.raw.flags.contains(BasicFlags::HIDDEN),
                    dead: world.is_dead(reference),
                },
                Category::Wall => Mark::Wall,
                Category::Scenery => Mark::Scenery {
                    pid: object.raw.pid,
                },
                _ => Mark::Other,
            };
            let seen = object.raw.flags.contains(BasicFlags::SEEN);
            let Some(key) = automap::color(mark, seen, screen.scanner, high) else {
                continue;
            };
            let color = painter.color(key);
            paint(&mut image, at, color);
            paint(&mut image, [at[0] + 1, at[1]], color);
            if mark == Mark::Player {
                for pixel in automap::player_cross(at) {
                    paint(&mut image, pixel, color);
                }
            }
        }
        // The area and elevation names, when the map has an index.
        let number = world.map.header.map_number;
        if let Ok(map) = u16::try_from(number)
            && let Some((area, level)) = saveload::location_messages(map, u16::from(elevation))
        {
            for (id, [x, y]) in [
                (area, automap::AREA_LABEL),
                (level, automap::ELEVATION_LABEL),
            ] {
                let text = painter.message("map.msg", id);
                painter.text(
                    &mut image,
                    101,
                    ColorKey::GREEN,
                    &text,
                    [wx + x, wy + y],
                    automap::LABEL_WIDTH,
                );
            }
        }
        Ok(image)
    }
}
