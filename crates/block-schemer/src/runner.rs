//! Answers the editor's runs: generate, run, and put what came back in a
//! speech bubble, or for the whole program, in the console.
//!
//! The whole program is the canvas read as one file: every definition stack
//! in reading order, then the program block's expression, run as one source
//! in a fresh session. Steel refuses a name it has not yet seen within a run,
//! so definitions run one at a time could not refer forward.
//!
//! Every run goes to a [`Dispatch`], so the UI never waits on one; answers
//! come back through `poll`.

use std::collections::HashMap;

use std::cmp::Ordering;
use std::path::Path;

use block_parse::ast::{Ast, Script};
use block_parse::host::{Overlay, RunCommand, RunStatus, Runner, Tab, TabContent, TabId, Toggle};
use block_parse::program::{Block, BlockId, Program};
use block_parse::Language;

use crate::codegen;
use crate::dispatch::{Dispatch, Job, Ticket};
use crate::scheme::Answer;

/// Columns `inspect` lays code out to; its tab wraps anything wider.
const INSPECT_WIDTH: usize = 48;

/// Stacks headed by these are the file's definitions; other loose blocks are
/// scratch, left out so their side effects do not run on every Play.
const DEFINITIONS: [&str; 2] = ["define", "define_procedure"];
const PROGRAM: &str = "program";
const HARNESS: &str = "harness";

pub struct SchemerRunner<D> {
    language: Language,
    dispatch: D,
    pending: HashMap<Ticket, Pending>,
    /// The latest double-click's; an earlier one's answer is not a bubble.
    latest: Option<Ticket>,
    answers: HashMap<BlockId, String>,
    console: String,
    /// Show the `__out` port in echoed and inspected code.
    harness: bool,
}

/// What a ticket's answer is for.
enum Pending {
    Evaluate { block: BlockId, echo: Option<String> },
    Play,
}

impl<D: Dispatch> SchemerRunner<D> {
    pub fn new(language: Language, dispatch: D) -> Self {
        Self {
            language,
            dispatch,
            pending: HashMap::new(),
            latest: None,
            answers: HashMap::new(),
            console: String::new(),
            harness: false,
        }
    }

    /// Everything the console has shown, input echoed.
    pub fn console(&self) -> &str {
        &self.console
    }

    /// Ends with a newline, so the next run starts on its own line.
    fn write(&mut self, text: &str) {
        self.console.push_str(text);
        if !text.is_empty() && !text.ends_with('\n') {
            self.console.push('\n');
        }
    }

    /// Each part through `generate`. `Err` says why it cannot run, naming
    /// the definition at fault.
    fn file(
        &self,
        program: &Program,
        generate: impl Fn(&Language, &Script) -> Result<String, String>,
    ) -> Result<Vec<String>, String> {
        let mut stacks: Vec<_> = program.stacks.iter().collect();
        stacks.sort_by(|a, b| {
            (a.pos[1], a.pos[0])
                .partial_cmp(&(b.pos[1], b.pos[0]))
                .unwrap_or(Ordering::Equal)
        });
        let heads: Vec<&Block> = stacks.iter().filter_map(|stack| stack.blocks.first()).collect();
        let programs: Vec<&Block> = heads.iter().copied().filter(|head| head.opcode == PROGRAM).collect();
        let main = match programs[..] {
            [] => return Err("Nothing to run: there is no program block.".into()),
            [main] => main,
            _ => {
                let count = programs.len();
                return Err(format!("Can't run: there are {count} program blocks, and only one may run."));
            }
        };
        let script = |head: &Block| {
            program
                .script_at(&self.language, head.id)
                .ok_or_else(|| format!("Can't run: block {} is not in the program", head.id.0))
        };
        let mut parts = Vec::new();
        for head in heads.iter().filter(|head| DEFINITIONS.contains(&head.opcode.as_str())) {
            let name = head
                .inputs
                .get("variable")
                .and_then(|input| input.literal.as_deref())
                .filter(|name| !name.is_empty());
            let what = name.map_or_else(|| "a definition".to_owned(), |name| format!("the definition of {name}"));
            let part = generate(&self.language, &script(head)?);
            parts.push(part.map_err(|problem| format!("Can't run {what}: {problem}"))?);
        }
        let part = generate(&self.language, &script(main)?);
        parts.push(part.map_err(|problem| format!("Can't run: {problem}"))?);
        Ok(parts)
    }

    /// The `> block-schemer` line is the command that will one day do the
    /// same from a shell.
    fn play(&mut self, program: &Program, path: Option<&Path>) {
        let file = path
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("untitled.{}", self.language.file.extension));
        self.write(&format!("> block-schemer {file}"));
        match self.file(program, codegen::script) {
            Ok(parts) => {
                let ticket = self.dispatch.send(Job {
                    source: parts.join("\n"),
                    fresh: true,
                });
                self.pending.insert(ticket, Pending::Play);
            }
            Err(why) => self.write(&why),
        }
    }

    /// Echoed with its answer, so the echo and what it said stay together.
    fn evaluate(&mut self, block: BlockId, script: &Script) {
        // Nothing to echo when even the reading form fails; the error follows.
        let echo = codegen::flat(&self.language, script, self.harness)
            .ok()
            .filter(|echo| !echo.is_empty());
        match codegen::script(&self.language, script) {
            Ok(source) => {
                let ticket = self.dispatch.send(Job { source, fresh: false });
                self.latest = Some(ticket);
                self.pending.insert(ticket, Pending::Evaluate { block, echo });
            }
            Err(problem) => {
                self.latest = None;
                self.answer(block, echo, Err(format!("Can't run: {problem}")), true);
            }
        }
    }

    /// The console is a transcript, so it gets every answer; a bubble, only
    /// the `latest`.
    fn answer(&mut self, block: BlockId, echo: Option<String>, result: Result<Answer, String>, latest: bool) {
        if let Some(echo) = echo {
            self.write(&format!("> {echo}"));
        }
        let (said, bubble) = match result {
            Ok(answer) if answer == Answer::default() => (String::new(), "ok".into()),
            Ok(answer) => (shown(&answer), shown(&answer)),
            Err(error) => (error.clone(), error),
        };
        self.write(&said);
        if latest {
            self.answers.insert(block, bubble);
        }
    }
}

impl<D: Dispatch> Runner for SchemerRunner<D> {
    fn overlay(&self) -> Overlay {
        Overlay {
            bubbles: self.answers.clone(),
            tabs: vec![Tab {
                id: TabId::from("console"),
                title: "Console".into(),
                closable: false,
                content: TabContent::Console {
                    output: self.console.clone(),
                },
            }],
            ..Overlay::default()
        }
    }

    fn status(&self) -> RunStatus {
        match self.dispatch.busy() {
            true => RunStatus::Running,
            false => RunStatus::Idle,
        }
    }

    fn supports(&self, command: RunCommand) -> bool {
        matches!(command, RunCommand::Start | RunCommand::Stop)
    }

    fn poll(&mut self) -> bool {
        let answers = self.dispatch.poll();
        let changed = !answers.is_empty();
        for (ticket, result) in answers {
            match self.pending.remove(&ticket) {
                Some(Pending::Evaluate { block, echo }) => {
                    let latest = self.latest == Some(ticket);
                    self.answer(block, echo, result, latest);
                }
                Some(Pending::Play) => self.write(&result.map_or_else(|error| error, |answer| shown(&answer))),
                None => {}
            }
        }
        changed
    }

    fn stop(&mut self) {
        self.dispatch.stop();
    }

    /// With several program blocks, which one is meant is not yet decided,
    /// so none runs.
    fn start(&mut self, program: &Program, path: Option<&Path>, _ast: &Ast) {
        self.play(program, path);
    }

    fn toggles(&self) -> Vec<Toggle> {
        vec![Toggle {
            id: HARNESS.into(),
            label: "Schemer Harness".into(),
            on: self.harness,
            hint: Some("Show code as it runs, with the port that carries display to the console".into()),
        }]
    }

    fn set_toggle(&mut self, id: &str, on: bool) {
        if id == HARNESS {
            self.harness = on;
        }
    }

    /// Echoed until programs can read input.
    fn console_input(&mut self, _tab: &TabId, line: &str) {
        self.write(&format!("{line}\n"));
    }

    /// Only the latest run's answer is kept as a bubble, as the app dismisses
    /// bubbles on the next edit or click. The program block plays.
    fn run_block(&mut self, program: &Program, path: Option<&Path>, block: BlockId, script: &Script) {
        self.answers.clear();
        if is_program(program, block) {
            self.play(program, path);
        } else {
            self.evaluate(block, script);
        }
    }

    /// The program block shows the whole file Play would run.
    fn inspect(&mut self, program: &Program, block: BlockId, script: &Script) -> Option<String> {
        let harness = self.harness;
        let pretty = |language: &Language, script: &Script| codegen::pretty(language, script, INSPECT_WIDTH, harness);
        let text = match is_program(program, block) {
            true => self.file(program, pretty).map(|parts| parts.join("\n\n")),
            false => pretty(&self.language, script).map_err(|problem| format!("Can't generate: {problem}")),
        };
        Some(text.unwrap_or_else(|why| why))
    }
}

fn shown(answer: &Answer) -> String {
    [answer.output.as_str(), answer.value.as_str()]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_program(program: &Program, block: BlockId) -> bool {
    program.find(block).is_some_and(|block| block.opcode == PROGRAM)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{Native, Steel};

    type Tested = SchemerRunner<Native<Steel>>;

    fn runner(language: &Language) -> Tested {
        SchemerRunner::new(language.clone(), Native::spawn(Steel::new))
    }

    /// Polls until every run is answered.
    fn settle(runner: &mut Tested) {
        let start = Instant::now();
        while runner.status() == RunStatus::Running {
            assert!(start.elapsed() < Duration::from_secs(20), "still running");
            runner.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn example() -> (Language, Program, Tested) {
        let language = crate::language();
        let program = Program::from_ron(include_str!("../examples/sum-of-squares.scmb")).unwrap();
        assert!(program.ast(&language).is_clean(), "{:#?}", program.ast(&language).problems());
        let runner = runner(&language);
        (language, program, runner)
    }

    /// Double-clicks `id`, and gives back its bubble and what the console gained.
    fn double_click(runner: &mut Tested, language: &Language, program: &Program, id: u64) -> (Option<String>, String) {
        let before = runner.console().len();
        let script = program.script_at(language, BlockId(id)).unwrap();
        runner.run_block(program, None, BlockId(id), &script);
        settle(runner);
        let bubble = runner.overlay().bubbles.get(&BlockId(id)).cloned();
        (bubble, runner.console()[before..].to_owned())
    }

    fn play(runner: &mut Tested, language: &Language, program: &Program) -> String {
        let before = runner.console().len();
        runner.start(program, None, &program.ast(language));
        settle(runner);
        runner.console()[before..].to_owned()
    }

    #[test]
    fn other_blocks_answer_in_a_bubble_within_the_session() {
        let (language, program, mut runner) = example();
        let mut run = |id| double_click(&mut runner, &language, &program, id);
        assert_eq!(
            run(1),
            (
                Some("ok".into()),
                "> (define (sum-of-squares xs) (fold + 0 (map (lambda (x) (* x x)) xs)))\n".into()
            )
        );
        assert_eq!(run(10).0.as_deref(), Some("hypotenuse squared:\n25"));
        let (bubble, console) = run(16);
        assert!(bubble.unwrap().contains("before its definition"), "a reporter inside runs alone, outside its `let`");
        assert!(console.starts_with("> (* a a)\nError: "), "{console}");

        let shown = |runner: &mut Tested| {
            let console = double_click(runner, &language, &program, 10).1;
            console.lines().next().unwrap().to_owned()
        };
        assert!(shown(&mut runner).contains("(display \"hypotenuse squared:\")"), "as entered");
        runner.set_toggle(HARNESS, true);
        assert!(runner.toggles()[0].on);
        let harnessed = format!("(display \"hypotenuse squared:\" {})", codegen::OUTPUT_PORT);
        assert!(shown(&mut runner).contains(&harnessed));
    }

    #[test]
    fn playing_runs_the_definitions_then_the_program_but_no_scratch() {
        let (language, program, mut runner) = example();
        let played = "> block-schemer untitled.scmb\n14\n";
        assert_eq!(play(&mut runner, &language, &program), played, "the `let` stack is scratch");
        assert_eq!(
            double_click(&mut runner, &language, &program, 20),
            (None, played.into()),
            "the program block plays"
        );

        let before = runner.console().len();
        let path = Path::new("/somewhere/sums.scmb");
        runner.start(&program, Some(path), &program.ast(&language));
        settle(&mut runner);
        assert_eq!(&runner.console()[before..], "> block-schemer sums.scmb\n14\n");
    }

    #[test]
    fn each_play_starts_a_fresh_session() {
        let (language, mut program, mut runner) = example();
        double_click(&mut runner, &language, &program, 1);
        program.stacks.retain(|stack| stack.blocks[0].opcode != "define_procedure");
        assert!(play(&mut runner, &language, &program).contains("sum-of-squares"), "defined only by an old run");
    }

    #[test]
    fn definitions_go_in_reading_order() {
        let (language, mut program, mut runner) = example();
        let define = |program: &mut Program, name: &str, value: &str, pos: [f32; 2]| {
            let block = program.instantiate(&language, "define").unwrap();
            let id = block.id;
            program.stacks.push(block_parse::Stack {
                pos,
                blocks: vec![block],
            });
            program.set_literal(id, &block_parse::Slot::input("variable"), name.into());
            program.set_literal(id, &block_parse::Slot::input("expression"), value.into());
        };
        define(&mut program, "c", "3", [300.0, 600.0]);
        define(&mut program, "b", "2", [600.0, 24.0]);
        define(&mut program, "a", "1", [600.0, 0.0]);
        let script = program.script_at(&language, BlockId(20)).unwrap();
        let file = runner.inspect(&program, BlockId(20), &script).unwrap();
        let at = |text: &str| file.find(text).unwrap_or_else(|| panic!("no {text} in {file}"));
        assert!(at("(define a 1)") < at("(define (sum-of-squares"), "higher first");
        assert!(at("(define (sum-of-squares") < at("(define b 2)"), "at one height, left first");
        assert!(at("(define b 2)") < at("(define c 3)"));
        assert!(at("(define c 3)") < at("(sum-of-squares (list"), "the program last, wherever it is");
    }

    #[test]
    fn a_broken_definition_stops_the_run() {
        let (language, mut program, mut runner) = example();
        program.set_literal(BlockId(1), &block_parse::Slot::input("variable"), String::new());
        let said = play(&mut runner, &language, &program);
        assert!(said.starts_with("> block-schemer untitled.scmb\nCan't run a definition: "), "{said}");

        program.set_literal(BlockId(1), &block_parse::Slot::input("variable"), "1x".into());
        let said = play(&mut runner, &language, &program);
        assert!(said.contains("\nCan't run the definition of 1x: "), "{said}");
    }

    #[test]
    fn playing_needs_exactly_one_program_block() {
        let (language, mut program, mut runner) = example();
        let mut copy = program.stacks.iter().find(|stack| stack.blocks[0].opcode == "program").unwrap().clone();
        copy.pos[1] += 200.0;
        copy.blocks[0].id = BlockId(100);
        copy.blocks[0].inputs.clear();
        program.stacks.push(copy);
        assert_eq!(
            play(&mut runner, &language, &program),
            "> block-schemer untitled.scmb\nCan't run: there are 2 program blocks, and only one may run.\n"
        );

        program.stacks.retain(|stack| stack.blocks[0].opcode != "program");
        let said = play(&mut runner, &language, &program);
        assert_eq!(said, "> block-schemer untitled.scmb\nNothing to run: there is no program block.\n");
        runner.console_input(&TabId::from("console"), "(+ 1 2)");
        assert!(runner.console().ends_with("block.\n(+ 1 2)\n"));
    }

    #[test]
    fn inspecting_shows_the_generated_scheme_without_running_it() {
        let (language, program, mut runner) = example();
        let mut inspect = |id| {
            let script = program.script_at(&language, BlockId(id)).unwrap();
            runner.inspect(&program, BlockId(id), &script).unwrap()
        };
        assert_eq!(inspect(16), "(* a a)");
        let file = inspect(20);
        assert!(file.starts_with("(define (sum-of-squares xs)"), "{file}");
        assert!(file.ends_with("\n\n(sum-of-squares (list 1 2 3))"), "{file}");
        assert!(!file.contains("hypotenuse"), "{file}");
        assert!(!inspect(10).contains(codegen::OUTPUT_PORT));
        runner.set_toggle(HARNESS, true);
        let script = program.script_at(&language, BlockId(10)).unwrap();
        assert!(runner.inspect(&program, BlockId(10), &script).unwrap().contains(codegen::OUTPUT_PORT));
        assert!(runner.overlay().bubbles.is_empty());
        assert!(runner.console().is_empty());
    }

    #[test]
    fn only_the_latest_double_click_answers_in_a_bubble() {
        let (language, program, mut runner) = example();
        for id in [10, 1] {
            let script = program.script_at(&language, BlockId(id)).unwrap();
            runner.run_block(&program, None, BlockId(id), &script);
        }
        settle(&mut runner);
        let bubbles = runner.overlay().bubbles;
        assert_eq!(bubbles.get(&BlockId(1)).map(String::as_str), Some("ok"));
        assert!(!bubbles.contains_key(&BlockId(10)), "superseded before it answered");
        assert!(runner.console().contains("hypotenuse squared:\n25\n"), "the transcript keeps it");
    }

    #[test]
    fn a_run_answers_later_and_stop_ends_a_runaway_one() {
        let language = crate::language();
        let program = Program::from_ron(
            r#"Program(language: "Block Schemer", version: 2, stacks: [
                (pos: (0.0, 0.0), blocks: [(id: 1, opcode: "define_procedure",
                    inputs: {"variable": (literal: "spin")},
                    lists: {"body": [(block: (id: 2, opcode: "call", inputs: {"operator": (literal: "spin")}))]})]),
                (pos: (0.0, 100.0), blocks: [(id: 3, opcode: "program",
                    inputs: {"main": (block: (id: 4, opcode: "call", inputs: {"operator": (literal: "spin")}))})]),
            ])"#,
        )
        .unwrap();
        let mut runner = runner(&language);
        runner.start(&program, None, &program.ast(&language));
        assert_eq!(runner.console(), "> block-schemer untitled.scmb\n", "written at once, before the answer");
        std::thread::sleep(Duration::from_millis(100));
        assert!(!runner.poll());
        assert_eq!(runner.status(), RunStatus::Running);

        runner.stop();
        settle(&mut runner);
        assert!(runner.console().ends_with("\nStopped.\n"), "{}", runner.console());
    }
}
