//! Stopping a conversion from another thread, or after a time limit.
//!
//! A [`CancelToken`] is made current on a thread by [`CancelToken::run`];
//! the pipeline calls [`check`] at its natural stopping points (each page,
//! each stage, each chunk of a download), and the worker threads it starts
//! inherit the token ([`current`], [`CancelToken::enter`]). Once the token is
//! cancelled, or past its deadline, the next [`check`] unwinds the stack up
//! to [`CancelToken::run`], which returns [`Cancelled`]: the functions of the
//! pipeline keep their signatures, and everything they hold is dropped on the
//! way, as for an early return.
//!
//! [`check`] reads one atomic flag (and the clock when a deadline is set):
//! the checks are at most a few per page, so they cost nothing measurable.
//! Without a current token, [`check`] does nothing.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Why a conversion stopped before its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelled {
    /// [`CancelToken::cancel`] was called (on the token or on its parent).
    Requested,
    /// The deadline of the token (or of its parent) passed.
    TimedOut,
}

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Requested => "the conversion was cancelled",
            Self::TimedOut => "the conversion took longer than its time limit",
        })
    }
}

impl std::error::Error for Cancelled {}

/// What a token holds, shared by its clones.
#[derive(Debug)]
struct Inner {
    /// Set by [`CancelToken::cancel`].
    cancelled: AtomicBool,
    /// When the work must stop, if it has a time limit.
    deadline: Option<Instant>,
    /// A token whose cancellation (or deadline) stops this one too.
    parent: Option<CancelToken>,
}

/// A request to stop a conversion, shared by the thread that runs it and
/// those that may stop it (clones share the same request).
///
/// ```
/// use kakemono::{CancelToken, Cancelled, Paper};
/// # fn main() -> anyhow::Result<()> {
/// let token = CancelToken::new();
/// let stopper = token.clone();
/// // Elsewhere, e.g. when the user closes the window: stopper.cancel();
/// # stopper.cancel();
/// let analysis = token.run(|| -> anyhow::Result<_> {
///     let paper = Paper::from_path("tests/data/1602.03837v1.pdf")?;
///     Ok(paper.analyze())
/// });
/// assert_eq!(analysis.err(), Some(Cancelled::Requested));
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CancelToken(Arc<Inner>);

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    /// A token with no time limit, not cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::make(None, None)
    }

    /// A token that times out `limit` from now.
    #[must_use]
    pub fn with_timeout(limit: Duration) -> Self {
        Self::make(Instant::now().checked_add(limit), None)
    }

    /// A token stopped by this one (by its cancellation or its deadline), and
    /// by its own: cancelling it does not cancel this one. `limit` is its own
    /// time limit, from now.
    #[must_use]
    pub fn child(&self, limit: Option<Duration>) -> Self {
        Self::make(
            limit.and_then(|limit| Instant::now().checked_add(limit)),
            Some(self.clone()),
        )
    }

    /// A token with these fields.
    fn make(deadline: Option<Instant>, parent: Option<Self>) -> Self {
        Self(Arc::new(Inner {
            cancelled: AtomicBool::new(false),
            deadline,
            parent,
        }))
    }

    /// Asks the work running with this token (and its children) to stop.
    /// It stops at its next stopping point, normally within a page.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }

    /// Why the work must stop, or `None` while it may go on.
    #[must_use]
    pub fn state(&self) -> Option<Cancelled> {
        let mut token = Some(self);
        while let Some(CancelToken(inner)) = token {
            if inner.cancelled.load(Ordering::Relaxed) {
                return Some(Cancelled::Requested);
            }
            if inner.deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Some(Cancelled::TimedOut);
            }
            token = inner.parent.as_ref();
        }
        None
    }

    /// Runs `work` on this thread with this token current, and returns its
    /// result, or why it stopped early. Work that ends normally keeps its
    /// result, even when the token was cancelled meanwhile; work is not
    /// started with a token already cancelled.
    ///
    /// # Errors
    /// [`Cancelled`] when the token was cancelled, or timed out, before
    /// `work` ended.
    ///
    /// # Panics
    /// When `work` panics: the panic goes on.
    pub fn run<R>(&self, work: impl FnOnce() -> R) -> Result<R, Cancelled> {
        if let Some(stopped) = self.state() {
            return Err(stopped);
        }
        let entered = self.enter();
        let outcome = catch_unwind(AssertUnwindSafe(work));
        drop(entered);
        match outcome {
            Ok(result) => Ok(result),
            Err(payload) => match payload.downcast::<Cancelled>() {
                Ok(stopped) => Err(*stopped),
                Err(payload) => resume_unwind(payload),
            },
        }
    }

    /// Makes this token current on this thread until the guard is dropped
    /// (then the one before is current again). A [`check`] that finds it
    /// cancelled unwinds: only call this where [`run`](Self::run) or a
    /// thread that hands panics on (such as a scoped thread) is above.
    pub(crate) fn enter(&self) -> Entered {
        Entered(CURRENT.with(|current| current.replace(Some(self.clone()))))
    }
}

thread_local! {
    /// The token of the work running on this thread.
    static CURRENT: RefCell<Option<CancelToken>> = const { RefCell::new(None) };
}

/// Makes the token before current again when dropped (see
/// [`CancelToken::enter`]).
pub(crate) struct Entered(Option<CancelToken>);

impl Drop for Entered {
    fn drop(&mut self) {
        let before = self.0.take();
        CURRENT.with(|current| *current.borrow_mut() = before);
    }
}

/// The token of the work running on this thread, for the threads it
/// starts: each makes it current with [`enter`].
pub(crate) fn current() -> Option<CancelToken> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Makes `token` current on this thread, if there is one (see
/// [`CancelToken::enter`]): for worker threads, with the token of the
/// thread that started them.
pub(crate) fn enter(token: Option<&CancelToken>) -> Option<Entered> {
    token.map(CancelToken::enter)
}

/// A stopping point: unwinds to [`CancelToken::run`] when the current token
/// is cancelled or timed out; does nothing otherwise, or without a token.
///
/// Call it only where no lock shared beyond the work is held and no value
/// shared beyond it is half changed: the unwinding drops what it crosses.
pub(crate) fn check() {
    let stopped = CURRENT.with(|current| current.borrow().as_ref().and_then(CancelToken::state));
    if let Some(stopped) = stopped {
        // `resume_unwind` does not call the panic hook: nothing is printed.
        resume_unwind(Box::new(stopped));
    }
}

/// A stopping point for loops whose steps are too small for a [`check`]
/// each (the events of an XML part): [`Pace::tick`] checks once every
/// [`Pace::STEPS`] steps.
#[derive(Debug, Default)]
pub(crate) struct Pace(u32);

impl Pace {
    /// Steps between two checks: a few microseconds of work each, so the
    /// checks stay far apart in time and close enough for a cancellation.
    pub(crate) const STEPS: u32 = 4096;

    /// One step: a [`check`] every [`Self::STEPS`] of them.
    pub(crate) fn tick(&mut self) {
        self.0 += 1;
        if self.0 >= Self::STEPS {
            self.0 = 0;
            check();
        }
    }
}

/// How often [`blocking`] and [`sleep`] look at the current token while
/// they wait: the longest they make a cancellation wait.
const WAIT_POLL: Duration = Duration::from_millis(10);

/// Runs `work`, a call that blocks without stopping points of its own (a
/// request waiting for its answer), and returns its result; with a current
/// token, on a thread of its own, while this one [`check`]s every
/// [`WAIT_POLL`]. Work left so goes on until it ends by itself, its result
/// dropped: it gets the token (`None` without one) to stop early where it
/// can, between the chunks of a download.
///
/// # Panics
/// When `work` panics, or its thread cannot be started.
pub(crate) fn blocking<R: Send + 'static>(work: impl FnOnce(Option<CancelToken>) -> R + Send + 'static) -> R {
    let Some(token) = current() else {
        return work(None);
    };
    check();
    let (sender, result) = std::sync::mpsc::sync_channel(1);
    let worker = token.clone();
    let handle = std::thread::Builder::new()
        .name("kakemono-wait".into())
        .spawn(move || {
            // The receiver is gone when the caller stopped waiting.
            let _ = sender.send(work(Some(worker)));
        })
        .expect("cannot start a thread");
    loop {
        match result.recv_timeout(WAIT_POLL) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => check(),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
                Err(payload) => resume_unwind(payload),
                Ok(()) => unreachable!("the worker sends its result before it ends"),
            },
        }
    }
}

/// Sleeps for `wait`, returning early (by unwinding, as [`check`]) when the
/// current token stops.
pub(crate) fn sleep(wait: Duration) {
    let end = Instant::now() + wait;
    loop {
        check();
        let now = Instant::now();
        if now >= end {
            return;
        }
        std::thread::sleep((end - now).min(WAIT_POLL));
    }
}

#[cfg(test)]
mod tests {
    use super::{CancelToken, Cancelled, check, current};
    use std::time::Duration;

    #[test]
    fn work_that_ends_keeps_its_result() {
        let token = CancelToken::new();
        assert_eq!(
            token.run(|| {
                check();
                7
            }),
            Ok(7)
        );
        assert!(current().is_none(), "the token is no longer current");
    }

    #[test]
    fn a_cancelled_token_stops_at_the_next_check() {
        let token = CancelToken::new();
        let mut reached = false;
        let outcome = token.run(|| {
            token.cancel();
            check();
            reached = true;
        });
        assert_eq!(outcome, Err(Cancelled::Requested));
        assert!(!reached);
        assert!(current().is_none());
    }

    #[test]
    fn work_does_not_start_with_a_cancelled_token() {
        let token = CancelToken::new();
        token.cancel();
        let mut started = false;
        assert_eq!(token.run(|| started = true), Err(Cancelled::Requested));
        assert!(!started);
    }

    #[test]
    fn a_deadline_times_out() {
        let token = CancelToken::with_timeout(Duration::from_millis(20));
        let outcome = token.run(|| {
            loop {
                check();
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        assert_eq!(outcome, Err(Cancelled::TimedOut));
    }

    #[test]
    fn a_parent_stops_its_children_but_not_the_reverse() {
        let parent = CancelToken::new();
        let child = parent.child(None);
        child.cancel();
        assert_eq!(parent.state(), None);
        let other = parent.child(Some(Duration::from_secs(60)));
        parent.cancel();
        assert_eq!(other.state(), Some(Cancelled::Requested));
    }

    #[test]
    fn other_panics_go_on() {
        let token = CancelToken::new();
        let outcome = std::panic::catch_unwind(|| token.run(|| panic!("boom")));
        assert!(outcome.is_err());
        assert!(current().is_none());
    }

    #[test]
    fn runs_nest() {
        let outer = CancelToken::new();
        let inner = CancelToken::new();
        let outcome = outer.run(|| {
            let nested = inner.run(|| {
                inner.cancel();
                check();
            });
            assert_eq!(nested, Err(Cancelled::Requested));
            check(); // the outer token is current again, and not cancelled
            1
        });
        assert_eq!(outcome, Ok(1));
    }
}
