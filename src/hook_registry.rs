//! Type-erased append registry for the crate-root level macros.
//!
//! **Feature gate:** compiled only when the `tracing` feature is off. With
//! the feature on, the crate re-exports `tracing`'s level macros and
//! records reach the active event through `WideLogCaptureLayer`, so no
//! registry exists.
//!
//! The `wide_log!` macro generates its level macros inside the crate that
//! invokes it, so a dependent crate that never invokes `wide_log!` has no
//! generated macro to import. The `#[macro_export]`-ed level macros on the
//! wide-log crate root close that gap: they append through a hook
//! installed by the innermost active wide-log guard instead of through a
//! schema crate's typed `CURRENT_EVENT` directly.
//!
//! Invariants:
//!
//! - Every guard-establishing path in a schema crate pushes one hook and
//!   pops it on drop, so the hook-stack nesting mirrors the
//!   `CURRENT_EVENT` / `TASK_EVENT` nesting exactly. The innermost hook
//!   serves appends.
//! - A sync guard must drop on the thread that built it (the same
//!   invariant that keeps `CURRENT_EVENT` restoration sound). A pop that
//!   finds nothing to remove signals a mis-nested stack — for example a
//!   guard dropped on a different thread — and is a no-op with a
//!   `debug_assert!` in debug builds.
//! - Appends are best-effort: with no active guard the macros are no-ops,
//!   and a re-entrant format-arg append while the format buffer is held
//!   is skipped rather than panicking.

use std::cell::RefCell;

/// A hook installed by an active wide-log guard. Called with the entry
/// level and the rendered message; the hook appends the `{level, message}`
/// entry to the event active on the calling thread (or task, under the
/// `tokio` feature).
pub type LogHook = for<'a> fn(&'static str, &'a str);

thread_local! {
    // Hook stack for sync guards. Pushed and popped by the generated
    // guard paths so the nesting mirrors `CURRENT_EVENT`.
    static LOG_HOOKS: RefCell<Vec<LogHook>> = const { RefCell::new(Vec::new()) };

    // Reusable format buffer backing the format-arg variants of the
    // crate-root level macros. Cleared (not freed) between calls so the
    // underlying allocation is reused across log calls on this thread.
    static LOG_FMT_BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

#[cfg(feature = "tokio")]
tokio::task_local! {
    // Hook stack for guards established inside an async `scope()`. Checked
    // before the thread-local stack, mirroring `current()`'s
    // task-local-first order.
    static TASK_LOG_HOOKS: RefCell<Vec<LogHook>>;
}

/// Runs `f` with `hook` installed as the task's log hook.
///
/// The generated async `scope()` family wraps its future in this call so
/// crate-root macro appends from anywhere in the task reach the scoped
/// event. The hook is active exactly while the task scope is: tokio
/// restores the previous stack on completion, on cancellation (the scope
/// future's drop re-enters the task-local), and during unwinding, so no
/// separate drop guard is needed.
#[cfg(feature = "tokio")]
#[inline]
pub async fn scope_log_hook<F: std::future::Future>(hook: LogHook, f: F) -> F::Output {
    TASK_LOG_HOOKS.scope(RefCell::new(vec![hook]), f).await
}

/// Number of hooks on the innermost active stack for the calling context:
/// the task stack under the `tokio` feature when a task scope is active,
/// the thread stack otherwise. Diagnostic surface for the hook-stack
/// nesting invariant; not part of the public API.
#[doc(hidden)]
#[inline]
pub fn log_hook_stack_depth() -> usize {
    #[cfg(feature = "tokio")]
    if let Ok(depth) = TASK_LOG_HOOKS.try_with(|cell| cell.borrow().len()) {
        return depth;
    }
    LOG_HOOKS.with(|cell| cell.borrow().len())
}

/// Removes the last occurrence of `hook` from `hooks`, returning whether
/// one was found. Address comparison through `fn_addr_eq`: each schema
/// crate emits exactly one hook shim function, so address identity is the
/// matching key (same-crate duplicates would be the same function; the
/// comparison never has to distinguish merged codegen units).
fn remove_hook(hooks: &mut Vec<LogHook>, hook: LogHook) -> bool {
    match hooks.iter().rposition(|h| std::ptr::fn_addr_eq(*h, hook)) {
        Some(pos) => {
            hooks.remove(pos);
            true
        }
        None => false,
    }
}

/// Installs `hook` as the innermost hook on the active stack: the
/// task-local stack under the `tokio` feature when a task scope is active,
/// the thread-local stack otherwise.
///
/// Every push must be matched by a pop for the same hook — see
/// [`pop_log_hook`] and [`LogHookGuard`] for the two teardown forms.
#[inline]
pub fn push_log_hook(hook: LogHook) {
    #[cfg(feature = "tokio")]
    if TASK_LOG_HOOKS
        .try_with(|cell| cell.borrow_mut().push(hook))
        .is_ok()
    {
        return;
    }
    LOG_HOOKS.with(|cell| cell.borrow_mut().push(hook));
}

/// Removes the innermost occurrence of `hook` from the active stack. A
/// no-op (with a `debug_assert!` in debug builds) when no active stack
/// contains it: a pop that finds nothing signals a mis-nested hook stack,
/// such as a sync guard dropped on a thread other than the one that built
/// it.
#[inline]
pub fn pop_log_hook(hook: LogHook) {
    #[cfg(feature = "tokio")]
    if TASK_LOG_HOOKS
        .try_with(|cell| remove_hook(&mut cell.borrow_mut(), hook))
        .unwrap_or(false)
    {
        return;
    }
    let removed = LOG_HOOKS.with(|cell| remove_hook(&mut cell.borrow_mut(), hook));
    debug_assert!(
        removed,
        "wide-log: log hook stack mis-nested on pop; \
         a guard dropped off its building thread or task scope"
    );
}

/// Appends a `{level, message}` entry through the innermost active hook.
///
/// A no-op when no guard is active (the hook stack is empty) — the same
/// best-effort contract as every wide-log macro. The hook is copied out of
/// the stack cell before the call, so a hook that logs again cannot
/// contend on the cell's borrow.
#[inline]
pub fn append_log_entry(level: &'static str, message: &str) {
    #[cfg(feature = "tokio")]
    {
        let hook = TASK_LOG_HOOKS
            .try_with(|cell| cell.borrow().last().copied())
            .ok()
            .flatten();
        if let Some(hook) = hook {
            hook(level, message);
            return;
        }
    }
    let hook = LOG_HOOKS.with(|cell| cell.borrow().last().copied());
    if let Some(hook) = hook {
        hook(level, message);
    }
}

/// Format-arg variant of [`append_log_entry`]: renders `args` into the
/// crate's reusable thread-local format buffer and appends the result.
///
/// The buffer borrow is held across the hook call, so a hook that logs
/// again with format args finds the buffer already borrowed and its
/// append is skipped (best-effort, never a panic); a re-entrant literal
/// append passes through to the same event.
#[inline]
pub fn append_log_entry_fmt(level: &'static str, args: std::fmt::Arguments<'_>) {
    LOG_FMT_BUF.with(|cell| {
        let mut buf = match cell.try_borrow_mut() {
            Ok(buf) => buf,
            // Already formatting on this thread (a hook logged with format
            // args re-entrantly): skip the entry rather than panic.
            Err(_) => return,
        };
        buf.clear();
        let _ = ::std::fmt::Write::write_fmt(&mut *buf, args);
        append_log_entry(level, buf.as_str());
    });
}

/// A guard that pops the hook installed by [`LogHookGuard::new`] on drop.
///
/// Scope teardown is panic-safe: the pop runs during unwinding as well as
/// on normal scope exit. Under the `tokio` feature the pop resolves
/// against the task-local stack while the task scope is active — the
/// guard must be dropped inside the task scope that installed its hook
/// (the generated `scope()` family installs and drops inside the scoped
/// future).
#[must_use = "the guard pops the hook on drop; binding to `_guard` is fine, \
              but `let _ = ...` discards the guard without popping"]
pub struct LogHookGuard {
    hook: LogHook,
}

impl LogHookGuard {
    /// Installs `hook` as the innermost hook and returns a guard that
    /// pops it on drop.
    #[inline]
    pub fn new(hook: LogHook) -> Self {
        push_log_hook(hook);
        Self { hook }
    }
}

impl Drop for LogHookGuard {
    fn drop(&mut self) {
        pop_log_hook(self.hook);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Each test owns a distinct recorder static so tests running in
    // parallel on different threads never observe each other's entries.
    // Every test leaves the hook stack as it found it (all pushes are
    // matched by pops), so the thread-local stack is empty at each test
    // start under any test-threads setting.

    static ORDER_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
    fn order_hook(level: &'static str, message: &str) {
        ORDER_CALLS
            .lock()
            .unwrap()
            .push((level, message.to_string()));
    }

    static FMT_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
    fn fmt_hook(level: &'static str, message: &str) {
        FMT_CALLS.lock().unwrap().push((level, message.to_string()));
    }

    static UNWIND_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
    fn unwind_hook(level: &'static str, message: &str) {
        UNWIND_CALLS
            .lock()
            .unwrap()
            .push((level, message.to_string()));
    }

    static REENTRANT_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
    fn reentrant_literal_hook(level: &'static str, message: &str) {
        REENTRANT_CALLS
            .lock()
            .unwrap()
            .push((level, message.to_string()));
        if message == "outer" {
            // An append from inside the hook reaches the same event.
            append_log_entry("debug", "nested");
        }
    }

    static REENTRANT_FMT_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
    fn reentrant_fmt_hook(level: &'static str, message: &str) {
        REENTRANT_FMT_CALLS
            .lock()
            .unwrap()
            .push((level, message.to_string()));
        // A format-arg append from inside the hook finds the format
        // buffer already borrowed and is skipped.
        append_log_entry_fmt("debug", format_args!("nested"));
    }

    #[test]
    fn append_without_active_hook_is_a_no_op() {
        // No guard active on this thread: the call must complete without
        // panicking and without recording anywhere.
        append_log_entry("info", "nothing active");
        append_log_entry_fmt("warn", format_args!("still nothing {}", 1));
    }

    #[test]
    fn innermost_hook_serves_and_pop_restores_the_previous_hook() {
        static INNER_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
        fn inner_hook(level: &'static str, message: &str) {
            INNER_CALLS
                .lock()
                .unwrap()
                .push((level, message.to_string()));
        }

        let _outer = LogHookGuard::new(order_hook);
        let _inner = LogHookGuard::new(inner_hook);

        append_log_entry("info", "innermost wins");
        drop(_inner);
        append_log_entry("warn", "outer serves again");
        append_log_entry_fmt("error", format_args!("outer fmt {}", 2));
        drop(_outer);

        assert_eq!(
            *INNER_CALLS.lock().unwrap(),
            vec![("info", "innermost wins".to_string())]
        );
        assert_eq!(
            *ORDER_CALLS.lock().unwrap(),
            vec![
                ("warn", "outer serves again".to_string()),
                ("error", "outer fmt 2".to_string()),
            ]
        );
    }

    #[test]
    fn format_args_render_through_the_crate_buffer() {
        let _guard = LogHookGuard::new(fmt_hook);
        append_log_entry_fmt("info", format_args!("x={} y={}", 1, "a"));
        // Longer than the buffer's current capacity: the buffer grows,
        // then is cleared (not freed) for the next call.
        let long = "l".repeat(4096);
        append_log_entry_fmt("debug", format_args!("{long}"));
        append_log_entry_fmt("warn", format_args!("short after long"));
        drop(_guard);

        assert_eq!(
            *FMT_CALLS.lock().unwrap(),
            vec![
                ("info", "x=1 y=a".to_string()),
                ("debug", long),
                ("warn", "short after long".to_string()),
            ]
        );
    }

    #[test]
    fn guard_pops_during_unwind() {
        let result = std::panic::catch_unwind(|| {
            let _guard = LogHookGuard::new(unwind_hook);
            append_log_entry("info", "before panic");
            panic!("scope torn down by unwind");
        });
        assert!(result.is_err());
        // The unwind popped the hook: this append is a no-op again.
        append_log_entry("warn", "after unwind");
        assert_eq!(
            *UNWIND_CALLS.lock().unwrap(),
            vec![("info", "before panic".to_string())]
        );
    }

    #[test]
    fn reentrant_literal_append_reaches_the_same_hook() {
        let _guard = LogHookGuard::new(reentrant_literal_hook);
        append_log_entry("info", "outer");
        drop(_guard);
        assert_eq!(
            *REENTRANT_CALLS.lock().unwrap(),
            vec![
                ("info", "outer".to_string()),
                ("debug", "nested".to_string()),
            ]
        );
    }

    #[test]
    fn reentrant_format_append_is_skipped_not_panicking() {
        let _guard = LogHookGuard::new(reentrant_fmt_hook);
        append_log_entry_fmt("info", format_args!("outer"));
        drop(_guard);
        assert_eq!(
            *REENTRANT_FMT_CALLS.lock().unwrap(),
            vec![("info", "outer".to_string())]
        );
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn task_local_stack_serves_appends_while_active() {
        static TASK_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
        static TLS_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
        fn task_hook(level: &'static str, message: &str) {
            TASK_CALLS
                .lock()
                .unwrap()
                .push((level, message.to_string()));
        }
        fn tls_hook(level: &'static str, message: &str) {
            TLS_CALLS.lock().unwrap().push((level, message.to_string()));
        }

        // No task scope active yet: the push lands on the thread-local
        // stack.
        push_log_hook(tls_hook);
        append_log_entry("info", "outside");
        TASK_LOG_HOOKS
            .scope(RefCell::new(Vec::new()), async {
                // Task scope active: the guard installs on the task-local
                // stack, which shadows the thread-local stack.
                let _guard = LogHookGuard::new(task_hook);
                append_log_entry("info", "inside");
            })
            .await;
        append_log_entry("warn", "after");
        pop_log_hook(tls_hook);

        assert_eq!(
            *TLS_CALLS.lock().unwrap(),
            vec![
                ("info", "outside".to_string()),
                ("warn", "after".to_string()),
            ]
        );
        assert_eq!(
            *TASK_CALLS.lock().unwrap(),
            vec![("info", "inside".to_string())]
        );
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn scope_log_hook_installs_and_restores_the_task_stack() {
        static SCOPE_CALLS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
        fn scope_hook(level: &'static str, message: &str) {
            SCOPE_CALLS
                .lock()
                .unwrap()
                .push((level, message.to_string()));
        }

        assert_eq!(log_hook_stack_depth(), 0, "no stacks active at start");
        scope_log_hook(scope_hook, async {
            assert_eq!(
                log_hook_stack_depth(),
                1,
                "the scope's hook is the innermost entry"
            );
            append_log_entry("info", "from the scoped task");
        })
        .await;
        assert_eq!(
            log_hook_stack_depth(),
            0,
            "the task stack is restored after the scope completes"
        );
        append_log_entry("warn", "after the scope");
        assert_eq!(
            *SCOPE_CALLS.lock().unwrap(),
            vec![("info", "from the scoped task".to_string())]
        );
    }
}
