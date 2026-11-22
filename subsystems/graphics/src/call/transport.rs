//! How pointer, string and buffer arguments reach the executor.
//!
//! A call carries up to seven words. Scalars are used as they are; a pointer
//! word is an address the transport understands. Wasm passes offsets into a
//! guest's linear memory, which the transport bounds-checks and translates;
//! native code passes host pointers directly, since native code is trusted.
//! The executor never dereferences an address itself.
//!
//! All multi-byte values behind an address are little-endian. Xila's targets
//! and Wasm are little-endian, so a native transport reads them in place.

use super::error::{Error, Result};
use core::{ffi::CStr, marker::PhantomData};

/// One argument word: as wide as the host's pointers, so a call costs no
/// double-word arithmetic on a 32-bit target. A Wasm guest's 32-bit or 64-bit
/// word fits, since guests with 64-bit memory only run on 64-bit hosts.
pub type Word = usize;

/// Argument memory of one caller.
pub trait Transport {
    /// Width in bits of the caller's pointer-sized word, 32 or 64, never wider
    /// than the host's `usize`. It tells a zero-extended 32-bit word from a
    /// sign-extended 64-bit one.
    fn word_bits(&self) -> u32;

    /// Borrow `length` bytes at `address`. A null address is valid only for an
    /// empty range.
    fn bytes(&self, address: Word, length: usize) -> Result<&[u8]>;

    /// Exclusively borrow `length` bytes at `address`.
    fn bytes_mut(&mut self, address: Word, length: usize) -> Result<&mut [u8]>;

    /// Borrow the NUL-terminated string at `address` without copying it.
    /// A string longer than `limit` bytes, NUL included, is `ResourceLimit`;
    /// a null address or a string that leaves memory is `InvalidMemory`.
    fn c_string(&self, address: Word, limit: usize) -> Result<&CStr>;
}

/// A scalar with a fixed little-endian encoding behind an address.
pub(crate) trait Scalar: Copy {
    const SIZE: usize;
    fn decode(bytes: &[u8]) -> Self;
    fn encode(self, bytes: &mut [u8]);
}

macro_rules! scalars {
    ($($ty:ty),*) => { $(
        impl Scalar for $ty {
            const SIZE: usize = core::mem::size_of::<$ty>();
            fn decode(bytes: &[u8]) -> Self {
                Self::from_le_bytes(bytes.try_into().expect("exact scalar size"))
            }
            fn encode(self, bytes: &mut [u8]) {
                bytes.copy_from_slice(&self.to_le_bytes());
            }
        }
    )* };
}
scalars!(u8, u16, u32, i32);

fn offset(address: Word, delta: usize) -> Result<Word> {
    address
        .checked_add(delta as Word)
        .ok_or(Error::InvalidMemory)
}

/// Typed access built on the three required methods.
impl dyn Transport + '_ {
    pub(crate) fn word_size(&self) -> usize {
        (self.word_bits() / 8) as usize
    }

    pub(crate) fn validate(&self, address: Word, length: usize) -> Result<()> {
        self.bytes(address, length).map(|_| ())
    }

    pub(crate) fn read<T: Scalar>(&self, address: Word) -> Result<T> {
        Ok(T::decode(self.bytes(address, T::SIZE)?))
    }

    pub(crate) fn write<T: Scalar>(&mut self, address: Word, value: T) -> Result<()> {
        value.encode(self.bytes_mut(address, T::SIZE)?);
        Ok(())
    }

    /// Write a result whose location `execute` validated before the call, so
    /// the write cannot fail. A panic path per call site would cost code for
    /// a case that only a broken transport can reach.
    pub(crate) fn put<T: Scalar>(&mut self, address: Word, value: T) {
        let written = self.write(address, value);
        debug_assert!(written.is_ok(), "result location was validated");
    }

    /// A pointer-sized word.
    pub(crate) fn read_word(&self, address: Word) -> Result<Word> {
        let bytes = self.bytes(address, self.word_size())?;
        let mut word = [0; size_of::<Word>()];
        word[..bytes.len()].copy_from_slice(bytes);
        Ok(Word::from_le_bytes(word))
    }

    pub(crate) fn put_word(&mut self, address: Word, value: Word) {
        let size = self.word_size();
        let written = self.bytes_mut(address, size);
        debug_assert!(written.is_ok(), "result location was validated");
        if let Ok(bytes) = written {
            bytes.copy_from_slice(&value.to_le_bytes()[..size]);
        }
    }

    /// Lazily decode `count` consecutive scalars.
    pub(crate) fn values<T: Scalar>(&self, address: Word, count: usize) -> Result<Values<'_, T>> {
        let length = count.checked_mul(T::SIZE).ok_or(Error::InvalidMemory)?;
        Ok(Values {
            bytes: self.bytes(address, length)?,
            marker: PhantomData,
        })
    }

    /// The `index`th scalar of a table.
    pub(crate) fn read_at<T: Scalar>(&self, address: Word, index: usize) -> Result<T> {
        self.read(offset(address, index * T::SIZE)?)
    }
}

/// Scalars decoded one at a time from borrowed bytes.
pub(crate) struct Values<'a, T> {
    bytes: &'a [u8],
    marker: PhantomData<T>,
}

impl<T: Scalar> Iterator for Values<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.bytes.is_empty() {
            return None;
        }
        let (value, rest) = self.bytes.split_at(T::SIZE);
        self.bytes = rest;
        Some(T::decode(value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.bytes.len() / T::SIZE;
        (remaining, Some(remaining))
    }
}

impl<T: Scalar> ExactSizeIterator for Values<'_, T> {}

/// A flat byte buffer addressed by offset: the transport for Wasm linear
/// memory, and for any caller whose pointers are offsets into one buffer.
/// Offset 0 is null.
pub struct LinearMemory<'a> {
    bytes: &'a mut [u8],
    word_bits: u32,
}

impl<'a> LinearMemory<'a> {
    pub fn new(bytes: &'a mut [u8], word_bits: u32) -> Self {
        assert!(word_bits == 32 || word_bits == 64);
        assert!(
            word_bits <= usize::BITS,
            "a 64-bit guest needs a 64-bit host"
        );
        Self { bytes, word_bits }
    }

    fn range(&self, address: Word, length: usize) -> Result<core::ops::Range<usize>> {
        if length != 0 && address == 0 {
            return Err(Error::InvalidMemory);
        }
        let start = address;
        let end = start.checked_add(length).ok_or(Error::InvalidMemory)?;
        self.bytes.get(start..end).ok_or(Error::InvalidMemory)?;
        Ok(start..end)
    }
}

impl Transport for LinearMemory<'_> {
    fn word_bits(&self) -> u32 {
        self.word_bits
    }

    fn bytes(&self, address: Word, length: usize) -> Result<&[u8]> {
        Ok(&self.bytes[self.range(address, length)?])
    }

    fn bytes_mut(&mut self, address: Word, length: usize) -> Result<&mut [u8]> {
        let range = self.range(address, length)?;
        Ok(&mut self.bytes[range])
    }

    fn c_string(&self, address: Word, limit: usize) -> Result<&CStr> {
        let start = self.range(address, 1)?.start;
        let window = &self.bytes[start..self.bytes.len().min(start.saturating_add(limit))];
        match window.iter().position(|&byte| byte == 0) {
            Some(end) => {
                Ok(CStr::from_bytes_with_nul(&window[..=end]).expect("first NUL ends the string"))
            }
            // No NUL within the limit: too long if the window is full, else it ran off memory.
            None if window.len() == limit => Err(Error::ResourceLimit),
            None => Err(Error::InvalidMemory),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_memory_allows_null_only_for_empty_ranges_and_bounds_every_range() {
        let mut bytes = [0; 8];
        let memory = LinearMemory::new(&mut bytes, 32);
        assert_eq!(memory.bytes(0, 0), Ok(&[][..]));
        assert_eq!(memory.bytes(0, 1), Err(Error::InvalidMemory));
        assert_eq!(memory.bytes(8, 0), Ok(&[][..]));
        assert_eq!(memory.bytes(9, 0), Err(Error::InvalidMemory));
        assert_eq!(memory.bytes(7, 2), Err(Error::InvalidMemory));
        assert_eq!(memory.bytes(Word::MAX, 2), Err(Error::InvalidMemory));
        assert_eq!(memory.c_string(0, 4), Err(Error::InvalidMemory));
        assert_eq!(memory.c_string(8, 4), Err(Error::InvalidMemory));
    }

    #[test]
    fn scalars_and_words_are_little_endian_at_unaligned_offsets() {
        let mut bytes = [0; 24];
        let mut memory = LinearMemory::new(&mut bytes, 32);
        let transport: &mut dyn Transport = &mut memory;
        transport.write(1, 0x1234_5678_u32).unwrap();
        transport.write(5, -2_i32).unwrap();
        transport.write(9, 0xabcd_u16).unwrap();
        transport.put_word(11, 0x0102_0304);
        assert_eq!(transport.read::<u32>(1), Ok(0x1234_5678));
        assert_eq!(transport.read::<i32>(5), Ok(-2));
        assert_eq!(transport.read_at::<u16>(9, 0), Ok(0xabcd));
        assert_eq!(transport.read_word(11), Ok(0x0102_0304));
        assert_eq!(transport.write(23, 0_u16), Err(Error::InvalidMemory));
        assert_eq!(
            transport.read_at::<u32>(1, usize::MAX / 4),
            Err(Error::InvalidMemory)
        );
        assert_eq!(
            &bytes[1..15],
            &[
                0x78, 0x56, 0x34, 0x12, 0xfe, 0xff, 0xff, 0xff, 0xcd, 0xab, 0x04, 0x03, 0x02, 0x01
            ]
        );
    }

    #[test]
    fn values_decode_lazily_and_reject_overflowing_counts() {
        let mut bytes = [0, 1, 0, 2, 0, 3, 0];
        let memory = LinearMemory::new(&mut bytes, 32);
        let transport: &dyn Transport = &memory;
        let values = transport.values::<u16>(1, 3).unwrap();
        assert_eq!(values.len(), 3);
        assert!(values.eq([1, 2, 3]));
        assert!(transport.values::<u16>(1, 4).is_err());
        assert!(transport.values::<u32>(1, usize::MAX).is_err());
        assert_eq!(transport.values::<u32>(0, 0).unwrap().count(), 0);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn sixty_four_bit_words_use_the_full_host_word() {
        let mut bytes = [0; 12];
        let mut memory = LinearMemory::new(&mut bytes, 64);
        let transport: &mut dyn Transport = &mut memory;
        transport.put_word(3, 0x0102_0304_0506_0708);
        assert_eq!(transport.read_word(3), Ok(0x0102_0304_0506_0708));
        assert_eq!(transport.read_word(5), Err(Error::InvalidMemory));
    }

    #[test]
    fn c_strings_borrow_guest_memory_in_place_up_to_the_end_of_memory() {
        let mut bytes = *b"\0abc\0de";
        let base = bytes.as_ptr() as usize;
        let memory = LinearMemory::new(&mut bytes, 32);
        let text = memory.c_string(1, 4).unwrap();
        assert_eq!(text.to_bytes(), b"abc");
        assert_eq!(text.as_ptr() as usize, base + 1);
        assert_eq!(memory.c_string(1, 3), Err(Error::ResourceLimit));
        // Runs off memory before the limit: invalid, not too long.
        assert_eq!(memory.c_string(5, 8), Err(Error::InvalidMemory));
        assert_eq!(memory.c_string(5, 2), Err(Error::ResourceLimit));
    }
}
