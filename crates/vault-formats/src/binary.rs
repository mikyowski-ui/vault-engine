/// Per-asset resource budget, also used by corpus readers before allocating.
pub const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_ENTRIES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{format} at byte {offset:#x}: {message}")]
pub struct ParseError {
    pub format: &'static str,
    pub offset: usize,
    pub message: String,
}

impl ParseError {
    pub(crate) fn new(format: &'static str, offset: usize, message: impl Into<String>) -> Self {
        Self {
            format,
            offset,
            message: message.into(),
        }
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pub pos: usize,
    pub format: &'static str,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8], format: &'static str) -> Result<Self, ParseError> {
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(ParseError::new(format, 0, "asset exceeds 64 MiB limit"));
        }
        Ok(Self {
            bytes,
            pos: 0,
            format,
        })
    }

    pub fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(self.format, self.pos, message)
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8], ParseError> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or_else(|| self.error("size overflow"))?;
        let data = self.bytes.get(self.pos..end).ok_or_else(|| {
            self.error(format!(
                "truncated input: need {count} bytes, have {}",
                self.bytes.len() - self.pos
            ))
        })?;
        self.pos = end;
        Ok(data)
    }

    pub fn be_i16(&mut self) -> Result<i16, ParseError> {
        Ok(i16::from_be_bytes(
            self.take(2)?.try_into().expect("fixed slice"),
        ))
    }

    pub fn be_i32(&mut self) -> Result<i32, ParseError> {
        Ok(i32::from_be_bytes(
            self.take(4)?.try_into().expect("fixed slice"),
        ))
    }

    pub fn word(&mut self) -> Result<u32, ParseError> {
        self.be_i32().map(|value| value as u32)
    }

    pub fn words<const N: usize>(&mut self) -> Result<[i32; N], ParseError> {
        let mut words = [0; N];
        for word in &mut words {
            *word = self.be_i32()?;
        }
        Ok(words)
    }

    pub fn count(&mut self, name: &str, limit: usize) -> Result<usize, ParseError> {
        let value = self.be_i32()?;
        let count = self.nonnegative(value, name)?;
        if count > limit {
            return Err(self.error(format!("{name} exceeds {limit}")));
        }
        Ok(count)
    }

    pub fn le_i32(&mut self) -> Result<i32, ParseError> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("fixed slice"),
        ))
    }

    pub fn nonnegative(&self, value: i32, field: &str) -> Result<usize, ParseError> {
        usize::try_from(value).map_err(|_| self.error(format!("negative {field}: {value}")))
    }
}

/// Big-endian serialization helpers shared by the SPEC-003 writers.
pub(crate) fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}
pub(crate) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
