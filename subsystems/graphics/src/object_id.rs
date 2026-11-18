//! Identity of guest-visible LVGL objects.
//!
//! LVGL keeps an opaque `id` word in every object (`LV_USE_OBJ_ID 1`, which
//! Core enables with `LV_USE_OBJ_ID_BUILTIN 0` so that the hooks below
//! replace LVGL's own). Core owns that word:
//!
//! - An [`ObjectTable`] belongs to one consumer (one Wasm VM). It issues its
//!   own nonzero, monotonic 32-bit identifiers, stores each one in the
//!   `id` word of the object it names, and keeps a sorted
//!   `(id, object)` array of those objects only: 8 B per named object on
//!   32-bit targets and nothing for any other widget. The array is sized to
//!   its content, grown and shrunk in small steps, never doubled, and
//!   looked up with a binary search.
//! - [`lv_obj_free_id`] runs at the end of the destructor of every object,
//!   whatever the deletion path, while the object's memory is still valid.
//!   It tells the registered [`FreeObserver`], which removes the object from
//!   its table **by pointer**.
//! - An identifier a consumer holds is never dereferenced: a foreign, stale
//!   or invented value is simply absent from the caller's table, and a
//!   recycled address cannot match it, because identifiers are never
//!   reissued. Identifiers are numbers, not capabilities across consumers:
//!   two tables may issue the same value for different objects.
//!
//! Never use LVGL's `lv_obj_find_by_id`: with a null root it searches the
//! active screen, whatever table the identifier belongs to.
//!
//! Every function here requires the graphics lock.

use crate::{Error, Result, ffi};
use alloc::vec::Vec;
use core::{
    ffi::{c_char, c_int, c_void},
    num::NonZeroU32,
    ptr::{self, NonNull},
};
use portable_atomic::{AtomicPtr, Ordering};

/// Called at the end of the destructor of an object that carries an
/// identifier, under the graphics lock. The object's memory is valid, but its
/// class destructors have run: the function must not call LVGL, and must not
/// keep the pointer.
pub type FreeObserver = fn(object: NonNull<ffi::lv_obj_t>, id: NonZeroU32);

static OBSERVER: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());

/// Registers the function that learns about every freed identified object.
///
/// One observer serves the whole process, set once, before the first
/// identifier is published. Registering the same function again is
/// accepted; a different one fails with [`Error::FailedToRegister`].
pub fn set_free_observer(observer: FreeObserver) -> Result<()> {
    let new = observer as usize as *mut ();
    match OBSERVER.load(Ordering::Acquire) {
        current if current.is_null() => {
            OBSERVER.store(new, Ordering::Release);
            Ok(())
        }
        current if current == new => Ok(()),
        _ => Err(Error::FailedToRegister),
    }
}

fn observer() -> Option<FreeObserver> {
    let pointer = OBSERVER.load(Ordering::Acquire);
    // SAFETY: only `set_free_observer` stores a non-null value, and it stores
    // a `FreeObserver`.
    (!pointer.is_null()).then(|| unsafe { core::mem::transmute::<*mut (), FreeObserver>(pointer) })
}

/// The object's identifier, zero if it has none.
///
/// # Safety
///
/// `object` must be a live LVGL object and the graphics lock held.
pub(crate) unsafe fn read_id(object: *const ffi::lv_obj_t) -> u32 {
    // SAFETY: the caller guarantees a live object.
    let word = unsafe { ffi::lv_obj_get_id(object) };
    // Only this module writes the word, and only values that fit.
    u32::try_from(word.addr()).unwrap_or(0)
}

/// The only write of an object's `id` word.
///
/// # Safety
///
/// `object` must be a live LVGL object and the graphics lock held.
unsafe fn write_id(object: *mut ffi::lv_obj_t, id: u32) {
    // SAFETY: the caller guarantees a live object.
    unsafe { ffi::lvgl_sys_obj_set_id_raw(object, ptr::without_provenance_mut(id as usize)) }
}

/// Entries added or removed at once. A table never reserves more than this
/// beyond its content, nor keeps twice that much free after deletions.
const STEP: usize = 4;

/// A consumer's identifiers and the objects they name.
///
/// Identifiers start at 1 and increase. When the counter runs out the table
/// refuses to name more objects; other tables are unaffected.
pub struct ObjectTable {
    /// Sorted by identifier.
    entries: Vec<(u32, NonNull<ffi::lv_obj_t>)>,
    last_issued: u32,
}

impl ObjectTable {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_issued: 0,
        }
    }

    /// A table whose counter already issued `last_issued` identifiers, to
    /// exercise exhaustion.
    #[doc(hidden)]
    pub const fn with_last_issued(last_issued: u32) -> Self {
        Self {
            entries: Vec::new(),
            last_issued,
        }
    }

    /// Number of objects named by this table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bytes reserved by the table.
    pub fn capacity_bytes(&self) -> usize {
        self.entries.capacity() * core::mem::size_of::<(u32, NonNull<ffi::lv_obj_t>)>()
    }

    /// The object this table named `id`, if it is still alive.
    pub fn find(&self, id: u32) -> Option<NonNull<ffi::lv_obj_t>> {
        let index = self.entries.binary_search_by_key(&id, |e| e.0).ok()?;
        Some(self.entries[index].1)
    }

    /// The identifier this table issued to `object`, if it did.
    ///
    /// # Safety
    ///
    /// `object` must be a live LVGL object and the graphics lock held.
    pub unsafe fn id_of(&self, object: NonNull<ffi::lv_obj_t>) -> Option<NonZeroU32> {
        // SAFETY: the caller guarantees a live object.
        let id = NonZeroU32::new(unsafe { read_id(object.as_ptr()) })?;
        (self.find(id.get())? == object).then_some(id)
    }

    /// Names `object`, which must carry no identifier yet.
    ///
    /// The object is not deleted on failure.
    ///
    /// # Errors
    ///
    /// [`Error::OutOfMemory`] if the table cannot grow,
    /// [`Error::NotAvailable`] if every identifier of the counter was issued,
    /// [`Error::AlreadyInUse`] if the object already carries one (of any
    /// table).
    ///
    /// # Safety
    ///
    /// `object` must be a live LVGL object and the graphics lock held.
    pub unsafe fn publish(&mut self, object: NonNull<ffi::lv_obj_t>) -> Result<NonZeroU32> {
        // SAFETY: the caller guarantees a live object.
        if unsafe { read_id(object.as_ptr()) } != 0 {
            return Err(Error::AlreadyInUse);
        }
        let id = self
            .last_issued
            .checked_add(1)
            .and_then(NonZeroU32::new)
            .ok_or(Error::NotAvailable)?;
        if self.entries.len() == self.entries.capacity() {
            self.entries
                .try_reserve_exact(STEP)
                .map_err(|_| Error::OutOfMemory)?;
        }
        self.last_issued = id.get();
        // The counter only grows, so the new identifier sorts last.
        self.entries.push((id.get(), object));
        // SAFETY: the caller guarantees a live object.
        unsafe { write_id(object.as_ptr(), id.get()) };
        Ok(id)
    }

    /// Forgets `object`, named `id`, if this table named it. Compares the
    /// pointer, so another table's identical number is left alone. Called by
    /// the [`FreeObserver`] while the object is being freed.
    pub fn remove(&mut self, id: u32, object: NonNull<ffi::lv_obj_t>) -> bool {
        let Ok(index) = self.entries.binary_search_by_key(&id, |e| e.0) else {
            return false;
        };
        if self.entries[index].1 != object {
            return false;
        }
        self.entries.remove(index);
        if self.entries.capacity() - self.entries.len() >= 2 * STEP {
            let keep = self.entries.len().next_multiple_of(STEP);
            self.entries.shrink_to(keep);
        }
        true
    }

    /// Takes every named object, in identifier order, leaving the table
    /// empty (its counter keeps running). Used to delete a consumer's
    /// objects at teardown; the free hook then finds nothing to remove.
    pub fn take_objects(&mut self) -> Vec<NonNull<ffi::lv_obj_t>> {
        let objects = self.entries.iter().map(|e| e.1).collect();
        self.entries = Vec::new();
        objects
    }
}

impl Default for ObjectTable {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY (all hooks below): LVGL calls them under the graphics lock with a live
// object (`obj`), or with its class when the object is created.

#[unsafe(no_mangle)]
unsafe extern "C" fn lv_obj_assign_id(_: *const ffi::lv_obj_class_t, _: *mut ffi::lv_obj_t) {
    // Objects start with no identifier: a table names only what it publishes.
}

#[unsafe(no_mangle)]
unsafe extern "C" fn lv_obj_free_id(object: *mut ffi::lv_obj_t) {
    let Some(object) = NonNull::new(object) else {
        return;
    };
    // SAFETY: LVGL passes the object being destroyed; its memory is valid.
    let Some(id) = NonZeroU32::new(unsafe { read_id(object.as_ptr()) }) else {
        return;
    };
    if let Some(observer) = observer() {
        observer(object, id);
    }
}

/// Only [`ObjectTable::publish`] sets an identifier. A null value untracks the
/// object (as LVGL's builtin does); other values are ignored.
#[unsafe(no_mangle)]
unsafe extern "C" fn lv_obj_set_id(object: *mut ffi::lv_obj_t, id: *mut c_void) {
    if object.is_null() || !id.is_null() {
        return;
    }
    // SAFETY: LVGL API contract: `object` is live.
    unsafe {
        lv_obj_free_id(object);
        write_id(object, 0);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn lv_obj_id_compare(a: *const c_void, b: *const c_void) -> c_int {
    (a != b) as c_int
}

/// Writes `#<id>` (`#0` for an object without one).
#[unsafe(no_mangle)]
unsafe extern "C" fn lv_obj_stringify_id(
    object: *mut ffi::lv_obj_t,
    buffer: *mut c_char,
    length: u32,
) -> *const c_char {
    if object.is_null() || buffer.is_null() || length == 0 {
        return ptr::null();
    }
    // SAFETY: LVGL passes a live object and a buffer of `length` bytes.
    let (mut id, out) = unsafe {
        (
            read_id(object),
            core::slice::from_raw_parts_mut(buffer.cast::<u8>(), length as usize),
        )
    };
    let mut digits = [0u8; 10];
    let mut count = 0;
    for digit in digits.iter_mut() {
        *digit = b'0' + (id % 10) as u8;
        count += 1;
        id /= 10;
        if id == 0 {
            break;
        }
    }
    let text = core::iter::once(b'#').chain(digits.iter().take(count).rev().copied());
    let mut written = 0;
    let room = out.len() - 1;
    for (destination, byte) in out.iter_mut().take(room).zip(text) {
        *destination = byte;
        written += 1;
    }
    if let Some(end) = out.get_mut(written) {
        *end = 0;
    }
    buffer
}

/// Keeps the hooks linked: LVGL's archive references them, but the linker
/// only pulls the object file defining them if Rust code does too.
pub(crate) fn keep() {
    let hooks: [*const (); 5] = [
        lv_obj_assign_id as *const (),
        lv_obj_free_id as *const (),
        lv_obj_set_id as *const (),
        lv_obj_id_compare as *const (),
        lv_obj_stringify_id as *const (),
    ];
    core::hint::black_box(hooks);
}
