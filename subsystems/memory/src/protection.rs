//! Memory protection flags and operations.
//!
//! This module provides functionality to represent and manipulate memory protection
//! settings (read, write, execute) and interfaces for applying these protections.

use core::fmt::Debug;

/// Represents memory protection flags.
///
/// This structure encapsulates read, write, and execute permissions for memory regions
/// in a compact bit representation.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Protection(u8);

impl Debug for Protection {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Protection_type")
            .field("Read", &self.get_read())
            .field("Write", &self.get_write())
            .field("Execute", &self.get_execute())
            .finish()
    }
}

impl Protection {
    /// Bit flag representing read permission.
    pub const READ_BIT: u8 = 1 << 0;

    /// Bit flag representing write permission.
    pub const WRITE_BIT: u8 = 1 << 1;

    /// Bit flag representing execute permission.
    pub const EXECUTE_BIT: u8 = 1 << 2;

    /// No memory access permissions.
    pub const NONE: Self = Self(0);

    /// Read-only memory access.
    pub const READ: Self = Self(Self::READ_BIT);

    /// Write-only memory access.
    pub const WRITE: Self = Self(Self::WRITE_BIT);

    /// Execute-only memory access.
    pub const EXECUTE: Self = Self(Self::EXECUTE_BIT);

    /// Read and write memory access.
    pub const READ_WRITE: Self = Self(Self::READ_BIT | Self::WRITE_BIT);

    /// Read and execute memory access.
    pub const READ_EXECUTE: Self = Self(Self::READ_BIT | Self::EXECUTE_BIT);

    /// Write and execute memory access.
    pub const WRITE_EXECUTE: Self = Self(Self::WRITE_BIT | Self::EXECUTE_BIT);

    /// Full memory access (read, write, and execute).
    pub const READ_WRITE_EXECUTE: Self = Self(Self::READ_BIT | Self::WRITE_BIT | Self::EXECUTE_BIT);

    /// Creates a new protection type with specified permissions.
    ///
    /// # Parameters
    /// - `Read`: Whether read permission is granted
    /// - `Write`: Whether write permission is granted
    /// - `Execute`: Whether execute permission is granted
    ///
    /// # Returns
    /// A new protection type with the specified permissions.
    pub const fn new(read: bool, write: bool, execute: bool) -> Self {
        Self(0).set_read(read).set_write(write).set_execute(execute)
    }

    /// Sets or clears specified bits in the protection flags.
    ///
    /// # Parameters
    /// - `Bits`: The bits to modify
    /// - `Value`: Whether to set or clear the bits
    ///
    /// # Returns
    /// A new protection type with the modified bits.
    const fn set_bits(mut self, bits: u8, value: bool) -> Self {
        if value {
            self.0 |= bits;
        } else {
            self.0 &= !bits;
        }
        self
    }

    /// Checks if any of the specified bits are set.
    ///
    /// # Parameters
    /// - `Bits`: The bits to check
    ///
    /// # Returns
    /// `true` if any of the specified bits are set, `false` otherwise.
    const fn get_bits(&self, bits: u8) -> bool {
        self.0 & bits != 0
    }

    /// Sets or clears the read permission.
    ///
    /// # Parameters
    /// - `Value`: Whether to grant or revoke read permission
    ///
    /// # Returns
    /// A new protection type with the modified read permission.
    pub const fn set_read(self, value: bool) -> Self {
        self.set_bits(Self::READ_BIT, value)
    }

    /// Sets or clears the write permission.
    ///
    /// # Parameters
    /// - `Value`: Whether to grant or revoke write permission
    ///
    /// # Returns
    /// A new protection type with the modified write permission.
    pub const fn set_write(self, value: bool) -> Self {
        self.set_bits(Self::WRITE_BIT, value)
    }

    /// Sets or clears the execute permission.
    ///
    /// # Parameters
    /// - `Value`: Whether to grant or revoke execute permission
    ///
    /// # Returns
    /// A new protection type with the modified execute permission.
    pub const fn set_execute(self, value: bool) -> Self {
        self.set_bits(Self::EXECUTE_BIT, value)
    }

    /// Checks if read permission is granted.
    ///
    /// # Returns
    /// `true` if read permission is granted, `false` otherwise.
    pub const fn get_read(&self) -> bool {
        self.get_bits(Self::READ_BIT)
    }

    /// Checks if write permission is granted.
    ///
    /// # Returns
    /// `true` if write permission is granted, `false` otherwise.
    pub const fn get_write(&self) -> bool {
        self.get_bits(Self::WRITE_BIT)
    }

    /// Checks if execute permission is granted.
    ///
    /// # Returns
    /// `true` if execute permission is granted, `false` otherwise.
    pub const fn get_execute(&self) -> bool {
        self.get_bits(Self::EXECUTE_BIT)
    }

    /// Converts the protection type to its raw u8 representation.
    ///
    /// # Returns
    /// The raw byte value representing the protection flags.
    pub const fn as_u8(&self) -> u8 {
        self.0
    }

    /// Creates a new protection type from a raw u8 value.
    ///
    /// # Parameters
    /// - `Value`: The raw protection flags
    ///
    /// # Returns
    /// A new protection type with the specified flags.
    pub const fn from_u8(value: u8) -> Self {
        Self(value)
    }
}

impl From<Protection> for u8 {
    fn from(protection: Protection) -> Self {
        protection.0
    }
}

impl From<u8> for Protection {
    fn from(protection: u8) -> Self {
        Self(protection)
    }
}

/// Interface for implementing memory protection operations.
///
/// This trait should be implemented by memory managers or other components
/// that can apply protection settings to memory regions.
pub trait Protectable {}
