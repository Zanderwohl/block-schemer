//! The interpreter behind Block Schemer, kept to one small trait so Steel can
//! be swapped for another Scheme, such as one that runs in WASM.

use std::sync::{Arc, Mutex};

use steel::SteelVal;
use steel::steel_vm::ThreadStateController;
use steel::steel_vm::engine::Engine;
use steel::steel_vm::register_fn::RegisterFn;

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

/// Steel builtins the prelude redefines, each kept as `__steel-<name>`.
/// Saved in a run of their own: in one program, Steel would take `=` in
/// `(define (= …) … =)` to mean the one being defined.
const STEEL_ORIGINALS: [&str; 17] = [
    "=",
    "gcd",
    "lcm",
    "atan",
    "member",
    "assoc",
    "error",
    "error-object?",
    "error-object-message",
    "close-port",
    "close-input-port",
    "close-output-port",
    "open-input-bytevector",
    "open-output-bytevector",
    "write-string",
    "flush-output-port",
    "make-parameter",
];

/// Evens out where Steel differs from R7RS, after [`STEEL_ORIGINALS`].
const STEEL_PRELUDE: &str = include_str!("prelude.scm");

/// Sends everything written without a port, errors included, to the console.
fn ports() -> String {
    format!("(current-output-port {OUTPUT_PORT}) (current-error-port {OUTPUT_PORT})")
}

/// Character classes R7RS defines by Unicode property, which Steel lacks.
/// `digit-value` knows only ASCII digits.
fn register_chars(engine: &mut Engine) {
    engine.register_fn("char-alphabetic?", |c: char| c.is_alphabetic());
    engine.register_fn("char-numeric?", |c: char| c.is_numeric());
    engine.register_fn("char-upper-case?", |c: char| c.is_uppercase());
    engine.register_fn("char-lower-case?", |c: char| c.is_lowercase());
    engine.register_fn("digit-value", |c: char| c.to_digit(10).map(|d| d as i64));
    engine.register_fn("__atan2", |y: f64, x: f64| y.atan2(x));
}

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
        register_chars(&mut engine);
        let originals: String = STEEL_ORIGINALS.iter().map(|name| format!("(define __steel-{name} {name})")).collect();
        engine.run(originals).expect("Steel has every original");
        engine.run(STEEL_PRELUDE).expect("the prelude runs");
        // Reading the app's own stdin would block the worker beyond Stop's reach.
        engine
            .run(format!(
                "(define {OUTPUT_PORT} (open-output-string)) {} (current-input-port (open-input-string \"\"))",
                ports()
            ))
            .expect("Steel opens string ports");
        engine
    }

    fn take_output(&mut self) -> String {
        let source = format!(
            "(define {OUTPUT_PORT}-text (get-output-string {OUTPUT_PORT})) \
             (set! {OUTPUT_PORT} (open-output-string)) {} \
             {OUTPUT_PORT}-text",
            ports()
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

    fn is_true(steel: &mut Steel, source: &str) -> bool {
        match steel.run(source) {
            Ok(answer) => answer.value == "#true",
            Err(error) => panic!("{source}: {error}"),
        }
    }

    /// Steel's are callable structs, which `procedure?` refuses.
    const PARAMETERS: [&str; 3] = ["current-input-port", "current-output-port", "current-error-port"];

    #[test]
    fn every_procedure_block_names_a_procedure() {
        let language = crate::language();
        let mut steel = Steel::new();
        let missing: Vec<&str> = language
            .blocks()
            .iter()
            .map(|block| block.opcode.as_str())
            .filter(|opcode| !crate::codegen::SYNTAX.contains(opcode) && !PARAMETERS.contains(opcode))
            .map(crate::codegen::scheme_name)
            .filter(|name| !is_true(&mut steel, &format!("(procedure? {name})")))
            .collect();
        assert!(missing.is_empty(), "not procedures in Steel: {missing:?}");
    }

    #[test]
    fn the_prelude_follows_r7rs() {
        let mut steel = Steel::new();
        for (expression, expected) in [
            ("(boolean=? #t #t #t)", "#t"),
            ("(boolean=? #f #f 1)", "#f"),
            ("(gcd 32 -36 8)", "4"),
            ("(gcd)", "0"),
            ("(lcm 32 -36)", "288"),
            ("(lcm)", "1"),
            ("(< (abs (- (atan 1 1) (/ (acos -1) 4))) 1e-12)", "#t"),
            ("(rationalize (exact .3) 1/10)", "1/3"),
            ("(rationalize 3/10 1/10)", "1/3"),
            ("(inexact? (rationalize .3 1/10))", "#t"),
            ("(rationalize -3/10 1/10)", "-1/3"),
            ("(make-list 2 'x)", "(x x)"),
            ("(list-copy '(1 2 . 3))", "(1 2 . 3)"),
            ("(member 2.0 '(1 2 3) =)", "(2 3)"),
            ("(member \"B\" '(\"a\" \"b\") string-ci=?)", "(\"b\")"),
            ("(member 2 '(1 2 3))", "(2 3)"),
            ("(assoc 2.0 '((1 1) (2 4) (3 9)) =)", "(2 4)"),
            ("(assoc 'b '((a 1) (b 2)))", "(b 2)"),
            ("(string-copy \"hello\" 1 3)", "\"el\""),
            ("(string-copy \"hello\" 2)", "\"llo\""),
            ("(string-map char-upcase \"abc\")", "\"ABC\""),
            ("(vector-map + #(1 2) #(10 20))", "#(11 22)"),
            (
                "(let ((n 0)) (vector-for-each (lambda (x) (set! n (+ n x))) #(1 2 3)) n)",
                "6",
            ),
            (
                "(let ((n 0)) (string-for-each (lambda (c) (set! n (+ n 1))) \"ab\") n)",
                "2",
            ),
            ("(char-alphabetic? #\\λ)", "#t"),
            ("(char-numeric? #\\a)", "#f"),
            ("(char-upper-case? #\\A)", "#t"),
            ("(char-lower-case? #\\A)", "#f"),
            ("(digit-value #\\7)", "7"),
            ("(digit-value #\\a)", "#f"),
            ("((make-parameter 5 (lambda (x) (* x 2))))", "10"),
            ("((make-parameter 5))", "5"),
            ("(force (make-promise 5))", "5"),
            ("(promise? (make-promise 5))", "#t"),
            ("(force 5)", "5"),
            ("(read-string 2 (open-input-string \"abc\"))", "\"ab\""),
            ("(eof-object? (read-string 2 (open-input-string \"\")))", "#t"),
            (
                "(let ((b (make-bytevector 4 0))) (read-bytevector! b (open-input-bytevector (bytevector 1 2)) 1) b)",
                "#u8(0 1 2 0)",
            ),
            (
                "(let ((b (bytevector 1 2 3 4 5))) (bytevector-copy! b 1 (bytevector 9 8 7) 1) b)",
                "#u8(1 8 7 4 5)",
            ),
            (
                "(let ((p (open-output-string))) (write-string \"abcd\" p 1 3) (get-output-string p))",
                "\"bc\"",
            ),
            (
                "(let ((p (open-input-string \"x\"))) (close-port p) (input-port-open? p))",
                "#f",
            ),
            ("(input-port-open? (open-input-string \"x\"))", "#t"),
            ("(binary-port? (open-input-bytevector (bytevector)))", "#t"),
            ("(textual-port? (open-input-string \"\"))", "#t"),
            ("(char-ready? (open-input-string \"\"))", "#t"),
            ("(eof-object? (read-char))", "#t"),
            ("(inexact? (current-second))", "#t"),
            ("(exact-integer? (current-jiffy))", "#t"),
        ] {
            let source = format!("(equal? {expression} '{expected})");
            assert!(is_true(&mut steel, &source), "{expression} should be {expected}");
        }
    }

    #[test]
    fn exceptions_follow_r7rs() {
        let mut steel = Steel::new();
        let escape = |handler: &str, body: &str| {
            format!("(call/cc (lambda (k) (with-exception-handler (lambda (e) (k {handler})) (lambda () {body}))))")
        };
        assert!(is_true(
            &mut steel,
            &format!("(equal? {} 42)", escape("e", "(raise 42)"))
        ));
        let message = escape(
            "(list (error-object-message e) (error-object-irritants e))",
            "(error \"boom\" 1 2)",
        );
        assert!(is_true(&mut steel, &format!("(equal? {message} '(\"boom\" (1 2)))")));
        assert!(is_true(
            &mut steel,
            &format!("(error-object? {})", escape("e", "(car '())"))
        ));
        assert!(is_true(
            &mut steel,
            &format!("(equal? {} 'inner)", escape("e", &escape("'inner", "(raise 'x)")))
        ));
        let continued = "(with-exception-handler (lambda (e) 10) (lambda () (+ 1 (raise-continuable 'c))))";
        assert!(is_true(&mut steel, &format!("(= {continued} 11)")));

        let calls = "(let ((calls 0)) \
             (call/cc (lambda (k) \
               (with-exception-handler (lambda (e) (k 'outer)) \
                 (lambda () \
                   (with-exception-handler (lambda (e) (set! calls (+ calls 1)) (car '())) \
                     (lambda () (raise 'x))))))) \
             calls)";
        assert!(is_true(&mut steel, &format!("(= {calls} 1)")), "an error in a handler goes outward, once");

        let returned = steel.run("(with-exception-handler (lambda (e) 0) (lambda () (raise 'oops)))");
        assert!(returned.is_err(), "a handler may not return from raise");
        let uncaught = steel.run("(error \"boom\" 1 2)").unwrap_err();
        assert!(uncaught.contains("boom 1 2"), "{uncaught}");
        assert!(steel.run("(raise 'oops)").unwrap_err().contains("oops"));
        assert!(
            is_true(&mut steel, &format!("(equal? {} 1)", escape("1", "(raise 'again)"))),
            "handlers work after an uncaught raise"
        );
    }

    #[test]
    fn writing_without_a_port_goes_to_the_console() {
        let mut steel = Steel::new();
        let answer = steel
            .run("(write \"a\") (newline) (write-char #\\b) (write-string \"c\") (display 1)")
            .unwrap();
        assert_eq!(answer.output, "\"a\"\nbc1");
        let answer = steel
            .run("(write 'x (current-error-port)) (for-each display '(1 2))")
            .unwrap();
        assert_eq!(answer.output, "x12", "and so does a procedure passed by name");
    }

    #[test]
    fn stop_ends_a_run_inside_a_handler() {
        let mut steel = Steel::new();
        let interrupter = steel.interrupter();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            interrupter.interrupt();
        });
        let source = "(let retry () \
             (call/cc (lambda (k) (with-exception-handler (lambda (e) (k 0)) (lambda () (let loop () (loop)))))) \
             (retry))";
        assert!(steel.run(source).is_err());
        stopper.join().unwrap();
    }
}
