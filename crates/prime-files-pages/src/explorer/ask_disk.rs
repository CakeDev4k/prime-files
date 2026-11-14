//! Questions to the disk that may never come back soon: whether a typed or
//! clicked path is a folder, whether a file is there. On a share that
//! stopped answering each takes about 21 s, and nothing can cut it short.
//!
//! Until 13/11/2026 they ran on gpui's background executor, so each stuck
//! question held one of its threads for those 21 s, and a person clicking a
//! dead place again and again held more and more, with no ceiling. Open
//! Salamander gives such questions threads of their own and stops at 30
//! (`salamdr5.cpp`, OTIMIZACAO.md §2.12); here the same: a thread per
//! question, thrown away when it answers, and past [`MOST`] stuck at once
//! the question is not asked and its answer is "unknown" (`None`).

use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{App, AppContext as _, Task};

/// Questions waiting on the disk at once, at most.
pub(crate) const MOST: usize = 30;

static WAITING: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
thread_local! {
    static ON_THE_EXECUTOR: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Asks `question` on a thread of its own. `None` when [`MOST`] are already
/// waiting, or no thread could be started.
pub(crate) fn ask<T: Send + 'static>(cx: &App, question: impl FnOnce() -> T + Send + 'static) -> Task<Option<T>> {
    // The other tests drive gpui's test executor, which only runs what it
    // was given: a real thread would answer after `run_until_parked`.
    #[cfg(test)]
    if ON_THE_EXECUTOR.get() {
        return cx.background_spawn(async move { Some(question()) });
    }
    let taken = WAITING.try_update(Ordering::AcqRel, Ordering::Acquire, |waiting| (waiting < MOST).then_some(waiting + 1));
    if taken.is_err() {
        tracing::debug!("{MOST} questions already waiting on the disk; not asking another");
        return Task::ready(None);
    }
    let (answer, answered) = async_channel::bounded(1);
    let spawned = std::thread::Builder::new().name("prime-files-ask-disk".into()).spawn(move || {
        // Counted out even if the question panics.
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                WAITING.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let _done = Done;
        answer.try_send(question()).ok();
    });
    if let Err(error) = spawned {
        WAITING.fetch_sub(1, Ordering::AcqRel);
        tracing::warn!("could not start a thread to ask the disk: {error}");
        return Task::ready(None);
    }
    cx.background_spawn(async move { answered.recv().await.ok() })
}

/// How many questions are waiting on the disk now.
#[cfg(test)]
pub(crate) fn waiting() -> usize {
    WAITING.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use gpui::TestAppContext;

    // A dead share holds each question; past the ceiling no more threads
    // are taken, and the questions held answer once the disk does.
    #[gpui::test]
    async fn stuck_questions_stop_at_the_ceiling(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        super::ON_THE_EXECUTOR.set(false);
        let release = Arc::new(AtomicBool::new(false));
        let before = super::waiting();
        let mut held = Vec::new();
        for _ in 0..super::MOST + 5 {
            let release = release.clone();
            held.push(cx.update(|cx| {
                super::ask(cx, move || {
                    while !release.load(std::sync::atomic::Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    true
                })
            }));
        }
        assert!(super::waiting() <= super::MOST, "the ceiling was passed");
        release.store(true, std::sync::atomic::Ordering::Release);
        let mut answered = 0;
        let mut refused = 0;
        for task in held {
            match task.await {
                Some(true) => answered += 1,
                _ => refused += 1,
            }
        }
        assert_eq!(answered + refused, super::MOST + 5);
        assert!(refused >= 5, "{refused} refused");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while super::waiting() > before && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(super::waiting() <= before, "threads were not counted out");
    }
}
