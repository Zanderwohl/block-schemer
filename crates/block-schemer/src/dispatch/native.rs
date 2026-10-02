//! One worker thread that owns the session.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

use super::{Dispatch, Event, Job, STOPPED, Ticket};
use crate::scheme::{Console, Interrupt, Scheme};

/// What every unanswered ticket gets when the worker dies, as by a panic.
const CRASHED: &str = "The interpreter stopped unexpectedly.";

type Make<S> = Arc<dyn Fn(Arc<dyn Console>) -> S + Send + Sync>;

pub struct Native<S> {
    make: Make<S>,
    worker: Worker,
    /// Bumped by `stop`. A job sent before the bump is skipped, or stopped
    /// if it is running.
    generation: Arc<AtomicU64>,
    terminal: Arc<Terminal>,
    next: u64,
    unanswered: VecDeque<Ticket>,
}

struct Worker {
    jobs: Sender<(Ticket, u64, Job)>,
    answers: Receiver<(Ticket, Result<String, String>)>,
    interrupter: Arc<dyn Interrupt>,
}

/// The console as the worker sees it. Output is collected here rather than
/// sent, so no sender outlives a dead worker inside its interpreter.
#[derive(Default)]
struct Terminal {
    state: Mutex<TerminalState>,
    entered: Condvar,
}

#[derive(Default)]
struct TerminalState {
    running: Option<Ticket>,
    output: Vec<(Ticket, String)>,
    /// Entered and not yet read; an empty one ends input.
    input: VecDeque<String>,
    waiting: bool,
    stopped: bool,
}

impl Terminal {
    fn state(&self) -> MutexGuard<'_, TerminalState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn enter(&self, line: String) {
        self.state().input.push_back(line);
        self.entered.notify_all();
    }

    fn stop(&self) {
        let mut state = self.state();
        state.stopped = true;
        state.input.clear();
        self.entered.notify_all();
    }
}

impl Console for Terminal {
    fn write(&self, text: &str) {
        let mut state = self.state();
        let Some(ticket) = state.running else {
            return;
        };
        match state.output.last_mut() {
            Some((last, output)) if *last == ticket => output.push_str(text),
            _ => state.output.push((ticket, text.to_owned())),
        }
    }

    fn read_line(&self) -> io::Result<String> {
        let mut state = self.state();
        loop {
            if state.stopped {
                state.waiting = false;
                return Err(io::Error::other(STOPPED));
            }
            if let Some(line) = state.input.pop_front() {
                state.waiting = false;
                return Ok(line);
            }
            state.waiting = true;
            state = self.entered.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl<S: Scheme + 'static> Native<S> {
    /// `make` runs on the worker, so a Scheme need not be `Send`, and again
    /// to replace a worker that died.
    pub fn spawn(make: impl Fn(Arc<dyn Console>) -> S + Send + Sync + 'static) -> Self {
        let make: Make<S> = Arc::new(make);
        let generation = Arc::new(AtomicU64::new(0));
        let terminal = Arc::new(Terminal::default());
        Self {
            worker: Worker::spawn(make.clone(), generation.clone(), terminal.clone()),
            make,
            generation,
            terminal,
            next: 0,
            unanswered: VecDeque::new(),
        }
    }
}

impl Worker {
    fn spawn<S: Scheme + 'static>(make: Make<S>, generation: Arc<AtomicU64>, terminal: Arc<Terminal>) -> Self {
        let (jobs, inbox) = mpsc::channel::<(Ticket, u64, Job)>();
        let (outbox, answers) = mpsc::channel();
        let (handover, interrupter) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("scheme".into())
            .spawn(move || {
                let mut scheme = make(terminal.clone());
                let interrupter = scheme.interrupter();
                if handover.send(interrupter.clone()).is_err() {
                    return;
                }
                let stopped = |sent| generation.load(Ordering::SeqCst) != sent;
                // A job from a later generation, as after any stop, even one
                // while idle, gets a fresh session.
                let mut session = generation.load(Ordering::SeqCst);
                for (ticket, sent, job) in inbox {
                    // Reset before clearing, and clear before the check, so a
                    // stop from the check on interrupts the engine that runs
                    // and any read it waits on.
                    if job.fresh || sent != session {
                        scheme.reset();
                        session = sent;
                    }
                    interrupter.clear();
                    {
                        let mut state = terminal.state();
                        state.stopped = false;
                        state.running = Some(ticket);
                    }
                    let answer = if stopped(sent) {
                        Err(STOPPED.to_owned())
                    } else {
                        let answer = scheme.run(&job.source);
                        if stopped(sent) { Err(STOPPED.to_owned()) } else { answer }
                    };
                    terminal.state().running = None;
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

    /// Answers are taken before output, so all of a job's output is in hand
    /// by the time its answer is.
    fn poll(&mut self) -> Vec<Event> {
        let mut answers = Vec::new();
        let crashed = loop {
            match self.worker.answers.try_recv() {
                Ok(answer) => answers.push(answer),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        let mut output = std::mem::take(&mut self.terminal.state().output);
        if crashed {
            answers.extend(self.unanswered.iter().map(|ticket| (*ticket, Err(CRASHED.to_owned()))));
            *self.terminal.state() = TerminalState::default();
            self.worker = Worker::spawn(self.make.clone(), self.generation.clone(), self.terminal.clone());
        }
        let mut events = Vec::new();
        for (ticket, answer) in answers {
            self.unanswered.retain(|unanswered| *unanswered != ticket);
            let (mine, rest) = output.into_iter().partition(|(from, _)| *from == ticket);
            output = rest;
            events.extend(mine.into_iter().map(|(from, text)| Event::Output(from, text)));
            events.push(Event::Done(ticket, answer));
        }
        events.extend(output.into_iter().map(|(from, text)| Event::Output(from, text)));
        if self.unanswered.is_empty() {
            self.terminal.state().input.clear();
        }
        events
    }

    fn busy(&self) -> bool {
        !self.unanswered.is_empty()
    }

    fn waiting(&self) -> bool {
        self.terminal.state().waiting
    }

    fn input(&mut self, line: &str) {
        self.terminal.enter(line.to_owned());
    }

    fn end_input(&mut self) {
        self.terminal.enter(String::new());
    }

    fn stop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.worker.interrupter.interrupt();
        self.terminal.stop();
    }
}

impl<S> Drop for Native<S> {
    /// The worker ends once its queue is gone; interrupting ends it promptly.
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.worker.interrupter.interrupt();
        self.terminal.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::Steel;

    /// Polls until every ticket is answered, or fails after a while.
    fn settle(dispatch: &mut impl Dispatch) -> Vec<Event> {
        let start = Instant::now();
        let mut events = Vec::new();
        while dispatch.busy() {
            assert!(start.elapsed() < Duration::from_secs(20), "still busy");
            events.extend(dispatch.poll());
            thread::sleep(Duration::from_millis(5));
        }
        events
    }

    /// Polls once the dispatch waits for input, for what it said first.
    fn wait_for_input(dispatch: &mut impl Dispatch) -> Vec<Event> {
        let start = Instant::now();
        while !dispatch.waiting() {
            assert!(start.elapsed() < Duration::from_secs(20), "never waited");
            thread::sleep(Duration::from_millis(5));
        }
        dispatch.poll()
    }

    fn done(events: Vec<Event>) -> Vec<(Ticket, Result<String, String>)> {
        events
            .into_iter()
            .filter_map(|event| match event {
                Event::Done(ticket, answer) => Some((ticket, answer)),
                Event::Output(..) => None,
            })
            .collect()
    }

    fn job(source: &str, fresh: bool) -> Job {
        Job {
            source: source.into(),
            fresh,
        }
    }

    fn all_stopped(answers: &[(Ticket, Result<String, String>)], why: &str) -> bool {
        answers.iter().all(|(_, answer)| answer.as_ref().err().map(String::as_str) == Some(why))
    }

    #[test]
    fn jobs_run_in_order_in_one_session_until_a_fresh_one() {
        let mut dispatch = Native::spawn(Steel::new);
        let first = dispatch.send(job("(define x 2)", false));
        let second = dispatch.send(job("(* x 3)", false));
        let third = dispatch.send(job("x", true));
        let answers = done(settle(&mut dispatch));
        assert_eq!(answers.iter().map(|(ticket, _)| *ticket).collect::<Vec<_>>(), [first, second, third]);
        assert_eq!(answers[1].1, Ok("6".into()));
        assert!(answers[2].1.as_ref().unwrap_err().contains("x"), "the fresh session forgot x");
    }

    #[test]
    fn each_job_s_output_comes_before_its_answer() {
        let mut dispatch = Native::spawn(Steel::new);
        let first = dispatch.send(job("(display 1) (display 2) 'a", false));
        let second = dispatch.send(job("(display 3) 'b", false));
        assert_eq!(
            settle(&mut dispatch),
            [
                Event::Output(first, "12".into()),
                Event::Done(first, Ok("a".into())),
                Event::Output(second, "3".into()),
                Event::Done(second, Ok("b".into())),
            ]
        );
    }

    #[test]
    fn a_read_waits_for_input_after_showing_what_came_before() {
        let mut dispatch = Native::spawn(Steel::new);
        let ticket = dispatch.send(job("(display \"Name? \") (string-append \"hi \" (read-line))", false));
        assert_eq!(wait_for_input(&mut dispatch), [Event::Output(ticket, "Name? ".into())]);
        assert!(dispatch.busy());
        dispatch.input("Ada\n");
        assert_eq!(settle(&mut dispatch), [Event::Done(ticket, Ok("\"hi Ada\"".into()))]);
        assert!(!dispatch.waiting());
    }

    #[test]
    fn input_entered_early_is_read_later_but_not_by_a_job_sent_after_all_answered() {
        let mut dispatch = Native::spawn(Steel::new);
        dispatch.send(job("(read-line)", false));
        dispatch.input("early\n");
        dispatch.input("unread\n");
        assert_eq!(done(settle(&mut dispatch))[0].1, Ok("\"early\"".into()));

        dispatch.send(job("(read-line)", false));
        wait_for_input(&mut dispatch);
        dispatch.end_input();
        assert_eq!(done(settle(&mut dispatch))[0].1, Ok("(eof)".into()));
    }

    #[test]
    fn stopping_ends_a_read_waiting_for_input() {
        let mut dispatch = Native::spawn(Steel::new);
        dispatch.send(job("(read-line)", false));
        dispatch.send(job("(+ 1 1)", false));
        wait_for_input(&mut dispatch);
        dispatch.stop();
        let answers = done(settle(&mut dispatch));
        assert!(all_stopped(&answers, STOPPED), "{answers:?}");
        assert!(!dispatch.waiting());
        dispatch.send(job("(+ 1 1)", false));
        assert_eq!(done(settle(&mut dispatch))[0].1, Ok("2".into()), "and the next one runs");
    }

    #[test]
    fn stopping_while_idle_still_loses_the_session() {
        let mut dispatch = Native::spawn(Steel::new);
        dispatch.send(job("(define kept 1)", false));
        settle(&mut dispatch);
        dispatch.stop();
        dispatch.send(job("kept", false));
        assert!(done(settle(&mut dispatch))[0].1.is_err(), "kept outlived the stop");
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
        let answers = done(settle(&mut dispatch));
        assert_eq!(answers.len(), 2);
        assert!(all_stopped(&answers, STOPPED), "{answers:?}");

        dispatch.send(job("kept", false));
        let answers = done(settle(&mut dispatch));
        assert!(answers[0].1.is_err(), "the session was lost: {answers:?}");
        dispatch.send(job("(+ 1 1)", false));
        assert_eq!(done(settle(&mut dispatch))[0].1, Ok("2".into()), "and the next one runs");
    }

    struct Fragile;

    struct Unstoppable;

    impl Interrupt for Unstoppable {
        fn interrupt(&self) {}
        fn clear(&self) {}
    }

    impl Scheme for Fragile {
        fn run(&mut self, source: &str) -> Result<String, String> {
            assert_ne!(source, "boom", "the worker dies");
            Ok(String::new())
        }
        fn reset(&mut self) {}
        fn interrupter(&self) -> Arc<dyn Interrupt> {
            Arc::new(Unstoppable)
        }
    }

    #[test]
    fn a_worker_that_dies_answers_for_its_queue_and_is_replaced() {
        let mut dispatch = Native::spawn(|_| Fragile);
        dispatch.send(job("boom", false));
        dispatch.send(job("queued", false));
        let answers = done(settle(&mut dispatch));
        assert!(all_stopped(&answers, CRASHED), "{answers:?}");
        dispatch.send(job("fine", false));
        assert!(done(settle(&mut dispatch))[0].1.is_ok());
    }
}
