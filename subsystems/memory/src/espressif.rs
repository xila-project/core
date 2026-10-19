use esp_idf_sys::{heap_caps_aligned_alloc, heap_caps_free, MALLOC_CAP_8BIT, MALLOC_CAP_EXEC};

use crate::{Flags, Layout_type, Memory_allocator_trait, Protection_type};
use core::ptr::NonNull;
use std::os::raw::c_void;

pub struct Memory_allocator_type;

impl Memory_allocator_type {
    pub const fn New() -> Self {
        Memory_allocator_type {}
    }
}

impl Memory_allocator_trait for Memory_allocator_type {
    unsafe fn Allocate_custom(
        &self,
        _: Option<NonNull<u8>>,
        Layout: Layout_type,
        Protection: Protection_type,
        _: Flags,
    ) -> Option<NonNull<u8>> {
        let _Caps = if Protection.get_execute() {
            MALLOC_CAP_EXEC
        } else {
            MALLOC_CAP_8BIT
        };

        let _Caps = esp_idf_sys::MALLOC_CAP_SPIRAM;

        let Address =
            heap_caps_aligned_alloc(Layout.get_alignment() as usize, Layout.get_size(), _Caps);

        if Address.is_null() {
            None
        } else {
            Some(NonNull::new_unchecked(Address as *mut u8))
        }
    }

    unsafe fn Deallocate_custom(&self, Address: NonNull<u8>, _: usize) -> bool {
        heap_caps_free(Address.as_ptr() as *mut c_void);

        true
    }

    unsafe fn Protect(&self, _: NonNull<u8>, _: usize, _: Protection_type) -> bool {
        true
    }

    fn get_page_size(&self) -> usize {
        4 * 1024
    }

    fn Flush_data_cache(&self) {
        Flush_data_cache()
    }
}

static Mutex: std::sync::Mutex<()> = std::sync::Mutex::new(());

extern "C" {
    fn Cache_WriteBack_All();
    fn Cache_Disable_ICache() -> u32;
    fn Cache_Enable_ICache(Preload: u32);
}

#[link_section = ".iram1"]
pub fn Flush_data_cache() {
    {
        let _Guard = Mutex
            .lock()
            .expect("Failed to lock mutex for data cache flush");

        unsafe {
            Cache_WriteBack_All();
            let Preload = Cache_Disable_ICache();
            Cache_Enable_ICache(Preload);
        }
    }
}
