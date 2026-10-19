#[macro_export]
macro_rules! generate_shadow_type {
    ($proxy_name:ident, $real_type:ty) => {
        /// Opaque shadow type generated for FFI pointer boundaries.
        #[repr(C)]
        pub struct $proxy_name {
            _private: [u8; 0], // Zero-sized type to prevent C instantiation but safe for raw pointers
        }

        impl $proxy_name {
            /// Converts a real allocated instance into an opaque raw pointer handle.
            ///
            /// Note: This moves the real object onto the heap via Box to ensure its
            /// memory remains valid after this function returns.
            pub const fn from_real(real: &mut $real_type) -> *mut Self {
                real as *mut $real_type as *mut Self
            }

            /// Consumes the opaque raw pointer handle and returns the original real type,
            /// reclaiming ownership and freeing the underlying heap allocation.
            ///
            /// # Safety
            /// The pointer must be non-null and must have been created by `from_real`.
            #[inline]
            pub unsafe fn into_real(ptr: *mut Self) -> *mut $real_type {
                ptr as *mut $real_type
            }

            /// Temporarily borrows the real type immutably from the opaque pointer.
            ///
            /// # Safety
            /// The pointer must be valid and dereferenceable for the duration of life 'a.
            #[inline]
            pub unsafe fn as_real<'a>(ptr: *const Self) -> &'a $real_type {
                unsafe { &*(ptr as *const $real_type) }
            }

            /// Temporarily borrows the real type mutably from the opaque pointer.
            ///
            /// # Safety
            /// The pointer must be valid, dereferenceable, and unaliased for the duration of life 'a.
            #[inline]
            pub unsafe fn as_real_mut<'a>(ptr: *mut Self) -> &'a mut $real_type {
                unsafe { &mut *(ptr as *mut $real_type) }
            }
        }

        impl core::ops::Deref for $proxy_name {
            type Target = $real_type;

            #[inline]
            fn deref(&self) -> &Self::Target {
                unsafe { Self::as_real(self as *const Self) }
            }
        }

        impl core::ops::DerefMut for $proxy_name {
            #[inline]
            fn deref_mut(&mut self) -> &mut Self::Target {
                unsafe { Self::as_real_mut(self as *mut Self) }
            }
        }

        impl AsRef<$real_type> for $proxy_name {
            #[inline]
            fn as_ref(&self) -> &$real_type {
                unsafe { Self::as_real(self as *const Self) }
            }
        }

        impl AsMut<$real_type> for $proxy_name {
            #[inline]
            fn as_mut(&mut self) -> &mut $real_type {
                unsafe { Self::as_real_mut(self as *mut Self) }
            }
        }
    };
}
