#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VmError {
    #[error("stack underflow")]
    Underflow,
    #[error("stack overflow (2048-byte stack)")]
    Overflow,
    #[error("invalid operand type {0:#06x}")]
    Type(u16),
    #[error("invalid instruction {0:#06x}")]
    Opcode(u16),
    #[error("invalid code address {0:#x}")]
    Address(u32),
    #[error("truncated instruction at {0:#x}")]
    Truncated(u32),
    #[error("invalid stack slot or frame base")]
    Slot,
    #[error("invalid procedure index {0}")]
    Procedure(u32),
    #[error("declared argument count mismatch")]
    Arguments,
    #[error("division by zero")]
    DivisionByZero,
    #[error("undefined or overflowing arithmetic conversion")]
    Arithmetic,
    #[error("invalid string reference {0}")]
    StringReference(u32),
    #[error("dynamic string storage limit exceeded")]
    StringLimit,
    #[error("symbol not found or conflicting ownership: {0}")]
    Symbol(String),
    #[error("host operation is unavailable: {0}")]
    Host(String),
    #[error("imported-call argument ABI is undefined in SPEC-004")]
    ImportedAbi,
    #[error("instruction hard limit exceeded")]
    InstructionLimit,
    #[error("invalid scheduled-condition return/frame contract")]
    Condition,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value {
    pub payload: u32,
    pub tag: u16,
}
impl Value {
    pub const INTEGER: u16 = 0xc001;
    pub const FLOAT: u16 = 0xa001;
    pub const STATIC_STRING: u16 = 0x9001;
    pub const DYNAMIC_STRING: u16 = 0x9801;
    pub const fn integer(value: i32) -> Self {
        Self {
            payload: value as u32,
            tag: Self::INTEGER,
        }
    }
    pub fn float(value: f32) -> Self {
        Self {
            payload: value.to_bits(),
            tag: Self::FLOAT,
        }
    }
    pub fn is_string(self) -> bool {
        self.tag & !0x0800 == Self::STATIC_STRING
    }
    pub fn integer_value(self) -> Result<i32, VmError> {
        if self.tag == Self::INTEGER {
            Ok(self.payload as i32)
        } else {
            Err(VmError::Type(self.tag))
        }
    }
}
#[derive(Clone)]
pub struct Stack {
    bytes: [u8; 2048],
    len: usize,
}
impl Default for Stack {
    fn default() -> Self {
        Self {
            bytes: [0; 2048],
            len: 0,
        }
    }
}
impl std::fmt::Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stack")
            .field("bytes", &self.bytes())
            .finish()
    }
}
impl Stack {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    pub fn byte_len(&self) -> usize {
        self.len
    }
    pub fn push(&mut self, value: Value) -> Result<(), VmError> {
        if self.len + 6 >= 2048 {
            return Err(VmError::Overflow);
        }
        self.bytes[self.len..self.len + 4].copy_from_slice(&value.payload.to_be_bytes());
        self.bytes[self.len + 4..self.len + 6].copy_from_slice(&value.tag.to_be_bytes());
        self.len += 6;
        Ok(())
    }
    pub fn pop(&mut self) -> Result<Value, VmError> {
        if self.len < 6 {
            return Err(VmError::Underflow);
        }
        let value = self.get(self.len - 6)?;
        self.len -= 6;
        Ok(value)
    }
    pub fn top(&self) -> Result<Value, VmError> {
        self.get(self.len.checked_sub(6).ok_or(VmError::Underflow)?)
    }
    pub fn get(&self, offset: usize) -> Result<Value, VmError> {
        if !offset.is_multiple_of(6) || offset.checked_add(6).is_none_or(|end| end > self.len) {
            return Err(VmError::Slot);
        }
        Ok(Value {
            payload: u32::from_be_bytes(
                self.bytes[offset..offset + 4]
                    .try_into()
                    .expect("slot payload"),
            ),
            tag: u16::from_be_bytes(
                self.bytes[offset + 4..offset + 6]
                    .try_into()
                    .expect("slot tag"),
            ),
        })
    }
    pub fn set(&mut self, offset: usize, value: Value) -> Result<(), VmError> {
        self.get(offset)?;
        self.bytes[offset..offset + 4].copy_from_slice(&value.payload.to_be_bytes());
        self.bytes[offset + 4..offset + 6].copy_from_slice(&value.tag.to_be_bytes());
        Ok(())
    }
    pub fn truncate(&mut self, bytes: usize) -> Result<(), VmError> {
        if !bytes.is_multiple_of(6) || bytes > self.len {
            return Err(VmError::Slot);
        }
        self.len = bytes;
        Ok(())
    }
    pub fn swap(&mut self) -> Result<(), VmError> {
        let b = self.pop()?;
        let a = self.pop()?;
        self.push(b)?;
        self.push(a)
    }
}
