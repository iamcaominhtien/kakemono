//! Running independent per-page work on several threads.
//!
//! The PDF caches of the hayro crates are not thread-safe, so every worker
//! thread builds its own state (renderer, font registry, ...) with `init`
//! and reuses it for all the items it takes. Items are handed out one at a
//! time, so a few slow pages do not hold up the others, and results come
//! back in item order.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// Stack size of worker threads: PDF interpretation recurses into nested
/// forms and patterns, so give workers as much room as the main thread.
const STACK_SIZE: usize = 16 << 20;

/// Number of worker threads to use: `requested`, or one per CPU when
/// `requested` is 0.
pub fn resolve_jobs(requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// Computes `func(state, i)` for every `i` in `0..n` on up to `jobs` threads
/// (0 = one per CPU) and returns the results in order.
///
/// `init` builds the per-thread state once per worker. With one job (or at
/// most one item) everything runs on the calling thread and no thread is
/// spawned. A panic in a worker is propagated to the caller once all
/// workers have finished.
///
/// # Panics
/// When a worker thread cannot be started, or when `init` or `func` panics.
pub fn map<S, R, I, F>(n: usize, jobs: usize, init: I, func: F) -> Vec<R>
where
    R: Send,
    I: Fn() -> S + Sync,
    F: Fn(&mut S, usize) -> R + Sync,
{
    let jobs = resolve_jobs(jobs).min(n);
    if jobs <= 1 {
        let mut state = init();
        return (0..n).map(|i| func(&mut state, i)).collect();
    }
    let next = AtomicUsize::new(0);
    let worker = || {
        let mut state = init();
        let mut done = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= n {
                break;
            }
            done.push((i, func(&mut state, i)));
        }
        done
    };
    let mut slots: Vec<Option<R>> = (0..n).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..jobs)
            .map(|_| {
                std::thread::Builder::new()
                    .stack_size(STACK_SIZE)
                    .spawn_scoped(scope, worker)
                    .expect("cannot start a worker thread")
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(done) => done.into_iter().for_each(|(i, result)| slots[i] = Some(result)),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| slot.expect("every item is computed"))
        .collect()
}

/// How far the workers of [`map_ordered`] have got.
struct Progress {
    /// The next item to hand out.
    next: usize,
    /// Items whose results the caller has consumed (all those before it).
    consumed: usize,
    /// Set when the caller stops early: the workers take no more items.
    stop: bool,
}

/// The progress of [`map_ordered`] and the signal that it moved.
struct Gate {
    progress: Mutex<Progress>,
    moved: Condvar,
}

impl Gate {
    fn lock(&self) -> MutexGuard<'_, Progress> {
        self.progress.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records that the caller consumed every item before `consumed`.
    fn consumed(&self, consumed: usize) {
        self.lock().consumed = consumed;
        self.moved.notify_all();
    }

    /// Tells the workers to take no more items.
    fn stop(&self) {
        self.lock().stop = true;
        self.moved.notify_all();
    }

    /// The next item for a worker, once it is at most `ahead` items past
    /// the first one not consumed; `None` when there is none left or the
    /// caller stopped.
    fn take(&self, n: usize, ahead: usize) -> Option<usize> {
        let mut progress = self.lock();
        while !progress.stop && progress.next < n && progress.next >= progress.consumed + ahead {
            progress = self.moved.wait(progress).unwrap_or_else(PoisonError::into_inner);
        }
        if progress.stop || progress.next >= n {
            return None;
        }
        progress.next += 1;
        Some(progress.next - 1)
    }
}

/// Stops the workers when dropped, so that they do not wait for ever on a
/// caller that left early (with an error or a panic).
struct StopOnDrop<'a>(&'a Gate);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// Like [`map`], but gives each result to `consume` on the calling thread,
/// in item order, as soon as it and all those before it are computed,
/// instead of returning them all at the end. The workers stay at most
/// `ahead` items (at least one per worker) past the first item not yet
/// consumed, so only about `ahead` results are held at any time.
///
/// Stops at the first error of `consume` and returns it.
///
/// # Errors
/// The first error returned by `consume`.
///
/// # Panics
/// When a worker thread cannot be started, or when `init`, `func` or
/// `consume` panics.
pub fn map_ordered<S, R, E, I, F, C>(
    n: usize,
    jobs: usize,
    ahead: usize,
    init: I,
    func: F,
    mut consume: C,
) -> Result<(), E>
where
    R: Send,
    I: Fn() -> S + Sync,
    F: Fn(&mut S, usize) -> R + Sync,
    C: FnMut(usize, R) -> Result<(), E>,
{
    let jobs = resolve_jobs(jobs).min(n);
    if jobs <= 1 {
        let mut state = init();
        for i in 0..n {
            consume(i, func(&mut state, i))?;
        }
        return Ok(());
    }
    let ahead = ahead.max(jobs);
    let gate = Gate {
        progress: Mutex::new(Progress {
            next: 0,
            consumed: 0,
            stop: false,
        }),
        moved: Condvar::new(),
    };
    let (sender, results) = mpsc::channel::<(usize, std::thread::Result<R>)>();
    let worker = |sender: mpsc::Sender<(usize, std::thread::Result<R>)>| {
        let mut state = init();
        while let Some(i) = gate.take(n, ahead) {
            // A panic is sent to the caller, which would otherwise wait for
            // this item for ever.
            let result = catch_unwind(AssertUnwindSafe(|| func(&mut state, i)));
            let failed = result.is_err();
            if sender.send((i, result)).is_err() || failed {
                break;
            }
        }
    };
    let mut outcome = Ok(());
    let mut panic = None;
    std::thread::scope(|scope| {
        let _stop = StopOnDrop(&gate);
        for _ in 0..jobs {
            let sender = sender.clone();
            std::thread::Builder::new()
                .stack_size(STACK_SIZE)
                .spawn_scoped(scope, || worker(sender))
                .expect("cannot start a worker thread");
        }
        drop(sender);
        let mut waiting: HashMap<usize, R> = HashMap::new();
        let mut next = 0;
        'results: for (i, result) in results {
            match result {
                Ok(result) => waiting.insert(i, result),
                Err(payload) => {
                    panic = Some(payload);
                    break;
                }
            };
            while let Some(result) = waiting.remove(&next) {
                if let Err(err) = consume(next, result) {
                    outcome = Err(err);
                    break 'results;
                }
                next += 1;
                gate.consumed(next);
            }
        }
    });
    if let Some(payload) = panic {
        resume_unwind(payload);
    }
    outcome
}

#[cfg(test)]
mod tests {
    /// Results come back in item order for any number of jobs, and an empty
    /// input gives an empty output.
    #[test]
    fn keeps_order() {
        for jobs in [0, 1, 3, 16] {
            let out = super::map(
                50,
                jobs,
                || 0usize,
                |calls, i| {
                    *calls += 1;
                    i * i
                },
            );
            assert_eq!(out, (0..50).map(|i| i * i).collect::<Vec<_>>());
        }
        assert!(super::map(0, 4, || (), |_, i| i).is_empty());
    }

    /// `map_ordered` gives every result in item order, for any number of
    /// jobs and any window, and never holds more than the window.
    #[test]
    fn map_ordered_keeps_order_within_the_window() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        for jobs in [0, 1, 3, 16] {
            for ahead in [1, 4, 100] {
                let started = AtomicUsize::new(0);
                let mut seen = Vec::new();
                let done: Result<(), ()> = super::map_ordered(
                    50,
                    jobs,
                    ahead,
                    || (),
                    |(), i| {
                        started.fetch_add(1, Ordering::SeqCst);
                        i * i
                    },
                    |i, square| {
                        assert_eq!(square, i * i);
                        let workers = super::resolve_jobs(jobs).min(50);
                        if workers > 1 {
                            assert!(started.load(Ordering::SeqCst) <= i + ahead.max(workers) + 1);
                        }
                        seen.push(i);
                        Ok(())
                    },
                );
                assert_eq!(done, Ok(()));
                assert_eq!(seen, (0..50).collect::<Vec<_>>());
            }
        }
    }

    /// An error of `consume` stops `map_ordered` and is returned.
    #[test]
    fn map_ordered_stops_at_the_first_error() {
        for jobs in [1, 4] {
            let mut seen = Vec::new();
            let done = super::map_ordered(
                1000,
                jobs,
                8,
                || (),
                |(), i| i,
                |i, _| {
                    seen.push(i);
                    if i == 10 { Err(i) } else { Ok(()) }
                },
            );
            assert_eq!(done, Err(10));
            assert_eq!(seen, (0..=10).collect::<Vec<_>>());
        }
    }

    /// A panic in a worker reaches the caller instead of leaving it
    /// waiting.
    #[test]
    #[should_panic(expected = "item 7")]
    fn map_ordered_propagates_panics() {
        let _ = super::map_ordered(
            100,
            4,
            8,
            || (),
            |(), i| {
                assert!(i != 7, "item 7");
                i
            },
            |_, _| Ok::<(), ()>(()),
        );
    }
}
