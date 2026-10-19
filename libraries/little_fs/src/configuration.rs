use core::{ffi::c_void, ptr::null_mut};

use alloc::boxed::Box;
use file_system::DirectBlockDevice;

use super::{callbacks, littlefs};

#[derive(Clone)]
pub struct Configuration {
    context: &'static dyn DirectBlockDevice,
    read_size: usize,
    program_size: usize,
    block_size: usize,
    block_count: usize,
    block_cycles: Option<u16>,
    maximum_file_size: Option<usize>,
    maximum_attributes_size: Option<usize>,
    maximum_name_size: Option<usize>,
    cache_size: usize,
    look_ahead_size: usize,
    compact_threshold: Option<usize>,
    metadata_maxium: Option<usize>,
    inline_maximum: Option<usize>,
}

impl Configuration {
    pub const DEFAULT_RAW: littlefs::lfs_config = littlefs::lfs_config {
        context: core::ptr::null_mut::<c_void>(),
        read: None,
        prog: None,
        erase: None,
        sync: None,
        read_size: 0,
        prog_size: 0,
        block_size: 0,
        block_count: 0,
        block_cycles: 0,
        cache_size: 0,
        lookahead_size: 0,
        read_buffer: core::ptr::null_mut::<c_void>(),
        prog_buffer: core::ptr::null_mut::<c_void>(),
        lookahead_buffer: core::ptr::null_mut::<c_void>(),
        name_max: 0,
        file_max: 0,
        attr_max: 0,
        compact_thresh: 0,
        metadata_max: 0,
        inline_max: 0,
    };

    pub fn new(
        device: &'static dyn DirectBlockDevice,
        block_size: usize,
        block_count: usize,
        cache_size: usize,
        look_ahead_size: usize,
    ) -> Option<Self> {
        if !(block_size % cache_size) == 0 {
            return None;
        }

        if !look_ahead_size.is_multiple_of(8) {
            return None;
        }

        Some(Self {
            context: device,
            read_size: 16,
            program_size: 16,
            block_size,
            block_count,
            block_cycles: None,
            maximum_file_size: None,
            maximum_attributes_size: None,
            maximum_name_size: None,
            cache_size,
            look_ahead_size,
            compact_threshold: None,
            metadata_maxium: None,
            inline_maximum: None,
        })
    }

    pub const fn set_read_size(mut self, read_size: usize) -> Self {
        self.read_size = read_size;
        self
    }

    pub const fn set_program_size(mut self, program_size: usize) -> Self {
        self.program_size = program_size;
        self
    }

    pub const fn set_block_cycles(mut self, block_cycles: u16) -> Self {
        self.block_cycles = Some(block_cycles);
        self
    }

    pub const fn set_maximum_file_size(mut self, maximum_file_size: usize) -> Self {
        self.maximum_file_size = Some(maximum_file_size);
        self
    }

    pub const fn set_maximum_attributes_size(mut self, maximum_attributes_size: usize) -> Self {
        self.maximum_attributes_size = Some(maximum_attributes_size);
        self
    }

    pub const fn set_maximum_name_size(mut self, maximum_name_size: usize) -> Self {
        self.maximum_name_size = Some(maximum_name_size);
        self
    }

    pub const fn set_cache_size(mut self, cache_size: usize) -> Self {
        self.cache_size = cache_size;
        self
    }

    pub const fn set_look_ahead_size(mut self, look_ahead_size: usize) -> Self {
        self.look_ahead_size = look_ahead_size;
        self
    }

    pub const fn set_compact_threshold(mut self, compact_threshold: usize) -> Self {
        self.compact_threshold = Some(compact_threshold);

        self
    }

    pub const fn set_metadata_maximum(mut self, metadata_maxium: usize) -> Self {
        self.metadata_maxium = Some(metadata_maxium);
        self
    }

    pub const fn set_inline_maximum(mut self, inline_maximum: usize) -> Self {
        self.inline_maximum = Some(inline_maximum);
        self
    }
}

impl TryFrom<Configuration> for littlefs::lfs_config {
    type Error = ();

    fn try_from(configuration: Configuration) -> Result<Self, Self::Error> {
        // Allocate buffers on the heap and leak them so littlefs can use them for the lifetime of the filesystem

        let context = Context::new(configuration.context);

        let lfs_configuration = littlefs::lfs_config {
            context: context as *mut _ as *mut _,
            read: Some(callbacks::read_callback),
            prog: Some(callbacks::programm_callback),
            erase: Some(callbacks::erase_callback),
            sync: Some(callbacks::flush_callback),
            read_size: configuration.read_size as u32,
            prog_size: configuration.program_size as u32,
            block_size: configuration.block_size as u32,
            block_count: configuration.block_count as u32,
            block_cycles: match configuration.block_cycles {
                Some(block_cycles) => block_cycles as i32,
                None => -1,
            },
            cache_size: configuration.cache_size as u32,
            lookahead_size: configuration.look_ahead_size as u32,
            read_buffer: null_mut(),
            prog_buffer: null_mut(),
            lookahead_buffer: null_mut(),
            name_max: configuration.maximum_name_size.unwrap_or(0) as u32, // Default value : 255 (LFS_NAME_MAX)
            file_max: configuration.maximum_file_size.unwrap_or(0) as u32, // Default value : 2,147,483,647 (2 GiB) (LFS_FILE_MAX)
            attr_max: configuration.maximum_attributes_size.unwrap_or(0) as u32, // Default value : 1022 (LFS_ATTR_MAX)
            compact_thresh: configuration.compact_threshold.unwrap_or(0) as u32, // Default value : 0 (LFS_COMPACT_THRESH)
            metadata_max: configuration.metadata_maxium.unwrap_or(0) as u32, // Default value : 0 (LFS_METADATA_MAX)
            inline_max: configuration.inline_maximum.unwrap_or(0) as u32, // Default value : 0 (LFS_INLINE_MAX)
        };

        Ok(lfs_configuration)
    }
}

pub struct Context {
    pub device: &'static dyn DirectBlockDevice,
}

impl Context {
    pub fn new(device: &'static dyn DirectBlockDevice) -> &'static mut Self {
        Box::leak(Box::new(Self { device }))
    }

    pub unsafe fn get_from_configuration(
        configuration: *const littlefs::lfs_config,
    ) -> &'static Self {
        unsafe { &*((*configuration).context as *const Self) }
    }

    #[allow(clippy::redundant_allocation)]
    pub unsafe fn take_from_configuration(
        configuration: *mut littlefs::lfs_config,
    ) -> Box<&'static dyn DirectBlockDevice> {
        unsafe {
            let raw_context = (*configuration).context as *mut _;

            (*configuration).context = null_mut();

            Box::from_raw(raw_context)
        }
    }
}
