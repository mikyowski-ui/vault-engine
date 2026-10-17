//! SPEC-006 ordered initializers, not a name/value configuration language.
use crate::{
    ParseError,
    binary::{MAX_ENTRIES, Reader},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamSection {
    Campaign,
    Map,
}
impl GamSection {
    pub fn marker(self) -> &'static [u8; 16] {
        match self {
            Self::Campaign => b"GAME_GLOBAL_VARS",
            Self::Map => b"MAP_GLOBAL_VARS:",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamFile {
    pub section: GamSection,
    pub found: bool,
    pub values: Vec<i32>,
}
impl GamFile {
    pub fn parse(bytes: &[u8], section: GamSection) -> Result<Self, ParseError> {
        Reader::new(bytes, "GAM")?;
        let mut result = Self {
            section,
            found: false,
            values: Vec::new(),
        };
        let mut position = 0;
        while position < bytes.len() {
            let offset = position;
            let mut chunk = Vec::with_capacity(257);
            while position < bytes.len() && chunk.len() < 257 {
                let mut byte = bytes[position];
                position += 1;
                if byte == b'\r' && bytes.get(position) == Some(&b'\n') {
                    byte = b'\n';
                    position += 1;
                }
                chunk.push(byte);
                if byte == b'\n' {
                    break;
                }
            }
            if !result.found {
                if chunk.starts_with(section.marker()) {
                    result.found = true;
                }
                continue;
            }
            if chunk.first() == Some(&b'\n') || chunk.starts_with(b"//") {
                continue;
            }
            if result.values.len() == MAX_ENTRIES {
                return Err(ParseError::new(
                    "GAM",
                    offset,
                    "initializer count limit exceeded",
                ));
            }
            // Text conversion treats an embedded NUL as the end of retained text.
            let end = chunk
                .iter()
                .position(|b| matches!(b, b';' | 0))
                .unwrap_or(chunk.len());
            let line = &chunk[..end];
            let value = if let Some(equal) = line.iter().position(|b| *b == b'=') {
                let text = line[equal + 1..].trim_ascii_start();
                let sign = usize::from(text.first().is_some_and(|b| matches!(b, b'+' | b'-')));
                let digits = text[sign..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if digits == 0 {
                    return Err(ParseError::new(
                        "GAM",
                        offset + equal + 1,
                        "assignment has no defined numeric value",
                    ));
                }
                std::str::from_utf8(&text[..sign + digits])
                    .expect("ASCII decimal")
                    .parse::<i32>()
                    .map_err(|_| {
                        ParseError::new(
                            "GAM",
                            offset + equal + 1,
                            "decimal initializer exceeds i32",
                        )
                    })?
            } else {
                0
            };
            result.values.push(value);
        }
        Ok(result)
    }
}
