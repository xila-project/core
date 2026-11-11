//! Aligned, owned byte storage for neural network models.
use alloc::alloc::{Layout, alloc, alloc_zeroed, dealloc, realloc};
use core::ptr::NonNull;
use file_system::{Error, Result};

/// An owned byte buffer guaranteed to be 16-byte aligned.
///
/// TensorFlow Lite FlatBuffer models require 16-byte alignment to enable zero-copy
/// parsing and memory mapping of weights. This type wraps heap storage allocated
/// with [`ModelBuffer::ALIGNMENT`] alignment.
pub struct ModelBuffer {
    ptr: NonNull<u8>,
    len: usize,
    capacity: usize,
}

// SAFETY: `ModelBuffer` owns its allocated memory uniquely like a `Vec<u8>`.
unsafe impl Send for ModelBuffer {}
// SAFETY: Shared immutable access provides only standard `&[u8]` slices.
unsafe impl Sync for ModelBuffer {}

impl ModelBuffer {
    /// Required memory alignment in bytes (16-byte boundary).
    pub const ALIGNMENT: usize = 16;

    /// Create a new, empty model buffer with no heap allocation.
    pub const fn new() -> Self {
        Self {
            ptr: NonNull::<[u8; Self::ALIGNMENT]>::dangling().cast(),
            len: 0,
            capacity: 0,
        }
    }

    /// Allocate an uninitialized model buffer with at least `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Ok(Self::new());
        }
        let aligned_capacity = Self::align_capacity(capacity)?;
        let layout = Layout::from_size_align(aligned_capacity, Self::ALIGNMENT)
            .map_err(|_| Error::NoMemory)?;
        // SAFETY: `aligned_capacity` is non-zero and alignment is valid.
        let raw = unsafe { alloc(layout) };
        let ptr = NonNull::new(raw).ok_or(Error::NoMemory)?;
        Ok(Self {
            ptr,
            len: 0,
            capacity: aligned_capacity,
        })
    }

    /// Allocate a zero-filled model buffer with exact length `len`.
    pub fn new_zeroed(len: usize) -> Result<Self> {
        if len == 0 {
            return Ok(Self::new());
        }
        let aligned_capacity = Self::align_capacity(len)?;
        let layout = Layout::from_size_align(aligned_capacity, Self::ALIGNMENT)
            .map_err(|_| Error::NoMemory)?;
        // SAFETY: `aligned_capacity` is non-zero and alignment is valid.
        let raw = unsafe { alloc_zeroed(layout) };
        let ptr = NonNull::new(raw).ok_or(Error::NoMemory)?;
        Ok(Self {
            ptr,
            len,
            capacity: aligned_capacity,
        })
    }

    /// Allocate a 16-byte-aligned model buffer and copy `slice` into it.
    pub fn try_from_slice(slice: &[u8]) -> Result<Self> {
        let mut buffer = Self::with_capacity(slice.len())?;
        if !slice.is_empty() {
            // SAFETY: buffer was allocated with at least `slice.len()` bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(slice.as_ptr(), buffer.ptr.as_ptr(), slice.len());
            }
            buffer.len = slice.len();
        }
        Ok(buffer)
    }

    /// Number of bytes currently stored in the buffer.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer contains zero bytes.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Allocated storage capacity in bytes.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Immutable view of the model buffer.
    pub fn as_slice(&self) -> &[u8] {
        if self.len == 0 {
            &[]
        } else {
            // SAFETY: `self.ptr` points to `self.len` initialized bytes.
            unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Mutable view of the model buffer.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        if self.len == 0 {
            &mut []
        } else {
            // SAFETY: `self.ptr` points to `self.len` initialized bytes with unique access.
            unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Raw pointer to the start of the buffer.
    pub const fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Raw mutable pointer to the start of the buffer.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Write bytes at `offset`.
    ///
    /// The write position must be contiguous from previous writes (`offset <= self.len()`).
    /// The buffer expands automatically if `offset + data.len() > self.capacity()`.
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        self.write_at_within(offset, data, usize::MAX)
    }

    /// Like [`Self::write_at`], but never grows the allocation beyond `limit` bytes
    /// (rounded up to [`Self::ALIGNMENT`]), so amortized growth cannot overshoot a size cap.
    pub fn write_at_within(&mut self, offset: usize, data: &[u8], limit: usize) -> Result<()> {
        if offset > self.len {
            return Err(Error::InvalidParameter);
        }
        let end = offset.checked_add(data.len()).ok_or(Error::FileTooLarge)?;
        if end > limit {
            return Err(Error::FileTooLarge);
        }
        if end > self.capacity {
            self.reserve_within(end, limit)?;
        }
        if !data.is_empty() {
            // SAFETY: `end <= self.capacity` guaranteed by reserve above.
            unsafe {
                core::ptr::copy(data.as_ptr(), self.ptr.as_ptr().add(offset), data.len());
            }
            self.len = self.len.max(end);
        }
        Ok(())
    }

    /// Ensure capacity for at least `minimum_capacity` bytes.
    pub fn reserve(&mut self, minimum_capacity: usize) -> Result<()> {
        self.reserve_within(minimum_capacity, usize::MAX)
    }

    fn reserve_within(&mut self, minimum_capacity: usize, limit: usize) -> Result<()> {
        if minimum_capacity <= self.capacity {
            return Ok(());
        }
        let target_capacity = minimum_capacity
            .max(self.capacity.saturating_mul(2))
            .max(Self::ALIGNMENT)
            .min(limit.max(minimum_capacity));
        let aligned_capacity = Self::align_capacity(target_capacity)?;
        let new_layout = Layout::from_size_align(aligned_capacity, Self::ALIGNMENT)
            .map_err(|_| Error::NoMemory)?;
        let new_ptr = if self.capacity == 0 {
            // SAFETY: `aligned_capacity` > 0 and layout is valid.
            unsafe { alloc(new_layout) }
        } else {
            let old_layout = Layout::from_size_align(self.capacity, Self::ALIGNMENT)
                .map_err(|_| Error::NoMemory)?;
            // SAFETY: `self.ptr` was allocated with `old_layout`.
            unsafe { realloc(self.ptr.as_ptr(), old_layout, aligned_capacity) }
        };
        self.ptr = NonNull::new(new_ptr).ok_or(Error::NoMemory)?;
        self.capacity = aligned_capacity;
        Ok(())
    }

    /// Consumes the buffer, returning the raw pointer, length, and capacity.
    ///
    /// # Safety
    /// The caller must ensure the memory is eventually deallocated using the global
    /// allocator with layout `Layout::from_size_align(capacity, ModelBuffer::ALIGNMENT)`.
    pub fn into_raw_parts(self) -> (NonNull<u8>, usize, usize) {
        let this = core::mem::ManuallyDrop::new(self);
        (this.ptr, this.len, this.capacity)
    }

    /// Reconstructs a [`ModelBuffer`] from raw parts.
    ///
    /// # Safety
    /// - `ptr` must have been allocated with [`ModelBuffer::ALIGNMENT`] alignment and
    ///   `capacity` bytes via the global allocator (or dangling if `capacity == 0`).
    /// - `len <= capacity`.
    pub unsafe fn from_raw_parts(ptr: NonNull<u8>, len: usize, capacity: usize) -> Self {
        Self { ptr, len, capacity }
    }

    const fn align_capacity(capacity: usize) -> Result<usize> {
        match capacity.checked_add(Self::ALIGNMENT - 1) {
            Some(aligned) => Ok(aligned & !(Self::ALIGNMENT - 1)),
            None => Err(Error::NoMemory),
        }
    }
}

impl Default for ModelBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ModelBuffer {
    fn drop(&mut self) {
        if self.capacity != 0 {
            // SAFETY: `self.ptr` was allocated with this exact layout.
            let layout =
                unsafe { Layout::from_size_align_unchecked(self.capacity, Self::ALIGNMENT) };
            unsafe {
                dealloc(self.ptr.as_ptr(), layout);
            }
        }
    }
}

impl core::ops::Deref for ModelBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl core::ops::DerefMut for ModelBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl AsRef<[u8]> for ModelBuffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for ModelBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_maintains_sixteen_byte_alignment() {
        let buffer = ModelBuffer::with_capacity(37).unwrap();
        assert_eq!((buffer.as_ptr() as usize) % ModelBuffer::ALIGNMENT, 0);
        assert!(buffer.capacity() >= 37);
        assert_eq!(buffer.capacity() % ModelBuffer::ALIGNMENT, 0);

        let zeroed = ModelBuffer::new_zeroed(64).unwrap();
        assert_eq!((zeroed.as_ptr() as usize) % ModelBuffer::ALIGNMENT, 0);
        assert_eq!(zeroed.len(), 64);
        assert!(zeroed.iter().all(|&b| b == 0));
    }

    #[test]
    fn buffer_supports_sequential_writes_and_reallocation() {
        let mut buffer = ModelBuffer::new();
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.as_slice(), &[]);

        // Writing at non-contiguous offset fails
        assert_eq!(buffer.write_at(5, &[1, 2, 3]), Err(Error::InvalidParameter));

        // Write first chunk
        buffer.write_at(0, &[1, 2, 3, 4]).unwrap();
        assert_eq!(buffer.len(), 4);
        assert_eq!(buffer.as_slice(), &[1, 2, 3, 4]);

        // Write contiguous second chunk
        buffer.write_at(4, &[5, 6, 7, 8]).unwrap();
        assert_eq!(buffer.len(), 8);
        assert_eq!(buffer.as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8]);

        // Overwrite prefix
        buffer.write_at(2, &[99, 100]).unwrap();
        assert_eq!(buffer.len(), 8);
        assert_eq!(buffer.as_slice(), &[1, 2, 99, 100, 5, 6, 7, 8]);
    }
}

#[cfg(test)]
mod capped_growth_tests {
    use super::ModelBuffer;

    #[test]
    fn growth_never_exceeds_the_limit() {
        let mut buffer = ModelBuffer::new();
        for offset in (0..1000).step_by(100) {
            buffer.write_at_within(offset, &[1; 100], 1000).unwrap();
            assert!(buffer.capacity() <= 1008);
        }
        assert_eq!(buffer.len(), 1000);
        assert!(buffer.write_at_within(1000, &[1], 1000).is_err());
        assert_eq!(buffer.as_ptr() as usize % ModelBuffer::ALIGNMENT, 0);
    }
}
