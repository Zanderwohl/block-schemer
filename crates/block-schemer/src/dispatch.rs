//! Runs Scheme away from the UI, so a long or endless run never freezes the
//! window. A [`Dispatch`] spawns, tracks and kills its workers; the runner
//! only sends source and polls for answers.
//!
//! Natively the worker is a thread. On the web it will be a Web Worker, which
//! can only be stopped by terminating it, so stopping is defined to lose the
//! session in every implementation.

pub mod native;

pub use native::Native;

use crate::scheme::Answer;

pub const STOPPED: &str = "Stopped.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticket(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub source: String,
    /// Run in a new session, which later jobs then continue.
    pub fresh: bool,
}

/// Jobs run one at a time, in the order sent, in one session.
pub trait Dispatch {
    fn send(&mut self, job: Job) -> Ticket;

    /// Answers since the last call, in the order sent. Every ticket gets
    /// exactly one.
    fn poll(&mut self) -> Vec<(Ticket, Result<Answer, String>)>;

    /// True while any ticket is unanswered.
    fn busy(&self) -> bool;

    /// Ends the running job and drops the queued ones, each answering
    /// `Err(STOPPED)`. The next job starts a fresh session.
    fn stop(&mut self);
}
