#[macro_export]
macro_rules! flags {
    () => {};

    // Entry point for enumerations without values
    ($(#[$attributes:meta])* $visibility:vis enum $identifier:ident: $t:ty { $($(#[$variant:meta])* $k:ident),+ $(,)* } $($next:tt)*) => {
        $crate::flags! {
            @count_and_gen $(#[$attributes])* $visibility enum $identifier: $t
            { } // accumulated variants
            [ $( ($(#[$variant])* $k) )+ ] // remaining to process
            [] // counter (empty = 0)
            $($next)*
        }
    };

    // Process each variant, incrementing the counter
    (@count_and_gen $(#[$attributes:meta])* $visibility:vis enum $identifier:ident: $t:ty
        { $($accumulated:tt)* }
        [ ($(#[$variant_meta:meta])* $current:ident) $(($($rest_items:tt)*))* ]
        [ $($counter:tt)* ]
        $($next:tt)*
    ) => {
        $crate::flags! {
            @count_and_gen $(#[$attributes])* $visibility enum $identifier: $t
            { $($accumulated)* $(#[$variant_meta])* $current = $crate::flags!(@bit_value [ $($counter)* ]), }
            [ $(($($rest_items)*))* ]
            [ $($counter)* + ] // increment counter
            $($next)*
        }
    };

    // When all variants are processed, generate the struct
    (@count_and_gen $(#[$attributes:meta])* $visibility:vis enum $identifier:ident: $t:ty
        { $($accumulated:tt)* }
        [ ]
        [ $($counter:tt)* ]
        $($next:tt)*
    ) => {
        $crate::flags! { $(#[$attributes])* $visibility enum $identifier: $t { $($accumulated)* } $($next)* }
    };

    // Convert counter tokens to bit shift value
    (@bit_value []) => { 1 << 0 };
    (@bit_value [+]) => { 1 << 1 };
    (@bit_value [+ +]) => { 1 << 2 };
    (@bit_value [+ + +]) => { 1 << 3 };
    (@bit_value [+ + + +]) => { 1 << 4 };
    (@bit_value [+ + + + +]) => { 1 << 5 };
    (@bit_value [+ + + + + +]) => { 1 << 6 };
    (@bit_value [+ + + + + + +]) => { 1 << 7 };
    (@bit_value [+ + + + + + + +]) => { 1 << 8 };
    (@bit_value [+ + + + + + + + +]) => { 1 << 9 };
    (@bit_value [+ + + + + + + + + +]) => { 1 << 10 };
    (@bit_value [+ + + + + + + + + + +]) => { 1 << 11 };
    (@bit_value [+ + + + + + + + + + + +]) => { 1 << 12 };
    (@bit_value [+ + + + + + + + + + + + +]) => { 1 << 13 };
    (@bit_value [+ + + + + + + + + + + + + +]) => { 1 << 14 };
    (@bit_value [+ + + + + + + + + + + + + + +]) => { 1 << 15 };
    (@bit_value [+ + + + + + + + + + + + + + + +]) => { 1 << 16 };
    (@bit_value [+ + + + + + + + + + + + + + + + +]) => { 1 << 17 };
    (@bit_value [+ + + + + + + + + + + + + + + + + +]) => { 1 << 18 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + +]) => { 1 << 19 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + +]) => { 1 << 20 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + +]) => { 1 << 21 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 22 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 23 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 24 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 25 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 26 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 27 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 28 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 29 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 30 };
    (@bit_value [+ + + + + + + + + + + + + + + + + + + + + + + + + + + + + + +]) => { 1 << 31 };

    // Entry point for enumerations with explicit values
    ($(#[$attributes:meta])* $visibility:vis enum $identifier:ident: $t:ty { $($(#[$variant:meta])*$k:ident = $v:expr),* $(,)* } $($next:tt)*) => {
        $(#[$attributes])*
        #[derive(Copy, Clone, PartialEq, Eq)]
        #[repr(transparent)]
        $visibility struct $identifier($t);

        impl $identifier {
            $(
                #[allow(non_upper_case_globals)]
                $(#[$variant])*
                $visibility const $k: Self = Self($v);
            )*

            #[allow(non_upper_case_globals)]
            $visibility const None: Self = Self(0);

            #[allow(non_upper_case_globals)]
            $visibility const All: Self = Self($( $v )|*);


            /// Checks if the flag set contains the specified flag(s)
            #[allow(dead_code)]
            $visibility const fn contains(&self, other: Self) -> bool {
                (self.0 & other.0) == other.0
            }

            /// Checks if the flag set contains any of the specified flag(s)
            #[allow(dead_code)]
            $visibility const fn intersects(&self, other: Self) -> bool {
                (self.0 & other.0) != 0
            }

            /// Inserts the specified flag(s) into the set
            #[allow(dead_code)]
            $visibility const fn insert(mut self, other: Self) -> Self {
                self.0 |= other.0;
                self
            }

            /// Removes the specified flag(s) from the set
            #[allow(dead_code)]
            $visibility const fn remove(mut self, other: Self) -> Self {
                self.0 &= !other.0;
                self
            }

            /// Toggles the specified flag(s) in the set
            #[allow(dead_code)]
            $visibility const fn toggle(mut self, other: Self) -> Self {
                self.0 ^= other.0;
                self
            }

            /// Sets or clears the specified flag(s) based on the passed value
            #[allow(dead_code)]
            $visibility const fn set(self, other: Self, value: bool) -> Self {
                if value {
                    self.insert(other)
                } else {
                    self.remove(other)
                }
            }

            /// Returns the intersection of the two flag sets
            #[allow(dead_code)]
            $visibility const fn intersection(self, other: Self) -> Self {
                Self(self.0 & other.0)
            }

            /// Returns the union of the two flag sets
            #[allow(dead_code)]
            $visibility const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            /// Returns the difference between the two flag sets
            $visibility const fn difference(self, other: Self) -> Self {
                Self(self.0 & !other.0)
            }

            /// Returns the symmetric difference between the two flag sets
            $visibility const fn symmetric_difference(self, other: Self) -> Self {
                Self(self.0 ^ other.0)
            }

            /// Returns the complement of the flag set
            $visibility const fn complement(self) -> Self {
                Self(!self.0)
            }

            /// Checks if the flag set is empty
            #[allow(dead_code)]
            $visibility const fn is_empty(&self) -> bool {
                self.0 == 0
            }

            /// Returns the raw value
            #[allow(dead_code)]
            $visibility const fn bits(&self) -> $t {
                self.0
            }

            /// Returns the number of bits required to represent all defined flags
            #[allow(dead_code)]
            $visibility const fn bits_used() -> u8 {
                let all_bits = Self::All.0;
                if all_bits == 0 {
                    0
                } else {
                    // Calculate the position of the highest set bit + 1
                    (core::mem::size_of::<$t>() * 8) as u8 - all_bits.leading_zeros() as u8
                }
            }

            /// Creates a flag set from raw bits
            #[allow(dead_code)]
            $visibility const fn from_bits(bits: $t) -> Option<Self> {
                let all_bits = Self::All.0;
                if (bits & !all_bits) == 0 {
                    Some(Self(bits))
                } else {
                    None
                }
            }

            /// Creates a flag set from raw bits, truncating any unknown bits
            #[allow(dead_code)]
            $visibility const fn from_bits_truncate(bits: $t) -> Self {
                let all_bits = Self::All.0;
                Self(bits & all_bits)
            }

            /// Creates a flag set from raw bits without checking validity
            #[allow(dead_code)]
            $visibility const unsafe fn from_bits_unchecked(bits: $t) -> Self {
                Self(bits)
            }

            /// Count the number of flags set in the flag set
            #[allow(dead_code)]
            $visibility const fn count_ones(&self) -> u32 {
                self.0.count_ones()
            }

        }

        impl core::fmt::Debug for $identifier {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let mut first = true;
                write!(f, "{} {{ ", stringify!($identifier))?;
                $(
                    if self.contains(Self::$k) {
                        if !core::mem::replace(&mut first, false) {
                            write!(f, " | ")?;
                        }
                        write!(f, "{}", stringify!($k))?;
                    }
                )*
                write!(f, " }}")
            }
        }

        impl core::ops::BitOr for $identifier {
            type Output = Self;

            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl core::ops::BitOrAssign for $identifier {
            fn bitor_assign(&mut self, other: Self) {
                self.insert(other);
            }
        }

        impl core::ops::BitAnd for $identifier {
            type Output = Self;

            fn bitand(self, other: Self) -> Self {
                self.intersection(other)
            }
        }

        impl core::ops::BitAndAssign for $identifier {
            fn bitand_assign(&mut self, other: Self) {
                *self = self.intersection(other);
            }
        }

        impl core::ops::BitXor for $identifier {
            type Output = Self;

            fn bitxor(self, other: Self) -> Self {
                self.symmetric_difference(other)
            }
        }

        impl core::ops::BitXorAssign for $identifier {
            fn bitxor_assign(&mut self, other: Self) {
                self.toggle(other);
            }
        }

        impl core::ops::Not for $identifier {
            type Output = Self;

            fn not(self) -> Self {
                self.complement()
            }
        }

        impl core::ops::Sub for $identifier {
            type Output = Self;

            fn sub(self, other: Self) -> Self {
                self.difference(other)
            }
        }

        impl core::ops::SubAssign for $identifier {
            fn sub_assign(&mut self, other: Self) {
                self.remove(other);
            }
        }

        $crate::flags! { $($next)* }
    };
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::format;

    flags! {
        pub enum TestFlags: u8 {
            FlagA,
            FlagB,
            FlagC,
        }
    }

    #[test]
    fn test_debug() {
        let flags = TestFlags::FlagA | TestFlags::FlagC;
        let debug_str = format!("{:?}", flags);
        assert_eq!(debug_str, "TestFlags { FlagA | FlagC }");
    }

    #[test]
    fn test_flagset_operations() {
        let flag_a = TestFlags::FlagA;
        let flag_b = TestFlags::FlagB;
        let flag_c = TestFlags::FlagC;

        // Test individual flags
        assert_eq!(flag_a.bits(), 1);
        assert_eq!(flag_b.bits(), 2);
        assert_eq!(flag_c.bits(), 4);

        // Test empty and all
        let empty = TestFlags::None;
        assert!(empty.is_empty());
        assert_eq!(empty.bits(), 0);

        let all = TestFlags::All;
        assert_eq!(all.bits(), 7); // 1 | 2 | 4
        assert!(!all.is_empty());

        // Test contains
        let mut flags = TestFlags::None;
        assert!(!flags.contains(flag_a));

        flags = flags.insert(flag_a);
        assert!(flags.contains(flag_a));
        assert!(!flags.contains(flag_b));

        // Test union/intersection
        let ab = flag_a | flag_b;
        assert!(ab.contains(flag_a));
        assert!(ab.contains(flag_b));
        assert!(!ab.contains(flag_c));

        // Test set
        let mut flags = TestFlags::None;
        flags = flags.set(flag_a, true);
        assert!(flags.contains(flag_a));
        flags = flags.set(flag_a, false);
        assert!(!flags.contains(flag_a));

        // Test remove
        let mut flags = flag_a | flag_b;
        flags = flags.remove(flag_a);
        assert!(!flags.contains(flag_a));
        assert!(flags.contains(flag_b));

        // Test toggle
        let mut flags = flag_a;
        flags = flags.toggle(flag_b);
        assert!(flags.contains(flag_a));
        assert!(flags.contains(flag_b));
        flags = flags.toggle(flag_a);
        assert!(!flags.contains(flag_a));
        assert!(flags.contains(flag_b));

        // Test intersects
        let flags1 = flag_a | flag_b;
        let flags2 = flag_b | flag_c;
        assert!(flags1.intersects(flags2));
        assert!(!flag_a.intersects(flag_c));

        // Test set method with conditional flag setting
        let mut flags = TestFlags::None;
        flags = flags.set(flag_a, true);
        assert!(flags.contains(flag_a));
        flags = flags.set(flag_b, false);
        assert!(!flags.contains(flag_b));
        flags = flags.set(flag_c, true);
        assert!(flags.contains(flag_c));

        // Test bits_used - with 3 flags (bits 0, 1, 2), we need 3 bits
        assert_eq!(TestFlags::bits_used(), 3);
    }

    flags! {
        pub enum CustomFlags: u16 {
            Read = 0b0001,
            Write = 0b0010,
            Execute = 0b0100,
            Admin = 0b1000,
        }
    }

    #[test]
    fn test_debug_custom() {
        let flags = CustomFlags::Read | CustomFlags::Execute;
        let debug_str = format!("{:?}", flags);
        assert_eq!(debug_str, "CustomFlags { Read | Execute }");

        let no_flags = CustomFlags::None;
        let debug_str_no_flags = format!("{:?}", no_flags);
        assert_eq!(debug_str_no_flags, "CustomFlags {  }");

        let all_flags = CustomFlags::All;
        let debug_str_all_flags = format!("{:?}", all_flags);
        assert_eq!(
            debug_str_all_flags,
            "CustomFlags { Read | Write | Execute | Admin }"
        );
    }

    #[test]
    fn test_explicit_values() {
        let read = CustomFlags::Read;
        let write = CustomFlags::Write;
        let execute = CustomFlags::Execute;
        let admin = CustomFlags::Admin;

        // Test explicit bit values
        assert_eq!(read.bits(), 0b0001);
        assert_eq!(write.bits(), 0b0010);
        assert_eq!(execute.bits(), 0b0100);
        assert_eq!(admin.bits(), 0b1000);

        // Test combinations
        let read_write = read | write;
        assert!(read_write.contains(read));
        assert!(read_write.contains(write));
        assert!(!read_write.contains(execute));

        // Test all permissions
        let all_perms = read | write | execute | admin;
        assert_eq!(all_perms.bits(), 0b1111);
        assert_eq!(all_perms, CustomFlags::All);

        // Test from_bits
        assert_eq!(CustomFlags::from_bits(0b0011), Some(read | write));
        assert_eq!(CustomFlags::from_bits(0b10000), None); // Invalid bit

        // Test from_bits_truncate
        assert_eq!(CustomFlags::from_bits_truncate(0b10011), read | write); // Truncates invalid bit

        // Test bits_used - with 4 flags at positions 0-3, we need 4 bits
        assert_eq!(CustomFlags::bits_used(), 4);
    }

    // Compatibility baseline for a potential bitflags 2 migration (#347).
    // These tests intentionally exercise behavior that differs from bitflags'
    // defaults or is relied upon by callers across Xila.
    #[test]
    fn compatibility_layout_and_unknown_bits() {
        assert_eq!(core::mem::size_of::<TestFlags>(), core::mem::size_of::<u8>());
        assert_eq!(core::mem::align_of::<TestFlags>(), core::mem::align_of::<u8>());
        assert_eq!(core::mem::size_of::<CustomFlags>(), core::mem::size_of::<u16>());
        assert_eq!(core::mem::align_of::<CustomFlags>(), core::mem::align_of::<u16>());

        assert_eq!(TestFlags::None.bits(), 0);
        assert_eq!(TestFlags::All.bits(), 0b111);
        assert_eq!(TestFlags::from_bits(0b1000), None);
        assert_eq!(TestFlags::from_bits_truncate(0b1001).bits(), 1);

        // The current complement retains unknown bits; bitflags' ! operator
        // normally restricts its result to defined flags.
        assert_eq!((!TestFlags::FlagA).bits(), !1u8);
        let raw = unsafe { TestFlags::from_bits_unchecked(0b1000) };
        assert_eq!(raw.bits(), 0b1000);
        assert_eq!(raw.intersection(TestFlags::All).bits(), 0);
        assert_eq!(raw.union(TestFlags::FlagA).bits(), 0b1001);
    }

    #[test]
    fn compatibility_const_api() {
        const FLAGS: TestFlags = TestFlags::None
            .insert(TestFlags::FlagA)
            .set(TestFlags::FlagB, true)
            .toggle(TestFlags::FlagA);
        const REMAINING: TestFlags = FLAGS.difference(TestFlags::FlagA);
        const USED: u8 = TestFlags::bits_used();
        assert_eq!(FLAGS.bits(), 0b010);
        assert_eq!(REMAINING.bits(), 0b010);
        assert_eq!(USED, 3);
        assert_eq!(FLAGS.count_ones(), 1);
    }

}
