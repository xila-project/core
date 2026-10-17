use shared::{AnyByLayout, flags};

use crate::{Error, Result};

flags! {
    /// The kinds of control commands.
    pub enum ControlDirectionFlags: u8 {
        /// Read data from the device.
        Read,
        /// Write data to the device.
        Write,
    }
}

struct ControlCommandInputOutput<I, O> {
    pub _input: I,
    pub _output: O,
}

pub trait ControlCommand: Clone + Copy {
    type Input;
    type Output;

    const IDENTIFIER: ControlCommandIdentifier;

    fn cast_input(input: &AnyByLayout) -> Result<&Self::Input> {
        input.cast().ok_or(Error::InvalidParameter)
    }

    fn cast_output(output: &mut AnyByLayout) -> Result<&mut Self::Output> {
        output.cast_mutable().ok_or(Error::InvalidParameter)
    }

    fn cast<'i, 'o>(
        input: &'i AnyByLayout,
        output: &'o mut AnyByLayout,
    ) -> Result<(&'i Self::Input, &'o mut Self::Output)> {
        Ok((Self::cast_input(input)?, Self::cast_output(output)?))
    }
}

#[macro_export]
macro_rules! define_command {
    ($name:ident, Read, $kind:expr, $number:expr, $I:ty, $O:ty) => {
        $crate::define_command!(
            $name,
            $crate::ControlDirectionFlags::Read,
            $kind,
            $number,
            $I,
            $O
        );
    };
    ($name:ident, Write, $kind:expr, $number:expr, $I:ty, $O:ty) => {
        $crate::define_command!(
            $name,
            $crate::ControlDirectionFlags::Write,
            $kind,
            $number,
            $I,
            $O
        );
    };
    ($name:ident, $direction:expr, $kind:expr, $number:expr, $I:ty, $O:ty) => {
        #[derive(Clone, Copy, Debug)]
        #[allow(non_camel_case_types)]
        pub struct $name;

        impl ControlCommand for $name {
            type Input = $I;
            type Output = $O;

            const IDENTIFIER: $crate::ControlCommandIdentifier =
                $crate::ControlCommandIdentifier::new::<$I, $O>($direction, $kind, $number);
        }
    };
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlCommandIdentifier(usize);

impl ControlCommandIdentifier {
    const NUMBER_SIZE: usize = 8;
    const KIND_SIZE: usize = 8;
    const SIZE_SIZE: usize = 14;
    const MAX_SIZE: usize = (1 << Self::SIZE_SIZE) - 1;
    const DIRECTION_SIZE: usize = 2;

    const NUMBER_SHIFT: usize = 0;
    const KIND_SHIFT: usize = Self::NUMBER_SHIFT + Self::NUMBER_SIZE; // 8
    const SIZE_SHIFT: usize = Self::KIND_SHIFT + Self::KIND_SIZE; // 16
    const DIRECTION_SHIFT: usize = Self::SIZE_SHIFT + Self::SIZE_SIZE; // 30

    pub const fn new<I, O>(direction: ControlDirectionFlags, kind: u8, number: u8) -> Self {
        let size = core::mem::size_of::<ControlCommandInputOutput<I, O>>();

        Self::new_with_size(direction, size, kind, number)
    }

    pub const fn new_read<I, O>(kind: u8, number: u8) -> Self {
        Self::new_with_size(
            ControlDirectionFlags::Read,
            size_of::<ControlCommandInputOutput<I, O>>(),
            kind,
            number,
        )
    }

    pub const fn new_write<I, O>(kind: u8, number: u8) -> Self {
        Self::new_with_size(
            ControlDirectionFlags::Write,
            size_of::<ControlCommandInputOutput<I, O>>(),
            kind,
            number,
        )
    }

    pub const fn new_with_size(
        direction: ControlDirectionFlags,
        size: usize,
        kind: u8,
        number: u8,
    ) -> Self {
        assert!(
            size <= Self::MAX_SIZE,
            "control command size exceeds its 14-bit field"
        );

        let direction = direction.bits() as usize;
        let kind = kind as usize;
        let number = number as usize;

        Self(
            (direction << Self::DIRECTION_SHIFT)
                | (size << Self::SIZE_SHIFT)
                | (kind << Self::KIND_SHIFT)
                | (number << Self::NUMBER_SHIFT),
        )
    }

    /// Reconstructs an identifier received through a raw control interface.
    ///
    /// The identifier's current encoding occupies at most 32 bits. This
    /// conversion preserves those bits without interpreting the command.
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw as usize)
    }

    /// Returns the stable raw encoding used by generic control interfaces.
    pub const fn raw(self) -> u32 {
        self.0 as u32
    }

    pub const fn get_direction(&self) -> ControlDirectionFlags {
        let direction = (self.0 >> Self::DIRECTION_SHIFT) & ((1 << Self::DIRECTION_SIZE) - 1);
        ControlDirectionFlags(direction as u8)
    }

    pub const fn get_size(&self) -> usize {
        (self.0 >> Self::SIZE_SHIFT) & ((1 << Self::SIZE_SIZE) - 1)
    }

    pub const fn get_kind(&self) -> u8 {
        ((self.0 >> Self::KIND_SHIFT) & ((1 << Self::KIND_SIZE) - 1)) as u8
    }

    pub const fn get_number(&self) -> u8 {
        ((self.0 >> Self::NUMBER_SHIFT) & ((1 << Self::NUMBER_SIZE) - 1)) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_ENCODED_SIZE: usize = (1 << 14) - 1;

    #[test]
    fn new_with_size_preserves_maximum_14_bit_value_in_raw_encoding() {
        let identifier = ControlCommandIdentifier::new_with_size(
            ControlDirectionFlags::Write,
            MAX_ENCODED_SIZE,
            0xAB,
            0xCD,
        );
        let expected = (2u32 << 30) | ((MAX_ENCODED_SIZE as u32) << 16) | (0xABu32 << 8) | 0xCD;

        assert_eq!(identifier.get_size(), MAX_ENCODED_SIZE);
        assert_eq!(identifier.raw(), expected);
    }

    #[test]
    #[should_panic(expected = "control command size exceeds its 14-bit field")]
    fn new_with_size_rejects_values_above_14_bit_boundary() {
        ControlCommandIdentifier::new_with_size(
            ControlDirectionFlags::Write,
            MAX_ENCODED_SIZE + 1,
            0,
            0,
        );
    }
}
