//! SPEC-014 §3.42 the character screen outside creation mode, with the
//! SPEC-011 §3.20-3.21 progression it runs: opening takes the Cancel
//! snapshot and reconciles skill points and perk picks, a pending pick
//! opens the perk chooser (Tag! and Mutate! run their steps in the same
//! window), unspent points move through the skill slider, and Done keeps
//! or Cancel restores the snapshot. The screen is `edtredt.frm` with the
//! level block, the Perks, Karma and Kills folders, the condition lines,
//! and the info card of the selected line. PRINT (§3.43) writes the
//! printed sheet through the save file dialog.
//!
//! `[May-improve]` choices: the decrement floor never refunds below the
//! points invested at opening; the hot regions clamp to their blocks; an
//! effect perk's card keeps its title; an empty trait slot shows the
//! Optional traits card; the second trait slot is always drawn at y = 461;
//! the chooser heading restore reads its own rows; an empty chooser
//! ignores Done; Mutate! puts the new trait in the first empty slot; the
//! printed sheet applies its four safe rules. Cancelling Tag! or Mutate!
//! still consumes the pick (SPEC-011 §3.21).
use super::{
    MapPreview, Screen,
    modal_kit::{at, offset},
};
use crate::{
    AppResult,
    editor_screen::{self, FileDialog, FileEvent, Notice, PrintWorld, SheetView, game_text},
    interface::Painter,
};
use vault_core::{
    CharacterSheet, Perk,
    progression::{LEVEL_UP_FLAG, PerkChoice},
    ui::{
        BUTTON_PRESS, ColorKey, ERROR_SOUND,
        character::{
            self, BUTTON_CAPTIONS, CHOOSER_CANCEL, CHOOSER_DONE, CHOOSER_DOWN, CHOOSER_ORIGIN,
            CHOOSER_UP, Card, CharacterControl, CharacterKey, CharacterOutcome, ChooserControl,
            ChooserKey, ChooserOutcome, Folder, FolderView, PerkChooser, SkillFloor, SkillRefusal,
            slider_buttons,
        },
        editor::{self, InfoLine},
        flow::DialogSize,
        sheet::{self, DialogKind, SheetFile},
    },
};
use vault_formats::map::ObjectState;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// SPEC-014 §3.8: a change of info card plays this.
const CARD_SOUND: &[u8] = character::CARD_SOUND.as_bytes();
/// The folder tabs play this on release.
const TAB_SOUND: &[u8] = character::TAB_SOUND.as_bytes();
/// The rolling big-number frame lasts this long per digit.
const ROLL_MS: u64 = editor::ROLL_MS;

/// What persists from one opening to the next: the core screen's selected
/// info line, folder and skill cursor.
pub(crate) type Remembered = character::CharacterScreen;

/// The chooser's step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// PICK A PERK (`editor.msg` 152).
    Perk,
    /// Tag!: PICK A NEW TAG SKILL (155).
    Tag,
    /// Mutate! step 1: LOSE A TRAIT (154).
    LoseTrait,
    /// Mutate! step 2: PICK A NEW TRAIT (153).
    NewTrait,
}
impl Step {
    fn heading(self) -> i32 {
        match self {
            Self::Perk => 152,
            Self::Tag => 155,
            Self::LoseTrait => 154,
            Self::NewTrait => 153,
        }
    }
    /// An entry's card: picture, title and description messages.
    fn card(self, index: i32) -> (i32, (&'static str, i32), (&'static str, i32)) {
        match self {
            Self::Perk => (
                72 + index,
                ("perk.msg", 101 + index),
                ("perk.msg", 201 + index),
            ),
            Self::Tag => (
                28 + index,
                ("skill.msg", 100 + index),
                ("skill.msg", 200 + index),
            ),
            Self::LoseTrait | Self::NewTrait => (
                55 + index,
                ("trait.msg", 100 + index),
                ("trait.msg", 200 + index),
            ),
        }
    }
}

/// §3.42 the perk chooser window, also running Tag! and Mutate!: the core
/// chooser (SPEC-020 §2.1: its list, held control and arrow repeat; `list`
/// reads through it) and the entries it lists.
pub(crate) struct Chooser {
    pub step: Step,
    /// Perk, skill or trait index with its display name, sorted by name.
    pub entries: Vec<(i32, Vec<u8>)>,
    window: PerkChooser,
    /// The card shown last, for its change sound (none: the list's first).
    card: Option<(i32, Vec<u8>)>,
}
impl std::ops::Deref for Chooser {
    type Target = PerkChooser;
    fn deref(&self) -> &PerkChooser {
        &self.window
    }
}

/// A window over the screen.
pub(crate) enum Modal {
    Chooser(Box<Chooser>),
    /// A skill refusal box (A, B or C).
    Refusal(Notice),
    /// The PRINT file dialog.
    Print(FileDialog),
    /// A §3.43 message, or the overwrite question with the file Yes writes.
    Notice(Notice, Option<Vec<u8>>),
}

/// The open screen: the core screen (SPEC-020 §2.1: the selected info
/// line, the folder, the skill cursor, the held button and the slider's
/// repeat; `selected`, `folder` and `cursor` read through it), and the
/// progression state and windows only this layer holds.
pub(crate) struct CharacterScreen {
    ui: character::CharacterScreen,
    /// The sheet on opening, restored by Cancel.
    saved: Option<CharacterSheet>,
    /// The skill values and invested points on opening.
    floor: SkillFloor,
    /// The tags before Tag! and the traits on opening, for their Cancel.
    tags_before: [i32; 4],
    traits_at_open: [i32; 2],
    pub modal: Option<Modal>,
    /// The card shown last, for its change sound.
    card: Option<(i32, Vec<u8>)>,
    /// The unspent-points number rolling from a value since a time.
    roll: Option<(i32, i32, u64)>,
}

impl std::ops::Deref for CharacterScreen {
    type Target = character::CharacterScreen;
    fn deref(&self) -> &character::CharacterScreen {
        &self.ui
    }
}
/// §3.8 big numbers rolling from `from` to `to`: cell 11 in the ones for
/// 123 ms, then the new ones; then the same for the tens if they changed.
fn rolling_cells(from: i32, to: i32, elapsed: u64) -> [u8; 2] {
    let old = editor::big_number_cells(from, from);
    let new = editor::big_number_cells(to, to);
    if elapsed < ROLL_MS {
        [old[0], editor::ROLL_CELL]
    } else if old[0] != new[0] && elapsed < 2 * ROLL_MS {
        [editor::ROLL_CELL, new[1]]
    } else {
        new
    }
}

impl MapPreview {
    fn character(&self) -> Option<&CharacterScreen> {
        match &self.screen {
            Screen::Character(screen) => Some(screen),
            _ => None,
        }
    }
    fn character_mut(&mut self) -> Option<&mut CharacterScreen> {
        match &mut self.screen {
            Screen::Character(screen) => Some(screen),
            _ => None,
        }
    }
    /// §3.42 opening: the snapshot, the reconciliation, and the perk
    /// chooser over the Perks folder when a pick is pending.
    pub(crate) fn open_character(&mut self) {
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let Some(saved) = self.host.world.player_sheet().cloned() else {
            return;
        };
        let pending = self
            .host
            .world
            .player_sheet_mut()
            .is_some_and(|sheet| sheet.reconcile_levels(context));
        self.status.clear();
        let memory = self.character_memory;
        self.screen = Screen::Character(Box::new(CharacterScreen {
            ui: character::CharacterScreen::new(memory.selected, memory.folder, memory.cursor),
            floor: SkillFloor::of(&saved, context),
            tags_before: saved.tags,
            traits_at_open: saved.traits,
            saved: Some(saved),
            modal: None,
            card: None,
            roll: None,
        }));
        if pending {
            let chooser = self.chooser(Step::Perk);
            if let Some(screen) = self.character_mut() {
                screen.ui.folder = Folder::Perks;
                screen.modal = Some(Modal::Chooser(Box::new(chooser)));
            }
        }
        // The first card after opening plays no sound.
        let card = self.card_key();
        if let Some(screen) = self.character_mut() {
            screen.card = card;
        }
    }
    /// Leave the screen: Done keeps the sheet, Cancel restores the
    /// snapshot (and with it the reconciliation and the pending pick).
    /// Every exit clears the pending level-up flag and remembers the info
    /// line, folder and skill cursor.
    pub(crate) fn close_character(&mut self, keep: bool) {
        let Screen::Character(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        self.character_memory =
            character::CharacterScreen::new(screen.selected, screen.folder, screen.cursor);
        if let Some(sheet) = self.host.world.player_sheet_mut() {
            if !keep && let Some(saved) = screen.saved {
                *sheet = saved;
            }
            sheet.flags &= !LEVEL_UP_FLAG;
        }
        let player = self.host.world.player;
        self.host.world.sync_hit_points(player);
        self.return_to_map();
    }
    /// The global campaign variable `index` (0 when absent).
    fn global(&self, index: usize) -> i32 {
        self.host
            .world
            .gvars
            .as_ref()
            .and_then(|globals| globals.get(index))
            .copied()
            .unwrap_or(0)
    }
    /// What the folders hold now.
    pub(super) fn folder_view(&self, folder: Folder) -> FolderView {
        let Some(sheet) = self.host.world.player_sheet() else {
            return FolderView::default();
        };
        FolderView {
            folder,
            perks: character::held_perks(sheet),
            traits: sheet.traits,
            karma: character::karma_rows(|index| self.global(index)),
            kills: character::kill_rows(&self.host.world.kills, |kind| {
                self.message("proto.msg", 450 + kind as i32)
            }),
            fallout2: self.folders2(),
        }
    }
    /// SPEC-019 §3.9 the Fallout 2 folders: the player's karma, the
    /// `genrep.txt` title it selects, the `karmavar.txt` rows whose global
    /// is nonzero, and kill types 0-18.
    fn folders2(&self) -> Option<character::Folders2> {
        let state = self.host.world.fallout2.as_deref()?;
        let karma = self
            .host
            .world
            .player_sheet()
            .map_or(0, |sheet| sheet.karma);
        let reputation =
            vault_formats::fallout2::lines::reputation_title(&state.tables.genrep, karma);
        let variables = state
            .tables
            .karmavar
            .iter()
            .filter(|record| self.global(record.global as usize) != 0 && record.global >= 0)
            .map(|record| (record.art, record.name, record.description))
            .collect();
        let counts: Vec<i32> = self
            .host
            .world
            .kills
            .iter()
            .chain(state.kills.iter())
            .copied()
            .collect();
        let kills = character::kill_rows2(&counts, |kind| self.kill_name(kind));
        Some(character::Folders2 {
            karma,
            reputation,
            variables,
            kills,
        })
    }
    /// §3.9 kill type t's name: `proto.msg` 1450 + t, else the English
    /// fallback.
    pub(crate) fn kill_name(&self, kind: usize) -> Vec<u8> {
        let (name, _) = vault_core::ui::fallout2::kill_messages(kind);
        self.painter().message_or(
            "proto.msg",
            name,
            vault_core::ui::fallout2::KILL_FALLBACKS
                .get(kind)
                .copied()
                .unwrap_or_default(),
        )
    }
    /// The selected line's card.
    fn current_card(&self) -> Option<Card> {
        let screen = self.character()?;
        Some(character::info_card(
            screen.selected,
            Some(&self.folder_view(screen.folder)),
        ))
    }
    /// The card's picture and title text, compared for its sound.
    fn card_key(&self) -> Option<(i32, Vec<u8>)> {
        let card = self.current_card()?;
        let painter = self.painter();
        Some((card.picture, editor_screen::card_title(&painter, &card)))
    }
    /// §3.42: a press selects an info line; a change of picture or title
    /// plays `isdxxxx1`.
    fn select_line(&mut self, line: InfoLine) {
        if let Some(screen) = self.character_mut() {
            screen.ui.select(line);
        }
        self.card_changed();
    }
    fn card_changed(&mut self) {
        let card = self.card_key();
        let Some(screen) = self.character_mut() else {
            return;
        };
        if screen.card != card {
            let sound = screen.card.is_some();
            screen.card = card;
            if sound {
                self.play_effect(CARD_SOUND, None);
            }
        }
    }
    /// One skill step on the cursor skill: the skill's card, then the
    /// refusal box or the rolling number. Returns whether it stepped.
    fn skill_step(&mut self, plus: bool) -> bool {
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let Some(screen) = self.character() else {
            return false;
        };
        let (skill, floor) = (screen.cursor, screen.floor);
        let Some(sheet) = self.host.world.player_sheet_mut() else {
            return false;
        };
        let before = sheet.unspent_skills;
        let result = if plus {
            character::skill_plus(sheet, skill, context)
        } else {
            character::skill_minus(sheet, skill, context, &floor, true)
        };
        let after = sheet.unspent_skills;
        self.select_line(InfoLine::Skill(skill));
        let clock = self.clock;
        match result {
            Ok(()) => {
                if let Some(screen) = self.character_mut() {
                    screen.roll = Some((before, after, clock));
                }
                true
            }
            Err(refusal) => {
                self.refuse(refusal, skill);
                false
            }
        }
    }
    /// A refusal: `iisxxxx1` and the large box at (192, 126), key 32328;
    /// B and C are titled with the skill name and ":".
    fn refuse(&mut self, refusal: SkillRefusal, skill: usize) {
        self.play_effect(ERROR_SOUND.as_bytes(), None);
        let (title, body) = refusal.messages();
        let title = match title {
            Some(id) => self.message("editor.msg", id),
            None => {
                let mut name = self.message("skill.msg", 100 + skill as i32);
                name.push(b':');
                name
            }
        };
        let lines = std::iter::once(title)
            .chain(body.iter().map(|id| self.message("editor.msg", *id)))
            .collect();
        let notice = Notice::new(
            editor::DIALOG_ORIGIN,
            lines,
            ColorKey::ERROR,
            Some(DialogSize::Large),
            false,
        );
        if let Some(screen) = self.character_mut() {
            screen.ui.stop_slider();
            screen.modal = Some(Modal::Refusal(notice));
        }
    }
    /// The chooser list of a step: eligible perks (SPEC-011 §3.21), the
    /// skills not tagged, the held traits, or the traits not held when the
    /// screen opened; each sorted by name in byte order.
    fn chooser(&self, step: Step) -> Chooser {
        let mut entries: Vec<(i32, Vec<u8>)> = match step {
            Step::Perk => self
                .host
                .perk_choices()
                .into_iter()
                .map(|(perk, name)| (perk.index() as i32, name))
                .collect(),
            Step::Tag => {
                let tags = self
                    .host
                    .world
                    .player_sheet()
                    .map_or([-1; 4], |sheet| sheet.tags);
                (0..18)
                    .filter(|skill| !tags.contains(skill))
                    .map(|skill| (skill, self.message("skill.msg", 100 + skill)))
                    .collect()
            }
            Step::LoseTrait => {
                let traits = self
                    .host
                    .world
                    .player_sheet()
                    .map_or([-1; 2], |sheet| sheet.traits);
                traits
                    .into_iter()
                    .filter(|t| *t >= 0)
                    .map(|t| (t, self.message("trait.msg", 100 + t)))
                    .collect()
            }
            Step::NewTrait => {
                let held = self.character().map_or([-1; 2], |s| s.traits_at_open);
                (0..16)
                    .filter(|t| !held.contains(t))
                    .map(|t| (t, self.message("trait.msg", 100 + t)))
                    .collect()
            }
        };
        entries.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        Chooser {
            step,
            window: PerkChooser::new(entries.len(), self.clock),
            entries,
            card: None,
        }
    }
    fn chooser_mut(&mut self) -> Option<&mut Chooser> {
        match &mut self.character_mut()?.modal {
            Some(Modal::Chooser(chooser)) => Some(chooser),
            _ => None,
        }
    }
    /// The chooser's card changes with the selection: `isdxxxx1` except for
    /// the first card of each list.
    fn chooser_moved(&mut self) {
        let painter = self.painter();
        let key = match &self.character().and_then(|s| s.modal.as_ref()) {
            Some(Modal::Chooser(chooser)) => chooser.list.selected().map(|entry| {
                let (picture, title, _) = chooser.step.card(chooser.entries[entry].0);
                (picture, painter.message(title.0, title.1))
            }),
            _ => return,
        };
        let Some(chooser) = self.chooser_mut() else {
            return;
        };
        if chooser.card != key {
            let sound = chooser.card.is_some();
            chooser.card = key;
            if sound {
                self.play_effect(CARD_SOUND, None);
            }
        }
    }
    /// Close the chooser window; the screen's lists, folder and card are
    /// drawn again.
    fn close_chooser(&mut self) {
        if let Some(screen) = self.character_mut() {
            screen.modal = None;
        }
        let player = self.host.world.player;
        self.host.world.sync_hit_points(player);
        self.card_changed();
    }
    /// Switch the window to a Tag! or Mutate! step.
    fn chooser_step(&mut self, step: Step) {
        let mut chooser = self.chooser(step);
        chooser.card = None;
        if let Some(screen) = self.character_mut() {
            screen.modal = Some(Modal::Chooser(Box::new(chooser)));
        }
        self.chooser_moved();
    }
    /// Done, Return or a double click: the selected entry's choice. An
    /// empty list ignores it (`[May-improve]` safe rule), keeping the pick.
    fn chooser_choose(&mut self) {
        let Some(chooser) = self.chooser_mut() else {
            return;
        };
        let Some(entry) = chooser.list.selected() else {
            return;
        };
        let (index, step) = (chooser.entries[entry].0, chooser.step);
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        match step {
            Step::Perk => {
                let Some(perk) = Perk::from_index(index) else {
                    return;
                };
                match self.host.choose_perk(perk) {
                    PerkChoice::Granted | PerkChoice::Refused => self.close_chooser(),
                    PerkChoice::TagSelection => {
                        let tags = self.host.world.player_sheet().map(|sheet| sheet.tags);
                        if let (Some(screen), Some(tags)) = (self.character_mut(), tags) {
                            screen.tags_before = tags;
                        }
                        self.chooser_step(Step::Tag);
                    }
                    PerkChoice::TraitSelection => {
                        let held = self
                            .host
                            .world
                            .player_sheet()
                            .is_some_and(|sheet| sheet.traits.iter().any(|t| *t >= 0));
                        self.chooser_step(if held {
                            Step::LoseTrait
                        } else {
                            Step::NewTrait
                        });
                    }
                }
            }
            // The skill fills the fourth tag slot and gains the tag bonus.
            Step::Tag => {
                if let Some(sheet) = self.host.world.player_sheet_mut() {
                    sheet.tags[3] = index;
                }
                self.close_chooser();
            }
            Step::LoseTrait => {
                if let Some(sheet) = self.host.world.player_sheet_mut() {
                    for slot in &mut sheet.traits {
                        if *slot == index {
                            *slot = -1;
                        }
                    }
                    sheet.recompute(context);
                }
                self.chooser_step(Step::NewTrait);
            }
            // The new trait applies at once, in the first empty slot (the
            // `[May-improve]` safe rule; the reference uses the second slot
            // whenever a trait was held at the start).
            Step::NewTrait => {
                if let Some(sheet) = self.host.world.player_sheet_mut() {
                    if let Some(slot) = sheet.traits.iter_mut().find(|slot| **slot < 0) {
                        *slot = index;
                    }
                    sheet.recompute(context);
                }
                self.close_chooser();
            }
        }
    }
    /// Esc or Cancel: the perk step closes without a choice (the pick stays
    /// pending); Tag! restores the tags and Mutate! the traits on opening,
    /// and both remove their rank, the pick still consumed.
    fn chooser_cancel(&mut self) {
        let Some(step) = self.chooser_mut().map(|chooser| chooser.step) else {
            return;
        };
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let (tags, traits) = self
            .character()
            .map_or(([-1; 4], [-1; 2]), |s| (s.tags_before, s.traits_at_open));
        if let Some(sheet) = self.host.world.player_sheet_mut() {
            match step {
                Step::Perk => {}
                Step::Tag => {
                    sheet.tags = tags;
                    sheet.cancel_perk_choice(Perk::Tag, context);
                }
                Step::LoseTrait | Step::NewTrait => {
                    sheet.traits = traits;
                    sheet.recompute(context);
                    sheet.cancel_perk_choice(Perk::Mutate, context);
                }
            }
        }
        self.close_chooser();
    }
    fn chooser_key(&mut self, key: KeyCode) -> bool {
        let input = match key {
            KeyCode::ArrowUp => ChooserKey::Up,
            KeyCode::ArrowDown => ChooserKey::Down,
            KeyCode::Home => ChooserKey::Home,
            KeyCode::End => ChooserKey::End,
            KeyCode::Enter | KeyCode::NumpadEnter => ChooserKey::Enter,
            KeyCode::Escape => ChooserKey::Escape,
            _ => ChooserKey::Other,
        };
        let Some(chooser) = self.chooser_mut() else {
            return false;
        };
        let outcome = chooser.window.key(input);
        // Return chooses after `ib1p1xx1`.
        self.chooser_outcome(outcome, true);
        true
    }
    /// Act on the core chooser's outcome (SPEC-020 §2.1 rule 4); `sound`
    /// plays `ib1p1xx1` before a choice made without a held Done button.
    fn chooser_outcome(&mut self, outcome: ChooserOutcome, sound: bool) {
        match outcome {
            ChooserOutcome::None => {}
            ChooserOutcome::Moved => self.chooser_moved(),
            ChooserOutcome::Choose => {
                if sound {
                    self.play_effect(BUTTON_PRESS.as_bytes(), None);
                }
                self.chooser_choose();
            }
            ChooserOutcome::Cancel => self.chooser_cancel(),
        }
    }
    /// A press: an arrow steps at once with `ib1p1xx1` and repeats while
    /// held; Done and Cancel show their down art; the list waits for its
    /// release.
    fn chooser_press(&mut self, point: [i32; 2]) -> bool {
        let clock = self.clock;
        let Some(chooser) = self.chooser_mut() else {
            return false;
        };
        let layout = chooser.layout();
        let control = layout.hit(point);
        let outcome = chooser.window.press(point, clock);
        let rect = control.and_then(|control| layout.rect(control));
        match (control, rect) {
            (Some(ChooserControl::Up | ChooserControl::Down), Some(rect)) => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                self.held = Some(rect);
            }
            (Some(ChooserControl::Done | ChooserControl::Cancel), Some(rect)) => {
                self.press_button(rect);
            }
            _ => {}
        }
        self.chooser_outcome(outcome, false);
        true
    }
    /// A release: Done and Cancel act over themselves; a list click
    /// selects its row, and a second click on it chooses with `ib1p1xx1`.
    fn chooser_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Some(chooser) = self.chooser_mut() else {
            return false;
        };
        let done = chooser.pressed() == Some(ChooserControl::Done);
        let outcome = chooser.window.release(point);
        self.chooser_outcome(outcome, !done);
        true
    }
    /// §3.43 PRINT: the save file dialog over `*.TXT` titled PRINT TO FILE
    /// with the default name PRINT.
    fn open_print(&mut self) {
        let names =
            editor_screen::sheet_list(&*self.host.assets.vfs, &self.save_root, SheetFile::Print);
        let title = game_text(&self.host.assets, "editor.msg", 602);
        let default = game_text(&self.host.assets, "editor.msg", 616);
        let dialog = FileDialog::new(DialogKind::Save, title, names, Some(&default), self.clock);
        if let Some(screen) = self.character_mut() {
            screen.modal = Some(Modal::Print(dialog));
        }
    }
    /// A chosen name gets `.TXT`; an existing file asks first (`editor.msg`
    /// 609 / 610, key 32328); No returns to the screen.
    fn print_event(&mut self, event: FileEvent) {
        match event {
            FileEvent::None => {}
            FileEvent::Cancel => {
                if let Some(screen) = self.character_mut() {
                    screen.modal = None;
                }
            }
            FileEvent::Chosen(name) => {
                let file = sheet::with_extension(&name, SheetFile::Print);
                if editor_screen::file_exists(&*self.host.assets.vfs, &self.save_root, &file) {
                    let mut title = file.to_ascii_uppercase();
                    title.push(b' ');
                    title.extend(game_text(&self.host.assets, "editor.msg", 609));
                    let body = game_text(&self.host.assets, "editor.msg", 610);
                    let notice = Notice::new(
                        sheet::MESSAGE_ORIGIN,
                        vec![title, body],
                        ColorKey::ERROR,
                        None,
                        true,
                    );
                    if let Some(screen) = self.character_mut() {
                        screen.modal = Some(Modal::Notice(notice, Some(file)));
                    }
                } else {
                    self.write_print(&file);
                }
            }
        }
    }
    /// The printed sheet of the player as shown; success shows nothing, a
    /// failure "Error saving NAME!" (large, key 32328) after `iisxxxx1`.
    fn write_print(&mut self, file: &[u8]) {
        let bytes = self.printed_sheet();
        let modal = match editor_screen::write_file(&self.save_root, file, &bytes) {
            Ok(()) => None,
            Err(error) => {
                tracing::warn!(%error, "printed sheet not written");
                self.play_effect(ERROR_SOUND.as_bytes(), None);
                let mut title = game_text(&self.host.assets, "editor.msg", 611);
                title.extend(file.to_ascii_uppercase());
                title.push(b'!');
                let notice = Notice::new(
                    sheet::MESSAGE_ORIGIN,
                    vec![title],
                    ColorKey::ERROR,
                    Some(DialogSize::Large),
                    false,
                );
                Some(Modal::Notice(notice, None))
            }
        };
        if let Some(screen) = self.character_mut() {
            screen.modal = modal;
        }
    }
    /// §3.43 the printed sheet's bytes, with its four safe rules.
    pub(crate) fn printed_sheet(&self) -> Vec<u8> {
        let world = &self.host.world;
        let player = world.player;
        let Some(sheet) = world.player_sheet() else {
            return Vec::new();
        };
        let text = |file: &str, id| game_text(&self.host.assets, file, id);
        let inventory = world.get(player).map_or_else(Vec::new, |object| {
            object
                .inventory
                .iter()
                .map(|(item, quantity)| {
                    let name = world.get(*item).map(|o| o.name.clone()).unwrap_or_default();
                    (*quantity, name)
                })
                .collect()
        });
        let kills = (0..character::KILL_TYPES)
            .filter(|kind| world.kills[*kind] != 0)
            .map(|kind| (text("proto.msg", 450 + kind as i32), world.kills[kind]))
            .collect();
        let shown = editor_screen::printed_sheet(
            sheet,
            world.context(player),
            text,
            PrintWorld {
                kills,
                inventory,
                weight: world.inventory_weight(player),
                ticks: world.clock.ticks,
            },
        );
        sheet::print_sheet(&shown, |id| text("editor.msg", id), true)
    }
    /// A notice's answer: Yes on the overwrite question writes the file.
    fn answer_character_notice(&mut self, yes: bool) {
        let Some(screen) = self.character_mut() else {
            return;
        };
        let file = match screen.modal.take() {
            Some(Modal::Notice(_, file)) => file,
            _ => None,
        };
        if let (true, Some(file)) = (yes, file) {
            self.write_print(&file);
        }
    }
    /// Play and clear a modal's queued sounds.
    fn modal_sounds(&mut self) {
        let sounds = match self.character_mut().and_then(|s| s.modal.as_mut()) {
            Some(Modal::Refusal(notice) | Modal::Notice(notice, _)) => {
                std::mem::take(&mut notice.sounds)
            }
            Some(Modal::Print(dialog)) => std::mem::take(&mut dialog.sounds),
            _ => Vec::new(),
        };
        for sound in sounds {
            self.play_effect(sound.as_bytes(), None);
        }
    }
    /// §3.42 keys in game.
    pub(crate) fn character_key(&mut self, key: KeyCode) -> bool {
        let [shift, control, _] = self.hud.modifiers;
        let Some(screen) = self.character_mut() else {
            return false;
        };
        match &mut screen.modal {
            Some(Modal::Chooser(_)) => return self.chooser_key(key),
            Some(Modal::Refusal(notice)) => {
                if notice.key(key).is_some() {
                    self.modal_sounds();
                    if let Some(screen) = self.character_mut() {
                        screen.modal = None;
                    }
                }
                self.modal_sounds();
                return true;
            }
            Some(Modal::Notice(notice, _)) => {
                if let Some(yes) = notice.key(key) {
                    self.modal_sounds();
                    self.answer_character_notice(yes);
                }
                self.modal_sounds();
                return true;
            }
            Some(Modal::Print(dialog)) => {
                let event = dialog.key(key, shift);
                self.modal_sounds();
                self.print_event(event);
                return true;
            }
            None => {}
        }
        let input = match key {
            // Return, `d` and `D` act as Done after `ib1p1xx1`.
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyD => CharacterKey::Done,
            KeyCode::Escape => CharacterKey::Escape,
            KeyCode::KeyQ | KeyCode::KeyX if control => CharacterKey::Quit,
            KeyCode::F10 => CharacterKey::Quit,
            KeyCode::F12 => CharacterKey::F12,
            KeyCode::KeyP => CharacterKey::Print,
            KeyCode::NumpadAdd | KeyCode::ArrowRight => CharacterKey::Plus,
            KeyCode::Equal | KeyCode::KeyN if shift => CharacterKey::Plus,
            KeyCode::Minus | KeyCode::NumpadSubtract | KeyCode::ArrowLeft => CharacterKey::Minus,
            KeyCode::KeyJ if shift => CharacterKey::Minus,
            _ => return false,
        };
        if input == CharacterKey::Done {
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
        }
        let Some(screen) = self.character_mut() else {
            return false;
        };
        let outcome = screen.ui.key(input);
        self.character_outcome(outcome);
        true
    }
    /// Act on the core screen's outcome (SPEC-020 §2.1 rule 4). Returns
    /// whether a skill step happened.
    fn character_outcome(&mut self, outcome: CharacterOutcome) -> bool {
        match outcome {
            CharacterOutcome::None => {}
            CharacterOutcome::Close { keep } => self.close_character(keep),
            CharacterOutcome::Print => self.open_print(),
            // The info card follows the selected line.
            CharacterOutcome::Select(_) => self.card_changed(),
            // The tabs act on release with `ib3p1xx1`: the folder by the
            // pointer's screen x and its general card, redrawn even when
            // already selected.
            CharacterOutcome::Folder(_) => {
                self.play_effect(TAB_SOUND, None);
                self.card_changed();
            }
            CharacterOutcome::SkillStep { plus } => return self.skill_step(plus),
            CharacterOutcome::QuitConfirm => self.confirm_quit(),
            CharacterOutcome::Screenshot => self.screenshot(),
        }
        false
    }
    /// A left press on the screen.
    pub(crate) fn character_click(&mut self, point: [i32; 2]) -> bool {
        let clock = self.clock;
        let Some(screen) = self.character_mut() else {
            return false;
        };
        match &mut screen.modal {
            Some(Modal::Chooser(_)) => return self.chooser_press(point),
            Some(Modal::Refusal(notice) | Modal::Notice(notice, _)) => {
                self.held = notice.press(point);
                self.modal_sounds();
                return true;
            }
            Some(Modal::Print(dialog)) => {
                self.held = dialog.press(point, clock);
                self.modal_sounds();
                return true;
            }
            None => {}
        }
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let Screen::Character(screen) = &mut self.screen else {
            return false;
        };
        let tab_mask =
            |point| editor_screen::masked(&painter, "fldrmask.frm", character::FOLDER_TABS, point);
        let layout = screen.ui.layout();
        let outcome = screen.ui.press(point, tab_mask);
        let pressed = screen.ui.pressed();
        match (pressed, outcome) {
            // The red buttons show their down art until the release.
            (
                Some(
                    control @ (CharacterControl::Done
                    | CharacterControl::Cancel
                    | CharacterControl::Print),
                ),
                _,
            ) => {
                if let Some(rect) = layout.rect(control) {
                    self.press_button(rect);
                }
            }
            // The slider's buttons: `ib1p1xx1` on press (no release
            // sound), one step, and the SPECIAL buttons' repeat while held.
            (_, CharacterOutcome::SkillStep { plus }) => {
                let control = if plus {
                    CharacterControl::SliderPlus
                } else {
                    CharacterControl::SliderMinus
                };
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                self.held = layout.rect(control);
                if self.character_outcome(outcome)
                    && let Some(screen) = self.character_mut()
                {
                    screen.ui.hold_slider(plus, clock);
                }
                return true;
            }
            // A press in the skill rows moves the cursor (and the slider);
            // elsewhere the info hot regions select their line.
            (_, CharacterOutcome::Select(_)) => {
                self.character_outcome(outcome);
            }
            (Some(CharacterControl::Tabs), _) => {}
            _ => return false,
        }
        true
    }
    /// A left release; `over` tells whether it is over the held button.
    pub(crate) fn character_release(&mut self, point: Option<[i32; 2]>, over: bool) -> bool {
        let Some(screen) = self.character_mut() else {
            return false;
        };
        match &mut screen.modal {
            Some(Modal::Chooser(_)) => return self.chooser_release(point),
            Some(Modal::Refusal(notice)) => {
                if notice.release(over).is_some() {
                    self.modal_sounds();
                    if let Some(screen) = self.character_mut() {
                        screen.modal = None;
                    }
                }
                self.modal_sounds();
                return true;
            }
            Some(Modal::Notice(notice, _)) => {
                if let Some(yes) = notice.release(over) {
                    self.modal_sounds();
                    self.answer_character_notice(yes);
                }
                self.modal_sounds();
                return true;
            }
            Some(Modal::Print(dialog)) => {
                let event = dialog.release(point);
                self.modal_sounds();
                self.print_event(event);
                return true;
            }
            None => {}
        }
        let tabs = screen.ui.pressed() == Some(CharacterControl::Tabs);
        let outcome = screen.ui.release(point);
        if outcome == CharacterOutcome::None {
            return tabs;
        }
        self.character_outcome(outcome);
        true
    }
    /// One frame: a held slider or chooser arrow repeats while the pointer
    /// stays on it, the chooser's idle timer runs, the print dialog ticks,
    /// and the unspent number rolls. Returns whether anything changed.
    pub(crate) fn character_tick(&mut self, now: u64) -> bool {
        let pointer = self.pointer.at;
        let Some(screen) = self.character_mut() else {
            return false;
        };
        let mut changed = screen
            .roll
            .is_some_and(|(_, _, start)| now.saturating_sub(start) <= 2 * ROLL_MS);
        if let Some(plus) = screen.ui.tick(now, pointer) {
            changed = true;
            if !self.skill_step(plus)
                && let Some(screen) = self.character_mut()
            {
                screen.ui.stop_slider();
            }
        }
        let Some(screen) = self.character_mut() else {
            return changed;
        };
        match &mut screen.modal {
            Some(Modal::Chooser(chooser)) => {
                if chooser.window.tick(now, pointer) {
                    self.chooser_moved();
                    changed = true;
                }
            }
            Some(Modal::Print(dialog)) => {
                changed |= dialog.tick(now, pointer);
                self.modal_sounds();
            }
            _ => {}
        }
        changed
    }
    pub(crate) fn character_image(&self) -> AppResult<IndexedImage> {
        let Screen::Character(screen) = &self.screen else {
            return Err("character screen is not open".into());
        };
        let mut image = IndexedImage::new(640, 480)?;
        let world = &self.host.world;
        let player = world.player;
        let context = world.context(player);
        let Some(sheet) = world.player_sheet() else {
            return Ok(image);
        };
        let painter = self.painter();
        painter.blit_opaque(&mut image, character::BACKGROUND, 0, [0, 0]);
        // Name, age and sex: the "off" images with their captions, not
        // buttons.
        editor_screen::draw_top_buttons(&painter, &mut image, sheet, context, None);
        let effects = match world.get(player).map(|object| &object.raw.state) {
            Some(ObjectState::Critter(state)) => state.results.0,
            _ => 0,
        };
        let view = SheetView {
            sheet,
            context,
            special: std::array::from_fn(|stat| sheet.stat(stat, context)),
            hit_points: sheet.current_hp,
            conditions: character::condition_lines(sheet.poison, sheet.radiation, effects),
            selected: screen.selected,
            game: true,
        };
        editor_screen::draw_special(&painter, &mut image, &view);
        editor_screen::draw_conditions(&painter, &mut image, &view);
        let skills = painter.message("editor.msg", 117);
        editor_screen::draw_skills(&painter, &mut image, &view, &skills);
        // The slider on the cursor row.
        let c = screen.cursor;
        painter.blit(&mut image, "slider.frm", 0, [592, 16 + 11 * c as i32]);
        let [plus, minus] = slider_buttons(c);
        for (rect, art) in [
            (plus, ("splsoff.frm", "splson.frm")),
            (minus, ("snegoff.frm", "snegon.frm")),
        ] {
            self.draw_button(&painter, &mut image, rect, art);
        }
        // SKILL POINTS and the unspent points (two digits, "99" above 99).
        let caption = painter.message("editor.msg", 112);
        painter.text(&mut image, 103, ColorKey::LABEL, &caption, [400, 233], 120);
        let unspent = sheet.unspent_skills;
        let cells = match screen.roll {
            Some((from, to, start)) if to == unspent => {
                rolling_cells(from, to, self.clock.saturating_sub(start))
            }
            _ => editor::big_number_cells(unspent, unspent),
        };
        for (cell, x) in cells.iter().zip([522, 536]) {
            painter.big_digit(&mut image, *cell, [x, 228], false);
        }
        // The level block.
        let values = character::level_values(sheet.level, sheet.experience);
        for (k, ((y, id), value)) in character::LEVEL_LINES.iter().zip(&values).enumerate() {
            let mut text = painter.message("editor.msg", *id);
            text.push(b' ');
            text.extend(value.bytes());
            let key = line_key(screen.selected == InfoLine::Level(k));
            painter.text(&mut image, 101, key, &text, [32, *y], 124);
        }
        let folders = self.folder_view(screen.folder);
        self.draw_folder(&painter, &mut image, screen, &folders);
        editor_screen::draw_card(
            &painter,
            &mut image,
            &character::info_card(screen.selected, Some(&folders)),
        );
        for (rect, caption, id) in BUTTON_CAPTIONS {
            let message = ("editor.msg", id);
            self.draw_captioned_red_button(&painter, &mut image, rect, message, caption, 80);
        }
        let held = self.held_button();
        match &screen.modal {
            Some(Modal::Chooser(chooser)) => self.draw_chooser(&painter, &mut image, chooser),
            Some(Modal::Refusal(notice) | Modal::Notice(notice, _)) => {
                notice.draw(&painter, &mut image, held)
            }
            Some(Modal::Print(dialog)) => dialog.draw(&painter, &mut image, held, self.clock),
            None => {}
        }
        Ok(image)
    }
    /// The folder tabs with their captions and the selected folder's list.
    fn draw_folder(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        screen: &CharacterScreen,
        view: &FolderView,
    ) {
        let tabs = character::FOLDER_TABS;
        painter.blit_opaque(image, screen.folder.tab_art(), 0, [tabs.x, tabs.y]);
        for folder in Folder::ALL {
            let (id, center) = folder.caption();
            let text = painter.message("editor.msg", id);
            let x = tabs.x + center - painter.width(103, &text) / 2;
            let key = if folder == screen.folder {
                ColorKey::LABEL
            } else {
                ColorKey::LABEL_PRESSED
            };
            painter.text(image, 103, key, &text, [x, tabs.y + 5], 100);
        }
        let lit = |line: usize| line_key(screen.selected == InfoLine::Folder(line));
        match screen.folder {
            Folder::Perks => {
                for (j, (perk, rank)) in view.perks.iter().take(character::PERK_ROWS).enumerate() {
                    let mut text = painter.message("perk.msg", 101 + perk.index() as i32);
                    if *rank > 1 {
                        text.extend(format!(" ({rank})").bytes());
                    }
                    painter.text(image, 101, lit(j), &text, [34, 364 + 11 * j as i32], 250);
                }
                let heading = painter.message("editor.msg", 156);
                let w = painter.width(101, &heading);
                painter.text(
                    image,
                    101,
                    ColorKey::GREEN,
                    &heading,
                    [(280 - w) / 2 + 31, 439],
                    w,
                );
                let span = (246 - w) / 2;
                let color = painter.color(ColorKey::GREEN);
                image.fill_rect([34, 444, span - 34 + 1, 1], color);
                image.fill_rect([(280 - w) / 2 + w + 45, 444, span + 1, 1], color);
                for (slot, t) in view.traits.iter().enumerate() {
                    if *t < 0 {
                        continue;
                    }
                    let y = character::trait_slot_y(slot, view.traits, true);
                    let text = painter.message("trait.msg", 100 + t);
                    painter.text(image, 101, lit(8 + slot), &text, [34, y], 250);
                }
            }
            Folder::Karma if view.fallout2.is_some() => {
                let folders = view.fallout2.as_ref().expect("Fallout 2 folders");
                let mut text = painter.message("editor.msg", 1000);
                text.extend(folders.karma.to_string().bytes());
                painter.text(image, 101, lit(0), &text, [34, 362], 270);
                // §3.9: rows beyond the folder area are not drawn.
                for (j, card) in folders.karma_rows().iter().enumerate().take(9) {
                    let text = editor_screen::card_title(painter, card);
                    painter.text(
                        image,
                        101,
                        lit(j + 1),
                        &text,
                        [34, 373 + 11 * j as i32],
                        270,
                    );
                }
            }
            Folder::Kills if view.fallout2.is_some() => {
                let folders = view.fallout2.as_ref().expect("Fallout 2 folders");
                for (index, (kind, count)) in folders.kills.iter().enumerate().take(20) {
                    let ([x, y], count_x) = character::kill_row_position(index);
                    let name = self.kill_name(*kind);
                    painter.text(image, 101, lit(index), &name, [x, y], count_x - x);
                    let count = count.to_string();
                    painter.text(image, 101, lit(index), count.as_bytes(), [count_x, y], 40);
                }
            }
            Folder::Karma => {
                let mut text = painter.message("editor.msg", 1000);
                text.extend(self.global(character::REPUTATION_VAR).to_string().bytes());
                painter.text(image, 101, lit(0), &text, [34, 362], 270);
                for (j, entry) in view.karma.iter().enumerate() {
                    let text = painter.message("editor.msg", 1000 + *entry as i32);
                    painter.text(
                        image,
                        101,
                        lit(j + 1),
                        &text,
                        [34, 373 + 11 * j as i32],
                        270,
                    );
                }
            }
            Folder::Kills => {
                for (index, (kind, count)) in view.kills.iter().enumerate() {
                    let ([x, y], count_x) = character::kill_row_position(index);
                    let name = painter.message("proto.msg", 450 + *kind as i32);
                    painter.text(image, 101, lit(index), &name, [x, y], count_x - x);
                    let count = count.to_string();
                    painter.text(image, 101, lit(index), count.as_bytes(), [count_x, y], 40);
                }
            }
        }
    }
    /// The perk chooser window at (33, 91): heading, list, arrows, Done and
    /// Cancel, and the selected entry's card.
    fn draw_chooser(&self, painter: &Painter, image: &mut IndexedImage, chooser: &Chooser) {
        let origin = CHOOSER_ORIGIN;
        let point = |x: i32, y: i32| offset(origin, [x, y]);
        painter.blit_opaque(image, "perkwin.frm", 0, origin);
        let heading = painter.message("editor.msg", chooser.step.heading());
        painter.text(image, 103, ColorKey::LABEL, &heading, point(49, 16), 206);
        for (rect, caption, id) in [
            (CHOOSER_DONE, [69, 186], 100),
            (CHOOSER_CANCEL, [171, 186], 102),
        ] {
            let (rect, caption) = (at(origin, rect), offset(origin, caption));
            let message = ("editor.msg", id);
            self.draw_captioned_red_button(painter, image, rect, message, caption, 80);
        }
        for (rect, art) in [
            (CHOOSER_UP, ("uparwoff.frm", "uparwon.frm")),
            (CHOOSER_DOWN, ("dnarwoff.frm", "dnarwon.frm")),
        ] {
            self.draw_button(painter, image, at(origin, rect), art);
        }
        let sheet = self.host.world.player_sheet();
        let rank = |index: i32| {
            (chooser.step == Step::Perk)
                .then(|| Perk::from_index(index))
                .flatten()
                .and_then(|perk| sheet.map(|sheet| sheet.rank(perk)))
                .unwrap_or(0)
        };
        let list = &chooser.list;
        for row in 0..character::PICK_ROWS {
            let Some((index, name)) = chooser.entries.get(list.top + row) else {
                break;
            };
            let key = line_key(row == list.row);
            let y = 43 + 12 * row as i32;
            painter.text(image, 101, key, name, point(45, y), 160);
            if rank(*index) > 0 {
                let text = format!("({})", rank(*index));
                painter.text(image, 101, key, text.as_bytes(), point(207, y), 30);
            }
        }
        let Some(entry) = list.selected() else {
            return;
        };
        let index = chooser.entries[entry].0;
        let (picture, title, description) = chooser.step.card(index);
        let path = editor_screen::skilldex_art(painter, picture);
        let column = path
            .as_deref()
            .map_or(0, |path| editor_screen::dark_column(painter, path));
        if let Some(path) = &path {
            painter.blit(image, path, 0, point(413, 64));
        }
        let title = painter.message(title.0, title.1);
        let width = painter.text(image, 102, ColorKey::BLACK, &title, point(280, 27), 280);
        if rank(index) > 0 {
            let text = format!("({})", rank(index));
            painter.text(
                image,
                101,
                ColorKey::BLACK,
                text.as_bytes(),
                point(288 + width, 41),
                40,
            );
        }
        let [rx, ry] = point(280, 55);
        image.fill_rect([rx, ry, 266, 2], painter.color(ColorKey::BLACK));
        let text = painter.message(description.0, description.1);
        let limit = character::chooser_wrap(column);
        for (k, line) in editor_screen::wrap_words(painter, &text, limit)
            .iter()
            .enumerate()
        {
            painter.text(
                image,
                101,
                ColorKey::BLACK,
                line,
                point(280, 70 + 11 * k as i32),
                limit,
            );
        }
    }
}

/// Folder and level-block text: key 992, or 32747 when selected.
fn line_key(selected: bool) -> ColorKey {
    if selected {
        ColorKey::SELECTED
    } else {
        ColorKey::GREEN
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        state_fixture,
        test_support::{interactive, local_corpus},
    };
    use super::*;
    use vault_core::ui::Rect;

    const EDITOR: &[u8] = b"{132}{}{At maximum level.}\n{133}{}{Unable to increment it.}\n\
        {134}{}{At minimum level.}\n{135}{}{Unable to decrement it.}\n\
        {136}{}{No free skill points available.}\n{602}{}{PRINT TO FILE}\n\
        {609}{}{already exists.}\n{610}{}{Do you wish to overwrite?}\n\
        {611}{}{Error saving }\n{616}{}{PRINT}\n{620}{}{FALLOUT}\n";

    fn preview(dir: &std::path::Path) -> MapPreview {
        state_fixture::write_corpus(dir);
        std::fs::write(dir.join("text/english/game/editor.msg"), EDITOR).unwrap();
        std::fs::write(
            dir.join("text/english/game/skill.msg"),
            b"{100}{}{Small Guns}\n",
        )
        .unwrap();
        let mut preview = interactive(dir);
        let save = dir.join("save");
        std::fs::create_dir_all(&save).unwrap();
        preview.save_root = save;
        preview
    }
    fn screen(preview: &MapPreview) -> &CharacterScreen {
        preview.character().expect("character screen")
    }
    fn refusal(preview: &MapPreview) -> Option<Vec<Vec<u8>>> {
        match &screen(preview).modal {
            Some(Modal::Refusal(notice)) => Some(notice.lines.clone()),
            _ => None,
        }
    }
    fn chooser(preview: &MapPreview) -> &Chooser {
        match &screen(preview).modal {
            Some(Modal::Chooser(chooser)) => chooser,
            _ => panic!("chooser"),
        }
    }
    /// Select `entry` of the chooser with Down presses.
    fn pick(preview: &mut MapPreview, index: i32) {
        let entry = chooser(preview)
            .entries
            .iter()
            .position(|(i, _)| *i == index)
            .expect("entry listed");
        for _ in 0..entry {
            preview.step(KeyCode::ArrowDown);
        }
        assert_eq!(chooser(preview).list.selected(), Some(entry));
        assert!(preview.step(KeyCode::Enter));
    }

    #[test]
    fn fixture_m_skill_spending_on_the_screen() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = preview(dir.path());
        let sheet = preview.host.world.player_sheet_mut().unwrap();
        sheet.unspent_skills = 2;
        let invested = sheet.invested[0];
        assert!(preview.step(KeyCode::KeyC));
        preview.image().unwrap();
        let unspent = |p: &MapPreview| p.host.world.player_sheet().unwrap().unspent_skills;
        assert!(preview.step(KeyCode::NumpadAdd));
        assert_eq!(unspent(&preview), 1);
        assert!(matches!(screen(&preview).roll, Some((2, 1, _))));
        assert!(preview.step(KeyCode::ArrowRight));
        assert_eq!(unspent(&preview), 0);
        assert!(preview.step(KeyCode::NumpadAdd));
        assert_eq!(
            refusal(&preview),
            Some(vec![b"No free skill points available.".to_vec()])
        );
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Escape));
        assert!(screen(&preview).modal.is_none());
        assert!(preview.step(KeyCode::NumpadSubtract));
        assert!(preview.step(KeyCode::ArrowLeft));
        assert_eq!(unspent(&preview), 2);
        assert!(preview.step(KeyCode::Minus));
        assert_eq!(
            refusal(&preview),
            Some(vec![
                b"Small Guns:".to_vec(),
                b"At minimum level.".to_vec(),
                b"Unable to decrement it.".to_vec(),
            ])
        );
        assert!(preview.step(KeyCode::Enter), "the box's Done");
        assert!(preview.step(KeyCode::NumpadAdd));
        // Cancel restores the snapshot.
        assert!(preview.step(KeyCode::Escape));
        let sheet = preview.host.world.player_sheet().unwrap();
        assert_eq!((sheet.invested[0], sheet.unspent_skills), (invested, 2));
    }

    #[test]
    fn fixture_l_hit_tests_tabs_and_remembered_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = preview(dir.path());
        preview.host.world.player_sheet_mut().unwrap().flags |= LEVEL_UP_FLAG;
        assert!(preview.step(KeyCode::KeyC));
        assert!(preview.click([60, 290], false));
        assert_eq!(screen(&preview).selected, InfoLine::Level(1));
        // The tab press acts on release: Karma and its general card.
        assert!(preview.click([200, 340], false));
        assert_eq!(screen(&preview).folder, Folder::Perks);
        assert!(preview.release(Some([200, 340]), false));
        assert_eq!(screen(&preview).folder, Folder::Karma);
        assert_eq!(screen(&preview).selected.number(), 42);
        // The conditions region clamps (safe rule).
        assert!(preview.click([250, 147], false));
        assert_eq!(screen(&preview).selected.number(), 50);
        assert!(preview.click([420, 60], false), "Unarmed row");
        assert_eq!(screen(&preview).cursor, 3);
        preview.image().unwrap();
        // Cancel clears the level-up flag; the folder, line and cursor are
        // remembered.
        assert!(preview.step(KeyCode::Escape));
        let flags = preview.host.world.player_sheet().unwrap().flags;
        assert_eq!(flags & LEVEL_UP_FLAG, 0);
        assert!(preview.step(KeyCode::KeyC));
        let open = screen(&preview);
        assert_eq!(
            (open.folder, open.selected.number(), open.cursor),
            (Folder::Karma, 64, 3)
        );
        // Every folder draws.
        for x in [60, 250] {
            assert!(preview.click([x, 340], false));
            assert!(preview.release(Some([x, 340]), false));
            preview.image().unwrap();
        }
        assert_eq!(screen(&preview).folder, Folder::Kills);
    }

    #[test]
    fn tag_and_mutate_run_in_the_chooser() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = preview(dir.path());
        let sheet = preview.host.world.player_sheet_mut().unwrap();
        sheet.level = 21;
        sheet.reconciled_level = 21;
        sheet.pending_perk = true;
        sheet.traits = [8, -1];
        let tags = sheet.tags;
        assert!(preview.step(KeyCode::KeyC));
        assert_eq!(chooser(&preview).step, Step::Perk);
        preview.image().unwrap();
        pick(&mut preview, Perk::Tag.index() as i32);
        assert_eq!(chooser(&preview).step, Step::Tag);
        let untagged = tags.iter().filter(|tag| **tag < 0).count();
        assert_eq!(chooser(&preview).entries.len(), 18 - (4 - untagged));
        preview.image().unwrap();
        // Cancelling Tag! restores the tags and removes the rank; the pick
        // is still consumed (SPEC-011 §3.21).
        assert!(preview.step(KeyCode::Escape));
        let sheet = preview.host.world.player_sheet().unwrap();
        assert!(screen(&preview).modal.is_none());
        assert_eq!(
            (sheet.tags, sheet.rank(Perk::Tag), sheet.pending_perk),
            (tags, 0, false)
        );
        // Cancelling the screen restores the pick for the next visit.
        assert!(preview.step(KeyCode::Escape));
        assert!(preview.host.world.player_sheet().unwrap().pending_perk);
        assert!(preview.step(KeyCode::KeyC));
        pick(&mut preview, Perk::Mutate.index() as i32);
        assert_eq!(chooser(&preview).step, Step::LoseTrait);
        assert_eq!(chooser(&preview).entries.len(), 1);
        assert!(preview.step(KeyCode::Enter));
        assert_eq!(chooser(&preview).step, Step::NewTrait);
        assert_eq!(chooser(&preview).entries.len(), 15);
        let new = chooser(&preview).entries[0].0;
        assert!(preview.step(KeyCode::Enter));
        let sheet = preview.host.world.player_sheet().unwrap();
        assert_eq!(sheet.traits, [new, -1], "the first empty slot");
        assert_eq!(sheet.rank(Perk::Mutate), 1);
        assert!(preview.step(KeyCode::Enter), "Done");
        assert!(matches!(preview.screen, Screen::Map));
        // A double click chooses in the Tag! step too.
        let sheet = preview.host.world.player_sheet_mut().unwrap();
        sheet.pending_perk = true;
        assert!(preview.step(KeyCode::KeyC));
        pick(&mut preview, Perk::Tag.index() as i32);
        let skill = chooser(&preview).entries[0].0;
        for _ in 0..2 {
            assert!(preview.click([100, 140], false));
            assert!(preview.release(Some([100, 140]), false));
        }
        let sheet = preview.host.world.player_sheet().unwrap();
        assert_eq!((sheet.tags[3], sheet.rank(Perk::Tag)), (skill, 1));
    }

    #[test]
    fn print_writes_the_sheet_and_asks_before_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = preview(dir.path());
        assert!(preview.step(KeyCode::KeyC));
        assert!(preview.step(KeyCode::KeyP));
        assert!(matches!(screen(&preview).modal, Some(Modal::Print(_))));
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Enter));
        assert!(screen(&preview).modal.is_none(), "no message on success");
        let path = preview.save_root.join("PRINT.TXT");
        let text = std::fs::read(&path).unwrap();
        assert!(text.starts_with(b"\r\n\r\n                                  FALLOUT\r\n"));
        assert!(text.ends_with(b"\r\n\r\n\r\n"));
        // An existing file asks first; No keeps it.
        std::fs::write(&path, b"old").unwrap();
        assert!(preview.step(KeyCode::KeyP));
        assert!(preview.step(KeyCode::Enter));
        let Some(Modal::Notice(notice, Some(_))) = &screen(&preview).modal else {
            panic!("overwrite question");
        };
        assert_eq!(notice.lines[0], b"PRINT.TXT already exists.");
        preview.image().unwrap();
        assert!(preview.step(KeyCode::KeyN));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert!(preview.step(KeyCode::KeyP));
        assert!(preview.step(KeyCode::Enter));
        assert!(preview.step(KeyCode::KeyY));
        assert_eq!(std::fs::read(&path).unwrap(), text);
        // A failed write shows the error box.
        preview.save_root = dir.path().join("missing");
        assert!(preview.step(KeyCode::KeyP));
        assert!(preview.step(KeyCode::Enter));
        let Some(Modal::Notice(notice, None)) = &screen(&preview).modal else {
            panic!("error box");
        };
        assert_eq!(notice.lines[0], b"Error saving PRINT.TXT!");
        assert!(preview.step(KeyCode::Escape));
        assert!(screen(&preview).modal.is_none());
    }

    #[test]
    fn the_unspent_number_rolls_digit_by_digit() {
        assert_eq!(rolling_cells(2, 1, 0), [0, editor::ROLL_CELL]);
        assert_eq!(rolling_cells(2, 1, ROLL_MS), [0, 1]);
        assert_eq!(rolling_cells(10, 9, ROLL_MS), [editor::ROLL_CELL, 9]);
        assert_eq!(rolling_cells(10, 9, 2 * ROLL_MS), [0, 9]);
        assert_eq!(slider_buttons(2)[1], Rect::new(614, 53, 17, 12));
    }

    #[test]
    #[ignore = "needs the local corpus"]
    fn corpus_character_screen_art_folders_and_printed_sheet() {
        let Some(vfs) = local_corpus() else {
            return;
        };
        let mut preview = MapPreview::load(vfs, "maps/v13ent.map", "english", None, 1).unwrap();
        let painter = Painter {
            assets: &preview.host.assets,
            palette: &preview.palette,
        };
        for art in [
            character::BACKGROUND,
            "perkwin.frm",
            "fldrmask.frm",
            "slider.frm",
        ] {
            assert!(painter.art(art).is_some(), "{art}");
        }
        assert_eq!(painter.size(character::BACKGROUND), Some([640, 480]));
        // Every card picture but the effect perks' resolves.
        assert!(editor_screen::skilldex_art(&painter, 0).is_some());
        assert!(editor_screen::skilldex_art(&painter, 83).is_some());
        assert!(editor_screen::skilldex_art(&painter, 72 + 53).is_none());
        // proto.msg 450-464: "DeathClaws" sorts before "Dogs".
        preview.host.world.kills[11] = 1;
        preview.host.world.kills[13] = 2;
        assert_eq!(preview.folder_view(Folder::Kills).kills, [(13, 2), (11, 1)]);
        let sheet = preview.host.world.player_sheet_mut().unwrap();
        sheet.level = 3;
        assert!(preview.step(KeyCode::KeyC));
        assert_eq!(chooser(&preview).step, Step::Perk);
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Escape), "the chooser closes");
        for x in [60, 150, 250] {
            assert!(preview.click([x, 340], false));
            assert!(preview.release(Some([x, 340]), false));
            preview.image().unwrap();
        }
        let text = preview.printed_sheet();
        assert!(text.starts_with(b"\r\n\r\n                                  FALLOUT\r\n"));
        let heading = b"\r\n ::: Skills :::                ::: Kills :::\r\n  ";
        assert!(text.windows(heading.len()).any(|w| w == heading));
        assert!(text.ends_with(b" lbs.\r\n\r\n\r\n"));
    }
}
