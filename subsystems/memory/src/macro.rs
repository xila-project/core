/// Macro to instantiate a global allocator using a Xila memory allocator.
///
/// This macro creates a static global allocator using the provided expression and
/// applies the `#[global_allocator]` attribute to it. This is the recommended way
/// to set up the global allocator in applications using Xila memory management.
#[macro_export]
macro_rules! instantiate_global_allocator {
    ($allocator:expr) => {
        #[global_allocator]
        #[unsafe(no_mangle)]
        pub static __XILA_MEMORY_MANAGER: $crate::Manager = $crate::Manager::new($allocator);
    };
}
