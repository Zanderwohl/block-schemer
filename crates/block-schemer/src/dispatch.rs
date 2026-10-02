//! Runs Scheme away from the UI, so a long or endless run never freezes the
//! window. A [`Dispatch`] spawns, tracks and kills its workers; the runner
//! only sends source and console input, and polls for what came back.
//!
//! Natively the worker is a thread. On the web it will be a Web Worker, which
//! can only be stopped by terminating it, so stopping is defined to lose the
//! session in every implementation.

pub mod native;

pub use native::Native;

pub const STOPPED: &str = "Stopped.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticket(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub source: String,
    /// Run in a new session, which later jobs then continue.
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Written to the console by the job as it ran.
    Output(Ticket, String),
    /// The written form of the job's last value, empty for none, or why it
    /// failed. Every ticket gets exactly one, after all its output.
    Done(Ticket, Result<String, String>),
}

/// Jobs run one at a time, in the order sent, in one session.
pub trait Dispatch {
    fn send(&mut self, job: Job) -> Ticket;

    /// What happened since the last call, in order.
    fn poll(&mut self) -> Vec<Event>;

    /// True while any ticket is unanswered.
    fn busy(&self) -> bool;

    /// True while a job waits for a line from the console.
    fn waiting(&self) -> bool;

    /// A line entered on the console, newline included, for reads by the
    /// jobs sent so far. Lines still unread when they are all answered are
    /// dropped.
    fn input(&mut self, line: &str);

    /// Ends input, so the next read that would wait sees end of file.
    fn end_input(&mut self);

    /// Ends the running job and drops the queued ones and any unread input,
    /// each job answering `Err(STOPPED)`. The next job starts a fresh session.
    fn stop(&mut self);
}
