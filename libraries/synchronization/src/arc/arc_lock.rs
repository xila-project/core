#![allow(dead_code)]

use alloc::boxed::Box;
use core::{alloc::Layout, cell::RefCell, fmt, marker::PhantomData, ops::Deref, ptr::NonNull};
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

/// Inner data structure for Arc containing the value and reference counts
struct ArcInner<T: ?Sized> {
    strong: Mutex<CriticalSectionRawMutex, RefCell<usize>>,
    weak: Mutex<CriticalSectionRawMutex, RefCell<usize>>,
    data: T,
}

/// A thread-safe reference-counting pointer for platforms without atomic operations.
///
/// This implementation uses Embassy's `Mutex` with `CriticalSectionRawMutex` for
/// synchronization instead of atomic operations, making it suitable for platforms
/// that don't support atomic pointer operations.
///
/// The type `Arc<T>` provides shared ownership of a value of type `T`, allocated
/// in the heap. Invoking `clone` on `Arc` produces a new `Arc` instance, which
/// points to the same allocation on the heap as the source `Arc`, while increasing
/// a reference count. When the last `Arc` pointer to a given allocation is destroyed,
/// the value stored in that allocation (often referred to as "inner value") is also dropped.
pub struct Arc<T: ?Sized> {
    ptr: NonNull<ArcInner<T>>,
    phantom: PhantomData<ArcInner<T>>,
}

unsafe impl<T: ?Sized + Sync + Send> Send for Arc<T> {}
unsafe impl<T: ?Sized + Sync + Send> Sync for Arc<T> {}

impl<T> Arc<T> {
    /// Constructs a new `Arc<T>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use synchronization::Arc;
    ///
    /// let five = Arc::new(5);
    /// ```
    pub fn new(data: T) -> Arc<T> {
        let inner = Box::new(ArcInner {
            strong: Mutex::new(RefCell::new(1)),
            weak: Mutex::new(RefCell::new(1)),
            data,
        });

        Arc {
            ptr: NonNull::new(Box::into_raw(inner)).unwrap(),
            phantom: PhantomData,
        }
    }

    /// Gets the number of strong pointers to this allocation.
    ///
    /// # Safety
    ///
    /// This method by itself is safe, but using it correctly requires extra care.
    /// The count can change at any time and should only be used for hinting or debugging.
    pub fn strong_count(this: &Self) -> usize {
        let inner = unsafe { this.ptr.as_ref() };
        inner.strong.lock(|count| *count.borrow())
    }

    /// Gets the number of weak pointers to this allocation.
    ///
    /// # Safety
    ///
    /// This method by itself is safe, but using it correctly requires extra care.
    /// The count can change at any time and should only be used for hinting or debugging.
    pub fn weak_count(this: &Self) -> usize {
        let inner = unsafe { this.ptr.as_ref() };
        inner.weak.lock(|count| *count.borrow()) - 1 // Subtract 1 for the implicit weak reference
    }

    /// Returns a mutable reference into the given `Arc`, if there are no other
    /// `Arc` or `Weak` pointers to the same allocation.
    ///
    /// Returns `None` otherwise, because it is not safe to mutate a shared value.
    pub fn get_mut(this: &mut Self) -> Option<&mut T> {
        let inner = unsafe { this.ptr.as_ref() };

        let strong = inner.strong.lock(|count| *count.borrow());
        let weak = inner.weak.lock(|count| *count.borrow());

        if strong == 1 && weak == 1 {
            // Safe because we have exclusive access
            Some(unsafe { &mut (*this.ptr.as_ptr()).data })
        } else {
            None
        }
    }

    /// Returns `true` if the two `Arc`s point to the same allocation.
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        this.ptr.as_ptr() == other.ptr.as_ptr()
    }
}

impl<T: ?Sized> Arc<T> {
    fn inner(&self) -> &ArcInner<T> {
        unsafe { self.ptr.as_ref() }
    }

    /// Increments the strong reference count.
    fn inc_strong(&self) {
        let inner = self.inner();
        inner.strong.lock(|count| {
            let mut c = count.borrow_mut();
            *c = c.checked_add(1).expect("Arc strong count overflow");
        });
    }

    /// Decrements the strong reference count.
    /// Returns true if this was the last strong reference.
    fn dec_strong(&self) -> bool {
        let inner = self.inner();
        let should_drop_data = inner.strong.lock(|count| {
            let mut c = count.borrow_mut();
            *c -= 1;
            *c == 0
        });

        if should_drop_data {
            // Drop the data
            unsafe {
                core::ptr::drop_in_place(&mut (*self.ptr.as_ptr()).data);
            }

            // Now decrement the weak count (removes the implicit weak reference that strong refs hold)
            let should_deallocate = inner.weak.lock(|count| {
                let mut c = count.borrow_mut();
                *c -= 1;
                *c == 0
            });

            if should_deallocate {
                // Deallocate the entire ArcInner without dropping it
                // (we already dropped the data field)
                unsafe {
                    // Drop the Mutex fields (which contain RefCells)
                    core::ptr::drop_in_place(&mut (*self.ptr.as_ptr()).strong);
                    core::ptr::drop_in_place(&mut (*self.ptr.as_ptr()).weak);

                    // Now deallocate the memory
                    let layout = Layout::for_value(self.ptr.as_ref());
                    alloc::alloc::dealloc(self.ptr.as_ptr() as *mut u8, layout);
                }
            }
        }

        should_drop_data
    }

    /// Creates a new `Weak` pointer to this allocation.
    pub fn downgrade(this: &Self) -> Weak<T> {
        let inner = this.inner();
        inner.weak.lock(|count| {
            let mut c = count.borrow_mut();
            *c = c.checked_add(1).expect("Arc weak count overflow");
        });

        Weak {
            ptr: this.ptr,
            phantom: PhantomData,
        }
    }
}

impl<T: ?Sized> Clone for Arc<T> {
    fn clone(&self) -> Arc<T> {
        self.inc_strong();
        Arc {
            ptr: self.ptr,
            phantom: PhantomData,
        }
    }
}

impl<T: ?Sized> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner().data
    }
}

impl<T: ?Sized> Drop for Arc<T> {
    fn drop(&mut self) {
        self.dec_strong();
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Arc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for Arc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: Default> Default for Arc<T> {
    fn default() -> Arc<T> {
        Arc::new(Default::default())
    }
}

impl<T: ?Sized + PartialEq> PartialEq for Arc<T> {
    fn eq(&self, other: &Arc<T>) -> bool {
        **self == **other
    }
}

impl<T: ?Sized + Eq> Eq for Arc<T> {}

// Weak pointer implementation

/// A weak version of `Arc`.
///
/// Weak references do not count towards determining if the value stored in the
/// allocation should be dropped, but they do keep the allocation itself alive.
pub struct Weak<T: ?Sized> {
    ptr: NonNull<ArcInner<T>>,
    phantom: PhantomData<ArcInner<T>>,
}

unsafe impl<T: ?Sized + Sync + Send> Send for Weak<T> {}
unsafe impl<T: ?Sized + Sync + Send> Sync for Weak<T> {}

impl<T: ?Sized> Weak<T> {
    fn inner(&self) -> &ArcInner<T> {
        unsafe { self.ptr.as_ref() }
    }

    /// Attempts to upgrade the `Weak` pointer to an `Arc`, returning `None` if
    /// the value has already been dropped.
    pub fn upgrade(&self) -> Option<Arc<T>> {
        let inner = self.inner();

        let upgraded = inner.strong.lock(|count| {
            let mut c = count.borrow_mut();
            if *c == 0 {
                false
            } else {
                *c = c.checked_add(1).expect("Arc strong count overflow");
                true
            }
        });

        if upgraded {
            Some(Arc {
                ptr: self.ptr,
                phantom: PhantomData,
            })
        } else {
            None
        }
    }

    /// Gets the number of strong pointers to this allocation.
    pub fn strong_count(&self) -> usize {
        let inner = self.inner();
        inner.strong.lock(|count| *count.borrow())
    }

    /// Gets the number of weak pointers to this allocation.
    pub fn weak_count(&self) -> usize {
        let inner = self.inner();
        inner.weak.lock(|count| *count.borrow()) - 1
    }

    /// Returns `true` if the two `Weak`s point to the same allocation.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        core::ptr::addr_eq(self.ptr.as_ptr(), other.ptr.as_ptr())
    }
}

impl<T: ?Sized> Clone for Weak<T> {
    fn clone(&self) -> Weak<T> {
        let inner = self.inner();
        inner.weak.lock(|count| {
            let mut c = count.borrow_mut();
            *c = c.checked_add(1).expect("Arc weak count overflow");
        });

        Weak {
            ptr: self.ptr,
            phantom: PhantomData,
        }
    }
}

impl<T: ?Sized> Drop for Weak<T> {
    fn drop(&mut self) {
        let inner = self.inner();
        let should_deallocate = inner.weak.lock(|count| {
            let mut c = count.borrow_mut();
            *c -= 1;
            *c == 0
        });

        if should_deallocate {
            // Deallocate the entire ArcInner without dropping it
            unsafe {
                // Drop the Mutex fields (which contain RefCells)
                core::ptr::drop_in_place(&mut (*self.ptr.as_ptr()).strong);
                core::ptr::drop_in_place(&mut (*self.ptr.as_ptr()).weak);

                // Now deallocate the memory
                let layout = Layout::for_value(self.ptr.as_ref());
                alloc::alloc::dealloc(self.ptr.as_ptr() as *mut u8, layout);
            }
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Weak<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(Weak)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn test_arc_new() {
        let arc = Arc::new(42);
        assert_eq!(*arc, 42);
    }

    #[test]
    fn test_arc_clone() {
        let arc1 = Arc::new(100);
        let arc2 = arc1.clone();

        assert_eq!(*arc1, 100);
        assert_eq!(*arc2, 100);
        assert_eq!(Arc::strong_count(&arc1), 2);
    }

    #[test]
    fn test_arc_strong_count() {
        let arc1 = Arc::new(String::from("hello"));
        assert_eq!(Arc::strong_count(&arc1), 1);

        let arc2 = arc1.clone();
        assert_eq!(Arc::strong_count(&arc1), 2);
        assert_eq!(Arc::strong_count(&arc2), 2);

        let arc3 = arc1.clone();
        assert_eq!(Arc::strong_count(&arc1), 3);

        drop(arc2);
        assert_eq!(Arc::strong_count(&arc1), 2);

        drop(arc3);
        assert_eq!(Arc::strong_count(&arc1), 1);
    }

    #[test]
    fn test_arc_drop() {
        let arc1 = Arc::new(Vec::from([1, 2, 3]));
        let arc2 = arc1.clone();

        drop(arc1);
        assert_eq!(Arc::strong_count(&arc2), 1);
        assert_eq!(*arc2, vec![1, 2, 3]);
    }

    #[test]
    fn test_arc_ptr_eq() {
        let arc1 = Arc::new(5);
        let arc2 = arc1.clone();
        let arc3 = Arc::new(5);

        assert!(Arc::ptr_eq(&arc1, &arc2));
        assert!(!Arc::ptr_eq(&arc1, &arc3));
    }

    #[test]
    fn test_arc_get_mut() {
        let mut arc = Arc::new(10);

        // Should get mutable reference when count is 1
        if let Some(val) = Arc::get_mut(&mut arc) {
            *val = 20;
        }
        assert_eq!(*arc, 20);

        // Should not get mutable reference when count > 1
        let arc2 = arc.clone();
        assert!(Arc::get_mut(&mut arc).is_none());

        drop(arc2);
        // Should get mutable reference again after dropping clone
        if let Some(val) = Arc::get_mut(&mut arc) {
            *val = 30;
        }
        assert_eq!(*arc, 30);
    }

    #[test]
    fn test_arc_equality() {
        let arc1 = Arc::new(42);
        let arc2 = Arc::new(42);
        let arc3 = Arc::new(99);

        assert_eq!(arc1, arc2);
        assert_ne!(arc1, arc3);
    }

    #[test]
    fn test_arc_default() {
        let arc: Arc<i32> = Arc::default();
        assert_eq!(*arc, 0);

        let arc_string: Arc<String> = Arc::default();
        assert_eq!(*arc_string, String::new());
    }

    #[test]
    fn test_weak_upgrade() {
        let arc = Arc::new(String::from("test"));
        let weak = Arc::downgrade(&arc);

        // Should be able to upgrade while Arc exists
        let upgraded = weak.upgrade();
        assert!(upgraded.is_some());
        assert_eq!(*upgraded.unwrap(), "test");

        drop(arc);

        // Should not be able to upgrade after Arc is dropped
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn test_weak_count() {
        let arc = Arc::new(100);
        assert_eq!(Arc::weak_count(&arc), 0);

        let weak1 = Arc::downgrade(&arc);
        assert_eq!(Arc::weak_count(&arc), 1);
        assert_eq!(weak1.weak_count(), 1);

        let weak2 = weak1.clone();
        assert_eq!(Arc::weak_count(&arc), 2);
        assert_eq!(weak1.weak_count(), 2);

        drop(weak1);
        assert_eq!(Arc::weak_count(&arc), 1);

        drop(weak2);
        assert_eq!(Arc::weak_count(&arc), 0);
    }

    #[test]
    fn test_weak_strong_count() {
        let arc1 = Arc::new(42);
        let weak = Arc::downgrade(&arc1);

        assert_eq!(weak.strong_count(), 1);

        let arc2 = arc1.clone();
        assert_eq!(weak.strong_count(), 2);

        drop(arc1);
        assert_eq!(weak.strong_count(), 1);

        drop(arc2);
        assert_eq!(weak.strong_count(), 0);
    }

    #[test]
    fn test_weak_ptr_eq() {
        let arc = Arc::new(5);
        let weak1 = Arc::downgrade(&arc);
        let weak2 = weak1.clone();

        let arc2 = Arc::new(5);
        let weak3 = Arc::downgrade(&arc2);

        assert!(weak1.ptr_eq(&weak2));
        assert!(!weak1.ptr_eq(&weak3));
    }

    #[test]
    fn test_weak_clone() {
        let arc = Arc::new(String::from("clone test"));
        let weak1 = Arc::downgrade(&arc);
        let weak2 = weak1.clone();

        assert_eq!(weak1.weak_count(), 2);
        assert_eq!(weak2.weak_count(), 2);

        let upgraded1 = weak1.upgrade().unwrap();
        let upgraded2 = weak2.upgrade().unwrap();

        assert_eq!(*upgraded1, "clone test");
        assert_eq!(*upgraded2, "clone test");
    }

    #[test]
    fn test_arc_with_weak_get_mut() {
        let mut arc = Arc::new(15);
        let weak = Arc::downgrade(&arc);

        // Should not get mutable reference when weak reference exists
        assert!(Arc::get_mut(&mut arc).is_none());

        drop(weak);

        // Should get mutable reference after dropping weak
        if let Some(val) = Arc::get_mut(&mut arc) {
            *val = 25;
        }
        assert_eq!(*arc, 25);
    }

    #[test]
    fn test_complex_scenario() {
        // Create an Arc
        let arc1 = Arc::new(Vec::from([1, 2, 3, 4, 5]));
        assert_eq!(Arc::strong_count(&arc1), 1);

        // Create multiple strong references
        let arc2 = arc1.clone();
        let arc3 = arc1.clone();
        assert_eq!(Arc::strong_count(&arc1), 3);

        // Create weak references
        let weak1 = Arc::downgrade(&arc1);
        let weak2 = Arc::downgrade(&arc2);
        assert_eq!(Arc::weak_count(&arc1), 2);

        // Upgrade weak references
        let arc4 = weak1.upgrade().unwrap();
        assert_eq!(Arc::strong_count(&arc1), 4);

        // Drop some strong references
        drop(arc2);
        drop(arc3);
        assert_eq!(Arc::strong_count(&arc1), 2);

        // Weak references should still work
        assert!(weak1.upgrade().is_some());
        assert!(weak2.upgrade().is_some());

        // Drop all strong references
        drop(arc1);
        drop(arc4);

        // Weak references should no longer upgrade
        assert!(weak1.upgrade().is_none());
        assert!(weak2.upgrade().is_none());
    }

    #[test]
    fn test_arc_debug_display() {
        let arc = Arc::new(42);
        let debug_str = alloc::format!("{:?}", arc);
        assert_eq!(debug_str, "42");

        let display_str = alloc::format!("{}", arc);
        assert_eq!(display_str, "42");
    }

    #[test]
    fn test_multiple_types() {
        // Test with different types
        let arc_int = Arc::new(123);
        assert_eq!(*arc_int, 123);

        let arc_string = Arc::new(String::from("hello world"));
        assert_eq!(*arc_string, "hello world");

        let arc_vec = Arc::new(Vec::from([1, 2, 3]));
        assert_eq!(*arc_vec, vec![1, 2, 3]);

        let arc_tuple = Arc::new((1, "two", 3.0));
        assert_eq!(*arc_tuple, (1, "two", 3.0));
    }

    #[test]
    fn test_nested_arc() {
        let inner = Arc::new(42);
        let outer = Arc::new(inner.clone());

        assert_eq!(**outer, 42);
        assert_eq!(Arc::strong_count(&inner), 2);
    }
}
