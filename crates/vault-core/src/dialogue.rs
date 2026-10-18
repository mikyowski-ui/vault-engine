use crate::{RandomSource, WorldError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Good,
    Neutral,
    Bad,
}
impl Mood {
    pub fn fidget(self) -> u8 {
        match self {
            Self::Good => 1,
            Self::Neutral => 4,
            Self::Bad => 7,
        }
    }
    pub fn speech(self) -> u8 {
        match self {
            Self::Good => 9,
            Self::Neutral => 10,
            Self::Bad => 11,
        }
    }
    pub fn category(self) -> i32 {
        match self {
            Self::Bad => 1,
            Self::Neutral => 2,
            Self::Good => 3,
        }
    }
    pub fn from_reaction(reaction: i32) -> Self {
        if reaction <= 25 {
            Self::Bad
        } else if reaction <= 75 {
            Self::Neutral
        } else {
            Self::Good
        }
    }
    pub fn transition(self, reaction: i32) -> Option<(u8, Self)> {
        match (self, reaction) {
            (Self::Good, 49) => Some((0, Self::Good)),
            (Self::Good, 51) => Some((2, Self::Neutral)),
            (Self::Neutral, 49) => Some((3, Self::Good)),
            (Self::Neutral, 51) => Some((5, Self::Bad)),
            (Self::Bad, 49) => Some((6, Self::Neutral)),
            (Self::Bad, 51) => Some((8, Self::Bad)),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadDefinition {
    pub index: u16,
    pub stem: String,
    pub counts: [i32; 3],
}
impl HeadDefinition {
    pub fn art_id(&self, animation: u8, variant: i32) -> Result<u32, WorldError> {
        if animation > 11 || variant < 0 {
            return Err(WorldError::Invalid("invalid head animation/variant".into()));
        }
        Ok(0x08000000
            + (u32::from(animation) << 16)
            + ((variant as u32 % 16) << 12)
            + u32::from(self.index % 4096))
    }
    pub fn path(&self, animation: u8, variant: i32) -> Result<String, WorldError> {
        let suffix = match animation {
            0 => "gv",
            1 => "gf",
            2 => "gn",
            3 => "ng",
            4 => "nf",
            5 => "nb",
            6 => "bn",
            7 => "bf",
            8 => "bv",
            9 => "gp",
            10 => "np",
            11 => "bp",
            _ => return Err(WorldError::Invalid("invalid head animation".into())),
        };
        if variant < 0 {
            return Err(WorldError::Invalid("negative fidget variant".into()));
        }
        Ok(format!(
            "art/heads/{}{}{}.frm",
            self.stem,
            suffix,
            if matches!(animation, 1 | 4 | 7) {
                (variant % 16).to_string()
            } else {
                String::new()
            }
        ))
    }
    pub fn count(&self, mood: Mood) -> i32 {
        self.counts[match mood {
            Mood::Good => 0,
            Mood::Neutral => 1,
            Mood::Bad => 2,
        }]
    }
}
pub const PHONEMES: [u8; 42] = [
    0, 3, 1, 1, 3, 1, 1, 1, 7, 8, 7, 3, 1, 8, 1, 7, 7, 6, 6, 2, 2, 2, 2, 4, 4, 5, 5, 2, 2, 2, 2, 2,
    6, 2, 2, 5, 8, 2, 2, 2, 2, 8,
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadPhase {
    Fidget,
    Waiting,
    Transition,
    Speech,
}
#[derive(Debug, Clone)]
pub struct TalkingHead {
    pub definition: HeadDefinition,
    pub mood: Mood,
    pub phase: HeadPhase,
    pub animation: u8,
    pub variant: i32,
    pub frame: usize,
    pub idle_seconds: i32,
    pub last_fidget_ms: u64,
    frame_ms: u64,
    idle_ms: u64,
    wait_ms: u64,
    pending: Option<(u8, Mood)>,
    destination: Option<Mood>,
}
pub fn choose_fidget(
    count: i32,
    idle: &mut i32,
    rng: &mut impl RandomSource,
) -> Result<i32, WorldError> {
    if count <= 0 {
        return Err(WorldError::Invalid("invalid fidget count".into()));
    }
    let draw = rng.range(1, 100)?.saturating_add(*idle / 2);
    Ok(match count {
        1 => 1,
        2 => {
            if draw < 68 {
                1
            } else {
                2
            }
        }
        3 => {
            *idle = 0;
            if draw < 52 {
                1
            } else if draw <= 76 {
                2
            } else {
                3
            }
        }
        _ => count,
    })
}
impl TalkingHead {
    pub fn new(
        definition: HeadDefinition,
        mood: Mood,
        now: u64,
        rng: &mut impl RandomSource,
    ) -> Result<Self, WorldError> {
        let mut idle = 0;
        let variant = choose_fidget(definition.count(mood), &mut idle, rng)?;
        Ok(Self {
            definition,
            mood,
            phase: HeadPhase::Fidget,
            animation: mood.fidget(),
            variant,
            frame: 0,
            idle_seconds: idle,
            last_fidget_ms: now,
            frame_ms: now,
            idle_ms: now,
            wait_ms: 10000,
            pending: None,
            destination: None,
        })
    }
    pub fn art_id(&self) -> Result<u32, WorldError> {
        self.definition.art_id(
            self.animation,
            if matches!(self.animation, 1 | 4 | 7) {
                self.variant
            } else {
                0
            },
        )
    }
    pub fn path(&self) -> Result<String, WorldError> {
        self.definition.path(self.animation, self.variant)
    }
    pub fn phoneme(&mut self, index: usize) -> Result<(), WorldError> {
        self.frame = usize::from(
            *PHONEMES
                .get(index)
                .ok_or_else(|| WorldError::Invalid("phoneme index must be 0..41".into()))?,
        );
        self.animation = self.mood.speech();
        self.phase = HeadPhase::Speech;
        Ok(())
    }
    pub fn speech_end(&mut self) {
        self.frame = 0;
        self.phase = HeadPhase::Waiting;
        self.idle_seconds = 3;
    }
    pub fn react(&mut self, reaction: i32) {
        if self.phase == HeadPhase::Speech {
            self.speech_end();
        }
        self.idle_seconds = 0;
        self.pending = self.mood.transition(reaction);
    }
    pub fn tick(
        &mut self,
        now: u64,
        frames: usize,
        fps: i16,
        rng: &mut impl RandomSource,
    ) -> Result<bool, WorldError> {
        if frames == 0 || fps < 0 {
            return Err(WorldError::Invalid(
                "invalid talking-head frame metadata".into(),
            ));
        }
        if self.phase == HeadPhase::Speech {
            return Ok(false);
        }
        if self.phase == HeadPhase::Waiting {
            let elapsed = now.saturating_sub(self.idle_ms) / 1000;
            self.idle_seconds = self
                .idle_seconds
                .saturating_add(elapsed.min(i32::MAX as u64) as i32);
            self.idle_ms = self.idle_ms.saturating_add(elapsed * 1000);
            if let Some((animation, mood)) = self.pending.take() {
                self.animation = animation;
                self.destination = Some(mood);
                self.phase = HeadPhase::Transition;
                self.frame = 0;
                self.frame_ms = now;
                return Ok(true);
            }
            if now.saturating_sub(self.last_fidget_ms) < self.wait_ms {
                return Ok(false);
            }
            self.variant = choose_fidget(
                self.definition.count(self.mood),
                &mut self.idle_seconds,
                rng,
            )?;
            self.animation = self.mood.fidget();
            self.phase = HeadPhase::Fidget;
            self.frame = 0;
            self.last_fidget_ms = now;
            self.frame_ms = now;
            self.wait_ms = rng.range(4, 7)? as u64 * 1000;
            return Ok(true);
        }
        let interval = 1000 / u64::from(if fps == 0 { 10 } else { fps as u16 });
        if now.saturating_sub(self.frame_ms) < interval {
            return Ok(false);
        }
        self.frame_ms = now;
        self.frame += 1;
        if self.frame >= frames {
            self.frame = 0;
            if self.phase == HeadPhase::Transition {
                self.mood = self.destination.take().unwrap_or(self.mood);
                self.variant = choose_fidget(
                    self.definition.count(self.mood),
                    &mut self.idle_seconds,
                    rng,
                )?;
                self.animation = self.mood.fidget();
                self.phase = HeadPhase::Fidget;
                self.last_fidget_ms = now;
            } else {
                self.phase = HeadPhase::Waiting;
                self.last_fidget_ms = now;
                self.idle_ms = now;
            }
        }
        Ok(true)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogueText {
    Literal(Vec<u8>),
    Message { list: i32, message: i32 },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueOption {
    pub text: DialogueText,
    pub procedure: u32,
    pub reaction: i32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewEntry {
    Reply(DialogueText),
    Choice(DialogueText),
}
#[derive(Debug, Clone)]
pub struct GDialog {
    pub open: bool,
    pub target: u32,
    pub program: u32,
    pub background: i32,
    pub head: Option<TalkingHead>,
    pub reply: Option<DialogueText>,
    pub options: Vec<DialogueOption>,
    pub review: Vec<ReviewEntry>,
    pub review_cursor: usize,
    pub depth: usize,
    pub waiting: bool,
    pub pending_barter: bool,
    pub barter_modifier: i32,
    pub resolved_reply: Vec<u8>,
    pub resolved_options: Vec<Vec<u8>>,
    pub page: usize,
    pub page_offsets: Vec<usize>,
    pub page_ms: u64,
    pub speech: Option<Vec<u8>>,
    pub reaction: i32,
}
impl Default for GDialog {
    fn default() -> Self {
        Self {
            open: false,
            target: 0,
            program: 0,
            background: 2,
            head: None,
            reply: None,
            options: Vec::new(),
            review: Vec::new(),
            review_cursor: 0,
            depth: 0,
            waiting: false,
            pending_barter: false,
            barter_modifier: 0,
            resolved_reply: Vec::new(),
            resolved_options: Vec::new(),
            page: 0,
            page_offsets: vec![0],
            page_ms: 0,
            speech: None,
            reaction: 0,
        }
    }
}
impl GDialog {
    pub fn start(
        &mut self,
        target: u32,
        program: u32,
        background: i32,
        head: Option<TalkingHead>,
    ) -> Result<(), WorldError> {
        if target == 0 || !(-1..4096).contains(&background) {
            return Err(WorldError::Invalid(
                "invalid dialogue target/background".into(),
            ));
        }
        if background >= 0 {
            self.background = background;
        }
        self.open = true;
        self.target = target;
        self.program = program;
        self.head = head;
        self.pending_barter = false;
        // SPEC-011 §3.17: the script barter modifier resets per dialogue.
        self.barter_modifier = 0;
        Ok(())
    }
    pub fn background_art(&self) -> u32 {
        0x09000000 | (self.background as u32 & 0xfff)
    }
    pub fn reset_options(&mut self) {
        self.options.clear();
        self.resolved_options.clear();
        self.review_cursor = 0;
    }
    pub fn set_reply(&mut self, program: u32, text: DialogueText) -> Result<(), WorldError> {
        if matches!(&text,DialogueText::Literal(text) if text.len()>899) {
            return Err(WorldError::Invalid("reply exceeds 899 bytes".into()));
        }
        if self
            .review
            .iter()
            .filter(|entry| matches!(entry, ReviewEntry::Reply(_)))
            .count()
            == 80
        {
            let end = self
                .review
                .iter()
                .skip(1)
                .position(|entry| matches!(entry, ReviewEntry::Reply(_)))
                .map_or(self.review.len(), |index| index + 1);
            self.review.drain(..end);
        }
        self.review.push(ReviewEntry::Reply(text.clone()));
        self.reply = Some(text);
        self.program = program;
        self.page = 0;
        self.page_offsets = vec![0];
        self.options.clear();
        self.resolved_options.clear();
        Ok(())
    }
    pub fn add_option(&mut self, option: DialogueOption) -> Result<(), WorldError> {
        if self.options.len() == 30 {
            return Err(WorldError::Invalid("dialogue option limit is 30".into()));
        }
        if matches!(&option.text,DialogueText::Literal(text) if text.len()>897) {
            return Err(WorldError::Invalid(
                "option exceeds 897 content bytes".into(),
            ));
        }
        self.options.push(option);
        Ok(())
    }
    pub fn iq_option(
        &mut self,
        option: DialogueOption,
        threshold: i32,
        intelligence: i32,
        smooth: u32,
    ) -> Result<bool, WorldError> {
        if !iq_allows(intelligence, smooth, threshold) {
            return Ok(false);
        }
        self.add_option(option)?;
        Ok(true)
    }
    pub fn begin(&mut self, now: u64) -> Result<bool, WorldError> {
        if matches!(self.reply, Some(DialogueText::Message { list: -1, .. })) {
            return Ok(false);
        }
        if !self.open || self.reply.is_none() {
            return Err(WorldError::Invalid("dialogue has no session/reply".into()));
        }
        if self.options.is_empty() {
            self.add_option(DialogueOption {
                text: DialogueText::Message {
                    list: -1,
                    message: -1,
                },
                procedure: 0,
                reaction: 50,
            })?;
        }
        if self.depth >= 16 {
            return Err(WorldError::Invalid("nested dialogue depth limit".into()));
        }
        self.depth += 1;
        self.waiting = true;
        self.page_ms = now;
        Ok(true)
    }
    pub fn choose(&mut self, index: Option<usize>) -> Result<Option<(u32, u32)>, WorldError> {
        if !self.waiting {
            return Err(WorldError::Invalid(
                "dialogue is not waiting for a choice".into(),
            ));
        }
        let choice = match index {
            Some(index) => self
                .options
                .get(index)
                .cloned()
                .ok_or_else(|| WorldError::Invalid("invalid dialogue option".into()))?,
            None => DialogueOption {
                text: DialogueText::Literal(Vec::new()),
                procedure: 0,
                reaction: 50,
            },
        };
        self.review.push(ReviewEntry::Choice(choice.text));
        self.speech = None;
        if let Some(head) = &mut self.head {
            head.react(choice.reaction);
        }
        self.options.clear();
        self.resolved_options.clear();
        self.waiting = false;
        if choice.procedure != 0 && self.depth < 2 {
            Ok(Some((self.program, choice.procedure)))
        } else {
            self.depth = self.depth.saturating_sub(1);
            Ok(None)
        }
    }
    pub fn after_callback(&mut self) -> bool {
        if self.options.is_empty() {
            self.depth = self.depth.saturating_sub(1);
            self.waiting = false;
            false
        } else {
            self.waiting = true;
            true
        }
    }
    pub fn end(&mut self) -> bool {
        if self.pending_barter {
            return false;
        }
        let background = self.background;
        *self = Self::default();
        self.background = background;
        true
    }
}
pub fn iq_allows(intelligence: i32, smooth: u32, threshold: i32) -> bool {
    let value = i64::from(intelligence) + i64::from(smooth);
    if threshold >= 0 {
        value >= i64::from(threshold)
    } else {
        value <= -i64::from(threshold)
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Reputation {
    pub value: i32,
    pub child_killer: i32,
    pub bad_monsters: i32,
    pub good_monsters: i32,
    pub champion: i32,
    pub berserker: i32,
}
pub fn npc_reaction(
    locals: &mut [i32],
    charisma: i32,
    presence: u32,
    cult: bool,
    reputation: Reputation,
) -> i32 {
    if locals.len() < 4 {
        return -1;
    }
    if locals[2] != 0 {
        return locals[0];
    }
    let rep = i64::from(reputation.value);
    let mut value = 25 + 5 * i64::from(charisma) + 10 * i64::from(presence);
    value += if cult {
        rep.abs()
    } else if locals[3] == 1 {
        -rep
    } else {
        rep
    };
    if reputation.child_killer > 2 {
        value -= 30;
    }
    if i64::from(reputation.bad_monsters) > 3 * i64::from(reputation.good_monsters)
        || reputation.champion == 1
    {
        value += 20;
    }
    if i64::from(reputation.good_monsters) > 2 * i64::from(reputation.bad_monsters)
        || reputation.berserker == 1
    {
        value -= 20;
    }
    let value = value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    locals[0] = value;
    locals[1] = Mood::from_reaction(value).category();
    locals[2] = 1;
    value
}
