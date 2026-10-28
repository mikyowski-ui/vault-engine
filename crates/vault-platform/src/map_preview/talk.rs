//! SPEC-014 §3.45 the dialogue control panel's controls over a
//! conversation: the Barter, Tell Me About and Review buttons on
//! `di_talk.frm` (keys `b` and `a` in the reply/choice loop), the Review
//! window over the review history, and the Ask About window with its input
//! field. SPEC-006 §3.9 owns the history and the Ask About lookup
//! (`vault_core::host_talk`), SPEC-013 §3.18 barter.
//!
//! Review stops talking-head speech and suspends the head updater until it
//! closes; Up and Down (or its arrow buttons) move the top entry, Esc or
//! Done closes it. Tell Me About tests the target, stops speech (safe
//! rule), hides the reply and option windows and opens Ask About: Enter or
//! DONE submits the typed text, whose answer shows in the reply area with
//! its speech; Esc or CANCEL stops speech and closes it, restoring the
//! saved reply and its page (safe rule).
use super::MapPreview;
use crate::{input::key_text, interface::Painter};
use vault_core::{
    assets::WorldAssets,
    host_talk::{ReviewText, TellMeAbout},
    ui::{
        self, BUTTON_PRESS, BUTTON_RELEASE, Rect,
        talk::{
            self, AskAboutWindow, AskKey, AskOutcome, PanelControl, ReviewKey, ReviewOutcome,
            ReviewWindow,
        },
    },
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// A window over the conversation.
pub(crate) enum TalkWindow {
    Review(Review),
    AskAbout(Box<AskAbout>),
    /// SPEC-019 §3.15 the combat control panel (and its custom panel).
    Control(super::control::ControlPanel),
}
/// The Review window: the core window (SPEC-020 §2.1: the top entry and
/// the held button) and the head updater's state before the window
/// suspended it.
pub(crate) struct Review {
    window: ReviewWindow,
    updater: bool,
}
impl std::ops::Deref for Review {
    type Target = ReviewWindow;
    fn deref(&self) -> &ReviewWindow {
        &self.window
    }
}
/// The Ask About window: the core window (its field, whether it is still
/// opening, whether an answer shows, and the held button) and the reply
/// text, page and remembered pages when it opened.
pub(crate) struct AskAbout {
    window: AskAboutWindow,
    saved: (Vec<u8>, usize, Vec<usize>),
}
impl std::ops::Deref for AskAbout {
    type Target = AskAboutWindow;
    fn deref(&self) -> &AskAboutWindow {
        &self.window
    }
}

/// A sound pair as the effect player takes it.
fn cues((press, release): (&'static str, &'static str)) -> (&'static [u8], &'static [u8]) {
    (press.as_bytes(), release.as_bytes())
}

impl MapPreview {
    /// Which of the reply and option windows the Ask About window hides:
    /// the reply until an answer shows, the options while it is open.
    pub(crate) fn talk_hides(&self) -> (bool, bool) {
        match &self.talk {
            Some(TalkWindow::AskAbout(ask)) => ask.hides(),
            _ => (false, false),
        }
    }
    /// The panel controls act only in the reply/choice loop.
    fn panel_ready(&self) -> bool {
        let dialogue = &self.host.world.dialogue;
        dialogue.open && dialogue.waiting && self.talk.is_none()
    }
    /// `a` asks about, `b` barters (SPEC-013 §3.18); no key opens Review.
    /// SPEC-019 §3.8: under Fallout 2 `a` does nothing and `c` opens the
    /// control panel for a party member with a control record.
    pub(crate) fn panel_key(&mut self, key: KeyCode) -> bool {
        if !self.panel_ready() {
            return false;
        }
        let letter = match key {
            KeyCode::KeyA => 'a',
            KeyCode::KeyB => 'b',
            KeyCode::KeyC => 'c',
            _ => return false,
        };
        let panel = self.panel();
        match talk::panel_key(letter, panel.fallout2, panel.control_member) {
            Some(control) => self.panel_action(control),
            None => return false,
        }
        true
    }
    /// The panel's controls: Barter, then Tell Me About (Fallout 1) or
    /// Combat Control (a Fallout 2 party member's), then Review.
    fn panel(&self) -> talk::Panel {
        talk::Panel::new(
            self.host.world.fallout2.is_some(),
            self.control_member().is_some(),
        )
    }
    /// A panel control acts.
    fn panel_action(&mut self, control: PanelControl) {
        match control {
            PanelControl::Barter => self.barter_button(),
            PanelControl::TellMeAbout => self.tell_me_about(),
            PanelControl::CombatControl => self.open_control(),
            PanelControl::Review => self.open_review(),
        }
    }
    /// A left press on a panel button, or in a window (modal: every press
    /// there is taken). The held control shows its down art and plays its
    /// press cue; its release cue follows on the release.
    pub(crate) fn talk_click(&mut self, point: [i32; 2]) -> bool {
        let panel = (self.talk.is_none() && self.panel_ready()).then(|| self.panel());
        let held = match &mut self.talk {
            Some(TalkWindow::Review(review)) => {
                let control = review.window.press(point);
                control.and_then(|control| {
                    let rect = review.layout().rect(control)?;
                    Some((rect, control.sounds()))
                })
            }
            Some(TalkWindow::AskAbout(ask)) => {
                let control = ask.window.press(point);
                control.and_then(|control| {
                    let rect = ask.layout().rect(control)?;
                    Some((rect, (BUTTON_PRESS, BUTTON_RELEASE)))
                })
            }
            Some(TalkWindow::Control(_)) => return self.control_click(point),
            None if let Some(panel) = panel => {
                let layout = panel.layout();
                let control = layout.hit(point);
                control.and_then(|control| Some((layout.rect(control)?, control.sounds())))
            }
            None => return false,
        };
        if let Some((rect, sounds)) = held {
            let (press, release) = cues(sounds);
            self.press_button_cue(press, release, rect);
            return true;
        }
        self.talk.is_some()
    }
    /// A left release: a window's held button acts when the release is
    /// over it; a panel button acts when the release is over the held
    /// button (`over`).
    pub(crate) fn talk_release(&mut self, point: Option<[i32; 2]>, over: Option<Rect>) -> bool {
        if !self.host.world.dialogue.open {
            return false;
        }
        let entries = self.host.world.dialogue.review.len();
        match &mut self.talk {
            Some(TalkWindow::Review(review)) => {
                if review.window.release(point, entries) == ReviewOutcome::Close {
                    self.close_review();
                }
            }
            Some(TalkWindow::AskAbout(ask)) => {
                let outcome = ask.window.release(point);
                self.ask_outcome(outcome);
            }
            Some(TalkWindow::Control(_)) => return self.control_release(point, over),
            None => {
                let Some(over) = over else {
                    return false;
                };
                let control = self
                    .panel()
                    .layout()
                    .controls()
                    .iter()
                    .find(|(_, rect)| *rect == over)
                    .map(|(control, _)| *control);
                match control {
                    Some(control) => self.panel_action(control),
                    None => return false,
                }
            }
        }
        true
    }
    /// Keys in a window: Review reads Esc, Up and Down and ignores every
    /// other key (Ctrl+Q and F10 included); Ask About handles no quit key.
    pub(crate) fn talk_key(&mut self, key: KeyCode) -> bool {
        let entries = self.host.world.dialogue.review.len();
        let shift = self.hud.modifiers[0];
        match &mut self.talk {
            Some(TalkWindow::Review(review)) => {
                let input = match key {
                    KeyCode::Escape => ReviewKey::Escape,
                    KeyCode::ArrowUp => ReviewKey::Up,
                    KeyCode::ArrowDown => ReviewKey::Down,
                    _ => ReviewKey::Other,
                };
                if review.window.key(input, entries) == ReviewOutcome::Close {
                    self.close_review();
                }
            }
            Some(TalkWindow::AskAbout(ask)) => {
                let input = match key {
                    KeyCode::Backspace => AskKey::Backspace,
                    KeyCode::Enter | KeyCode::NumpadEnter => AskKey::Enter,
                    KeyCode::Escape => AskKey::Escape,
                    _ => match key_text(key, shift) {
                        Some(code) => AskKey::Code(i32::from(code)),
                        None => return true,
                    },
                };
                let font = self.host.assets.font_number(talk::FIELD_FONT);
                let measure = |text: &[u8]| {
                    font.as_ref()
                        .map_or(0, |font| font.text_width(text, false) as i32)
                };
                let glyph = |code: u8| {
                    font.as_ref()
                        .and_then(|font| font.character_width(code))
                        .map_or(0, |width| width as i32)
                };
                let outcome = ask.window.key(input, measure, glyph);
                self.ask_outcome(outcome);
            }
            Some(TalkWindow::Control(_)) => return self.control_key(key),
            None => return false,
        }
        true
    }
    /// Act on the Ask About window's outcome (SPEC-020 §2.1 rule 4).
    fn ask_outcome(&mut self, outcome: AskOutcome) {
        match outcome {
            AskOutcome::None | AskOutcome::Changed => {}
            AskOutcome::Submit(text) => self.submit_ask(&text),
            AskOutcome::Cancel => self.cancel_ask(),
        }
    }
    /// The Barter button or `b`: the SPEC-013 §3.18 merchant test, keeping
    /// the script modifier; a refusal shows `proto.msg` 903.
    fn barter_button(&mut self) {
        self.host.barter_entry();
        self.drain_effects();
    }
    /// The Tell Me About button or `a` (SPEC-014 §3.45 tests).
    fn tell_me_about(&mut self) {
        if self.host.tell_me_about() == TellMeAbout::Open {
            let dialogue = &self.host.world.dialogue;
            self.talk = Some(TalkWindow::AskAbout(Box::new(AskAbout {
                window: AskAboutWindow::new(self.clock),
                saved: (
                    dialogue.resolved_reply.clone(),
                    dialogue.page,
                    dialogue.page_offsets.clone(),
                ),
            })));
        }
        self.drain_effects();
    }
    /// Enter or DONE: the field's text goes to the lookup (the field has
    /// cleared); the window stays open.
    fn submit_ask(&mut self, text: &[u8]) {
        let Some(TalkWindow::AskAbout(ask)) = &mut self.talk else {
            return;
        };
        match self.host.ask_about(text) {
            Ok(true) => ask.window.answer_shown(),
            Ok(false) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
        self.drain_effects();
    }
    /// Esc or CANCEL: speech stops, the window closes, and the saved reply
    /// shows again with the option window. The reference redraws it from
    /// the shared page offset, which an answer may have moved; safe rule:
    /// the original page offset is restored.
    fn cancel_ask(&mut self) {
        let Some(TalkWindow::AskAbout(ask)) = self.talk.take() else {
            return;
        };
        self.host.stop_speech();
        let (reply, page, offsets) = ask.saved;
        let dialogue = &mut self.host.world.dialogue;
        dialogue.resolved_reply = reply;
        dialogue.page = page;
        dialogue.page_offsets = offsets;
        dialogue.page_ms = self.host.world.elapsed_ms;
        self.drain_effects();
    }
    /// Review opens: playing speech stops, the head updater is suspended
    /// (the head keeps a frozen frame), and the oldest entry is at the top.
    fn open_review(&mut self) {
        self.host.stop_speech();
        let updater = std::mem::replace(&mut self.host.world.dialogue.updater, false);
        self.talk = Some(TalkWindow::Review(Review {
            window: ReviewWindow::new(),
            updater,
        }));
        self.drain_effects();
    }
    /// Review closes: the head updater resumes and the reply/choice loop
    /// continues; stopped speech is not restarted.
    fn close_review(&mut self) {
        if let Some(TalkWindow::Review(review)) = self.talk.take() {
            self.host.world.dialogue.updater = review.updater;
        }
    }
    /// An input poll: the Ask About cursor blinks. A window left over a
    /// closed conversation goes away. Returns whether the frame changed.
    pub(crate) fn talk_tick(&mut self, now: u64) -> bool {
        if self.talk.is_some() && !self.host.world.dialogue.open {
            self.talk = None;
            return true;
        }
        match &mut self.talk {
            Some(TalkWindow::AskAbout(ask)) => ask.window.tick(now),
            _ => false,
        }
    }
    /// The panel's buttons over the composed conversation, then any window.
    pub(crate) fn draw_talk(&mut self, image: &mut IndexedImage) {
        let review = match &self.talk {
            Some(TalkWindow::Review(review)) => Some(review.top),
            _ => None,
        };
        let entries = review.map(|_| self.host.review_texts());
        let name = |object| {
            self.host
                .world
                .get(object)
                .map(|found| found.name.clone())
                .unwrap_or_default()
        };
        let names = (
            name(self.host.world.dialogue.target),
            name(self.host.world.player),
        );
        let painter = self.painter();
        let shown = self.panel().layout();
        for (rect, (normal, pressed), opaque) in [
            (talk::BARTER, talk::SMALL_ART, false),
            (talk::TELL_ME_ABOUT, talk::SMALL_ART, false),
            (talk::REVIEW, talk::REVIEW_ART, true),
        ] {
            let rect = talk::panel_button(rect);
            // SPEC-019 §3.8: no Tell Me About under Fallout 2; the Combat
            // Control button takes its place for a party member.
            if !shown.controls().iter().any(|(_, button)| *button == rect) {
                continue;
            }
            if opaque {
                self.draw_button_opaque(&painter, image, rect, (normal, pressed));
            } else {
                self.draw_button(&painter, image, rect, (normal, pressed));
            }
        }
        match (&self.talk, review, entries) {
            (Some(TalkWindow::Review(_)), Some(top), Some(entries)) => {
                self.draw_review(&painter, image, &entries, top, &names);
            }
            (Some(TalkWindow::AskAbout(ask)), _, _) if !ask.opening => {
                self.draw_ask(&painter, image, ask)
            }
            (Some(TalkWindow::Control(panel)), _, _) => self.draw_control(&painter, image, panel),
            _ => {}
        }
    }
    /// The Review window: `review.frm` over the whole screen, the
    /// transcript from the top entry, and the three buttons. The whole
    /// window is redrawn from its art each frame, which covers the §3.45
    /// `[May-improve]` band restore of (88, 76) through (434, 433).
    fn draw_review(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        entries: &[ReviewText],
        top: usize,
        (target, player): &(Vec<u8>, Vec<u8>),
    ) {
        painter.blit_opaque(image, talk::REVIEW_BACKGROUND, 0, [0, 0]);
        let font = painter.font(talk::REVIEW_FONT);
        let height = font.as_ref().map_or(10, |font| font.height() as i32);
        let measure = |text: &[u8]| painter.width(talk::REVIEW_FONT, text);
        for line in talk::review_lines(entries, top, target, player, height, measure) {
            painter.text(
                image,
                talk::REVIEW_FONT,
                line.key,
                &line.text,
                line.origin,
                line.limit,
            );
        }
        let layout = talk::review_layout();
        for (control, (_, art)) in [
            (talk::ReviewControl::ScrollUp, talk::SCROLL_UP),
            (talk::ReviewControl::ScrollDown, talk::SCROLL_DOWN),
            (talk::ReviewControl::Done, talk::DONE),
        ] {
            if let Some(rect) = layout.rect(control) {
                self.draw_button(painter, image, rect, art);
            }
        }
    }
    /// The Ask About window: `lsgbox.frm`, its title and button labels, the
    /// red buttons, and the field with the typed text and cursor.
    fn draw_ask(&self, painter: &Painter, image: &mut IndexedImage, ask: &AskAbout) {
        let [ox, oy] = talk::ASK_ORIGIN;
        let [width, _] = talk::ASK_SIZE;
        painter.blit_opaque(image, talk::ASK_BACKGROUND, 0, talk::ASK_ORIGIN);
        let title = painter.message(talk::ASK_TITLE.0, talk::ASK_TITLE.1);
        let x = ui::centered(width, painter.width(talk::ASK_FONT, &title));
        painter.text(
            image,
            talk::ASK_FONT,
            talk::ASK_KEY,
            &title,
            [ox + x, oy + talk::ASK_TITLE_Y],
            width,
        );
        for (id, [x, y]) in [talk::ASK_DONE_LABEL, talk::ASK_CANCEL_LABEL] {
            let label = painter.message("dbox.msg", id);
            painter.text(
                image,
                talk::ASK_FONT,
                talk::ASK_KEY,
                &label,
                [ox + x, oy + y],
                width - x,
            );
        }
        for (_, rect) in ask.layout().controls() {
            self.draw_red_button(painter, image, *rect);
        }
        let field = talk::FIELD;
        image.fill_rect(
            [ox + field.x, oy + field.y, field.w, field.h],
            painter.color(talk::FIELD_FILL),
        );
        painter.text(
            image,
            talk::FIELD_FONT,
            talk::FIELD_KEY,
            &ask.field.shown(),
            [ox + field.x, oy + field.y],
            field.w,
        );
    }
}

#[cfg(test)]
#[path = "talk_tests.rs"]
mod tests;
