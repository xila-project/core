//! Plain data: types for which every bit pattern is a valid value.
//!
//! Control command payloads cross the host/guest boundary as raw bytes, so a
//! payload type must accept every byte pattern a guest can send. [`PlainData`]
//! is that marker, in the style of `bytemuck::AnyBitPattern` or
//! `zerocopy::FromBytes`.

/// Marker trait for plain data: a type for which every bit pattern of
/// `size_of::<Self>()` bytes is a valid value.
///
/// # Safety
///
/// A type may implement `PlainData` only when every bit pattern of its size is
/// a valid value of the type. In particular, a `PlainData` type must not
/// contain `bool`, `char`, enum, reference or pointer fields, and code must not
/// read padding bytes of a `PlainData` structure as values.
///
/// The integer and float primitives, `()` and arrays of plain data implement
/// `PlainData`. Use [`plain_data!`](crate::plain_data) for `#[repr(C)]`
/// structures: it implements `PlainData` only when every field type is itself
/// plain data, which it checks at compile time.
pub unsafe trait PlainData: Sized {}

unsafe impl PlainData for u8 {}
unsafe impl PlainData for u16 {}
unsafe impl PlainData for u32 {}
unsafe impl PlainData for u64 {}
unsafe impl PlainData for u128 {}
unsafe impl PlainData for usize {}
unsafe impl PlainData for i8 {}
unsafe impl PlainData for i16 {}
unsafe impl PlainData for i32 {}
unsafe impl PlainData for i64 {}
unsafe impl PlainData for i128 {}
unsafe impl PlainData for isize {}
// Every bit pattern is a valid `f32`/`f64` (NaN payloads included).
unsafe impl PlainData for f32 {}
unsafe impl PlainData for f64 {}
unsafe impl PlainData for () {}

unsafe impl<T: PlainData, const N: usize> PlainData for [T; N] {}

/// Declares a plain-data structure and implements [`PlainData`](crate::PlainData)
/// for it.
///
/// The macro emits the structure itself, so the checked field list cannot drift
/// from the declaration. Every field type must implement
/// [`PlainData`](crate::PlainData); otherwise the generated compile-time check
/// fails to compile.
///
/// Two forms are accepted:
///
/// - a named-field structure, which must be `#[repr(C)]` so that the guest and
///   the host agree on the layout;
/// - a tuple structure, which must be `#[repr(C)]`, or `#[repr(transparent)]`
///   when it has a single field.
///
/// # Safety contract
///
/// The structure must be `#[repr(C)]` or `#[repr(transparent)]` as required by
/// each form, and every field type must be plain data. Padding bytes of a
/// `#[repr(C)]` structure are never read as values: derived trait
/// implementations compare or format fields, not padding.
///
/// # Examples
///
/// ```
/// use shared::plain_data;
///
/// plain_data! {
///     #[repr(C)]
///     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
///     pub struct Example {
///         pub version: u16,
///         pub enabled: u8,
///         pub reserved: [u8; 2],
///     }
/// }
///
/// plain_data! {
///     #[repr(C)]
///     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
///     pub struct Pair(pub u16, pub u16);
/// }
///
/// plain_data! {
///     #[repr(transparent)]
///     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
///     pub struct Wrapper(pub u8);
/// }
/// ```
///
/// A field that is not plain data does not compile:
///
/// ```compile_fail
/// use shared::plain_data;
///
/// plain_data! {
///     #[repr(C)]
///     pub struct Bad {
///         pub flag: bool,
///     }
/// }
/// ```
#[macro_export]
macro_rules! plain_data {
    // Named-field structure; `#[repr(C)]` is mandatory and matched literally.
    (
        #[repr(C)]
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $field_ty:ty
            ),* $(,)?
        }
    ) => {
        #[repr(C)]
        $(#[$meta])*
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field : $field_ty
            ),*
        }

        const _: () = {
            // Compile-time check: every field type accepts every bit pattern.
            #[allow(dead_code)]
            fn require_plain_data<T: $crate::PlainData>() {}
            $(
                let _: fn() = require_plain_data::<$field_ty>;
            )*
        };

        // SAFETY: the structure is `#[repr(C)]` and every field type
        // implements `PlainData` (checked above), so every bit pattern is a
        // valid value. Padding bytes are never read as values.
        unsafe impl $crate::PlainData for $name {}
    };
    // Tuple structure with `#[repr(C)]`; every field must be plain data.
    (
        #[repr(C)]
        $(#[$meta:meta])*
        $vis:vis struct $name:ident (
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field_ty:ty
            ),+ $(,)?
        ) $(;)?
    ) => {
        #[repr(C)]
        $(#[$meta])*
        $vis struct $name(
            $(
                $(#[$field_meta])*
                $field_vis $field_ty
            ),+
        );

        const _: () = {
            // Compile-time check: every field type accepts every bit pattern.
            #[allow(dead_code)]
            fn require_plain_data<T: $crate::PlainData>() {}
            $(
                let _: fn() = require_plain_data::<$field_ty>;
            )+
        };

        // SAFETY: the structure is `#[repr(C)]` and every field type
        // implements `PlainData` (checked above), so every bit pattern is a
        // valid value. Padding bytes are never read as values.
        unsafe impl $crate::PlainData for $name {}
    };
    // Single-field tuple structure; `#[repr(transparent)]` is mandatory and
    // matched literally.
    (
        #[repr(transparent)]
        $(#[$meta:meta])*
        $vis:vis struct $name:ident (
            $(#[$field_meta:meta])*
            $field_vis:vis $field_ty:ty
        ) $(;)?
    ) => {
        #[repr(transparent)]
        $(#[$meta])*
        $vis struct $name(
            $(#[$field_meta])*
            $field_vis $field_ty
        );

        const _: () = {
            // Compile-time check: the wrapped type accepts every bit pattern.
            #[allow(dead_code)]
            fn require_plain_data<T: $crate::PlainData>() {}
            let _: fn() = require_plain_data::<$field_ty>;
        };

        // SAFETY: the structure is `#[repr(transparent)]` over a type that
        // implements `PlainData` (checked above), so it has the same layout
        // and every bit pattern is a valid value.
        unsafe impl $crate::PlainData for $name {}
    };
}

#[cfg(test)]
mod tests {
    plain_data! {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Nested {
            a: u32,
            b: [u8; 3],
        }
    }

    plain_data! {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Example {
            version: u16,
            nested: Nested,
            array: [u16; 4],
        }
    }

    plain_data! {
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Wrapper(pub u8);
    }

    #[test]
    fn plain_data_structs_have_the_declared_layout() {
        assert_eq!(core::mem::size_of::<Example>(), 20);
        assert_eq!(core::mem::align_of::<Example>(), 4);
        assert_eq!(core::mem::size_of::<Wrapper>(), 1);
        assert_eq!(core::mem::size_of::<[u8; 0]>(), 0);
    }
}
