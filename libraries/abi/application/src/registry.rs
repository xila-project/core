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
//! uses two pointers in front of the object, 8 bytes on a 32-bit target. The registry uses
//! one pointer in the process's host state. The startup streams and the home directory
//! are not handles: the host state owns them directly and carries no link for them.
//! Unlinking needs no registry, which is why closing and reclaiming a handle works from
//! any context.

extern crate alloc;

use alloc::boxed::Box;
use core::{cell::Cell, ptr};

use kind::{Kind, release};

// Store one tag bit in the aligned next pointer and two in the aligned
// previous pointer. Clear these bits before using either pointer.
const NEXT_TAG_MASK: usize = 0b1;
const PREVIOUS_TAG_MASK: usize = 0b11;

struct Next(Cell<*const Link>);

impl Next {
    const fn new(tag: usize) -> Self {
        Self(Cell::new(ptr::without_provenance(tag)))
    }

    fn get(&self) -> *const Link {
        self.0.get().map_addr(|address| address & !NEXT_TAG_MASK)
    }

    fn set(&self, link: *const Link) {
        // Relinking must preserve the tag of the link that owns this pointer.
        let tag = self.tag();
        self.0.set(link.map_addr(|address| address | tag));
    }

    fn tag(&self) -> usize {
        self.0.get().addr() & NEXT_TAG_MASK
    }
}

/// The node a registered object embeds. It stays at one address from [`Link::register`] to
/// [`Link::unlink`]: the objects are boxed before they are registered.
struct Link {
    next: Next,
    /// Points to the registry's `first` or the previous link's `next`.
    previous: Cell<*const Next>,
}

const _: () = assert!(core::mem::align_of::<Link>() > NEXT_TAG_MASK);
const _: () = assert!(core::mem::align_of::<Next>() > PREVIOUS_TAG_MASK);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<Link>() == 8);

impl Link {
    fn new(kind: Kind) -> Self {
        let tag = kind as usize;
        Self {
            next: Next::new(tag & NEXT_TAG_MASK),
            previous: Cell::new(ptr::without_provenance(tag >> 1)),
        }
    }

    fn tag(&self) -> usize {
        ((self.previous.get().addr() & PREVIOUS_TAG_MASK) << 1) | self.next.tag()
    }

    fn set_previous(&self, previous: *const Next) {
        let tag = self.previous.get().addr() & PREVIOUS_TAG_MASK;
        self.previous
            .set(previous.map_addr(|address| address | tag));
    }

    /// Add this node to the registry of the process the calling coroutine belongs to.
    /// Outside a native process there is none, and the object stays the caller's own.
    ///
    /// # Safety
    /// `link` must point to the first field of a live `Registered` allocation.
    /// The allocation must stay at its address until the link is unlinked.
    unsafe fn register(link: *const Self) {
        // SAFETY: the registry belongs to the host state of the running process, which
        // outlives every coroutine of it.
        let Some(registry) = (unsafe { current() }) else {
            return;
        };
        // SAFETY: the caller guarantees that the link is live.
        let node = unsafe { &*link };
        let first = registry.first.get();
        node.next.set(first);
        // SAFETY: a registered link is live until it unlinks itself.
        if let Some(first) = unsafe { first.as_ref() } {
            first.set_previous(&raw const node.next);
        }
        node.set_previous(&raw const registry.first);
        // Keep the allocation pointer so release can access the full object.
        registry.first.set(link);
    }

    /// Remove this node from its registry, if it is in one. Idempotent.
    fn unlink(&self) {
        let previous = self
            .previous
            .get()
            .map_addr(|address| address & !PREVIOUS_TAG_MASK);
        if previous.is_null() {
            return;
        }
        let next = self.next.get();
        // SAFETY: `previous` is the registry head or the `next` of a live link, which
        // cannot be freed while it still points at this one.
        unsafe { (*previous).set(next) };
        // SAFETY: as above, for the link after this one.
        if let Some(next) = unsafe { next.as_ref() } {
            next.set_previous(previous);
        }
        self.next.set(ptr::null());
        self.set_previous(ptr::null());
    }
}

/// The objects a process has opened and not closed. One pointer per process.
pub(crate) struct Registry {
    first: Next,
}

impl Registry {
    pub(crate) const fn new() -> Self {
        Self {
            first: Next::new(0),
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
        loop {
            let link = self.first.get();
            if link.is_null() {
                break;
            }
            // SAFETY: the head points to a live allocation. Release closes the object,
            // which unlinks it when it is reclaimed.
            unsafe { release(link) };
        }
    }

    /// Number of registered objects.
    pub(crate) fn len(&self) -> usize {
        let mut count = 0;
        // SAFETY: a registered link is live until it unlinks itself.
        let mut link = unsafe { self.first.get().as_ref() };
        while let Some(current) = link {
            count += 1;
            // SAFETY: the next registered link is live until it unlinks itself.
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
pub(crate) trait Handle: Sized + kind::Tagged {
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
        link: Link::new(T::KIND),
        value,
    }));
    // SAFETY: the box was just allocated and stays where it is until it is reclaimed.
    unsafe {
        Link::register(&raw const (*registered).link);
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
    // SAFETY: `registered` came from `Box::into_raw` and is reclaimed once.
    unsafe { Box::from_raw(registered) }.value
}

/// Close an object the registry still holds. The link is the first field of its allocation.
unsafe fn release_handle<T: Handle>(link: *const Link) {
    let registered = link.cast::<Registered<T>>().cast_mut();
    // SAFETY: the tag identifies the type of the live allocation that `register` made.
    unsafe { T::close(&raw mut (*registered).value) }
}

// This private module seals the mapping from handle types to tags.
mod kind {
    use super::{Link, NEXT_TAG_MASK, PREVIOUS_TAG_MASK, release_handle};
    use crate::{Child, Directory, DirectoryIterator, File};
    #[cfg(feature = "network")]
    use crate::{Tcp, TcpListener, Udp};

    macro_rules! handle_kinds {
        ($($(#[$attribute:meta])* $handle:ident = $tag:literal),+ $(,)?) => {
            #[repr(u8)]
            pub enum Kind {
                $($(#[$attribute])* $handle = $tag),+
            }

            pub trait Tagged {
                const KIND: Kind;
            }

            $(
                $(#[$attribute])*
                impl Tagged for $handle {
                    const KIND: Kind = Kind::$handle;
                }

                $(#[$attribute])*
                const _: () = assert!(
                    Kind::$handle as usize & !((PREVIOUS_TAG_MASK << 1) | NEXT_TAG_MASK) == 0
                );
            )+

            pub(super) unsafe fn release(link: *const Link) {
                // SAFETY: the registry head points to a live link.
                let tag = unsafe { (*link).tag() };
                // SAFETY: only register sets the tag, using the same type as the allocation.
                unsafe {
                    match tag {
                        $($(#[$attribute])* $tag => release_handle::<$handle>(link),)+
                        _ => unreachable!(),
                    }
                }
            }
        };
    }

    handle_kinds! {
        File = 0,
        Directory = 1,
        DirectoryIterator = 2,
        Child = 3,
        #[cfg(feature = "network")]
        Tcp = 4,
        #[cfg(feature = "network")]
        TcpListener = 5,
        #[cfg(feature = "network")]
        Udp = 6,
    }
}
