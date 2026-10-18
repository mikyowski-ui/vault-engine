//! SPEC-012 §2.9/§3.10 `.SVE` subtitles: `<frame number>:<text>` lines kept
//! in file order, and their frame-counter timing.
use crate::ai::decimal_prefix;
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitle {
    pub frame: i32,
    pub text: Vec<u8>,
}

/// Parse subtitle lines. Each line is cut at its first LF, then at its first
/// CR. The part before the first `:` is a leading integer (sign allowed, no
/// digits gives 0) and the rest, colons included, is the text; a line
/// without a colon is ignored. Safe rule: whole lines are read (the
/// reference splits lines longer than 258 bytes across reads).
pub fn parse(bytes: &[u8]) -> Vec<Subtitle> {
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .filter_map(|line| {
            let line = &line[..line.iter().position(|b| *b == b'\n').unwrap_or(line.len())];
            let line = &line[..line.iter().position(|b| *b == b'\r').unwrap_or(line.len())];
            let colon = line.iter().position(|b| *b == b':')?;
            Some(Subtitle {
                frame: decimal_prefix(&line[..colon]).0,
                text: line[colon + 1..].to_vec(),
            })
        })
        .collect()
}

/// §3.10 step 4 timing over the entries in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleTrack {
    pending: VecDeque<Subtitle>,
    shown: Option<Vec<u8>>,
}
impl SubtitleTrack {
    pub fn new(entries: Vec<Subtitle>) -> Self {
        Self {
            pending: entries.into(),
            shown: None,
        }
    }
    /// After a step that did not finish the movie, with frame counter
    /// `frame`: while the first remaining entry's number is at most the
    /// counter, draw and remove it. Returns the entries drawn, in order; the
    /// last one stays visible (empty text blanks the band).
    pub fn update(&mut self, frame: u32) -> Vec<Subtitle> {
        let mut drawn = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|entry| i64::from(entry.frame) <= i64::from(frame))
        {
            let entry = self.pending.pop_front().expect("checked front");
            self.shown = Some(entry.text.clone());
            drawn.push(entry);
        }
        drawn
    }
    /// The subtitle currently visible.
    pub fn shown(&self) -> Option<&[u8]> {
        self.shown.as_deref()
    }
    pub fn remaining(&self) -> usize {
        self.pending.len()
    }
}
