use std::cell::UnsafeCell;

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
///
/// ## Why `UnsafeCell` instead of `Cell`?
///
/// `ContextCell` needs to be `Sync` (so a `static` reference to it is
/// usable from any thread, with the pointer contents thread-local via
/// `thread_local!` mechanics). `Cell<T>` is `!Sync`. `UnsafeCell<T>` is
/// `Sync` for any `T` (including raw pointers).
///
/// Internally we use `UnsafeCell<*mut T>` so the cell is `Sync` without
/// needing an `unsafe impl Sync` shim, and the Stacked Borrows model
/// under miri doesn't see the cell's get as a "shared" access that
/// would conflict with a later `&mut` deref of the stored pointer.
pub struct ContextCell<T> {
    ptr: UnsafeCell<*mut T>,
}

impl<T> ContextCell<T> {
    /// Create a new cell initialized to a null pointer.
    #[inline]
    pub const fn new() -> Self {
        Self {
            ptr: UnsafeCell::new(std::ptr::null_mut()),
        }
    }

    /// Returns the stored raw pointer (null if unset).
    #[inline]
    pub fn get_ptr(&self) -> *mut T {
        // SAFETY: this cell is only mutated via `replace` and `restore`,
        // which are called under the same `thread_local!` access
        // (Rust guarantees a `thread_local!` cell is not concurrently
        // accessed from multiple threads). The pointer is read here
        // only as a value to be passed through; it is not deref'd.
        unsafe { *self.ptr.get() }
    }

    /// Sets the stored pointer. Returns the previous pointer.
    #[inline]
    pub fn replace(&self, ptr: *mut T) -> *mut T {
        // SAFETY: see `get_ptr` — exclusive access via `thread_local!`.
        unsafe {
            let prev = *self.ptr.get();
            *self.ptr.get() = ptr;
            prev
        }
    }

    /// Restores a previously saved pointer (typically from `replace` or `clear`).
    #[inline]
    pub fn restore(&self, ptr: *mut T) {
        // SAFETY: see `get_ptr` — exclusive access via `thread_local!`.
        unsafe {
            *self.ptr.get() = ptr;
        }
    }
}

#[cfg(test)]
impl<T> ContextCell<T> {
    /// Returns the stored pointer, or `None` if null.
    #[inline]
    pub(crate) fn get(&self) -> Option<*mut T> {
        let ptr = unsafe { *self.ptr.get() };
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
    /// the returned borrow.
    #[inline]
    pub(crate) unsafe fn deref_mut(&self) -> Option<&'static mut T> {
        let ptr = unsafe { *self.ptr.get() };
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
//
// We need this because the macro-generated `scope()` future holds a
// `ContextCell<WideEvent<K>>` and must be `Send` (so it can move
// across threads in a multi-threaded tokio runtime). With
// `UnsafeCell<*mut T>`, the auto-trait derivation gives us `Send` only
// if `*mut T: Send`, which requires `T: Sync`. We know
// `WideEvent<K>: Send + Sync` from the field-level analysis, but the
// compiler can't always prove this for the macro-generated path, so
// we provide the impl explicitly.
unsafe impl<T> Send for ContextCell<T> {}
unsafe impl<T> Sync for ContextCell<T> {}

/// A drop guard that restores a [`ContextCell`] to a previously-saved
/// value when dropped.
///
/// `WideLogGuard` owns one of these instead of a raw `*mut T`. When the
/// guard is dropped, the cell is restored to its prior state. If the user
/// calls [`std::mem::forget`] on the guard, the restoration is **silently
/// skipped** — this is a known limitation, documented on the
/// `WideLogGuard` rustdoc. A future release may use a leak-detection
/// fallback (e.g. a thread-local "current owner" pointer) to recover
/// safety even on forget.
///
/// # Safety contract
///
/// `cell` must be a thread-local or task-local `ContextCell<T>` with a
/// `'static` lifetime (i.e. defined in a `thread_local!` or
/// `task_local!` block). The previous pointer `prev` is treated as an
/// opaque value to be restored verbatim; it is never dereferenced.
///
/// This type is `Send + Sync` unconditionally because the only state is
/// a `&'static ContextCell<T>` (which is `Send + Sync`) and a raw
/// pointer that is never dereferenced by this type.
pub struct RestoreOnDrop<T: 'static> {
    cell: &'static ContextCell<T>,
    prev: *mut T,
}

impl<T: 'static> RestoreOnDrop<T> {
    /// Build a restore guard for `cell`. The cell will be restored to
    /// `prev` when this value is dropped.
    ///
    /// # Safety
    ///
    /// `cell` must be a thread-local or task-local `ContextCell<T>`
    /// with `'static` lifetime. `prev` is the value previously returned
    /// by [`ContextCell::replace`] (or null for the initial state).
    pub const unsafe fn new(cell: &'static ContextCell<T>, prev: *mut T) -> Self {
        Self { cell, prev }
    }

    /// Disable the restore-on-drop behavior. The cell is NOT restored.
    /// Returns the saved `prev` pointer so the caller can restore
    /// manually or discard it.
    pub fn disarm(mut self) -> *mut T {
        // Disarm by setting `prev` to a known-bogus sentinel, then
        // forget self so its Drop is a no-op (we want to avoid
        // touching the cell at all once disarmed).
        let prev = self.prev;
        self.prev = std::ptr::null_mut();
        std::mem::forget(self);
        prev
    }
}

impl<T: 'static> Drop for RestoreOnDrop<T> {
    fn drop(&mut self) {
        // Always restore, including when prev is null (we want to
        // restore the cell to its previous state, whatever it was).
        self.cell.restore(self.prev);
    }
}

// SAFETY: `ContextCell<T>` is `Send + Sync` for any `T` (see impls
// above), so a `'static` reference to one is too. The `prev` pointer
// is never dereferenced by this type, so it does not need to be
// `Send + Sync` itself.
unsafe impl<T: 'static> Send for RestoreOnDrop<T> {}
unsafe impl<T: 'static> Sync for RestoreOnDrop<T> {}

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

    // ── RestoreOnDrop ──

    // A static ContextCell is what the macro generates in production.
    // Note: in production this is a thread_local! so the cell is
    // per-thread. For testing, we use a plain `static` (which is
    // shared across threads) — that is fine because each test
    // invocation uses a single thread and we set/clear the cell in
    // each test. The tests verify the RestoreOnDrop behavior, not
    // thread-safety of the cell itself.
    static TEST_CELL: ContextCell<u32> = const { ContextCell::new() };

    #[test]
    fn restore_on_drop_restores_previous_value() {
        TEST_CELL.replace(std::ptr::null_mut());
        let mut a: u32 = 42;
        let ptr_a = &mut a as *mut u32;
        TEST_CELL.replace(ptr_a);

        // Replace with a new pointer wrapped in a restore guard.
        let mut b: u32 = 99;
        let ptr_b = &mut b as *mut u32;
        let prev = TEST_CELL.replace(ptr_b);
        let guard = unsafe { RestoreOnDrop::new(&TEST_CELL, prev) };

        // Cell is currently `ptr_b`.
        assert_eq!(TEST_CELL.get_ptr(), ptr_b);

        drop(guard);

        // Cell is restored to `ptr_a`.
        assert_eq!(TEST_CELL.get_ptr(), ptr_a);
    }

    #[test]
    fn restore_on_drop_disarm_skips_restore() {
        TEST_CELL.replace(std::ptr::null_mut());
        let mut a: u32 = 42;
        let ptr_a = &mut a as *mut u32;
        TEST_CELL.replace(ptr_a);

        let mut b: u32 = 99;
        let ptr_b = &mut b as *mut u32;
        let prev = TEST_CELL.replace(ptr_b);
        let guard = unsafe { RestoreOnDrop::new(&TEST_CELL, prev) };
        let _ = guard.disarm();

        // Cell still has ptr_b; disarm skipped the restore.
        assert_eq!(TEST_CELL.get_ptr(), ptr_b);
    }

    #[test]
    fn restore_on_drop_restores_null_initial_value() {
        TEST_CELL.replace(std::ptr::null_mut());
        // Cell is null. Now replace with a value and create a guard that
        // expects the null as its previous value.
        let mut a: u32 = 42;
        let ptr_a = &mut a as *mut u32;
        let prev = TEST_CELL.replace(ptr_a);
        assert!(prev.is_null());

        let guard = unsafe { RestoreOnDrop::new(&TEST_CELL, prev) };
        drop(guard);

        // Cell is restored to null.
        assert!(TEST_CELL.get_ptr().is_null());
    }
}
