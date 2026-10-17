//! SPEC-001: packed sequential blocks, with adjacent shared direction offsets.
use crate::{ParseError, binary::Reader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrmFrame {
    pub width: u16,
    pub height: u16,
    pub displacement: [i16; 2],
    pub file_offset: usize,
    pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl FrmFrame {
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Inclusive world rectangle; displacement is already accumulated by the caller.
    pub fn world_rect(&self, anchor: [i32; 2], shift: [i16; 2], displacement: [i32; 2]) -> Rect {
        let x = i64::from(anchor[0]) + i64::from(shift[0]) + i64::from(displacement[0]);
        let bottom = i64::from(anchor[1]) + i64::from(shift[1]) + i64::from(displacement[1]);
        let left = x - i64::from(self.width / 2);
        Rect {
            left,
            top: bottom - i64::from(self.height) + 1,
            right: left + i64::from(self.width) - 1,
            bottom,
        }
    }

    pub fn cursor_hotspot(&self, shift: [i16; 2]) -> [i32; 2] {
        [
            i32::from(self.width / 2) - i32::from(shift[0]),
            i32::from(self.height) - 1 - i32::from(shift[1]),
        ]
    }

    /// Unscaled UI placement ignores world shifts and displacement.
    pub fn centered_origin(&self, origin: [i32; 2], size: [u32; 2]) -> [i64; 2] {
        [
            i64::from(origin[0]) + (i64::from(size[0]) - i64::from(self.width)) / 2,
            i64::from(origin[1]) + (i64::from(size[1]) - i64::from(self.height)) / 2,
        ]
    }
}

/// An object's animation position: selecting a frame record does not itself
/// accumulate displacement; entering/leaving frames through the archive does.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameCursor {
    /// Current frame, or `None` before frame zero of a fresh pass.
    pub frame: Option<usize>,
    /// The object's current extra screen displacement.
    pub displacement: [i32; 2],
}

#[derive(Debug, Clone)]
pub struct FrmArchive {
    pub version: u32,
    pub fps: i16,
    pub action_frame: i16,
    pub shifts: [[i16; 2]; 6],
    pub offsets: [u32; 6],
    /// Header metadata. Split animations may retain the larger unsplit total.
    pub frame_area_len: u32,
    stored_frame_area_len: u32,
    frame_count: usize,
    direction_sequences: [usize; 6],
    sequences: Vec<Vec<FrmFrame>>,
}

impl FrmArchive {
    /// The suffix `.FR0`–`.FR5` does not change this binary layout.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut reader = Reader::new(bytes, "FRM")?;
        let version = reader.be_i32()? as u32;
        let fps = reader.be_i16()?;
        let action_frame = reader.be_i16()?;
        let count = reader.be_i16()?;
        if count <= 0 {
            return Err(reader.error("frame count must be positive"));
        }
        let frame_count = count as usize;
        let mut shifts = [[0; 2]; 6];
        for axis in 0..2 {
            for shift in &mut shifts {
                shift[axis] = reader.be_i16()?;
            }
        }
        let mut offsets = [0; 6];
        for offset in &mut offsets {
            let value = reader.be_i32()?;
            *offset = reader.nonnegative(value, "direction offset")? as u32;
        }
        let area = reader.be_i32()?;
        let frame_area_len = reader.nonnegative(area, "frame area length")? as u32;
        let mut direction_sequences = [0; 6];
        let mut sequences = Vec::new();
        for direction in 0..6 {
            if direction > 0 && offsets[direction] == offsets[direction - 1] {
                direction_sequences[direction] = direction_sequences[direction - 1];
                continue;
            }
            if offsets[direction] as usize != reader.pos - 62 {
                return Err(reader.error(format!(
                    "direction {direction} offset must match the next packed block"
                )));
            }
            let mut frames = Vec::new();
            for _ in 0..frame_count {
                let file_offset = reader.pos;
                let width = reader.be_i16()?;
                let height = reader.be_i16()?;
                if width <= 0 || height <= 0 {
                    return Err(reader.error("frame dimensions must be positive"));
                }
                let size = reader.be_i32()?;
                let size = reader.nonnegative(size, "pixel byte count")?;
                if size != width as usize * height as usize {
                    return Err(reader.error("pixel byte count does not equal width × height"));
                }
                let displacement = [reader.be_i16()?, reader.be_i16()?];
                let pixels = reader.take(size)?.to_vec();
                frames.push(FrmFrame {
                    width: width as u16,
                    height: height as u16,
                    displacement,
                    file_offset,
                    pixels,
                });
            }
            direction_sequences[direction] = sequences.len();
            sequences.push(frames);
        }
        let stored_frame_area_len = (reader.pos - 62) as u32;
        // The six slots describe shared blocks, not six mandatory sequences.
        // Split FR0–FR5 assets keep the unsplit animation's declared area total.
        // Reads are bounded by actual bytes; never allocate or seek using that total.
        if stored_frame_area_len > frame_area_len
            || (sequences.len() != 1 && stored_frame_area_len != frame_area_len)
        {
            return Err(
                reader.error("declared frame area length does not cover the stored sequences")
            );
        }
        Ok(Self {
            version,
            fps,
            action_frame,
            shifts,
            offsets,
            frame_area_len,
            stored_frame_area_len,
            frame_count,
            direction_sequences,
            sequences,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }
    pub fn stored_sequences(&self) -> usize {
        self.sequences.len()
    }
    /// Actual stored bytes, counting shared direction sequences only once.
    pub fn stored_frame_area_len(&self) -> u32 {
        self.stored_frame_area_len
    }
    pub fn effective_fps(&self) -> i16 {
        if self.fps == 0 { 10 } else { self.fps }
    }
    pub fn frame_interval_ms(&self) -> Option<u64> {
        u64::try_from(self.effective_fps())
            .ok()
            .filter(|fps| *fps > 0)
            .map(|fps| 1000 / fps)
    }
    pub fn frame(&self, direction: usize, frame: usize) -> Option<&FrmFrame> {
        self.sequences
            .get(*self.direction_sequences.get(direction)?)?
            .get(frame)
    }
    /// Forward animation: enter the cursor's next frame in `direction`, adding
    /// that frame's displacement. Returns `false` (unchanged) past the last frame.
    pub fn advance(&self, cursor: &mut FrameCursor, direction: usize) -> bool {
        let next = cursor.frame.map_or(0, |frame| frame + 1);
        let Some(frame) = self.frame(direction, next) else {
            return false;
        };
        cursor.frame = Some(next);
        cursor.displacement[0] += i32::from(frame.displacement[0]);
        cursor.displacement[1] += i32::from(frame.displacement[1]);
        true
    }
    /// Reverse animation: leave the current frame, subtracting its displacement.
    /// Leaving frame zero returns to the position before the forward pass.
    pub fn reverse(&self, cursor: &mut FrameCursor, direction: usize) -> bool {
        let Some(frame) = cursor
            .frame
            .and_then(|current| self.frame(direction, current))
        else {
            return false;
        };
        cursor.displacement[0] -= i32::from(frame.displacement[0]);
        cursor.displacement[1] -= i32::from(frame.displacement[1]);
        cursor.frame = cursor.frame.and_then(|current| current.checked_sub(1));
        true
    }
    /// Displacement for an isolated forward pass, starting before frame zero.
    pub fn accumulated_displacement(&self, direction: usize, frame: usize) -> Option<[i32; 2]> {
        let sequence = self
            .sequences
            .get(*self.direction_sequences.get(direction)?)?;
        let frames = sequence.get(..=frame)?;
        Some(frames.iter().fold([0; 2], |sum, frame| {
            [
                sum[0] + i32::from(frame.displacement[0]),
                sum[1] + i32::from(frame.displacement[1]),
            ]
        }))
    }
}
