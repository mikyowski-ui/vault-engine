//! SPEC-011 §3.20-3.21 character screen: opening it reconciles levels
//! (skill points and the pending-perk byte); the sheet shows SPECIAL,
//! derived statistics, skills, traits, and perks. Unspent points buy skill
//! points, the perk chooser lists eligible perks by display name (Tag! and
//! Mutate! open their sub-selections; cancelling them removes the rank),
//! and Cancel restores the sheet saved on opening, so reconciliation
//! repeats on the next visit.
//!
//! The screen is the character editor outside creation mode. SPEC-014 §3.8
//! lays out only creation mode, so the parts the two modes share are drawn
//! as §3.8 draws them (`editor_screen`'s pieces): the SPECIAL column with
//! big numbers and level words, the condition and derived lists, the skill
//! list, the info card's title and its hot regions, and the DONE and
//! CANCEL red buttons with their `editor.msg` captions. SPEC-013 §3.20
//! lights the Poisoned and Radiated lines (`editor.msg` 312 and 313).
//!
//! Gaps (no specification defines them; the stand-ins are kept):
//! - the in-game background: the creation background `edtrcrte.frm` (the
//!   only editor background a specification names) stands in;
//! - where the level, experience, unspent skill points, traits and perks
//!   go, and the perk chooser and Tag!/Mutate! windows: they reuse the
//!   creation-only areas (character points box, skill-panel counter,
//!   optional-traits box, skill list);
//! - which `editor.msg` captions name the level and experience lines (the
//!   §2.4 ranges cover 100-102, 104, 107, 108 and 116-158 only), so those
//!   two stay built-in English;
//! - when condition lines 314-318 are lit, so they stay inactive;
//! - how points are spent (a click on a skill row spends one) and the
//!   refusal texts of the chooser and sub-selections.
use super::{MapPreview, Screen};
use crate::{
    AppResult,
    editor_screen::{self, EDITOR_CANCEL, EDITOR_DONE, EDITOR_OPTIONS, SheetView, row_key},
    interface::Painter,
};
use vault_core::{
    CharacterSheet, Perk,
    progression::PerkChoice,
    ui::{
        BUTTON_PRESS, ColorKey, Rect,
        editor::{self, InfoLine},
    },
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// SPEC-014 §3.8: a change of info card plays this.
const CARD_SOUND: &[u8] = b"isdxxxx1";
/// The rows of the skill list (§3.8 `skill_y`), which the chooser and the
/// sub-selections reuse. G-31: only the rows drawn can be picked.
const LIST_ROWS: usize = 18;

/// What the screen presents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Sheet,
    /// The perk chooser: eligible perks with their names.
    Chooser(Vec<(Perk, Vec<u8>)>),
    /// Tag!: choose a fourth tag skill.
    Tag,
    /// Mutate!: the trait to drop, then the trait to take.
    Mutate(Option<i32>),
}
pub(crate) struct CharacterScreen {
    /// The sheet on opening, restored by Cancel.
    saved: Option<CharacterSheet>,
    pub mode: Mode,
    /// §3.8: the info line, starting at the Strength card.
    pub selected: InfoLine,
}

/// §3.8 the red buttons' captions: DONE (`editor.msg` 100) at (475, 454),
/// CANCEL (102) at (571, 454); the pending-perk button takes the OPTIONS
/// slot (caption at (363, 454)) with `editor.msg` 124.
const DONE_CAPTION: [i32; 2] = [475, 454];
const CANCEL_CAPTION: [i32; 2] = [571, 454];
const PERKS_CAPTION: [i32; 2] = [363, 454];

/// The list row a press in the skill list picks (§3.8 skill hot region).
fn list_row(point: [i32; 2]) -> Option<usize> {
    match editor::info_hit(point) {
        Some(InfoLine::Skill(k)) => Some(k),
        _ => None,
    }
}

impl MapPreview {
    pub(crate) fn open_character(&mut self) {
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let saved = self.host.world.player_sheet().cloned();
        let pending = self
            .host
            .world
            .player_sheet_mut()
            .is_some_and(|sheet| sheet.reconcile_levels(context));
        self.status.clear();
        self.screen = Screen::Character(Box::new(CharacterScreen {
            saved,
            mode: if pending {
                Mode::Chooser(self.host.perk_choices())
            } else {
                Mode::Sheet
            },
            selected: InfoLine::Special(0),
        }));
    }
    fn character_mode(&self) -> Option<Mode> {
        match &self.screen {
            Screen::Character(screen) => Some(screen.mode.clone()),
            _ => None,
        }
    }
    fn set_mode(&mut self, mode: Mode) {
        if let Screen::Character(screen) = &mut self.screen {
            screen.mode = mode;
        }
    }
    /// §3.8: a press in an info hot region selects that line; a change of
    /// card title plays `isdxxxx1`.
    fn select_card(&mut self, line: InfoLine) {
        let Screen::Character(screen) = &mut self.screen else {
            return;
        };
        if screen.selected == line {
            return;
        }
        let changed = editor_screen::card_title(screen.selected) != editor_screen::card_title(line);
        screen.selected = line;
        self.status.clear();
        if changed {
            self.play_effect(CARD_SOUND, None);
        }
    }
    /// Leave the screen: Done keeps the sheet, Cancel restores the saved
    /// one (and with it the reconciliation and the pending byte).
    fn close_character(&mut self, keep: bool) {
        let Screen::Character(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        if !keep
            && let (Some(saved), Some(sheet)) = (screen.saved, self.host.world.player_sheet_mut())
        {
            *sheet = saved;
        }
        let player = self.host.world.player;
        self.host.world.sync_hit_points(player);
        self.return_to_map();
    }
    pub(crate) fn character_key(&mut self, key: KeyCode) -> bool {
        match (key, self.character_mode()) {
            (KeyCode::Escape, Some(Mode::Chooser(_))) => self.set_mode(Mode::Sheet),
            (KeyCode::Escape, Some(Mode::Tag | Mode::Mutate(_))) => self.cancel_sub_selection(),
            (KeyCode::Escape, _) => self.close_character(false),
            // SPEC-014 §3.9: Return, `d` and `D` act as Done after
            // `ib1p1xx1`.
            (KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyD, Some(Mode::Sheet)) => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                self.close_character(true);
            }
            _ => return false,
        }
        true
    }
    /// A cancelled Tag!/Mutate! removes the rank and keeps the pending pick.
    fn cancel_sub_selection(&mut self) {
        let perk = match self.character_mode() {
            Some(Mode::Tag) => Perk::Tag,
            Some(Mode::Mutate(_)) => Perk::Mutate,
            _ => return,
        };
        let context = self.host.world.context(self.host.world.player);
        if let Some(sheet) = self.host.world.player_sheet_mut() {
            sheet.cancel_perk_choice(perk, context);
        }
        self.set_mode(Mode::Chooser(self.host.perk_choices()));
    }
    pub(crate) fn character_click(&mut self, point: [i32; 2]) -> bool {
        let Some(mode) = self.character_mode() else {
            return false;
        };
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        if EDITOR_CANCEL.contains(point) {
            self.press_button(EDITOR_CANCEL);
            match mode {
                Mode::Tag | Mode::Mutate(_) => self.cancel_sub_selection(),
                Mode::Chooser(_) => self.set_mode(Mode::Sheet),
                Mode::Sheet => self.close_character(false),
            }
            return true;
        }
        match mode {
            Mode::Sheet => {
                let pending = self
                    .host
                    .world
                    .player_sheet()
                    .is_some_and(|sheet| sheet.pending_perk);
                if EDITOR_DONE.contains(point) {
                    self.press_button(EDITOR_DONE);
                    self.close_character(true);
                    return true;
                }
                if pending && EDITOR_OPTIONS.contains(point) {
                    self.press_button(EDITOR_OPTIONS);
                    self.set_mode(Mode::Chooser(self.host.perk_choices()));
                    return true;
                }
                // §3.8 info hot regions the in-game sheet shares; the
                // creation-only regions (character points, optional traits,
                // trait rows, tag skills card) are not.
                let line = match editor::info_hit(point) {
                    Some(
                        line @ (InfoLine::Special(_)
                        | InfoLine::Condition(_)
                        | InfoLine::Derived(_)
                        | InfoLine::SkillsCard
                        | InfoLine::Skill(_)),
                    ) => line,
                    _ => return false,
                };
                self.select_card(line);
                if let InfoLine::Skill(skill) = line
                    && !self
                        .host
                        .world
                        .player_sheet_mut()
                        .is_some_and(|sheet| sheet.buy_skill(skill))
                {
                    // `editor.msg` 136 "No free skill points available."
                    self.status = self.message("editor.msg", 136);
                }
            }
            Mode::Chooser(choices) => {
                let rows = choices.len().min(LIST_ROWS);
                let Some(index) = list_row(point).filter(|index| *index < rows) else {
                    return false;
                };
                self.press();
                let perk = choices[index].0;
                match self.host.choose_perk(perk) {
                    PerkChoice::Granted => self.set_mode(Mode::Sheet),
                    PerkChoice::TagSelection => self.set_mode(Mode::Tag),
                    PerkChoice::TraitSelection => self.set_mode(Mode::Mutate(None)),
                    PerkChoice::Refused => self.status = b"That perk cannot be chosen".to_vec(),
                }
            }
            Mode::Tag => {
                let Some(skill) = list_row(point) else {
                    return false;
                };
                let tagged = self.host.world.player_sheet_mut().is_some_and(|sheet| {
                    let free = sheet.tags.iter().position(|tag| *tag < 0);
                    match free {
                        Some(slot) if !sheet.tags.contains(&(skill as i32)) => {
                            sheet.tags[slot] = skill as i32;
                            true
                        }
                        _ => false,
                    }
                });
                if tagged {
                    self.set_mode(Mode::Sheet);
                } else {
                    self.status = b"That skill cannot be tagged".to_vec();
                }
            }
            Mode::Mutate(drop) => {
                let Some(chosen) = list_row(point).filter(|row| *row < 16) else {
                    return false;
                };
                let chosen = chosen as i32;
                let Some(sheet) = self.host.world.player_sheet_mut() else {
                    return false;
                };
                match drop {
                    None if sheet.traits.contains(&chosen) => {
                        self.set_mode(Mode::Mutate(Some(chosen)));
                    }
                    Some(old) if !sheet.traits.contains(&chosen) => {
                        if let Some(slot) = sheet.traits.iter().position(|t| *t == old) {
                            sheet.traits[slot] = chosen;
                        }
                        sheet.recompute(context);
                        self.host.world.sync_hit_points(player);
                        self.set_mode(Mode::Sheet);
                    }
                    _ => self.status = b"Choose a trait you have, then a new one".to_vec(),
                }
            }
        }
        true
    }
    pub(crate) fn character_image(&self) -> AppResult<IndexedImage> {
        let Screen::Character(screen) = &self.screen else {
            return Err("character screen is not open".into());
        };
        let mut image = IndexedImage::new(640, 480)?;
        let world = &self.host.world;
        let context = world.context(world.player);
        let Some(sheet) = world.player_sheet() else {
            return Ok(image);
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        // Gap: the in-game background is unspecified; the creation one
        // stands in.
        painter.blit_opaque(&mut image, editor::BACKGROUND, 0, [0, 0]);
        editor_screen::draw_top_buttons(&painter, &mut image, sheet, context, None);
        let view = SheetView {
            sheet,
            context,
            special: std::array::from_fn(|stat| sheet.stat(stat, context)),
            hit_points: sheet.current_hp,
            // SPEC-013 §3.20: Poisoned and Radiated light when the
            // player's poison or radiation is nonzero. Gap: 314-318.
            conditions: [
                true,
                sheet.poison != 0,
                sheet.radiation != 0,
                false,
                false,
                false,
                false,
                false,
            ],
            selected: screen.selected,
        };
        editor_screen::draw_special(&painter, &mut image, &view);
        editor_screen::draw_conditions(&painter, &mut image, &view);
        editor_screen::draw_card(&painter, &mut image, screen.selected);
        // The skill panel: the skills, the chooser, or a sub-selection,
        // under an `editor.msg` caption (117 SKILLS, 152 PICK A PERK,
        // 153 PICK A NEW TRAIT, 154 LOSE A TRAIT, 155 PICK A NEW TAG SKILL).
        let caption = |id| painter.message("editor.msg", id);
        let row = |image: &mut IndexedImage, k: usize, text: &[u8], key: ColorKey| {
            painter.text(image, 101, key, text, [380, editor::skill_y(k)], 190);
        };
        match &screen.mode {
            Mode::Sheet => editor_screen::draw_skills(&painter, &mut image, &view, &caption(117)),
            Mode::Tag => editor_screen::draw_skills(&painter, &mut image, &view, &caption(155)),
            Mode::Chooser(choices) => {
                painter.text(
                    &mut image,
                    103,
                    ColorKey::LABEL,
                    &caption(152),
                    [380, 5],
                    200,
                );
                for (k, (_, name)) in choices.iter().enumerate().take(LIST_ROWS) {
                    row(&mut image, k, name, ColorKey::GREEN);
                }
            }
            Mode::Mutate(drop) => {
                let id = if drop.is_some() { 153 } else { 154 };
                painter.text(
                    &mut image,
                    103,
                    ColorKey::LABEL,
                    &caption(id),
                    [380, 5],
                    200,
                );
                for k in 0..16 {
                    let held = sheet.traits.contains(&k);
                    let text = painter.message("trait.msg", 100 + k);
                    row(
                        &mut image,
                        k as usize,
                        &text,
                        row_key(held, *drop == Some(k)),
                    );
                }
            }
        }
        // The skill panel's counter (the creation tag counter's place):
        // unspent skill points under `editor.msg` 130.
        painter.text(
            &mut image,
            103,
            ColorKey::LABEL,
            &caption(130),
            [422, 233],
            100,
        );
        for (cell, x) in editor::big_number_cells(sheet.unspent_skills, sheet.unspent_skills)
            .iter()
            .zip([522, 536])
        {
            painter.big_digit(&mut image, *cell, [x, 228], false);
        }
        // The character points box: the level in big numbers, experience
        // and the next level's threshold (built-in English, see above).
        painter.text(&mut image, 103, ColorKey::LABEL, b"LEVEL", [14, 276], 100);
        for (cell, x) in editor::big_number_cells(sheet.level, sheet.level)
            .iter()
            .zip([126, 140])
        {
            painter.big_digit(&mut image, *cell, [x, 282], false);
        }
        let next = vault_core::progression::xp_threshold(sheet.level);
        let experience = if next > 0 {
            format!("Exp {}/{next}", sheet.experience)
        } else {
            format!("Exp {}", sheet.experience)
        };
        painter.text(
            &mut image,
            101,
            ColorKey::GREEN,
            experience.as_bytes(),
            [14, 296],
            108,
        );
        self.draw_traits_and_perks(&painter, &mut image, sheet);
        // §3.8 red buttons: DONE and the pending-perk button on the sheet,
        // CANCEL always.
        let mut buttons = vec![(EDITOR_CANCEL, CANCEL_CAPTION, 102)];
        if screen.mode == Mode::Sheet {
            buttons.push((EDITOR_DONE, DONE_CAPTION, 100));
            if sheet.pending_perk {
                buttons.push((EDITOR_OPTIONS, PERKS_CAPTION, 124));
            }
        }
        for (rect, caption, id) in buttons {
            self.draw_red_button(&painter, &mut image, rect);
            let text = painter.message("editor.msg", id);
            painter.text(&mut image, 103, ColorKey::LABEL, &text, caption, 80);
        }
        // Refusals and other notes on the info card's description area.
        painter.text(
            &mut image,
            101,
            ColorKey::BLACK,
            &self.status,
            [348, 315],
            266,
        );
        Ok(image)
    }
    fn draw_red_button(&self, painter: &Painter, image: &mut IndexedImage, rect: Rect) {
        let art = self.button_art(rect, "lilredup.frm", "lilreddn.frm");
        painter.blit(image, art, 0, [rect.x, rect.y]);
    }
    /// The optional-traits box: TRAITS (`editor.msg` 156) over the left
    /// column's held traits (`trait.msg` 100 + trait), Perks (124) over the
    /// right column's perks (`perk.msg` 101 + perk, with the rank); perks
    /// past the right column continue below the traits. §3.8 trait rows:
    /// x = 47 or 199, y = trunc(353 + 13.56k), font 101.
    fn draw_traits_and_perks(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        sheet: &CharacterSheet,
    ) {
        for (id, x) in [(156, 52), (124, 199)] {
            let caption = painter.message("editor.msg", id);
            painter.text(image, 103, ColorKey::LABEL, &caption, [x, 326], 110);
        }
        let traits: Vec<i32> = sheet
            .traits
            .iter()
            .copied()
            .filter(|value| *value >= 0)
            .collect();
        for (row, value) in traits.iter().enumerate() {
            let text = painter.message("trait.msg", 100 + value);
            painter.text(
                image,
                101,
                ColorKey::GREEN,
                &text,
                [47, editor::trait_label_y(row)],
                140,
            );
        }
        let slots = (0..8)
            .map(|row| [199, editor::trait_label_y(row)])
            .chain((traits.len() + 1..8).map(|row| [47, editor::trait_label_y(row)]));
        let perks = sheet
            .perks
            .iter()
            .filter(|(perk, rank)| **rank > 0 && perk.selectable());
        for (slot, (perk, rank)) in slots.zip(perks) {
            let mut text = painter.message("perk.msg", 101 + perk.index() as i32);
            text.extend(format!(" ({rank})").bytes());
            painter.text(image, 101, ColorKey::GREEN, &text, slot, 140);
        }
    }
}
