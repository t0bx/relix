/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::sync::{Mutex, MutexGuard};

#[derive(Debug)]
struct JobState<H> {
    active: bool,
    handle: Option<H>,
    cancel_requested: bool,
}

/// Tracks the server-side job of the statement that is currently executing so a cancel
/// request can reach it. A cancel that arrives before the server has assigned the job
/// handle is remembered and reported once the handle is attached.
#[derive(Debug)]
pub struct RunningJob<H> {
    state: Mutex<JobState<H>>,
}

impl<H: Clone> Default for RunningJob<H> {
    fn default() -> Self {
        Self {
            state: Mutex::new(JobState {
                active: false,
                handle: None,
                cancel_requested: false,
            }),
        }
    }
}

impl<H: Clone> RunningJob<H> {
    pub fn start(&self) -> RunningJobGuard<'_, H> {
        let mut state = self.lock();
        state.active = true;
        state.handle = None;
        state.cancel_requested = false;
        RunningJobGuard { job: self }
    }

    /// Records the server handle and returns whether a cancel was already requested.
    pub fn attach(&self, handle: H) -> bool {
        let mut state = self.lock();
        state.handle = Some(handle);
        state.cancel_requested
    }

    /// Marks the running job as cancelled and returns its handle when it is known.
    pub fn request_cancel(&self) -> Option<H> {
        let mut state = self.lock();
        if !state.active {
            return None;
        }
        state.cancel_requested = true;
        state.handle.clone()
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.lock().cancel_requested
    }

    fn finish(&self) {
        let mut state = self.lock();
        state.active = false;
        state.handle = None;
    }

    fn lock(&self) -> MutexGuard<'_, JobState<H>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct RunningJobGuard<'a, H: Clone> {
    job: &'a RunningJob<H>,
}

impl<H: Clone> Drop for RunningJobGuard<'_, H> {
    fn drop(&mut self) {
        self.job.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_cancel_is_reported_when_the_handle_arrives() {
        let job = RunningJob::<String>::default();
        let guard = job.start();
        assert_eq!(job.request_cancel(), None);
        assert!(job.attach("h1".into()));
        assert_eq!(job.request_cancel(), Some("h1".into()));
        drop(guard);
        assert_eq!(job.request_cancel(), None);
        assert!(!job.start().job.is_cancel_requested());
    }

    #[test]
    fn cancel_without_running_job_does_nothing() {
        let job = RunningJob::<u32>::default();
        assert_eq!(job.request_cancel(), None);
        let _guard = job.start();
        assert!(!job.attach(7));
    }
}
