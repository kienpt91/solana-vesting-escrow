//! Bounds-checked little-endian cursors for account data and instruction data. Nothing here indexes a slice
//! directly: a short buffer is an error, never a panic.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};

/// Reads fixed-size fields in order from a byte slice.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    error: ProgramError,
}

impl<'a> Reader<'a> {
    /// `error` is returned when the buffer is too short.
    pub fn new(buf: &'a [u8], error: ProgramError) -> Self {
        Self { buf, pos: 0, error }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], ProgramError> {
        let end = self.pos.checked_add(N).ok_or(self.error.clone())?;
        let bytes = self.buf.get(self.pos..end).ok_or(self.error.clone())?;
        self.pos = end;
        <[u8; N]>::try_from(bytes).map_err(|_| self.error.clone())
    }

    pub fn u8(&mut self) -> Result<u8, ProgramError> {
        Ok(u8::from_le_bytes(self.take::<1>()?))
    }

    pub fn u64(&mut self) -> Result<u64, ProgramError> {
        Ok(u64::from_le_bytes(self.take::<8>()?))
    }

    pub fn i64(&mut self) -> Result<i64, ProgramError> {
        Ok(i64::from_le_bytes(self.take::<8>()?))
    }

    pub fn bytes<const N: usize>(&mut self) -> Result<[u8; N], ProgramError> {
        self.take::<N>()
    }

    pub fn pubkey(&mut self) -> Result<Pubkey, ProgramError> {
        Ok(Pubkey::new_from_array(self.take::<32>()?))
    }

    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
}

/// Writes fixed-size fields in order into a byte slice.
pub struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
    error: ProgramError,
}

impl<'a> Writer<'a> {
    pub fn new(buf: &'a mut [u8], error: ProgramError) -> Self {
        Self { buf, pos: 0, error }
    }

    pub fn put(&mut self, bytes: &[u8]) -> Result<(), ProgramError> {
        let end = self.pos.checked_add(bytes.len()).ok_or(self.error.clone())?;
        self.buf.get_mut(self.pos..end).ok_or(self.error.clone())?.copy_from_slice(bytes);
        self.pos = end;
        Ok(())
    }

    pub fn u8(&mut self, v: u8) -> Result<(), ProgramError> {
        self.put(&v.to_le_bytes())
    }

    pub fn u64(&mut self, v: u64) -> Result<(), ProgramError> {
        self.put(&v.to_le_bytes())
    }

    pub fn i64(&mut self, v: i64) -> Result<(), ProgramError> {
        self.put(&v.to_le_bytes())
    }

    pub fn pubkey(&mut self, v: &Pubkey) -> Result<(), ProgramError> {
        self.put(v.as_ref())
    }
}
