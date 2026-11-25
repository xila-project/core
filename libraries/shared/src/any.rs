use core::mem::align_of;
use core::slice;

use crate::PlainData;

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq)]
pub struct AnyByLayout([u8]);

impl<'a, T: PlainData> From<&'a mut T> for &'a mut AnyByLayout {
    fn from(argument: &'a mut T) -> Self {
        AnyByLayout::from_mutable(argument)
    }
}

impl<'a, T: PlainData> From<&'a T> for &'a AnyByLayout {
    fn from(argument: &'a T) -> Self {
        AnyByLayout::from(argument)
    }
}

impl AnyByLayout {
    pub const NONE: &mut Self = Self::from_mutable(&mut [0u8; 0]);

    /// Creates an `AnyByLayout` from raw parts.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it creates a reference from a raw pointer.
    /// The caller must ensure that the pointer is valid for reads and writes
    /// for `size` bytes and properly aligned.
    pub unsafe fn from_raw_parts<'a>(data: *mut u8, size: usize) -> &'a mut Self {
        unsafe {
            let slice = slice::from_raw_parts_mut(data, size);
            &mut *(slice as *mut [u8] as *mut Self)
        }
    }

    /// Views a plain-data value as raw bytes.
    ///
    /// `T` must be [`PlainData`], so the bytes can always be read back as a
    /// valid `T` (for example through [`AnyByLayout::cast`]); with a `bool` or
    /// enum `T`, safe code could write an invalid value such as `2` for a
    /// `bool` through [`AnyByLayout::as_mutable_bytes`].
    pub const fn from_mutable<T: PlainData>(argument: &mut T) -> &mut Self {
        unsafe {
            let slice = slice::from_raw_parts_mut(argument as *mut T as *mut u8, size_of::<T>());
            &mut *(slice as *mut [u8] as *mut Self)
        }
    }

    /// Views a plain-data value as raw bytes; see [`AnyByLayout::from_mutable`]
    /// for the `T: PlainData` requirement.
    pub fn from<T: PlainData>(argument: &T) -> &Self {
        unsafe {
            let slice = slice::from_raw_parts(argument as *const T as *const u8, size_of::<T>());
            &*(slice as *const [u8] as *const Self)
        }
    }

    /// Returns the payload as a `T` only when it is an exact fit: the payload
    /// length equals `size_of::<T>()` and its address is aligned for `T`.
    ///
    /// `T` must be [`PlainData`], so every payload byte pattern is a valid `T`;
    /// a `bool` or enum payload can never hold an invalid value this way.
    pub fn cast_mutable<T: PlainData>(&mut self) -> Option<&mut T> {
        if !self.is_exact_fit::<T>() {
            return None;
        }
        // SAFETY: `is_exact_fit` proved that the payload is exactly
        // `size_of::<T>()` bytes long and aligned for `T`, and `T: PlainData`
        // proves that every bit pattern of that size is a valid `T`. The
        // exclusive borrow of `self` gives unique access.
        Some(unsafe { &mut *self.0.as_mut_ptr().cast::<T>() })
    }

    /// Returns the payload as a `T` only when it is an exact fit: the payload
    /// length equals `size_of::<T>()` and its address is aligned for `T`.
    ///
    /// `T` must be [`PlainData`], so every payload byte pattern is a valid `T`;
    /// a `bool` or enum payload can never hold an invalid value this way.
    pub fn cast<T: PlainData>(&self) -> Option<&T> {
        if !self.is_exact_fit::<T>() {
            return None;
        }
        // SAFETY: `is_exact_fit` proved that the payload is exactly
        // `size_of::<T>()` bytes long and aligned for `T`, and `T: PlainData`
        // proves that every bit pattern of that size is a valid `T`.
        Some(unsafe { &*self.0.as_ptr().cast::<T>() })
    }

    fn is_exact_fit<T>(&self) -> bool {
        let address = self.0.as_ptr() as usize;
        self.0.len() == size_of::<T>() && address.is_multiple_of(align_of::<T>())
    }

    pub fn get_size(&self) -> usize {
        self.0.len()
    }

    pub fn get_alignment(&self) -> usize {
        self.0.as_ptr().align_offset(1)
    }

    pub fn as_mutable_bytes(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use alloc::vec;

    #[test]
    fn test_from_mutable_primitive() {
        let mut value: u32 = 42;
        let any = AnyByLayout::from_mutable(&mut value);

        assert_eq!(any.get_size(), size_of::<u32>());
        assert_eq!(any.as_mutable_bytes(), &[42, 0, 0, 0]);
    }

    #[test]
    fn test_from_immutable_primitive() {
        let value: u64 = 0x0102030405060708;
        let any = AnyByLayout::from(&value);

        assert_eq!(any.get_size(), size_of::<u64>());
    }

    #[test]
    fn test_from_trait_mutable() {
        let mut value: i32 = -100;
        let any: &mut AnyByLayout = (&mut value).into();

        assert_eq!(any.get_size(), size_of::<i32>());
    }

    #[test]
    fn test_from_trait_immutable() {
        let value: i16 = 256;
        let any: &AnyByLayout = (&value).into();

        assert_eq!(any.get_size(), size_of::<i16>());
    }

    #[test]
    fn test_cast_mutable_success() {
        let mut value: u32 = 12345;
        let any = AnyByLayout::from_mutable(&mut value);

        let cast_back = any.cast_mutable::<u32>();
        assert!(cast_back.is_some());

        if let Some(cast_ref) = cast_back {
            assert_eq!(*cast_ref, 12345);
            // Modify through the cast reference
            *cast_ref = 67890;
        }
        assert_eq!(value, 67890);
    }

    #[test]
    fn test_cast_immutable_success() {
        let value: i64 = -9876543210;
        let any = AnyByLayout::from(&value);

        let cast_back = any.cast::<i64>();
        assert!(cast_back.is_some());
        assert_eq!(*cast_back.unwrap(), -9876543210);
    }

    #[test]
    fn test_cast_wrong_type_size() {
        let value: u32 = 42;
        let any = AnyByLayout::from(&value);

        // Try to cast u32 (4 bytes) as u64 (8 bytes)
        let cast_result = any.cast::<u64>();
        assert!(cast_result.is_none());
    }

    #[test]
    fn test_cast_accepts_only_an_exact_fit() {
        let value: u32 = 0xAABBCCDD;
        let any = AnyByLayout::from(&value);
        assert_eq!(any.cast::<u32>().copied(), Some(0xAABBCCDD));

        // Too short.
        let short: u16 = 0x1234;
        assert!(AnyByLayout::from(&short).cast::<u32>().is_none());

        // Too long.
        let long: u64 = 0x1122334455667788;
        assert!(AnyByLayout::from(&long).cast::<u32>().is_none());
        assert!(any.cast::<u64>().is_none());
    }

    #[test]
    fn test_cast_mutable_accepts_only_an_exact_fit() {
        let mut value: u32 = 7;
        let any = AnyByLayout::from_mutable(&mut value);
        *any.cast_mutable::<u32>().unwrap() = 8;
        assert_eq!(value, 8);

        let mut long: u64 = 1;
        assert!(
            AnyByLayout::from_mutable(&mut long)
                .cast_mutable::<u32>()
                .is_none()
        );
        let mut short: u16 = 1;
        assert!(
            AnyByLayout::from_mutable(&mut short)
                .cast_mutable::<u32>()
                .is_none()
        );
    }

    #[test]
    fn test_cast_rejects_a_misaligned_payload() {
        #[repr(align(8))]
        struct Aligned([u8; 8]);

        let mut bytes = Aligned([0; 8]);
        let misaligned = unsafe { AnyByLayout::from_raw_parts(bytes.0.as_mut_ptr().add(1), 4) };
        assert!(misaligned.cast::<u32>().is_none());
        assert!(misaligned.cast_mutable::<u32>().is_none());

        // A longer misaligned payload holds one aligned `u32` after a prefix: the
        // old check accepted it and read the value at the wrong offset.
        let misaligned_longer =
            unsafe { AnyByLayout::from_raw_parts(bytes.0.as_mut_ptr().add(1), 7) };
        assert!(misaligned_longer.cast::<u32>().is_none());
        assert!(misaligned_longer.cast_mutable::<u32>().is_none());

        let aligned = unsafe { AnyByLayout::from_raw_parts(bytes.0.as_mut_ptr(), 4) };
        assert!(aligned.cast::<u32>().is_some());
        assert!(aligned.cast_mutable::<u32>().is_some());
    }

    #[test]
    fn test_cast_zero_sized_type_needs_an_empty_payload() {
        assert!(AnyByLayout::from(&()).cast::<()>().is_some());
        assert!(AnyByLayout::from(&1u8).cast::<()>().is_none());
    }

    #[test]
    fn test_from_raw_parts() {
        let mut data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let any = unsafe { AnyByLayout::from_raw_parts(data.as_mut_ptr(), data.len()) };

        assert_eq!(any.get_size(), 8);
        assert_eq!(any.as_mutable_bytes(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn test_empty_constant() {
        let empty = AnyByLayout::NONE;
        assert_eq!(empty.get_size(), 0);
    }

    #[test]
    fn test_as_mutable_bytes_modify() {
        let mut value: u32 = 0x12345678;
        let any = AnyByLayout::from_mutable(&mut value);

        let bytes = any.as_mutable_bytes();
        bytes[0] = 0xFF;

        // Value should be modified
        assert_eq!(value & 0xFF, 0xFF);
    }

    #[test]
    fn test_struct_conversion() {
        crate::plain_data! {
            #[repr(C)]
            #[derive(Debug, PartialEq, Clone)]
            struct TestStruct {
                a: u32,
                b: u16,
                c: u8,
                reserved: u8,
            }
        }

        let mut original = TestStruct {
            a: 100,
            b: 200,
            c: 50,
            reserved: 0,
        };
        let expected = original.clone();
        let any = AnyByLayout::from_mutable(&mut original);

        assert_eq!(any.get_size(), size_of::<TestStruct>());

        let cast_back = any.cast_mutable::<TestStruct>();
        assert!(cast_back.is_some());

        if let Some(cast_ref) = cast_back {
            assert_eq!(*cast_ref, expected);
            // Modify and verify
            cast_ref.a = 999;
        }
        assert_eq!(original.a, 999);
    }

    #[test]
    fn test_array_conversion() {
        let mut array = [1u8, 2, 3, 4, 5];
        let any = AnyByLayout::from_mutable(&mut array);

        assert_eq!(any.get_size(), 5);
        assert_eq!(any.as_mutable_bytes(), &[1, 2, 3, 4, 5]);

        let cast_back = any.cast_mutable::<[u8; 5]>();
        assert!(cast_back.is_some());
        assert_eq!(*cast_back.unwrap(), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_zero_sized_type() {
        let mut unit = ();
        let any = AnyByLayout::from_mutable(&mut unit);

        assert_eq!(any.get_size(), 0);
    }

    #[test]
    fn test_equality() {
        let value1: u32 = 42;
        let value2: u32 = 42;
        let value3: u32 = 43;

        let any1 = AnyByLayout::from(&value1);
        let any2 = AnyByLayout::from(&value2);
        let any3 = AnyByLayout::from(&value3);

        assert_eq!(any1, any2);
        assert_ne!(any1, any3);
    }

    #[test]
    fn test_roundtrip_conversion() {
        let original: f64 = core::f64::consts::PI;
        let any = AnyByLayout::from(&original);
        let cast_back = any.cast::<f64>().unwrap();

        assert_eq!(*cast_back, original);
    }
}
