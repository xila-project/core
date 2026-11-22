//! The handles a native process owns, so the host can close them when the process ends
//! without running any of its code.
//!
//! A cancelled coroutine's frames are discarded (see `stackful::Coroutine::new`), so no
//! destructor of an object the program holds runs. Every object the ABI hands out as an
//! owned pointer (a file, a directory, a listing, a child, a socket) is therefore boxed
//! behind a link that lists it in the registry of the process that opened it
//! ([`register`]), and unlinks itself when the object is reclaimed ([`reclaim`]). The
//! program holds only the pointer to the object. When the process ends, however it ends,
//! the host walks the registry and closes what is left, newest first.
//!
//! The list is intrusive, so tracking a handle allocates nothing and cannot fail: the link
//! is three pointers in front of the object, 12 bytes on a 32-bit target, and the registry
//! is one pointer in the process's host state. The startup streams and the home directory
//! are not handles: the host state owns them directly and carries no link for them.
//! Unlinking needs no registry, which is why closing and reclaiming a handle works from
//! any context.

extern crate alloc;

use alloc::boxed::Box;
use core::{cell::Cell, ptr};

/// The node a registered object embeds. It stays at one address from [`Link::register`] to
/// [`Link::unlink`]: the objects are boxed before they are registered.
pub(crate) struct Link {
    next: Cell<*const Link>,
    /// The pointer that points at this link: the registry's `first` or the previous
    /// link's `next`. Null while the link is not in a registry.
    previous: Cell<*const Cell<*const Link>>,
    /// Closes and frees the object that embeds this link, which unlinks it.
    release: unsafe fn(*const Link),
}

impl Link {
    /// An unlinked node. `release` reclaims and closes the object that embeds it.
    pub(crate) const fn new(release: unsafe fn(*const Link)) -> Self {
        Self {
            next: Cell::new(ptr::null()),
            previous: Cell::new(ptr::null()),
            release,
        }
    }

    /// Add this node to the registry of the process the calling coroutine belongs to.
    /// Outside a native process there is none, and the object stays the caller's own.
    ///
    /// The node must stay at its address until it is unlinked.
    pub(crate) fn register(&self) {
        // SAFETY: the registry belongs to the host state of the running process, which
        // outlives every coroutine of it.
        let Some(registry) = (unsafe { current() }) else {
            return;
        };
        let first = registry.first.get();
        self.next.set(first);
        // SAFETY: a registered link is live until it unlinks itself.
        if let Some(first) = unsafe { first.as_ref() } {
            first.previous.set(&raw const self.next);
        }
        self.previous.set(&raw const registry.first);
        registry.first.set(self);
    }

    /// Remove this node from its registry, if it is in one. Idempotent.
    pub(crate) fn unlink(&self) {
        let previous = self.previous.get();
        if previous.is_null() {
            return;
        }
        let next = self.next.get();
        // SAFETY: `previous` is the registry head or the `next` of a live link, which
        // cannot be freed while it still points at this one.
        unsafe { (*previous).set(next) };
        // SAFETY: as above, for the link after this one.
        if let Some(next) = unsafe { next.as_ref() } {
            next.previous.set(previous);
        }
        self.next.set(ptr::null());
        self.previous.set(ptr::null());
    }
}

/// The objects a process has opened and not closed. One pointer per process.
pub(crate) struct Registry {
    first: Cell<*const Link>,
}

impl Registry {
    pub(crate) const fn new() -> Self {
        Self {
            first: Cell::new(ptr::null()),
        }
    }

    /// Close and free every registered object, newest first. Each release unlinks its
    /// object, so a release that suspends and is then cancelled leaves the rest, and the
    /// object it was closing, for the host to release again.
    ///
    /// # Safety
    /// The registry must not move while it holds objects, and no code of the process may
    /// run concurrently or later.
    pub(crate) unsafe fn release_all(&self) {
        // SAFETY: a registered link is live until it unlinks itself.
        while let Some(link) = unsafe { self.first.get().as_ref() } {
            // SAFETY: the release function reclaims the object, which unlinks it, so the
            // loop ends once every object is gone.
            unsafe { (link.release)(link) };
        }
    }

    /// Number of registered objects.
    pub(crate) fn len(&self) -> usize {
        let mut count = 0;
        // SAFETY: a registered link is live until it unlinks itself.
        let mut link = unsafe { self.first.get().as_ref() };
        while let Some(current) = link {
            count += 1;
            link = unsafe { current.next.get().as_ref() };
        }
        count
    }
}

/// The registry of the process the calling coroutine belongs to, if it belongs to one.
///
/// # Safety
/// The reference must not be used after the process's host state is released.
#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32",
            target_arch = "xtensa"
        ),
        target_os = "none"
    )
))]
unsafe fn current<'a>() -> Option<&'a Registry> {
    // SAFETY: the caller's contract is this function's.
    unsafe { crate::threads::registry() }
}

/// No stack switching: a native entry runs to completion on its task and cannot be
/// cancelled in the middle, so nothing needs the host to close for it.
#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32",
            target_arch = "xtensa"
        ),
        target_os = "none"
    )
)))]
unsafe fn current<'a>() -> Option<&'a Registry> {
    None
}

/// An object the ABI hands to a program as an owned pointer.
pub(crate) trait Handle: Sized {
    /// Close and free the object, as the program's own close call does.
    ///
    /// # Safety
    /// `raw` came from [`register`] and is reclaimed exactly once.
    unsafe fn close(raw: *mut Self);
}

/// The allocation behind an owned pointer: the link, then the object the pointer addresses.
#[repr(C)]
struct Registered<T> {
    link: Link,
    value: T,
}

/// Box `value` and list it in the calling process's registry, if it is in one. The pointer
/// addresses the object; [`reclaim`] takes it back.
pub(crate) fn register<T: Handle>(value: T) -> *mut T {
    let registered = Box::into_raw(Box::new(Registered {
        link: Link::new(release::<T>),
        value,
    }));
    // SAFETY: the box was just allocated and stays where it is until it is reclaimed.
    unsafe {
        (*registered).link.register();
        &raw mut (*registered).value
    }
}

/// Unlink the object and free its allocation, returning the object.
///
/// # Safety
/// `raw` came from [`register`], and is reclaimed exactly once.
pub(crate) unsafe fn reclaim<T: Handle>(raw: *mut T) -> T {
    // SAFETY: the object is the `value` field of the allocation `register` made.
    let registered = unsafe {
        raw.byte_sub(core::mem::offset_of!(Registered<T>, value))
            .cast::<Registered<T>>()
    };
    // SAFETY: the allocation is live until the box below frees it.
    unsafe { (*registered).link.unlink() };
    unsafe { Box::from_raw(registered) }.value
}

/// Close an object the registry still holds. The link is the first field of its allocation.
unsafe fn release<T: Handle>(link: *const Link) {
    let registered = link.cast::<Registered<T>>().cast_mut();
    // SAFETY: the registry holds only live allocations that `register` made.
    unsafe { T::close(&raw mut (*registered).value) }
}
