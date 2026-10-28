//! SPEC-019 §3.8 and §3.15 under the Fallout 2 profile: the dialogue
//! panel's Combat Control button for a party member with a control record,
//! the combat control panel that replaces the dialogue panel at screen
//! (0, 290) with the member's name, stats, weapon and armor, the five
//! disposition rows, Use Best Weapon, Custom and Talk; and the modal custom
//! panel, a window the size of its background (at (0, 290) with the
//! retail 640-by-190 art, at (170, 145) with the stand-in) with its six
//! cycling rows and Done. The rules are
//! `vault_core::fallout2_party`'s; the host applies them
//! (`choose_disposition`, `cycle_control`, `use_best_weapon`).
//!
//! SPEC-020 §2.1: the panels' layouts, the custom panel's open state and
//! the held button are `vault_core::ui::fallout2::ControlPanelWindow`'s;
//! this draws them, plays their sounds and applies their outcomes.
use super::{MapPreview, talk::TalkWindow};
use crate::interface::Painter;
use vault_core::{
    ObjectRef, fallout2_party as party,
    ui::{
        ColorKey, ERROR_SOUND, RED_BUTTON, Rect,
        fallout2::{
            self as ui2, ControlKey, ControlOutcome, ControlPanelWindow, control, custom, text,
        },
    },
};
use vault_formats::fallout2::party::ControlField;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The panel over a conversation with a party member: the member, and the
/// core window that owns the panels' state.
pub(crate) struct ControlPanel {
    pub member: ObjectRef,
    pub window: ControlPanelWindow,
}

/// A red button's screen rectangle at `at` in the window at `origin`.
fn screen(origin: [i32; 2], at: [i32; 2]) -> Rect {
    control::button_rect(at).offset(origin)
}

impl MapPreview {
    /// §3.8 the dialogue target, when it is a party member with a control
    /// record in a Fallout 2 game.
    pub(crate) fn control_member(&self) -> Option<ObjectRef> {
        let target = self.host.world.dialogue.target;
        self.host.control_of(target).map(|_| target)
    }
    /// SPEC-019 §3.8 the panel art: `di_talkp.frm` for a party member with
    /// a control record, else `di_talk.frm`, both by name; `None` keeps
    /// Fallout 1's row 99.
    pub(crate) fn fallout2_talk_panel(&self) -> Option<String> {
        self.host.world.fallout2.as_ref()?;
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let role = if self.control_member().is_some() {
            ui2::PARTY_TALK_PANEL
        } else {
            ui2::ArtRole {
                candidates: &["di_talk.frm"],
                ..ui2::PARTY_TALK_PANEL
            }
        };
        // Neither name loads: the panel of SPEC-014's list row stands in,
        // so the conversation still opens (§2.2 rule 3).
        let (name, _) = painter.role(&role)?;
        Some(format!("art/intrface/{name}"))
    }
    /// The Combat Control button or `c`: the control panel opens.
    pub(crate) fn open_control(&mut self) {
        if let Some(member) = self.control_member() {
            let allowed = self
                .host
                .party_record_of(member)
                .map(|record| record.allowed(ControlField::Disposition).to_vec())
                .unwrap_or_default();
            let window = ControlPanelWindow::new(allowed, self.custom_background());
            self.talk = Some(TalkWindow::Control(ControlPanel { member, window }));
        }
    }
    /// §3.15.3 (Revision 2) the custom panel's background size: the
    /// first frame of `custom.frm`, `None` for the stand-in.
    fn custom_background(&self) -> Option<[i32; 2]> {
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        painter.role(&ui2::CUSTOM_PANEL).and_then(|(_, art)| {
            art.frame(0, 0)
                .map(|frame| [i32::from(frame.width), i32::from(frame.height)])
        })
    }
    /// §3.15.3 (Revision 2) the custom panel's window: the size of
    /// `custom.frm`'s first frame, or the stand-in's.
    #[cfg(test)]
    pub(crate) fn custom_window(&self) -> Rect {
        custom::window(self.custom_background())
    }
    /// A press on a panel control: its sounds; it acts on release.
    pub(crate) fn control_click(&mut self, point: [i32; 2]) -> bool {
        let Some(TalkWindow::Control(panel)) = &mut self.talk else {
            return false;
        };
        let held = panel
            .window
            .press(point)
            .and_then(|button| Some((button, panel.window.layout().rect(button)?)));
        if let Some((button, rect)) = held {
            let (press, release) = button.sounds();
            self.press_button_cue(press.as_bytes(), release.as_bytes(), rect);
        }
        true
    }
    /// A left release at `point` over the held control (`over`, the held
    /// rectangle under the release) acts on it.
    pub(crate) fn control_release(&mut self, point: Option<[i32; 2]>, over: Option<Rect>) -> bool {
        let Some(TalkWindow::Control(panel)) = &mut self.talk else {
            return false;
        };
        let outcome = panel.window.release(over.and(point));
        if outcome != ControlOutcome::None {
            self.control_outcome(outcome);
            self.drain_effects();
        }
        true
    }
    /// Act on the core window's outcome for the panel's member.
    fn control_outcome(&mut self, outcome: ControlOutcome) {
        let Some(TalkWindow::Control(panel)) = &self.talk else {
            return;
        };
        let member = panel.member;
        match outcome {
            ControlOutcome::None | ControlOutcome::Custom(_) => {}
            ControlOutcome::Disposition(k) => {
                self.host.choose_disposition(member, k as i32);
            }
            ControlOutcome::Talk => self.talk = None,
            ControlOutcome::Cycle(field) => {
                self.host.cycle_control(member, field);
            }
            ControlOutcome::BestWeapon => match self.host.use_best_weapon(member) {
                Ok(true) => {}
                Ok(false) => self.play_effect(ERROR_SOUND.as_bytes(), None),
                Err(error) => self.status = error.to_string().into_bytes(),
            },
            ControlOutcome::QuitConfirm => self.confirm_quit(),
            ControlOutcome::Screenshot => self.screenshot(),
        }
    }
    /// §3.15.2 keys: `t` and Esc return to dialogue, `1`-`5` choose the
    /// disposition rows when allowed, `u` is Use Best Weapon, Ctrl+Q,
    /// Ctrl+X and F10 ask to quit, F12 takes a screenshot. §3.15.3: Enter
    /// and Esc close the custom panel.
    pub(crate) fn control_key(&mut self, key: KeyCode) -> bool {
        let [_, control_held, _] = self.hud.modifiers;
        let Some(TalkWindow::Control(panel)) = &mut self.talk else {
            return false;
        };
        let input = match key {
            KeyCode::KeyT => ControlKey::Talk,
            KeyCode::Escape => ControlKey::Escape,
            KeyCode::Enter | KeyCode::NumpadEnter => ControlKey::Enter,
            KeyCode::KeyU => ControlKey::Best,
            KeyCode::Digit1 => ControlKey::Digit(0),
            KeyCode::Digit2 => ControlKey::Digit(1),
            KeyCode::Digit3 => ControlKey::Digit(2),
            KeyCode::Digit4 => ControlKey::Digit(3),
            KeyCode::Digit5 => ControlKey::Digit(4),
            KeyCode::KeyQ | KeyCode::KeyX if control_held => ControlKey::Quit,
            KeyCode::F10 => ControlKey::Quit,
            KeyCode::F12 => ControlKey::F12,
            _ => ControlKey::Other,
        };
        let outcome = panel.window.key(input);
        self.control_outcome(outcome);
        // The panel's own buttons drain the world's effects.
        if matches!(
            outcome,
            ControlOutcome::Disposition(_) | ControlOutcome::BestWeapon
        ) {
            self.drain_effects();
        }
        true
    }
    /// The control panel over the conversation, and the custom panel over
    /// it when open.
    pub(crate) fn draw_control(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        panel: &ControlPanel,
    ) {
        let origin = control::ORIGIN;
        let at = |p: [i32; 2]| [origin[0] + p[0], origin[1] + p[1]];
        painter.blit_role(image, &ui2::CONTROL_PANEL, origin);
        let world = &self.host.world;
        let member = panel.member;
        let record = self.host.control_of(member);
        let table = self.host.party_record_of(member);
        painter.text(
            image,
            103,
            ColorKey::LABEL,
            text::CONTROL_TITLE.as_bytes(),
            at(control::TITLE),
            620,
        );
        let name = world
            .get(member)
            .map(|object| object.name.clone())
            .unwrap_or_default();
        painter.text(
            image,
            101,
            ColorKey::GREEN,
            &name,
            at(control::NAME),
            control::NAME_CLIP,
        );
        let stat = |index: usize| {
            world
                .get(member)
                .and_then(|object| object.character.as_ref())
                .map_or(0, |sheet| sheet.stat(index, world.context(member)))
        };
        let stats = [
            text::hp(world.hit_points(member).unwrap_or(0), stat(7)),
            text::ac(stat(9)),
            text::level(record.map_or(0, |record| record.stage) + 1),
        ];
        for (line, place) in stats.iter().zip(control::STATS) {
            painter.text(image, 101, ColorKey::GREEN, line.as_bytes(), at(place), 120);
        }
        // The weapon and armor boxes: the items' inventory art scaled in.
        let worn = world.get(member).and_then(|object| {
            object.inventory.iter().map(|(item, _)| *item).find(|item| {
                world.get(*item).is_some_and(|object| {
                    object
                        .raw
                        .flags
                        .contains(vault_formats::pro::BasicFlags::WORN)
                })
            })
        });
        let weapon = world.wielded(member, vault_core::critical::hit_mode::RIGHT_PRIMARY);
        for (caption, place, rect, item) in [
            (
                text::WEAPON,
                control::WEAPON_CAPTION,
                control::WEAPON_BOX,
                weapon,
            ),
            (
                text::ARMOR,
                control::ARMOR_CAPTION,
                control::ARMOR_BOX,
                worn,
            ),
        ] {
            painter.text(
                image,
                101,
                ColorKey::GREEN,
                caption.as_bytes(),
                at(place),
                90,
            );
            if let Some(item) = item
                && let Some(art) = self.item_icon(item)
                && let Some(frame) = art.frame(0, 0)
            {
                let [x, y] = at([rect.x, rect.y]);
                image.draw_frame(frame, [x as u32, y as u32], [rect.w as u32, rect.h as u32]);
            }
        }
        let button = |image: &mut IndexedImage, place: [i32; 2]| {
            let rect = screen(origin, place);
            let art = self.button_art(rect, RED_BUTTON.0, RED_BUTTON.1);
            if !painter.blit(image, art, 0, [rect.x, rect.y]) {
                painter.stand_in_button(image, [rect.x, rect.y]);
            }
        };
        button(image, control::BEST_BUTTON);
        painter.text(
            image,
            103,
            ColorKey::LABEL,
            text::BEST.as_bytes(),
            at(control::BEST_CAPTION),
            200,
        );
        let current = record.map_or(0, |record| party::value(&record, ControlField::Disposition));
        let allowed = table
            .as_ref()
            .map(|member| member.allowed(ControlField::Disposition).to_vec())
            .unwrap_or_default();
        for (k, label) in text::DISPOSITIONS.iter().enumerate() {
            button(image, control::disposition_button(k));
            let key = if !allowed.contains(&(k as i32)) {
                ColorKey::DARK_GREEN
            } else if current == k as i32 {
                ColorKey::SELECTED
            } else {
                ColorKey::GREEN
            };
            painter.text(
                image,
                101,
                key,
                label.as_bytes(),
                at(control::disposition_label(k)),
                200,
            );
        }
        button(image, control::CUSTOM_BUTTON);
        painter.text(
            image,
            103,
            ColorKey::LABEL,
            text::CUSTOM.as_bytes(),
            at(control::CUSTOM_CAPTION),
            200,
        );
        let talk = control::TALK;
        let rect = Rect::new(origin[0] + talk.x, origin[1] + talk.y, talk.w, talk.h);
        let art = self.button_art(rect, control::TALK_ART.0, control::TALK_ART.1);
        painter.blit(image, art, 0, [rect.x, rect.y]);
        if panel.window.custom {
            self.draw_custom(painter, image, member, panel.window.background);
        }
    }
    /// §3.15.3 the custom panel.
    fn draw_custom(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        member: ObjectRef,
        background: Option<[i32; 2]>,
    ) {
        let window = custom::window(background);
        let origin = [window.x, window.y];
        let at = |p: [i32; 2]| [origin[0] + p[0], origin[1] + p[1]];
        if painter.role(&ui2::CUSTOM_PANEL).is_some() {
            painter.blit_role(image, &ui2::CUSTOM_PANEL, origin);
        } else {
            let [w, h] = [window.w, window.h];
            image.fill_rect([origin[0], origin[1], w, h], painter.color(ColorKey::GREEN));
            image.fill_rect(
                [origin[0] + 1, origin[1] + 1, w - 2, h - 2],
                painter.color(ColorKey::BLACK),
            );
        }
        let record = self.host.control_of(member);
        let button = |image: &mut IndexedImage, place: [i32; 2]| {
            let rect = screen(origin, place);
            let art = self.button_art(rect, RED_BUTTON.0, RED_BUTTON.1);
            if !painter.blit(image, art, 0, [rect.x, rect.y]) {
                painter.stand_in_button(image, [rect.x, rect.y]);
            }
        };
        for (j, field) in ui2::CUSTOM_FIELDS.iter().enumerate() {
            painter.text(
                image,
                101,
                ColorKey::GREEN,
                text::ROWS[j].as_bytes(),
                at(custom::label(j)),
                130,
            );
            button(image, custom::button(j));
            let value = record.map_or_else(String::new, |record| {
                ui2::value_text(*field, party::value(&record, *field))
            });
            painter.text(
                image,
                101,
                ColorKey::SELECTED,
                value.as_bytes(),
                at(custom::value(j)),
                125,
            );
        }
        button(image, custom::DONE_BUTTON);
        painter.text(
            image,
            103,
            ColorKey::LABEL,
            text::DONE.as_bytes(),
            at(custom::DONE_CAPTION),
            100,
        );
    }
}
