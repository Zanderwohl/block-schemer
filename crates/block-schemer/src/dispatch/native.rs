//! One worker thread that owns the session.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;

use super::{Dispatch, Job, STOPPED, Ticket};
use crate::scheme::{Answer, Interrupt, Scheme};

/// What every unanswered ticket gets when the worker dies, as by a panic.
const CRASHED: &str = "The interpreter stopped unexpectedly.";

type Make<S> = Arc<dyn Fn() -> S + Send + Sync>;

pub struct Native<S> {
    make: Make<S>,
    worker: Worker,
    /// Bumped by `stop`. A job sent before the bump is skipped, or stopped
    /// if it is running.
    generation: Arc<AtomicU64>,
    next: u64,
    unanswered: VecDeque<Ticket>,
}

struct Worker {
    jobs: Sender<(Ticket, u64, Job)>,
    answers: Receiver<(Ticket, Result<Answer, String>)>,
    interrupter: Arc<dyn Interrupt>,
}

impl<S: Scheme + 'static> Native<S> {
    /// `make` runs on the worker, so a Scheme need not be `Send`, and again
    /// to replace a worker that died.
    pub fn spawn(make: impl Fn() -> S + Send + Sync + 'static) -> Self {
        let make: Make<S> = Arc::new(make);
        let generation = Arc::new(AtomicU64::new(0));
        Self {
            worker: Worker::spawn(make.clone(), generation.clone()),
            make,
            generation,
            next: 0,
            unanswered: VecDeque::new(),
        }
    }
}

impl Worker {
    fn spawn<S: Scheme + 'static>(make: Make<S>, generation: Arc<AtomicU64>) -> Self {
        let (jobs, inbox) = mpsc::channel::<(Ticket, u64, Job)>();
        let (outbox, answers) = mpsc::channel();
        let (handover, interrupter) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("scheme".into())
            .spawn(move || {
                let mut scheme = make();
                let interrupter = scheme.interrupter();
                if handover.send(interrupter.clone()).is_err() {
                    return;
                }
                let stopped = |sent| generation.load(Ordering::SeqCst) != sent;
                for (ticket, sent, job) in inbox {
                    // Reset before clearing, and clear before the check, so a
                    // stop from the check on interrupts the engine that runs.
                    if job.fresh {
                        scheme.reset();
                    }
                    interrupter.clear();
                    let answer = if stopped(sent) {
                        Err(STOPPED.to_owned())
                    } else {
                        let answer = scheme.run(&job.source);
                        if stopped(sent) {
                            scheme.reset();
                            Err(STOPPED.to_owned())
                        } else {
                            answer
                        }
                    };
                    if outbox.send((ticket, answer)).is_err() {
                        return;
                    }
                }
            })
            .expect("the OS starts a thread");
        let interrupter = interrupter.recv().expect("the worker builds its interpreter");
        Self {
            jobs,
            answers,
            interrupter,
        }
    }
}

impl<S: Scheme + 'static> Dispatch for Native<S> {
    fn send(&mut self, job: Job) -> Ticket {
        let ticket = Ticket(self.next);
        self.next += 1;
        let sent = self.generation.load(Ordering::SeqCst);
        // A dead worker is found and replaced by the next `poll`.
        let _ = self.worker.jobs.send((ticket, sent, job));
        self.unanswered.push_back(ticket);
        ticket
    }

    fn poll(&mut self) -> Vec<(Ticket, Result<Answer, String>)> {
        let mut answers = Vec::new();
        loop {
            match self.worker.answers.try_recv() {
                Ok(answer) => {
                    self.unanswered.retain(|ticket| *ticket != answer.0);
                    answers.push(answer);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    answers.extend(self.unanswered.drain(..).map(|ticket| (ticket, Err(CRASHED.to_owned()))));
                    self.worker = Worker::spawn(self.make.clone(), self.generation.clone());
                    break;
                }
            }
        }
        answers
    }

    fn busy(&self) -> bool {
        !self.unanswered.is_empty()
    }

    fn stop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.worker.interrupter.interrupt();
    }
}

impl<S> Drop for Native<S> {
    /// The worker ends once its queue is gone; interrupting ends it promptly.
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.worker.interrupter.interrupt();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::Steel;

    /// Polls until every ticket is answered, or fails after a while.
    fn settle(dispatch: &mut impl Dispatch) -> Vec<(Ticket, Result<Answer, String>)> {
        let start = Instant::now();
        let mut answers = Vec::new();
        while dispatch.busy() {
            assert!(start.elapsed() < Duration::from_secs(20), "still busy");
            answers.extend(dispatch.poll());
            thread::sleep(Duration::from_millis(5));
        }
        answers
    }

    fn job(source: &str, fresh: bool) -> Job {
        Job {
            source: source.into(),
            fresh,
        }
    }

    fn value(answer: &Result<Answer, String>) -> &str {
        &answer.as_ref().unwrap().value
    }

    #[test]
    fn jobs_run_in_order_in_one_session_until_a_fresh_one() {
        let mut dispatch = Native::spawn(Steel::new);
        let first = dispatch.send(job("(define x 2)", false));
        let second = dispatch.send(job("(* x 3)", false));
        let third = dispatch.send(job("x", true));
        let answers = settle(&mut dispatch);
        assert_eq!(answers.iter().map(|(ticket, _)| *ticket).collect::<Vec<_>>(), [first, second, third]);
        assert_eq!(value(&answers[1].1), "6");
        assert!(answers[2].1.as_ref().unwrap_err().contains("x"), "the fresh session forgot x");
    }

    #[test]
    fn stopping_ends_a_runaway_job_drops_the_queue_and_loses_the_session() {
        let mut dispatch = Native::spawn(Steel::new);
        dispatch.send(job("(define kept 1)", false));
        settle(&mut dispatch);
        dispatch.send(job("(let spin () (spin))", false));
        dispatch.send(job("(+ 1 1)", false));
        thread::sleep(Duration::from_millis(100));
        assert!(dispatch.busy());
        assert!(dispatch.poll().is_empty(), "the loop is still running");

        dispatch.stop();
        let answers = settle(&mut dispatch);
        assert_eq!(answers.len(), 2);
        assert!(answers.iter().all(|(_, answer)| answer.as_ref().err().map(String::as_str) == Some(STOPPED)), "{answers:?}");

        dispatch.send(job("kept", false));
        let answers = settle(&mut dispatch);
        assert!(answers[0].1.is_err(), "the session was lost: {answers:?}");
        dispatch.send(job("(+ 1 1)", false));
        assert_eq!(value(&settle(&mut dispatch)[0].1), "2", "and the next one runs");
    }

    struct Fragile;

    struct Unstoppable;

    impl Interrupt for Unstoppable {
        fn interrupt(&self) {}
        fn clear(&self) {}
    }

    impl Scheme for Fragile {
        fn run(&mut self, source: &str) -> Result<Answer, String> {
            assert_ne!(source, "boom", "the worker dies");
            Ok(Answer::default())
        }
        fn reset(&mut self) {}
        fn interrupter(&self) -> Arc<dyn Interrupt> {
            Arc::new(Unstoppable)
        }
    }

    #[test]
    fn a_worker_that_dies_answers_for_its_queue_and_is_replaced() {
        let mut dispatch = Native::spawn(|| Fragile);
        dispatch.send(job("boom", false));
        dispatch.send(job("queued", false));
        let answers = settle(&mut dispatch);
        assert!(answers.iter().all(|(_, answer)| answer.as_ref().err().map(String::as_str) == Some(CRASHED)), "{answers:?}");
        dispatch.send(job("fine", false));
        assert!(settle(&mut dispatch)[0].1.is_ok());
    }
}
