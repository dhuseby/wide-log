//! The future type behind the generated async `scope()` family.
//!
//! **Feature gate:** compiled only with the `tokio` feature.
//!
//! The generated `scope()` functions used to be `async fn`s. An `async fn`
//! that awaits a future it took as an argument stores that future twice:
//! once as the captured argument and once inside the awaited value, and
//! the compiler does not let the two share bytes. [`ScopeFuture`] is a
//! hand-written future that preserves lazy initialization while storing
//! the wrapped future once: its "not started" and "running" states are
//! variants of one enum, so they share the same bytes.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project_lite::pin_project;

pin_project! {
    /// Runs `init` on the first poll, then polls the future that `init`
    /// returned to completion, then drops that future and the guard.
    ///
    /// The generated `scope()` family returns this future. `init` is a
    /// closure that owns the user's future and builds the event; it
    /// returns the user's future inside the task-local scope(s) and the
    /// guard that owns the event and emits it on drop.
    ///
    /// Lazy initialization, normal completion, and cancellation behave
    /// like this `async fn`:
    ///
    /// ```text
    /// async fn scope_future(init: I) -> Fut::Output {
    ///     let (future, guard) = init();
    ///     future.await
    /// }
    /// ```
    ///
    /// - Nothing runs before the first poll. A `ScopeFuture` that is
    ///   dropped before its first poll never calls `init`.
    /// - On completion, the future drops first and the guard second,
    ///   inside the `poll` call that returns `Poll::Ready`.
    /// - On cancellation (the `ScopeFuture` is dropped before completion),
    ///   the future drops first and the guard second, so the event is
    ///   still alive while the wrapped future's destructors run.
    ///
    /// # Panics
    ///
    /// `poll` panics if it is called again after it returned
    /// `Poll::Ready`, after `init` panicked, or after the wrapped future's
    /// `poll` panicked. If a caller catches a panic from the wrapped
    /// future, the future and guard remain owned by this `ScopeFuture`
    /// until it is dropped; the guard emits when that drop occurs.
    #[project = ScopeProj]
    #[project_replace = ScopeProjReplace]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub enum ScopeFuture<I, Fut, G> {
        /// Not polled yet.
        Init {
            init: I,
        },
        /// `init` has run. `future` is declared before `guard`: fields
        /// drop in declaration order.
        Running {
            #[pin]
            future: Fut,
            guard: G,
            // Set before polling `future` and cleared after `Pending`.
            // A panic therefore poisons this state against another poll.
            poll_panicked: bool,
        },
        /// Completed, or `init` panicked.
        Done,
    }
}

impl<I, Fut, G> ScopeFuture<I, Fut, G>
where
    I: FnOnce() -> (Fut, G),
{
    /// Creates a future that calls `init` on its first poll.
    #[inline]
    pub fn new(init: I) -> Self {
        Self::Init { init }
    }
}

impl<I, Fut, G> Future for ScopeFuture<I, Fut, G>
where
    I: FnOnce() -> (Fut, G),
    Fut: Future,
{
    type Output = Fut::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if matches!(*self, Self::Init { .. }) {
            // The state is `Done` while `init` runs, so a panic in `init`
            // leaves this future in a valid, finished state.
            if let ScopeProjReplace::Init { init } = self.as_mut().project_replace(Self::Done) {
                let (future, guard) = init();
                self.set(Self::Running {
                    future,
                    guard,
                    poll_panicked: false,
                });
            }
        }
        match self.as_mut().project() {
            ScopeProj::Running {
                future,
                poll_panicked,
                ..
            } => {
                if *poll_panicked {
                    panic!("`ScopeFuture` polled after its wrapped future panicked");
                }
                *poll_panicked = true;
                match future.poll(cx) {
                    Poll::Pending => {
                        *poll_panicked = false;
                        Poll::Pending
                    }
                    Poll::Ready(output) => {
                        // Drops the future, then the guard, before the
                        // output is returned.
                        self.set(Self::Done);
                        Poll::Ready(output)
                    }
                }
            }
            // `Init` cannot occur here: the block above replaced it.
            ScopeProj::Init { .. } | ScopeProj::Done => {
                panic!("`ScopeFuture` polled after completion")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::pin::pin;
    use std::rc::Rc;
    use std::task::Waker;

    type DropLog = Rc<RefCell<Vec<&'static str>>>;

    /// Records `name` in the log when dropped.
    struct RecordDrop {
        name: &'static str,
        log: DropLog,
    }

    impl Drop for RecordDrop {
        fn drop(&mut self) {
            self.log.borrow_mut().push(self.name);
        }
    }

    /// Pending on the first poll, ready with `7` on the second. Records
    /// its own drop as `"future"`.
    struct PendingOnce {
        polled: bool,
        _on_drop: RecordDrop,
    }

    impl Future for PendingOnce {
        type Output = u32;

        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<u32> {
            if self.polled {
                Poll::Ready(7)
            } else {
                self.polled = true;
                Poll::Pending
            }
        }
    }

    /// Panics when polled and counts every poll. Records its own drop as
    /// `"future"`.
    struct PanicOnPoll {
        polls: Rc<Cell<u32>>,
        _on_drop: RecordDrop,
    }

    impl Future for PanicOnPoll {
        type Output = u32;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<u32> {
            self.polls.set(self.polls.get() + 1);
            panic!("inner poll panic");
        }
    }

    /// A `ScopeFuture` whose `init` counts its calls in `init_calls` and
    /// whose future and guard record their drops in `log`.
    fn scope_future(log: &DropLog, init_calls: &Rc<Cell<u32>>) -> impl Future<Output = u32> {
        let log = log.clone();
        let init_calls = init_calls.clone();
        ScopeFuture::new(move || {
            init_calls.set(init_calls.get() + 1);
            let future = PendingOnce {
                polled: false,
                _on_drop: RecordDrop {
                    name: "future",
                    log: log.clone(),
                },
            };
            let guard = RecordDrop { name: "guard", log };
            (future, guard)
        })
    }

    #[test]
    fn init_runs_once_on_the_first_poll() {
        let log = DropLog::default();
        let init_calls = Rc::new(Cell::new(0));
        let mut fut = pin!(scope_future(&log, &init_calls));
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(init_calls.get(), 0, "init does not run before a poll");

        assert_eq!(fut.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(init_calls.get(), 1);

        assert_eq!(fut.as_mut().poll(&mut cx), Poll::Ready(7));
        assert_eq!(init_calls.get(), 1);
    }

    #[test]
    fn completion_drops_the_future_then_the_guard() {
        let log = DropLog::default();
        let init_calls = Rc::new(Cell::new(0));
        let mut fut = pin!(scope_future(&log, &init_calls));
        let mut cx = Context::from_waker(Waker::noop());

        assert_eq!(fut.as_mut().poll(&mut cx), Poll::Pending);
        assert!(log.borrow().is_empty(), "both live while pending");

        assert_eq!(fut.as_mut().poll(&mut cx), Poll::Ready(7));
        assert_eq!(
            *log.borrow(),
            vec!["future", "guard"],
            "both drop inside the completing poll"
        );
    }

    #[test]
    fn cancellation_drops_the_future_then_the_guard() {
        let log = DropLog::default();
        let init_calls = Rc::new(Cell::new(0));
        {
            let mut fut = pin!(scope_future(&log, &init_calls));
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(fut.as_mut().poll(&mut cx), Poll::Pending);
        }
        assert_eq!(*log.borrow(), vec!["future", "guard"]);
    }

    #[test]
    fn unpolled_drop_never_runs_init() {
        let log = DropLog::default();
        let init_calls = Rc::new(Cell::new(0));
        drop(scope_future(&log, &init_calls));
        assert_eq!(init_calls.get(), 0);
        assert!(log.borrow().is_empty());
    }

    #[test]
    #[should_panic(expected = "polled after completion")]
    fn poll_after_completion_panics() {
        let log = DropLog::default();
        let init_calls = Rc::new(Cell::new(0));
        let mut fut = pin!(scope_future(&log, &init_calls));
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(fut.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(fut.as_mut().poll(&mut cx), Poll::Ready(7));
        let _ = fut.as_mut().poll(&mut cx);
    }

    #[test]
    fn inner_poll_panic_poisons_until_the_scope_future_is_dropped() {
        let log = DropLog::default();
        let polls = Rc::new(Cell::new(0));
        let future_log = log.clone();
        let future_polls = polls.clone();
        let guard_log = log.clone();
        let mut future = Box::pin(ScopeFuture::new(move || {
            let future = PanicOnPoll {
                polls: future_polls,
                _on_drop: RecordDrop {
                    name: "future",
                    log: future_log,
                },
            };
            let guard = RecordDrop {
                name: "guard",
                log: guard_log,
            };
            (future, guard)
        }));
        let mut cx = Context::from_waker(Waker::noop());

        let first_poll = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut cx)));
        assert!(first_poll.is_err());
        assert_eq!(polls.get(), 1);
        assert!(log.borrow().is_empty(), "state is retained after panic");

        let repoll = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut cx)));
        assert!(repoll.is_err());
        assert_eq!(polls.get(), 1, "the panicked inner future was repolled");
        assert!(log.borrow().is_empty(), "state lives until drop");

        drop(future);
        assert_eq!(*log.borrow(), vec!["future", "guard"]);
    }

    #[test]
    fn init_and_running_states_share_storage() {
        // The closure owns a 4096-byte value and the running state owns
        // the same value. The enum must hold it once, not twice.
        const BIG_SIZE: usize = 4096;
        let payload = [0u8; BIG_SIZE];
        let fut = ScopeFuture::new(move || (std::future::ready(payload), ()));
        let size = std::mem::size_of_val(&fut);
        assert!(
            size < BIG_SIZE + BIG_SIZE / 2,
            "ScopeFuture is {size} bytes for a {BIG_SIZE}-byte payload"
        );
    }
}
