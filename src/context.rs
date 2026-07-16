use std::cell::Cell;

/// A thread-local cell storing a raw pointer to the current wide event.
///
/// This type provides safe methods for setting, getting, and saving/restoring
/// the pointer, which is used by the generated guard to manage nested scopes.
///
/// # Safety Invariants
///
/// The raw pointer stored in this cell is sound because:
///
/// 1. **The guard owns the event.** The `WideEvent<K>` is a field of the guard,
///    which is on the stack (or in the `scope()` future's state). The guard
///    outlives any code that calls `current()` within the same scope.
/// 2. **Set on creation, cleared on drop.** The guard's `new` sets the pointer;
///    its `Drop` restores the previous pointer (or null) before the inner
///    `ScopedGuard::Drop` runs.
/// 3. **Nested scopes** save/restore the previous pointer via `replace` and the
///    `prev_ptr` field on the guard.
/// 4. **No cross-thread access.** `thread_local!` means each thread has its own
///    cell. For async, `tokio::task_local!` moves with the task across threads.
///
/// The `&'static` lifetime returned by accessors is a "valid for the duration
/// of the guard" lifetime — the standard pattern used by `thread_local`
/// accessors (e.g., `tracing`'s `Span::current()`).
pub struct ContextCell<T> {
    ptr: Cell<*mut T>,
}

impl<T> ContextCell<T> {
    /// Create a new cell initialized to a null pointer.
    #[inline]
    pub const fn new() -> Self {
        Self {
            ptr: Cell::new(std::ptr::null_mut()),
        }
    }

    /// Returns the stored raw pointer (null if unset).
    #[inline]
    pub fn get_ptr(&self) -> *mut T {
        self.ptr.get()
    }

    /// Sets the stored pointer. Returns the previous pointer.
    #[inline]
    pub fn replace(&self, ptr: *mut T) -> *mut T {
        let prev = self.ptr.get();
        self.ptr.set(ptr);
        prev
    }

    /// Restores a previously saved pointer (typically from `replace` or `clear`).
    #[inline]
    pub fn restore(&self, ptr: *mut T) {
        self.ptr.set(ptr);
    }
}

#[cfg(test)]
impl<T> ContextCell<T> {
    /// Returns the stored pointer, or `None` if null.
    #[inline]
    pub(crate) fn get(&self) -> Option<*mut T> {
        let ptr = self.ptr.get();
        if ptr.is_null() { None } else { Some(ptr) }
    }

    /// Clears the stored pointer to null. Returns the previous pointer.
    #[inline]
    pub(crate) fn clear(&self) -> *mut T {
        self.replace(std::ptr::null_mut())
    }

    /// Returns a `&'static mut T` reference to the event if the pointer is non-null.
    ///
    /// # Safety
    ///
    /// The caller must ensure the pointer is valid and that the referenced `T`
    /// will not be accessed mutably from another reference for the duration of
    /// the returned borrow. In practice, this is guaranteed by the guard's
    /// lifetime — the guard outlives all `current()` calls within its scope.
    #[inline]
    pub(crate) unsafe fn deref_mut(&self) -> Option<&'static mut T> {
        let ptr = self.ptr.get();
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }
}

impl<T> Default for ContextCell<T> {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: The raw pointer inside `ContextCell` is only accessed via the
// thread-local or task-local storage. For thread-local access, each thread
// has its own cell, so there is no cross-thread sharing. For task-local
// access, the task moves as a unit across threads — the pointer is only
// dereferenced on the thread currently executing the task. The pointer is
// never accessed concurrently from multiple threads.
unsafe impl<T> Send for ContextCell<T> {}
unsafe impl<T> Sync for ContextCell<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_null() {
        let cell = ContextCell::<u32>::new();
        assert!(cell.get().is_none());
    }

    #[test]
    fn replace_returns_previous() {
        let cell = ContextCell::<u32>::new();
        let mut a: u32 = 1;
        let mut b: u32 = 2;
        let ptr_a = &mut a as *mut u32;
        let ptr_b = &mut b as *mut u32;

        let prev = cell.replace(ptr_a);
        assert!(prev.is_null());
        assert_eq!(cell.get(), Some(ptr_a));

        let prev = cell.replace(ptr_b);
        assert_eq!(prev, ptr_a);
        assert_eq!(cell.get(), Some(ptr_b));
    }

    #[test]
    fn clear_returns_previous() {
        let cell = ContextCell::<u32>::new();
        let mut a: u32 = 42;
        let ptr_a = &mut a as *mut u32;

        cell.replace(ptr_a);
        let prev = cell.clear();
        assert_eq!(prev, ptr_a);
        assert!(cell.get().is_none());
    }

    #[test]
    fn restore_sets_pointer() {
        let cell = ContextCell::<u32>::new();
        let mut a: u32 = 10;
        let ptr_a = &mut a as *mut u32;

        cell.restore(ptr_a);
        assert_eq!(cell.get(), Some(ptr_a));
    }

    #[test]
    fn deref_mut_returns_reference() {
        let cell = ContextCell::<u32>::new();
        let mut a: u32 = 99;
        let ptr_a = &mut a as *mut u32;
        cell.replace(ptr_a);

        let r = unsafe { cell.deref_mut() }.unwrap();
        assert_eq!(*r, 99);
        *r = 100;
        assert_eq!(a, 100);
    }

    #[test]
    fn deref_mut_none_when_null() {
        let cell = ContextCell::<u32>::new();
        assert!(unsafe { cell.deref_mut() }.is_none());
    }

    #[test]
    fn default_is_null() {
        let cell = ContextCell::<u32>::default();
        assert!(cell.get().is_none());
    }
}
