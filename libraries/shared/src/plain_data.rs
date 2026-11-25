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
/// A `PlainData` type must also have harmless drop glue: writing a payload over
/// an existing one (`*output = ..`) drops the bytes the guest supplied, so a
/// type with a `Drop` implementation (for example a password buffer that
/// zeroises itself on drop, like `WifiPassword`) must not implement
/// `PlainData`.
///
/// The integer and float primitives, `()` and arrays of plain data implement
/// `PlainData`. Use [`plain_data!`](crate::plain_data) for `#[repr(C)]`
/// structures: it implements `PlainData` only when every field type is itself
/// plain data and the structure has no padding, which it checks at compile
/// time.
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
/// [`PlainData`](crate::PlainData), and the structure must have no padding
/// bytes; both are checked at compile time.
///
/// Two forms are accepted:
///
/// - a named-field structure, which must be `#[repr(C)]` so that the guest and
///   the host agree on the layout;
/// - a tuple structure, which must be `#[repr(C)]`, or `#[repr(transparent)]`
///   when it has a single field.
///
/// Other attributes (doc comments, `#[derive(..)]`, ...) may appear before or
/// after the representation attribute. `#[repr(packed)]` on its own matches no
/// form; combined with the `#[repr(C)]` the macro adds, rustc accepts the pair,
/// so the macro rejects packing with a compile-time alignment check instead (a
/// packed structure would expose misaligned field references).
///
/// # Safety contract
///
/// The structure must be `#[repr(C)]` or `#[repr(transparent)]` as required by
/// each form, and every field type must be plain data. Padding bytes of a
/// `#[repr(C)]` structure are never read as values: derived trait
/// implementations compare or format fields, not padding. Because the
/// padding check requires `size_of::<Self>()` to equal the sum of the field
/// sizes, a `plain_data!` structure never has padding, so writing a payload
/// never copies uninitialised memory across the host/guest boundary.
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
///         pub version: u32,
///         pub enabled: u8,
///         pub reserved: [u8; 3],
///     }
/// }
///
/// // Other attributes may also come first.
/// plain_data! {
///     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
///     #[repr(C)]
///     pub struct Pair {
///         pub first: u16,
///         pub second: u16,
///     }
/// }
///
/// plain_data! {
///     #[repr(C)]
///     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
///     pub struct TuplePair(pub u16, pub u16);
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
/// ```compile_fail,E0277
/// use shared::plain_data;
///
/// plain_data! {
///     #[repr(C)]
///     pub struct Bad {
///         pub flag: bool,
///     }
/// }
/// ```
///
/// A structure with padding does not compile either:
///
/// ```compile_fail,E0080
/// use shared::plain_data;
///
/// plain_data! {
///     #[repr(C)]
///     pub struct Padded {
///         pub a: u8,
///         pub b: u32,
///     }
/// }
/// ```
///
/// A packed structure does not compile (rustc accepts `#[repr(C)]` combined
/// with `#[repr(packed)]`, so the macro rejects it with an alignment check):
///
/// ```compile_fail,E0080
/// use shared::plain_data;
///
/// plain_data! {
///     #[repr(C)]
///     #[repr(packed)]
///     pub struct Packed {
///         pub a: u8,
///         pub b: u32,
///     }
/// }
/// ```
#[macro_export]
macro_rules! plain_data {
    // Named-field structure; `#[repr(C)]` is mandatory. Attributes before it
    // were collected by the muncher arms below and are re-emitted first.
    (
        @munch [$($acc:tt)*]
        #[repr(C)]
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $field_ty:ty
            ),* $(,)?
        }
    ) => {
        $($acc)*
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
            // Compile-time check: no padding bytes, so a payload write never
            // copies uninitialised memory.
            assert!(
                core::mem::size_of::<$name>()
                    == 0 $(+ core::mem::size_of::<$field_ty>())*,
                "plain_data! structure has padding bytes"
            );
            // Compile-time check: not packed. A packed structure has alignment
            // 1, so its fields could be referenced misaligned.
            let mut required_alignment = 1usize;
            $(
                if core::mem::align_of::<$field_ty>() > required_alignment {
                    required_alignment = core::mem::align_of::<$field_ty>();
                }
            )*
            assert!(
                core::mem::align_of::<$name>() == required_alignment,
                "plain_data! structure must not be packed"
            );
        };

        // SAFETY: the structure is `#[repr(C)]`, every field type implements
        // `PlainData` (checked above) and the structure has no padding (checked
        // above), so every bit pattern is a valid value and no padding byte is
        // ever read as a value.
        unsafe impl $crate::PlainData for $name {}
    };
    // Tuple structure with `#[repr(C)]`; every field must be plain data.
    (
        @munch [$($acc:tt)*]
        #[repr(C)]
        $(#[$meta:meta])*
        $vis:vis struct $name:ident (
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field_ty:ty
            ),+ $(,)?
        ) $(;)?
    ) => {
        $($acc)*
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
            // Compile-time check: no padding bytes.
            assert!(
                core::mem::size_of::<$name>()
                    == 0 $(+ core::mem::size_of::<$field_ty>())+,
                "plain_data! structure has padding bytes"
            );
            // Compile-time check: not packed (see the named-field arm).
            let mut required_alignment = 1usize;
            $(
                if core::mem::align_of::<$field_ty>() > required_alignment {
                    required_alignment = core::mem::align_of::<$field_ty>();
                }
            )+
            assert!(
                core::mem::align_of::<$name>() == required_alignment,
                "plain_data! structure must not be packed"
            );
        };

        // SAFETY: the structure is `#[repr(C)]`, every field type implements
        // `PlainData` (checked above) and the structure has no padding (checked
        // above), so every bit pattern is a valid value.
        unsafe impl $crate::PlainData for $name {}
    };
    // Single-field tuple structure; `#[repr(transparent)]` is mandatory.
    (
        @munch [$($acc:tt)*]
        #[repr(transparent)]
        $(#[$meta:meta])*
        $vis:vis struct $name:ident (
            $(#[$field_meta:meta])*
            $field_vis:vis $field_ty:ty
        ) $(;)?
    ) => {
        $($acc)*
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
            // Compile-time check: `repr(transparent)` has no padding.
            assert!(
                core::mem::size_of::<$name>() == core::mem::size_of::<$field_ty>(),
                "plain_data! structure has padding bytes"
            );
            assert!(
                core::mem::align_of::<$name>() == core::mem::align_of::<$field_ty>(),
                "plain_data! structure must not be packed"
            );
        };

        // SAFETY: the structure is `#[repr(transparent)]` over a type that
        // implements `PlainData` (checked above), so it has the same layout
        // and every bit pattern is a valid value.
        unsafe impl $crate::PlainData for $name {}
    };
    // Munch one leading attribute that is not the representation (a doc
    // comment, `#[derive(..)]`, ...) and retry, so attributes may appear
    // before `#[repr(C)]` / `#[repr(transparent)]`.
    (
        @munch [$($acc:tt)*]
        #[$attr:meta] $($rest:tt)*
    ) => {
        $crate::plain_data!(@munch [$($acc)* #[$attr]] $($rest)*);
    };
    // No representation attribute found.
    (
        @munch [$($acc:tt)*]
        $($rest:tt)*
    ) => {
        compile_error!("plain_data! requires #[repr(C)] or #[repr(transparent)]");
    };
    ($($tokens:tt)*) => {
        $crate::plain_data!(@munch [] $($tokens)*);
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
            reserved: u8,
        }
    }

    plain_data! {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Example {
            nested: Nested,
            array: [u16; 4],
            version: u32,
        }
    }

    // Attributes before the representation attribute are accepted too.
    plain_data! {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        #[repr(C)]
        struct DerivedFirst {
            a: u32,
            b: u16,
            reserved: [u8; 2],
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

    #[test]
    fn plain_data_structs_have_no_padding() {
        // The macro already rejects padding at compile time; these assertions
        // pin the exact layouts the host and the guests agree on.
        assert_eq!(
            core::mem::size_of::<Nested>(),
            core::mem::size_of::<u32>()
                + core::mem::size_of::<[u8; 3]>()
                + core::mem::size_of::<u8>()
        );
        assert_eq!(core::mem::size_of::<DerivedFirst>(), 8);
        assert_eq!(
            core::mem::size_of::<Example>(),
            core::mem::size_of::<Nested>()
                + core::mem::size_of::<[u16; 4]>()
                + core::mem::size_of::<u32>()
        );
    }
}
