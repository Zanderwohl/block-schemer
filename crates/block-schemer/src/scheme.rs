//! The interpreter behind Block Schemer, kept to one small trait so Steel can
//! be swapped for another Scheme, such as one that runs in WASM.

use std::sync::{Arc, Mutex};

use steel::SteelVal;
use steel::steel_vm::ThreadStateController;
use steel::steel_vm::engine::Engine;

use crate::codegen::OUTPUT_PORT;

/// One session: definitions from earlier runs stay in scope.
pub trait Scheme {
    /// Runs `source` and gives back what `display` wrote and the written form
    /// of the last value, empty for none.
    fn run(&mut self, source: &str) -> Result<Answer, String>;

    /// A fresh session, with no definitions but the prelude's.
    fn reset(&mut self);

    /// Stops this session's runs from another thread. The same one serves
    /// across `reset`.
    fn interrupter(&self) -> Arc<dyn Interrupt>;
}

/// Once interrupted, every run ends in an error until `clear`, so an
/// interrupt that lands just before a run starts still stops it.
pub trait Interrupt: Send + Sync {
    fn interrupt(&self);
    fn clear(&self);
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Answer {
    pub output: String,
    pub value: String,
}

/// Where Steel differs from R7RS. Its own `=` takes exactly two arguments,
/// so it is kept under a reserved name and `=` compares neighbors with it.
/// Run one at a time: in one program, Steel would take the first `=` to mean
/// the one being defined.
const STEEL_PRELUDE: [&str; 2] = [
    "(define __steel= =)",
    "(define (= first . rest)
       (let loop ((a first) (rest rest))
         (or (null? rest)
             (and (__steel= a (car rest)) (loop (car rest) (cdr rest))))))",
];

/// Steel's sandboxed engine, which loads no native libraries.
pub struct Steel {
    engine: Engine,
    interrupter: Arc<SteelInterrupt>,
}

/// Swapped to each new engine's controller on `reset`.
struct SteelInterrupt(Mutex<ThreadStateController>);

impl SteelInterrupt {
    fn controller(&self) -> std::sync::MutexGuard<'_, ThreadStateController> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Interrupt for SteelInterrupt {
    fn interrupt(&self) {
        self.controller().interrupt();
    }

    fn clear(&self) {
        self.controller().resume();
    }
}

impl Steel {
    pub fn new() -> Self {
        let engine = Self::engine();
        let interrupter = Arc::new(SteelInterrupt(Mutex::new(engine.get_thread_state_controller())));
        Self { engine, interrupter }
    }

    fn engine() -> Engine {
        let mut engine = Engine::new_sandboxed();
        for definition in STEEL_PRELUDE {
            engine.run(definition).expect("the prelude runs");
        }
        engine
            .run(format!("(define {OUTPUT_PORT} (open-output-string))"))
            .expect("Steel opens a string port");
        engine
    }

    fn take_output(&mut self) -> String {
        let source = format!(
            "(define {OUTPUT_PORT}-text (get-output-string {OUTPUT_PORT})) \
             (set! {OUTPUT_PORT} (open-output-string)) \
             {OUTPUT_PORT}-text"
        );
        match self.engine.run(source).ok().and_then(|values| values.into_iter().last()) {
            Some(SteelVal::StringV(text)) => text.to_string(),
            _ => String::new(),
        }
    }
}

impl Default for Steel {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheme for Steel {
    fn run(&mut self, source: &str) -> Result<Answer, String> {
        let result = self.engine.run(source.to_owned());
        let output = self.take_output();
        let values = result.map_err(|error| error.to_string())?;
        let value = match values.last() {
            None | Some(SteelVal::Void) => String::new(),
            Some(value) => value.to_string(),
        };
        Ok(Answer { output, value })
    }

    fn reset(&mut self) {
        self.engine = Self::engine();
        *self.interrupter.controller() = self.engine.get_thread_state_controller();
    }

    fn interrupter(&self) -> Arc<dyn Interrupt> {
        self.interrupter.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_last_and_display_is_captured_per_run() {
        let mut steel = Steel::new();
        assert_eq!(steel.run("(define (sq x) (* x x))").unwrap(), Answer::default());
        let answer = steel.run(&format!("(display \"n=\" {OUTPUT_PORT}) (sq 5)")).unwrap();
        assert_eq!(answer, Answer { output: "n=".into(), value: "25".into() });
        assert_eq!(steel.run("(sq 2)").unwrap().output, "", "output does not carry over");
        assert!(steel.run("(car '())").is_err());
    }

    #[test]
    fn equals_takes_any_number_of_operands() {
        let mut steel = Steel::new();
        let value = |steel: &mut Steel, source: &str| steel.run(source).unwrap().value;
        assert_eq!(value(&mut steel, "(= 2 2 2)"), value(&mut steel, "#t"));
        assert_eq!(value(&mut steel, "(= 2 2 3)"), value(&mut steel, "#f"));
        assert_eq!(value(&mut steel, "(= 3 2 2)"), value(&mut steel, "#f"));
        assert_eq!(value(&mut steel, "(= 1)"), value(&mut steel, "#t"));
        assert_eq!(value(&mut steel, "(apply = (list 1 1))"), value(&mut steel, "#t"));
    }
}
