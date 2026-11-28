//! Work over an index, in order whatever the threads.

use crate::failed::{Cancel, Failed};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// `f` of 0..`count` on `threads` threads, the results in order. A thread takes the next index
/// when it has finished one, so a result must depend on its index alone. A thread that cannot be
/// started leaves its work to the others; a panic of one goes on here.
pub fn parallel<T: Send>(count: usize, threads: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let work = || {
        let mut done = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= count {
                return done;
            }
            done.push((i, f(i)));
        }
    };
    let mut out: Vec<(usize, T)> = std::thread::scope(|s| {
        let others: Vec<_> = (1..threads.clamp(1, count.max(1))).filter_map(|_| std::thread::Builder::new().spawn_scoped(s, work).ok()).collect();
        let mut out = work();
        for t in others {
            out.extend(t.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)));
        }
        out
    });
    out.sort_unstable_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, t)| t).collect()
}

/// [`parallel`], with `cancel` asked before every one, on its thread.
pub fn cancellable<T: Send>(count: usize, threads: usize, cancel: Cancel, f: impl Fn(usize) -> T + Sync) -> Result<Vec<T>, Failed> {
    let stopped = AtomicBool::new(false);
    let one = |i: usize| {
        if stopped.load(Ordering::Relaxed) || cancel() {
            stopped.store(true, Ordering::Relaxed);
            return None;
        }
        Some(f(i))
    };
    parallel(count, threads, one).into_iter().collect::<Option<_>>().ok_or(Failed::Cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_index_once_and_in_order() {
        assert_eq!(parallel(5, 3, |i| i * i), [0, 1, 4, 9, 16]);
        assert!(parallel(0, 3, |i| i).is_empty() && parallel(1, 0, |i| i) == [0]);
        for (count, threads) in [(0, 3), (1, 1), (29, 3), (100, 12), (7, 0)] {
            assert_eq!(cancellable(count, threads, &|| false, |i| i), Ok((0..count).collect()));
        }
    }

    #[test]
    fn panics_go_on() {
        for at in [0, 37] {
            let out = std::panic::catch_unwind(|| parallel(40, 4, |i| assert_ne!(i, at, "expected here")));
            assert!(out.is_err());
        }
    }

    /// Once before every one, and no more of them is worked than were asked for.
    #[test]
    fn it_stops() {
        for threads in [1, 4] {
            let (asked, worked) = (AtomicUsize::new(0), AtomicUsize::new(0));
            let cancel = || asked.fetch_add(1, Ordering::Relaxed) >= 20;
            let work = |_| worked.fetch_add(1, Ordering::Relaxed);
            assert_eq!(cancellable(20, threads, &cancel, work).map(|w| w.len()), Ok(20));
            worked.store(0, Ordering::Relaxed);
            asked.store(0, Ordering::Relaxed);
            assert!(cancellable(100, threads, &cancel, work) == Err(Failed::Cancelled) && worked.load(Ordering::Relaxed) == 20);
        }
    }
}
