//! The interpreter behind Block Schemer, kept to one small trait so Steel can
//! be swapped for another Scheme, such as one that runs in WASM.

use steel::SteelVal;
use steel::steel_vm::engine::Engine;

use crate::codegen::OUTPUT_PORT;

/// One session: definitions from earlier runs stay in scope.
pub trait Scheme {
    /// Runs `source` and gives back what `display` wrote and the written form
    /// of the last value, empty for none.
    fn run(&mut self, source: &str) -> Result<Answer, String>;
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Answer {
    pub output: String,
    pub value: String,
}

/// Steel's sandboxed engine, which loads no native libraries.
pub struct Steel {
    engine: Engine,
}

impl Steel {
    pub fn new() -> Self {
        let mut engine = Engine::new_sandboxed();
        engine
            .run(format!("(define {OUTPUT_PORT} (open-output-string))"))
            .expect("Steel opens a string port");
        Self { engine }
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
}
