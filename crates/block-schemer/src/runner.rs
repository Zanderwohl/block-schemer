//! Answers the editor's runs: generate, run, and put what came back in a
//! speech bubble.

use std::collections::HashMap;

use block_parse::ast::Script;
use block_parse::host::{Overlay, Runner};
use block_parse::program::{BlockId, Program};
use block_parse::Language;

use crate::codegen;
use crate::scheme::Scheme;

pub struct SchemerRunner<S> {
    language: Language,
    scheme: S,
    answers: HashMap<BlockId, String>,
}

impl<S: Scheme> SchemerRunner<S> {
    pub fn new(language: Language, scheme: S) -> Self {
        Self {
            language,
            scheme,
            answers: HashMap::new(),
        }
    }

    /// What running `script` says: output, then the value, or why it failed.
    pub fn answer(&mut self, script: &Script) -> String {
        let source = match codegen::script(&self.language, script) {
            Ok(source) => source,
            Err(problem) => return format!("Can't run: {problem}"),
        };
        match self.scheme.run(&source) {
            Ok(answer) => {
                let lines: Vec<&str> = [answer.output.as_str(), answer.value.as_str()]
                    .into_iter()
                    .filter(|line| !line.is_empty())
                    .collect();
                if lines.is_empty() { "ok".into() } else { lines.join("\n") }
            }
            Err(error) => error,
        }
    }
}

impl<S: Scheme> Runner for SchemerRunner<S> {
    fn overlay(&self) -> Overlay {
        Overlay {
            bubbles: self.answers.clone(),
            ..Overlay::default()
        }
    }

    fn run_block(&mut self, _program: &Program, block: BlockId, script: &Script) {
        let answer = self.answer(script);
        self.answers.insert(block, answer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Steel;

    #[test]
    fn the_example_runs_once_its_procedure_is_defined() {
        let language = crate::language();
        let program = Program::from_ron(include_str!("../examples/sum-of-squares.scmb")).unwrap();
        assert!(program.ast(&language).is_clean(), "{:#?}", program.ast(&language).problems());
        let mut runner = SchemerRunner::new(language.clone(), Steel::new());
        let mut run = |id: u64| {
            let block = BlockId(id);
            let script = program.script_at(&language, block).unwrap();
            runner.run_block(&program, block, &script);
            runner.overlay().bubbles[&block].clone()
        };

        assert!(run(20).contains("sum-of-squares"), "not defined yet");
        assert_eq!(run(1), "ok");
        assert_eq!(run(20), "14");
        assert_eq!(run(10), "hypotenuse squared:\n25");
        assert!(run(16).contains("before its definition"), "a reporter inside runs alone, outside its `let`");
    }
}
