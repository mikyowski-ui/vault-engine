//! SPEC-014 §2.2 splash images (`art/splash/splash<n>.rix`): ten skipped
//! header bytes, a 256-entry six-bit palette, then 640 by 480 uncompressed
//! palette indexes, row-major from the top row.
use crate::{ParseError, binary::Reader};

pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 480;
/// Bytes `0x00`-`0x09` (signature, little-endian size words, two more
/// bytes) are skipped without validation.
pub const HEADER_LEN: usize = 10;
/// Header, palette and pixels: the corpus length.
pub const FILE_LEN: usize = HEADER_LEN + 768 + WIDTH * HEIGHT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splash {
    /// Six-bit RGB levels, as stored.
    pub palette: [[u8; 3]; 256],
    /// `WIDTH * HEIGHT` palette indexes.
    pub pixels: Vec<u8>,
}

impl Splash {
    /// The reference reads whatever is present; under the safe rule a file
    /// shorter than `FILE_LEN` is rejected (and no splash is shown). Bytes
    /// after the pixels are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut reader = Reader::new(bytes, "RIX")?;
        if bytes.len() < FILE_LEN {
            return Err(reader.error(format!(
                "splash needs {FILE_LEN} bytes, found {}",
                bytes.len()
            )));
        }
        reader.take(HEADER_LEN)?;
        let mut palette = [[0; 3]; 256];
        for (entry, triple) in palette.iter_mut().zip(reader.take(768)?.as_chunks::<3>().0) {
            *entry = *triple;
        }
        Ok(Self {
            palette,
            pixels: reader.take(WIDTH * HEIGHT)?.to_vec(),
        })
    }
    pub fn pixel(&self, x: usize, y: usize) -> Option<u8> {
        (x < WIDTH && y < HEIGHT).then(|| self.pixels[y * WIDTH + x])
    }
}
