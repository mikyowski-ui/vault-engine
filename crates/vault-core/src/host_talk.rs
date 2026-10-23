//! SPEC-006 §3.9 dialogue speech requests (the one speech-enabled message
//! lookup shared by replies, `message_str` and Ask About answers), the
//! review history's texts, and the Ask About lookup; SPEC-014 §3.45 the
//! Tell Me About tests that come before the Ask About window.
use crate::{
    RandomSource, WorldError,
    assets::WorldAssets,
    dialogue::{DialogueText, REVIEW_CAPACITY},
    host::WorldHost,
};
use vault_formats::{msg::Message, pro::Category};

/// The reply area copies at most 899 bytes (SPEC-006 §3.9).
const REPLY_LIMIT: usize = 899;
/// SPEC-014 §3.45: on map 35 (`V13ENT`) no critter has anything to tell.
pub const SILENT_MAP: i32 = 35;
/// `proto.msg` 904 "This person has nothing to tell you about."
pub const NOTHING_TO_TELL: i32 = 904;
/// `proto.msg` 650 "[Done]", the Review text of the Done option.
pub const REVIEW_DONE: i32 = 650;
/// Ask About keywords are messages 1000-1099; a keyword k answers with
/// message k + 100. List 1's keywords for map M start at `600 M + 1000`.
const KEYWORDS: i32 = 1000;
const ANSWER_OFFSET: i32 = 100;
const MAP_KEYWORD_STRIDE: i32 = 600;
/// The target's own-name answers, then the fallback answers.
const NAME_ANSWERS: (i32, i32) = (970, 979);
const FALLBACK_ANSWERS: (i32, i32) = (980, 999);
/// The placeholder a missing message reads as.
const ERROR: &[u8] = b"Error";

/// What pressing Tell Me About (or `a`) did (SPEC-014 §3.45).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TellMeAbout {
    /// The target is not a critter: nothing happens.
    Nothing,
    /// `proto.msg` 904 is in the reply area: a bad reaction (safe rule)
    /// or map 35.
    Refused,
    /// The Ask About window opens; speech was stopped (safe rule).
    Open,
}

/// One review entry's texts as the Review window draws them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewText {
    pub reply: Vec<u8>,
    pub choice: Option<Vec<u8>>,
}

/// SPEC-006 §3.9 Ask About tokens: the submitted text cut at space, tab,
/// `.` and `,`, empty pieces dropped.
pub fn ask_tokens(text: &[u8]) -> Vec<&[u8]> {
    text.split(|byte| matches!(byte, b' ' | b'\t' | b'.' | b','))
        .filter(|token| !token.is_empty())
        .collect()
}

/// Whether a message text counts as an answer: anything but the `Error`
/// placeholder, compared without case.
fn answer(text: &[u8]) -> bool {
    !text.eq_ignore_ascii_case(ERROR)
}

impl<A: WorldAssets> WorldHost<A> {
    /// SPEC-006 §3.9: a lookup may request speech during a talk session
    /// whose dialogue head is talking-head art. The reference tests the
    /// head left by the most recent `start_gdialog` of any session, so a
    /// `message_str` in `talk_p_proc` before this session's `start_gdialog`
    /// can play a stale head's speech; `[May-improve]` safe rule: no
    /// request until `start_gdialog` has set up a talking head in the
    /// current session (an open dialogue with a head).
    pub fn speech_enabled(&self) -> bool {
        let dialogue = &self.world.dialogue;
        dialogue.talking && dialogue.open && dialogue.head.is_some()
    }
    /// SPEC-006 §3.9 the speech-enabled message lookup that every dialogue
    /// speech starts through: reply text at display time, `message_str`,
    /// and Ask About answers. While speech is enabled, a message with a
    /// sound field runs the SPEC-010 §3.12 loading and start sequence at
    /// once (stopping any earlier speech), so its lip-sync is the same
    /// whichever opcode supplied the text. A missing message (`None`, read
    /// as `Error`) or an empty sound field requests nothing, and speech
    /// already playing continues. The text is returned unchanged.
    pub(crate) fn spoken_message(&mut self, list: i32, id: i32) -> Option<Message> {
        let message = self.script_message(list, id)?;
        if !message.sound.is_empty() && self.speech_enabled() {
            self.request_speech(&message.sound);
        }
        Some(message)
    }
    /// `gsay_reply` / `gsay_message`: install the reply and record it in
    /// the review history. An 81st reply is not recorded and only logged.
    pub(crate) fn reply(&mut self, program: u32, text: DialogueText) -> Result<(), WorldError> {
        if !self.world.dialogue.set_reply(program, text)? {
            self.world.diagnostic(format!(
                "dialogue review history is full ({REVIEW_CAPACITY} entries); reply not recorded"
            ));
        }
        Ok(())
    }
    /// SPEC-014 §3.45 a review text, looked up again at every redraw: a
    /// message-backed reply or option by its list and number, without
    /// speech (`Error` when it does not exist; when its list cannot be
    /// loaded the reference stops the program, safe rule: `Error`); a
    /// literal as stored; the continuation option empty; the Done option
    /// `proto.msg` 650.
    pub fn review_text(&mut self, text: &DialogueText) -> Vec<u8> {
        match text {
            DialogueText::Literal(text) => text.clone(),
            DialogueText::Message {
                list: -1,
                message: -1,
            } => Vec::new(),
            DialogueText::Message {
                list: -2,
                message: -2,
            } => self
                .game_text("proto.msg", REVIEW_DONE)
                .unwrap_or_else(|| ERROR.to_vec()),
            DialogueText::Message { list, message } => self
                .script_message(*list, *message)
                .map_or_else(|| ERROR.to_vec(), |found| found.text),
        }
    }
    /// The review history's texts, oldest entry first.
    pub fn review_texts(&mut self) -> Vec<ReviewText> {
        self.world
            .dialogue
            .review
            .clone()
            .iter()
            .map(|entry| ReviewText {
                reply: self.review_text(&entry.reply),
                choice: entry.choice.as_ref().map(|choice| self.review_text(choice)),
            })
            .collect()
    }
    /// SPEC-014 §3.45 the Tell Me About tests, in order: a target that is
    /// not a critter does nothing; a numeric reaction of at most 25
    /// (initialized on first use) looks up `proto.msg` 904 but shows
    /// nothing in the reference, `[May-improve]` safe rule: show it as for
    /// map 35; on map 35 it is shown in the reply area. Otherwise the Ask
    /// About window opens. The reference means to stop speech first but
    /// lets it play on; `[May-improve]` safe rule: stop it, as Review and
    /// Barter do.
    pub fn tell_me_about(&mut self) -> TellMeAbout {
        let target = self.world.dialogue.target;
        if self
            .world
            .get(target)
            .is_none_or(|object| object.category() != Category::Critter)
        {
            return TellMeAbout::Nothing;
        }
        if self.reaction(target) <= 25 || self.world.map.header.map_number == SILENT_MAP {
            let text = self
                .game_text("proto.msg", NOTHING_TO_TELL)
                .unwrap_or_else(|| ERROR.to_vec());
            self.show_reply(text);
            return TellMeAbout::Refused;
        }
        self.stop_speech();
        TellMeAbout::Open
    }
    /// Put `text` in the reply area: at most 899 bytes, drawn from its
    /// start (SPEC-014 §3.45 safe rule: an Ask About reply does not reuse
    /// the shared page offset).
    fn show_reply(&mut self, mut text: Vec<u8>) {
        text.truncate(REPLY_LIMIT);
        let dialogue = &mut self.world.dialogue;
        dialogue.resolved_reply = text;
        dialogue.page = 0;
        dialogue.page_offsets = vec![0];
        dialogue.page_ms = self.world.elapsed_ms;
    }
    /// A message as Ask About reads it: `Error` when missing.
    fn ask_text(&mut self, list: i32, id: i32) -> Vec<u8> {
        self.script_message(list, id)
            .map_or_else(|| ERROR.to_vec(), |found| found.text)
    }
    /// Show Ask About answer `id` of `list` through the speech-enabled
    /// lookup, exactly as a message-backed reply: `Error` and no speech
    /// when it does not exist.
    fn show_answer(&mut self, list: i32, id: i32) {
        let text = self
            .spoken_message(list, id)
            .map_or_else(|| ERROR.to_vec(), |found| found.text);
        self.show_reply(text);
    }
    /// The word lookup: messages 1000-1099 of the target's list, then list
    /// 1's block for the current map, compared without ASCII case; the
    /// first match `(list, k)`.
    fn ask_keyword(&mut self, list: Option<i32>, token: &[u8]) -> Option<(i32, i32)> {
        let map = self.world.map.header.map_number;
        let block = map
            .saturating_mul(MAP_KEYWORD_STRIDE)
            .saturating_add(KEYWORDS);
        let searches = list
            .map(|list| (list, KEYWORDS))
            .into_iter()
            .chain([(1, block)]);
        for (list, first) in searches {
            for id in first..first.saturating_add(ANSWER_OFFSET) {
                if self.ask_text(list, id).eq_ignore_ascii_case(token) {
                    return Some((list, id));
                }
            }
        }
        None
    }
    /// The non-`Error` answers in `range` of the target's list, else of
    /// list 1, in ascending id order.
    fn ask_answers(&mut self, list: i32, (first, last): (i32, i32)) -> Option<(i32, Vec<i32>)> {
        [list, 1].into_iter().find_map(|list| {
            let ids = (first..=last)
                .filter(|id| answer(&self.ask_text(list, *id)))
                .collect::<Vec<_>>();
            (!ids.is_empty()).then_some((list, ids))
        })
    }
    /// Show one of the answers in `range` with a single draw in 1 through
    /// their count (SPEC-006 §3.4); `false` when there are none.
    fn ask_draw(&mut self, list: i32, range: (i32, i32)) -> Result<bool, WorldError> {
        let Some((list, ids)) = self.ask_answers(list, range) else {
            return Ok(false);
        };
        let pick = self.world.rng.range(1, ids.len() as i32)?;
        self.show_answer(list, ids[(pick - 1) as usize]);
        Ok(true)
    }
    /// SPEC-006 §3.9 the Ask About lookup for submitted `text`: no script
    /// procedure runs. Each token tries the word lookup, then the name
    /// lookup (a critter target's own display name, without case: one of
    /// its answers 970-979); the first matching token wins. With none, one
    /// of the fallback answers 980-999 is shown. A target with no script
    /// gets no name lookup and no fallback, and its word lookup searches
    /// list 1 only. Tokenizing makes keywords holding a space, tab, `.` or
    /// `,` unreachable; `[May-improve]` safe rule: the whole text, trimmed,
    /// tries the word lookup first. Returns whether an answer was shown.
    pub fn ask_about(&mut self, text: &[u8]) -> Result<bool, WorldError> {
        let target = self.world.dialogue.target;
        let list = self
            .world
            .script_for(target)
            .and_then(|sid| self.world.scripts.get(&sid))
            .map(|script| script.file.saturating_add(1));
        let (name, critter) = self
            .world
            .get(target)
            .map_or((Vec::new(), false), |object| {
                (object.name.clone(), object.category() == Category::Critter)
            });
        let whole = text.trim_ascii();
        if !whole.is_empty()
            && let Some((list, id)) = self.ask_keyword(list, whole)
        {
            self.show_answer(list, id + ANSWER_OFFSET);
            return Ok(true);
        }
        for token in ask_tokens(text) {
            if let Some((list, id)) = self.ask_keyword(list, token) {
                self.show_answer(list, id + ANSWER_OFFSET);
                return Ok(true);
            }
            if let Some(list) = list
                && critter
                && token.eq_ignore_ascii_case(&name)
                && self.ask_draw(list, NAME_ANSWERS)?
            {
                return Ok(true);
            }
        }
        match list {
            Some(list) => self.ask_draw(list, FALLBACK_ANSWERS),
            None => Ok(false),
        }
    }
}
