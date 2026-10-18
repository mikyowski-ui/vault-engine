//! SPEC-011 §3.20-3.21 character screen: opening it reconciles levels
//! (skill points and the pending-perk byte); the sheet shows SPECIAL,
//! derived statistics, skills, traits, and perks. Unspent points buy skill
//! points, the perk chooser lists eligible perks by display name (Tag! and
//! Mutate! open their sub-selections; cancelling them removes the rank),
//! and Cancel restores the sheet saved on opening, so reconciliation
//! repeats on the next visit.
use super::{MapPreview, Screen, button, inventory::inside, panel_text};
use crate::AppResult;
use vault_core::{CharacterSheet, Perk, Trait, assets::WorldAssets, progression::PerkChoice};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const SKILLS: [&str; 18] = [
    "Small Guns",
    "Big Guns",
    "Energy Weapons",
    "Unarmed",
    "Melee Weapons",
    "Throwing",
    "First Aid",
    "Doctor",
    "Sneak",
    "Lockpick",
    "Steal",
    "Traps",
    "Science",
    "Repair",
    "Speech",
    "Barter",
    "Gambling",
    "Outdoorsman",
];
const TRAITS: [(&str, Trait); 16] = [
    ("Fast Metabolism", Trait::FastMetabolism),
    ("Bruiser", Trait::Bruiser),
    ("Small Frame", Trait::SmallFrame),
    ("One Hander", Trait::OneHander),
    ("Finesse", Trait::Finesse),
    ("Kamikaze", Trait::Kamikaze),
    ("Heavy Handed", Trait::HeavyHanded),
    ("Fast Shot", Trait::FastShot),
    ("Bloody Mess", Trait::BloodyMess),
    ("Jinxed", Trait::Jinxed),
    ("Good Natured", Trait::GoodNatured),
    ("Chem Reliant", Trait::ChemReliant),
    ("Chem Resistant", Trait::ChemResistant),
    ("Night Person", Trait::NightPerson),
    ("Skilled", Trait::Skilled),
    ("Gifted", Trait::Gifted),
];
const SPECIAL: [&str; 7] = ["ST", "PE", "EN", "CH", "IN", "AG", "LK"];
const LINE: i32 = 16;
const DONE: [i32; 4] = [420, 440, 90, 20];
const CANCEL: [i32; 4] = [520, 440, 90, 20];
const PERKS: [i32; 4] = [310, 440, 100, 20];

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
}

fn line(column: i32, index: usize) -> [i32; 4] {
    [column, 70 + index as i32 * LINE, 200, LINE - 2]
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
        self.screen = Screen::Character(Box::new(CharacterScreen {
            saved,
            mode: if pending {
                Mode::Chooser(self.host.perk_choices())
            } else {
                Mode::Sheet
            },
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
            (KeyCode::Enter, Some(Mode::Sheet)) => self.close_character(true),
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
        if inside(CANCEL, point) {
            self.press();
            match mode {
                Mode::Tag | Mode::Mutate(_) => self.cancel_sub_selection(),
                Mode::Chooser(_) => self.set_mode(Mode::Sheet),
                Mode::Sheet => self.close_character(false),
            }
            return true;
        }
        match mode {
            Mode::Sheet => {
                if inside(DONE, point) {
                    self.press();
                    self.close_character(true);
                } else if inside(PERKS, point)
                    && self
                        .host
                        .world
                        .player_sheet()
                        .is_some_and(|sheet| sheet.pending_perk)
                {
                    self.press();
                    self.set_mode(Mode::Chooser(self.host.perk_choices()));
                } else if let Some(skill) = (0..18).find(|index| inside(line(200, *index), point)) {
                    if !self
                        .host
                        .world
                        .player_sheet_mut()
                        .is_some_and(|sheet| sheet.buy_skill(skill))
                    {
                        self.status = b"No skill points left".to_vec();
                    }
                } else {
                    return false;
                }
            }
            Mode::Chooser(choices) => {
                let Some(index) = (0..choices.len()).find(|index| inside(line(200, *index), point))
                else {
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
                let Some(skill) = (0..18).find(|index| inside(line(200, *index), point)) else {
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
                let Some(index) = (0..16).find(|index| inside(line(200, *index), point)) else {
                    return false;
                };
                let chosen = TRAITS[index].1 as i32;
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
        let Some(mode) = self.character_mode() else {
            return Err("character screen is not open".into());
        };
        let palette = &self.palette;
        let mut image = IndexedImage::new(640, 480)?;
        let Some(font) = &self.font else {
            return Ok(image);
        };
        let green = palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
        let white = palette.lookup_rgb5([31, 31, 31]).unwrap_or(1);
        let dim = palette.lookup_rgb5([0, 15, 0]).unwrap_or(1);
        let world = &self.host.world;
        let player = world.player;
        let context = world.context(player);
        let Some(sheet) = world.player_sheet() else {
            return Ok(image);
        };
        let text = |file: &str, id: i32, fallback: String| {
            self.host
                .assets
                .game_messages(file)
                .ok()
                .and_then(|catalog| catalog.get(id).map(|message| message.text.clone()))
                .unwrap_or_else(|| fallback.into_bytes())
        };
        let header = [
            sheet.name.as_slice(),
            format!(
                "   Level {}   Experience {} / {}",
                sheet.level,
                sheet.experience,
                vault_core::progression::xp_threshold(sheet.level)
            )
            .as_bytes(),
        ]
        .concat();
        panel_text(&mut image, font, palette, &header, [20, 30], white);
        // SPECIAL and derived statistics.
        for (index, name) in SPECIAL.iter().enumerate() {
            let entry = format!("{name}  {}", sheet.stat(index, context));
            panel_text(
                &mut image,
                font,
                palette,
                entry.as_bytes(),
                [20, line(20, index)[1]],
                green,
            );
        }
        for (row, (label, value)) in [
            (
                "Hit Points",
                format!("{} / {}", sheet.current_hp, sheet.stat(7, context)),
            ),
            ("Armor Class", sheet.stat(9, context).to_string()),
            ("Action Points", sheet.stat(8, context).to_string()),
            ("Carry Weight", sheet.stat(12, context).to_string()),
            ("Skill points", sheet.unspent_skills.to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            let entry = format!("{label}  {value}");
            panel_text(
                &mut image,
                font,
                palette,
                entry.as_bytes(),
                [20, line(20, 8 + row)[1]],
                green,
            );
        }
        // The middle column: skills, the chooser, or a sub-selection.
        match &mode {
            Mode::Sheet | Mode::Tag => {
                for (index, name) in SKILLS.iter().enumerate() {
                    let tagged = if sheet.tags.contains(&(index as i32)) {
                        "*"
                    } else {
                        " "
                    };
                    let entry = format!("{tagged}{name}  {}%", sheet.skill(index, context));
                    let rect = line(200, index);
                    panel_text(
                        &mut image,
                        font,
                        palette,
                        entry.as_bytes(),
                        [rect[0], rect[1]],
                        green,
                    );
                }
                if mode == Mode::Tag {
                    panel_text(
                        &mut image,
                        font,
                        palette,
                        b"Tag!: choose a skill to tag",
                        [200, 50],
                        white,
                    );
                }
            }
            Mode::Chooser(choices) => {
                panel_text(
                    &mut image,
                    font,
                    palette,
                    b"Choose a perk",
                    [200, 50],
                    white,
                );
                for (index, (_, name)) in choices.iter().enumerate().take(22) {
                    let rect = line(200, index);
                    panel_text(&mut image, font, palette, name, [rect[0], rect[1]], green);
                }
            }
            Mode::Mutate(drop) => {
                let title: &[u8] = if drop.is_some() {
                    b"Mutate!: choose a new trait"
                } else {
                    b"Mutate!: choose a trait to drop"
                };
                panel_text(&mut image, font, palette, title, [200, 50], white);
                for (index, (name, value)) in TRAITS.iter().enumerate() {
                    let held = sheet.traits.contains(&(*value as i32));
                    let rect = line(200, index);
                    panel_text(
                        &mut image,
                        font,
                        palette,
                        name.as_bytes(),
                        [rect[0], rect[1]],
                        if held { white } else { green },
                    );
                }
            }
        }
        // Traits and perks.
        let mut row = 0;
        panel_text(&mut image, font, palette, b"Traits", [430, 50], white);
        for value in sheet.traits.iter().filter(|value| **value >= 0) {
            let name = TRAITS.get(*value as usize).map_or("?", |(name, _)| name);
            panel_text(
                &mut image,
                font,
                palette,
                name.as_bytes(),
                [430, line(430, row)[1]],
                green,
            );
            row += 1;
        }
        row += 1;
        panel_text(
            &mut image,
            font,
            palette,
            b"Perks",
            [430, line(430, row)[1]],
            white,
        );
        row += 1;
        for (perk, rank) in sheet
            .perks
            .iter()
            .filter(|(perk, rank)| **rank > 0 && perk.selectable())
        {
            let index = perk.index() as i32;
            let name = text("perk.msg", 101 + index, format!("Perk {index}"));
            let entry = [name.as_slice(), format!(" ({rank})").as_bytes()].concat();
            if row < 22 {
                panel_text(
                    &mut image,
                    font,
                    palette,
                    &entry,
                    [430, line(430, row)[1]],
                    green,
                );
            }
            row += 1;
        }
        if mode == Mode::Sheet {
            button(&mut image, font, palette, b"Done", DONE);
            if sheet.pending_perk {
                button(&mut image, font, palette, b"Perks", PERKS);
            }
        }
        button(&mut image, font, palette, b"Cancel", CANCEL);
        let hint: &[u8] = if mode == Mode::Sheet {
            b"Click a skill to spend a point"
        } else {
            b""
        };
        panel_text(&mut image, font, palette, hint, [20, 444], dim);
        panel_text(&mut image, font, palette, &self.status, [22, 466], green);
        Ok(image)
    }
}
