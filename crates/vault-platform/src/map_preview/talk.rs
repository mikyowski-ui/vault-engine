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
    ui::{self, BUTTON_PRESS, BUTTON_RELEASE, RED_BUTTON, Rect, talk},
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
pub(crate) struct Review {
    /// The entry at the top of the transcript.
    pub top: usize,
    /// The head updater's state before the window suspended it.
    updater: bool,
}
pub(crate) struct AskAbout {
    pub field: talk::AskField,
    /// Tell Me About was pressed and the reply and option windows are
    /// hidden; the window itself opens on the next head-updater tick, from
    /// the reply/choice loop's next input pass (SPEC-014 §3.45).
    pub opening: bool,
    /// An answer is shown, so the reply window is visible again.
    pub answered: bool,
    /// The reply text, page and remembered pages when the window opened.
    saved: (Vec<u8>, usize, Vec<usize>),
}

/// A button's press and release sounds.
type Cues = (&'static [u8], &'static [u8]);

/// The Ask About red buttons' screen rectangles: DONE, CANCEL.
fn ask_buttons() -> [Rect; 2] {
    let [ox, oy] = talk::ASK_ORIGIN;
    let [w, h] = RED_BUTTON.2;
    [talk::ASK_DONE, talk::ASK_CANCEL].map(|[x, y]| Rect::new(ox + x, oy + y, w, h))
}

impl MapPreview {
    /// Which of the reply and option windows the Ask About window hides:
    /// the reply until an answer shows, the options while it is open.
    pub(crate) fn talk_hides(&self) -> (bool, bool) {
        match &self.talk {
            Some(TalkWindow::AskAbout(ask)) => (ask.opening || !ask.answered, true),
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
        let fallout2 = self.host.world.fallout2.is_some();
        match key {
            KeyCode::KeyA if !fallout2 => self.tell_me_about(),
            KeyCode::KeyC if self.control_member().is_some() => self.open_control(),
            KeyCode::KeyB => self.barter_button(),
            _ => return false,
        }
        true
    }
    /// The panel's buttons and their cues: Barter, then Tell Me About
    /// (Fallout 1) or Combat Control (a Fallout 2 party member's), then
    /// Review.
    fn panel_buttons(&self) -> Vec<(Rect, Cues)> {
        let small = (talk::SMALL_PRESS.as_bytes(), talk::SMALL_RELEASE.as_bytes());
        let review = (BUTTON_PRESS.as_bytes(), BUTTON_RELEASE.as_bytes());
        let mut buttons = vec![(talk::panel_button(talk::BARTER), small)];
        if self.host.world.fallout2.is_none() {
            buttons.push((talk::panel_button(talk::TELL_ME_ABOUT), small));
        } else if self.control_member().is_some() {
            let rect = vault_core::ui::fallout2::COMBAT_CONTROL_BUTTON;
            buttons.push((talk::panel_button(rect), small));
        }
        buttons.push((talk::panel_button(talk::REVIEW), review));
        buttons
    }
    /// A left press on a panel button, or in a window (modal: every press
    /// there is taken).
    pub(crate) fn talk_click(&mut self, point: [i32; 2]) -> bool {
        let (small, review) = (
            (talk::SMALL_PRESS.as_bytes(), talk::SMALL_RELEASE.as_bytes()),
            (BUTTON_PRESS.as_bytes(), BUTTON_RELEASE.as_bytes()),
        );
        let buttons: Vec<(Rect, Cues)> = match &self.talk {
            Some(TalkWindow::Review(_)) => vec![
                (talk::SCROLL_UP.0, small),
                (talk::SCROLL_DOWN.0, small),
                (talk::DONE.0, review),
            ],
            Some(TalkWindow::AskAbout(ask)) if ask.opening => return true,
            Some(TalkWindow::AskAbout(_)) => ask_buttons()
                .into_iter()
                .map(|rect| (rect, review))
                .collect(),
            Some(TalkWindow::Control(_)) => return self.control_click(point),
            None if self.panel_ready() => self.panel_buttons(),
            None => return false,
        };
        if let Some((rect, (press, release))) =
            buttons.into_iter().find(|(rect, _)| rect.contains(point))
        {
            self.press_button_cue(press, release, rect);
            return true;
        }
        self.talk.is_some()
    }
    /// A left release over the held button (`over`) acts on it.
    pub(crate) fn talk_release(&mut self, over: Option<Rect>) -> bool {
        if !self.host.world.dialogue.open {
            return false;
        }
        let Some(over) = over else {
            return self.talk.is_some();
        };
        match &self.talk {
            Some(TalkWindow::Review(_)) => {
                if over == talk::SCROLL_UP.0 {
                    self.scroll_review(false);
                } else if over == talk::SCROLL_DOWN.0 {
                    self.scroll_review(true);
                } else if over == talk::DONE.0 {
                    self.close_review();
                }
            }
            Some(TalkWindow::AskAbout(ask)) if ask.opening => {}
            Some(TalkWindow::AskAbout(_)) => {
                let [done, cancel] = ask_buttons();
                if over == done {
                    self.submit_ask();
                } else if over == cancel {
                    self.cancel_ask();
                }
            }
            Some(TalkWindow::Control(_)) => return self.control_release(Some(over)),
            None if !self.panel_buttons().iter().any(|(rect, _)| *rect == over) => return false,
            None if over == talk::panel_button(talk::BARTER) => self.barter_button(),
            // Fallout 2's Combat Control sits where Tell Me About was.
            None if over == talk::panel_button(talk::TELL_ME_ABOUT)
                && self.host.world.fallout2.is_some() =>
            {
                self.open_control();
            }
            None if over == talk::panel_button(talk::TELL_ME_ABOUT) => self.tell_me_about(),
            None if over == talk::panel_button(talk::REVIEW) => self.open_review(),
            None => return false,
        }
        true
    }
    /// Keys in a window: Review reads Esc, Up and Down and ignores every
    /// other key (Ctrl+Q and F10 included); Ask About handles no quit key.
    pub(crate) fn talk_key(&mut self, key: KeyCode) -> bool {
        match &mut self.talk {
            Some(TalkWindow::Review(_)) => match key {
                KeyCode::Escape => self.close_review(),
                KeyCode::ArrowUp => self.scroll_review(false),
                KeyCode::ArrowDown => self.scroll_review(true),
                _ => {}
            },
            Some(TalkWindow::AskAbout(ask)) if ask.opening => {}
            Some(TalkWindow::AskAbout(ask)) => match key {
                KeyCode::Backspace => ask.field.backspace(),
                KeyCode::Enter | KeyCode::NumpadEnter => self.submit_ask(),
                KeyCode::Escape => self.cancel_ask(),
                _ => {
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
                    if let Some(code) = key_text(key, self.hud.modifiers[0]) {
                        ask.field.type_code(i32::from(code), measure, glyph);
                    }
                }
            },
            Some(TalkWindow::Control(_)) => return self.control_key(key),
            None => return false,
        }
        true
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
                field: talk::AskField::new(self.clock),
                opening: true,
                answered: false,
                saved: (
                    dialogue.resolved_reply.clone(),
                    dialogue.page,
                    dialogue.page_offsets.clone(),
                ),
            })));
        }
        self.drain_effects();
    }
    /// Enter or DONE: the field's text goes to the lookup and the field
    /// clears; the window stays open.
    fn submit_ask(&mut self) {
        let Some(TalkWindow::AskAbout(ask)) = &mut self.talk else {
            return;
        };
        let Some(text) = ask.field.submit() else {
            return;
        };
        match self.host.ask_about(&text) {
            Ok(true) => ask.answered = true,
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
        self.talk = Some(TalkWindow::Review(Review { top: 0, updater }));
        self.drain_effects();
    }
    /// Review closes: the head updater resumes and the reply/choice loop
    /// continues; stopped speech is not restarted.
    fn close_review(&mut self) {
        if let Some(TalkWindow::Review(review)) = self.talk.take() {
            self.host.world.dialogue.updater = review.updater;
        }
    }
    /// Up or Down by one whole entry.
    fn scroll_review(&mut self, down: bool) {
        let entries = self.host.world.dialogue.review.len();
        if let Some(TalkWindow::Review(review)) = &mut self.talk
            && let Some(top) = talk::scroll(review.top, entries, down)
        {
            review.top = top;
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
            Some(TalkWindow::AskAbout(ask)) if ask.opening => {
                ask.opening = false;
                true
            }
            Some(TalkWindow::AskAbout(ask)) => ask.field.blink(now),
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
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let shown = self.panel_buttons();
        for (rect, (normal, pressed), opaque) in [
            (talk::BARTER, talk::SMALL_ART, false),
            (talk::TELL_ME_ABOUT, talk::SMALL_ART, false),
            (talk::REVIEW, talk::REVIEW_ART, true),
        ] {
            let rect = talk::panel_button(rect);
            // SPEC-019 §3.8: no Tell Me About under Fallout 2; the Combat
            // Control button takes its place for a party member.
            if !shown.iter().any(|(button, _)| *button == rect) {
                continue;
            }
            let art = self.button_art(rect, normal, pressed);
            if opaque {
                painter.blit_opaque(image, art, 0, [rect.x, rect.y]);
            } else {
                painter.blit(image, art, 0, [rect.x, rect.y]);
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
        for (rect, (normal, pressed)) in [talk::SCROLL_UP, talk::SCROLL_DOWN, talk::DONE] {
            let art = self.button_art(rect, normal, pressed);
            painter.blit(image, art, 0, [rect.x, rect.y]);
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
        for rect in ask_buttons() {
            let art = self.button_art(rect, RED_BUTTON.0, RED_BUTTON.1);
            painter.blit(image, art, 0, [rect.x, rect.y]);
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
