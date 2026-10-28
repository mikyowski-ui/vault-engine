//! SPEC-013 §3.15 the Skilldex window over the world: its layout, the
//! row's closing wait and the choice it leads to (SPEC-020 §2.1). The
//! eight skill rows and Cancel act on their press. A row's press holds the
//! window open [`skills::SKILLDEX_CLOSE_MS`], discarding input meanwhile;
//! then Sneak toggles, another row enters that skill's targeting mode, or,
//! opened from the action menu's Skill entry (SPEC-014 §3.28), uses the
//! skill on that target.
use super::{Layout, Rect};
use crate::{ObjectRef, skills};

/// A row button's size (`skldxoff.frm` / `skldxon.frm`).
pub const ROW_SIZE: [i32; 2] = [86, 33];
/// The row buttons' up and down art.
pub const ROW_ART: (&str, &str) = ("skldxoff.frm", "skldxon.frm");
/// A row press's sounds.
pub const ROW_SOUNDS: (&str, &str) = ("ib3p1xx1", "ib3lu1x1");
/// Cancel's red button, window-relative.
pub const CANCEL: Rect = Rect::new(48, 338, 15, 16);

/// A control of the Skilldex window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkilldexControl {
    /// Row 0-7, in [`skills::SKILLDEX_SKILLS`] order (`ROW_SOUNDS`).
    Row(usize),
    /// The red Cancel button (`BUTTON_PRESS` / `BUTTON_RELEASE`).
    Cancel,
}

/// Row `row`'s screen rectangle.
pub fn row_rect(row: usize) -> Rect {
    Rect::at(skills::skilldex_button(row), ROW_SIZE).offset(skills::SKILLDEX_ORIGIN)
}
/// The window's controls on screen: the eight rows, then Cancel.
pub fn layout() -> Layout<SkilldexControl> {
    let mut layout: Layout<SkilldexControl> = (0..skills::SKILLDEX_SKILLS.len())
        .map(|row| (SkilldexControl::Row(row), row_rect(row)))
        .collect();
    layout.push(
        SkilldexControl::Cancel,
        CANCEL.offset(skills::SKILLDEX_ORIGIN),
    );
    layout
}

/// What a Skilldex input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkilldexOutcome {
    /// Not accepted (a key): the caller passes it on.
    None,
    /// Swallowed: nothing under the press, or a row's wait is under way.
    Busy,
    /// Row `row` (skill `skill`) was pressed: its sounds and down art; the
    /// window closes [`skills::SKILLDEX_CLOSE_MS`] later
    /// ([`SkilldexWindow::due`]).
    Chosen { row: usize, skill: i32 },
    /// Cancel, Esc or Enter: the window closes and nothing is chosen. The
    /// Cancel button plays its red-button pair; Enter plays
    /// `BUTTON_PRESS` alone.
    Cancel,
}
/// A key in the Skilldex window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkilldexKey {
    Escape,
    Enter,
    Other,
}
/// What the closing window does with the chosen skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkilldexAction {
    /// Toggle Sneak, or enter the skill's targeting mode.
    Choose(i32),
    /// Use the skill on the action menu's target at once.
    UseOn { target: ObjectRef, skill: i32 },
}

/// §3.15 the open window: the action menu's target and the chosen row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkilldexWindow {
    /// The action menu's target (`None`: opened from the bar or `s`).
    pub target: Option<ObjectRef>,
    /// The chosen skill and the clock at its press.
    chosen: Option<(i32, u64)>,
}
impl SkilldexWindow {
    pub fn new(target: Option<ObjectRef>) -> Self {
        Self {
            target,
            chosen: None,
        }
    }
    pub fn layout(&self) -> Layout<SkilldexControl> {
        layout()
    }
    /// The chosen skill, while the window waits to close.
    pub fn chosen(&self) -> Option<i32> {
        self.chosen.map(|(skill, _)| skill)
    }
    /// A press at `now`: a row is chosen, Cancel closes; anything else is
    /// swallowed, as is every press during a row's wait.
    pub fn press(&mut self, point: [i32; 2], now: u64) -> SkilldexOutcome {
        if self.chosen.is_some() {
            return SkilldexOutcome::Busy;
        }
        match self.layout().hit(point) {
            Some(SkilldexControl::Row(row)) => {
                let Some(skill) = skills::SKILLDEX_SKILLS.get(row).copied() else {
                    return SkilldexOutcome::Busy;
                };
                self.chosen = Some((skill, now));
                SkilldexOutcome::Chosen { row, skill }
            }
            Some(SkilldexControl::Cancel) => SkilldexOutcome::Cancel,
            None => SkilldexOutcome::Busy,
        }
    }
    /// Esc cancels; Enter also cancels after `BUTTON_PRESS`. Every key is
    /// swallowed during a row's wait.
    pub fn key(&self, key: SkilldexKey) -> SkilldexOutcome {
        if self.chosen.is_some() {
            return SkilldexOutcome::Busy;
        }
        match key {
            SkilldexKey::Escape | SkilldexKey::Enter => SkilldexOutcome::Cancel,
            SkilldexKey::Other => SkilldexOutcome::None,
        }
    }
    /// The chosen row's action once its wait has passed at `now`: Sneak
    /// always toggles; another skill is used on the target when there is
    /// one, else enters targeting.
    pub fn due(&self, now: u64) -> Option<SkilldexAction> {
        let (skill, at) = self.chosen?;
        if now < at + skills::SKILLDEX_CLOSE_MS {
            return None;
        }
        Some(match self.target {
            Some(target) if skill != skills::skill::SNEAK => {
                SkilldexAction::UseOn { target, skill }
            }
            _ => SkilldexAction::Choose(skill),
        })
    }
}
