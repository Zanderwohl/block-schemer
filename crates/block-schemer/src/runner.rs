//! Answers the editor's runs: generate, run, and put what came back in a
//! speech bubble, or for the whole program, in the console. Output reaches
//! the console as it is written, and a run waiting on a read takes the
//! console's next line.
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
use crate::dispatch::{Dispatch, Event, Job, Ticket};

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
    /// As last polled, so `poll` reports a change.
    waiting: bool,
    /// Lines the runner wrote while runs were going, each to follow a run's
    /// answer.
    held: Vec<(Ticket, String)>,
    /// Show the `__out` port in echoed and inspected code.
    harness: bool,
}

enum Pending {
    /// `echo` is taken when written, before the first of its output, which
    /// `output` keeps for the bubble.
    Evaluate {
        block: BlockId,
        echo: Option<String>,
        output: String,
    },
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
            waiting: false,
            held: Vec::new(),
            harness: false,
        }
    }

    /// Everything the console has shown, input echoed.
    pub fn console(&self) -> &str {
        &self.console
    }

    /// On a line of its own, so it never runs on from what a program wrote.
    fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.console.is_empty() && !self.console.ends_with('\n') {
            self.console.push('\n');
        }
        self.console.push_str(text);
        if !text.ends_with('\n') {
            self.console.push('\n');
        }
    }

    /// Before the first of a double-click's output, its answer, or any
    /// input it waits for.
    fn echo(&mut self, ticket: Ticket) {
        if let Some(Pending::Evaluate { echo, .. }) = self.pending.get_mut(&ticket)
            && let Some(echo) = echo.take()
        {
            self.write(&format!("> {echo}"));
        }
    }

    /// As `write`, but after every run sent so far, so it never lands in
    /// one's output or between its prompt and the line entered for it.
    fn write_after_runs(&mut self, text: &str) {
        match self.pending.keys().max_by_key(|ticket| ticket.0) {
            Some(last) => self.held.push((*last, text.to_owned())),
            None => self.write(text),
        }
    }

    /// Jobs run in the order sent, so the earliest unanswered one is running.
    fn echo_running(&mut self) {
        if let Some(running) = self.pending.keys().min_by_key(|ticket| ticket.0) {
            self.echo(*running);
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
        self.write_after_runs(&format!("> block-schemer {file}"));
        match self.file(program, codegen::script) {
            Ok(parts) => {
                let ticket = self.dispatch.send(Job {
                    source: parts.join("\n"),
                    fresh: true,
                });
                self.pending.insert(ticket, Pending::Play);
            }
            Err(why) => self.write_after_runs(&why),
        }
    }

    fn evaluate(&mut self, block: BlockId, script: &Script) {
        // Nothing to echo when even the reading form fails; the error follows.
        let echo = codegen::flat(&self.language, script, self.harness)
            .ok()
            .filter(|echo| !echo.is_empty());
        match codegen::script(&self.language, script) {
            Ok(source) => {
                let ticket = self.dispatch.send(Job { source, fresh: false });
                self.latest = Some(ticket);
                let output = String::new();
                self.pending.insert(ticket, Pending::Evaluate { block, echo, output });
            }
            Err(problem) => {
                self.latest = None;
                if let Some(echo) = echo {
                    self.write_after_runs(&format!("> {echo}"));
                }
                let said = format!("Can't run: {problem}");
                self.write_after_runs(&said);
                self.answers.insert(block, said);
            }
        }
    }

    /// The console is a transcript, so it gets every answer; a bubble, only
    /// the `latest`.
    fn done(&mut self, ticket: Ticket, result: Result<String, String>) {
        self.echo(ticket);
        let said = result.unwrap_or_else(|error| error);
        self.write(&said);
        if let Some(Pending::Evaluate { block, output, .. }) = self.pending.remove(&ticket)
            && self.latest == Some(ticket)
        {
            let bubble = [output.as_str(), said.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            self.answers.insert(block, if bubble.is_empty() { "ok".into() } else { bubble });
        }
        let (now, later) = std::mem::take(&mut self.held).into_iter().partition(|(after, _)| *after == ticket);
        self.held = later;
        for (_, text) in now {
            self.write(&text);
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
                    waiting: self.waiting,
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
        let events = self.dispatch.poll();
        let waiting = self.dispatch.waiting();
        let changed = !events.is_empty() || waiting != self.waiting;
        self.waiting = waiting;
        for event in events {
            match event {
                Event::Output(ticket, text) => {
                    self.echo(ticket);
                    self.console.push_str(&text);
                    if let Some(Pending::Evaluate { output, .. }) = self.pending.get_mut(&ticket) {
                        output.push_str(&text);
                    }
                }
                Event::Done(ticket, result) => self.done(ticket, result),
            }
        }
        if waiting {
            self.echo_running();
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

    /// Echoed after what the run wrote, as a terminal does, and only echoed
    /// while nothing runs.
    fn console_input(&mut self, _tab: &TabId, line: &str) {
        let line = format!("{line}\n");
        if self.dispatch.busy() {
            self.echo_running();
            self.dispatch.input(&line);
        }
        self.console.push_str(&line);
    }

    fn console_end(&mut self, _tab: &TabId) {
        if self.dispatch.busy() {
            self.dispatch.end_input();
        }
    }

    /// The program block plays.
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

    const GREET: &str = r#"Program(language: "Block Schemer", version: 2, stacks: [
        (pos: (0.0, 0.0), blocks: [(id: 1, opcode: "define_procedure",
            inputs: {"variable": (literal: "greet")},
            lists: {"body": [
                (block: (id: 2, opcode: "display", inputs: {"obj": (block: (id: 3, opcode: "string", inputs: {"text": (literal: "Name? ")}))})),
                (block: (id: 4, opcode: "string-append", lists: {"string": [
                    (block: (id: 5, opcode: "string", inputs: {"text": (literal: "hi ")})),
                    (block: (id: 6, opcode: "read-line")),
                ]})),
            ]})]),
        (pos: (0.0, 200.0), blocks: [(id: 7, opcode: "program",
            inputs: {"main": (block: (id: 8, opcode: "call", inputs: {"operator": (literal: "greet")}))})]),
    ])"#;

    fn waiting(runner: &Tested) -> bool {
        matches!(runner.overlay().tabs[0].content, TabContent::Console { waiting: true, .. })
    }

    fn wait_for_input(runner: &mut Tested) {
        let start = Instant::now();
        while !waiting(runner) {
            assert!(start.elapsed() < Duration::from_secs(20), "never waited");
            runner.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_run_reading_the_console_waits_for_a_line_after_showing_its_prompt() {
        let language = crate::language();
        let program = Program::from_ron(GREET).unwrap();
        assert!(program.ast(&language).is_clean(), "{:#?}", program.ast(&language).problems());
        let mut runner = runner(&language);
        let console = TabId::from("console");
        runner.start(&program, None, &program.ast(&language));
        wait_for_input(&mut runner);
        assert_eq!(runner.console(), "> block-schemer untitled.scmb\nName? ");
        runner.console_input(&console, "Ada");
        settle(&mut runner);
        assert!(!waiting(&runner));
        assert_eq!(runner.console(), "> block-schemer untitled.scmb\nName? Ada\n\"hi Ada\"\n");

        let before = runner.console().len();
        let script = program.script_at(&language, BlockId(4)).unwrap();
        runner.run_block(&program, None, BlockId(4), &script);
        runner.console_input(&console, "Grace");
        settle(&mut runner);
        assert_eq!(
            &runner.console()[before..],
            "> (string-append \"hi \" (read-line))\nGrace\n\"hi Grace\"\n",
            "the echo comes before the line typed for it"
        );
        assert_eq!(runner.overlay().bubbles.get(&BlockId(4)).map(String::as_str), Some("\"hi Grace\""));

        let script = program.script_at(&language, BlockId(6)).unwrap();
        runner.run_block(&program, None, BlockId(6), &script);
        wait_for_input(&mut runner);
        assert!(runner.console().ends_with("> (read-line)\n"), "echoed while it waits: {}", runner.console());
        runner.console_end(&console);
        settle(&mut runner);
        assert_eq!(runner.overlay().bubbles[&BlockId(6)], "(eof)", "ended by Ctrl+D");

        runner.console_input(&console, "idle");
        assert!(runner.console().ends_with("\nidle\n"), "echoed");
        let read_line = program.script_at(&language, BlockId(6)).unwrap();
        runner.run_block(&program, None, BlockId(6), &read_line);
        wait_for_input(&mut runner);

        let before = runner.console().len();
        let script = program.script_at(&language, BlockId(7)).unwrap();
        runner.run_block(&program, None, BlockId(7), &script);
        assert_eq!(&runner.console()[before..], "", "held until the waiting run is answered");
        runner.console_input(&console, "x");
        let start = Instant::now();
        while !runner.console().ends_with("Name? ") {
            assert!(start.elapsed() < Duration::from_secs(20), "never played: {}", runner.console());
            runner.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            runner.console().ends_with("> (read-line)\nx\n\"x\"\n> block-schemer untitled.scmb\nName? "),
            "the idle line was never read: {}",
            runner.console()
        );
        runner.stop();
        settle(&mut runner);
    }

    #[test]
    fn the_guessing_game_can_be_won_by_halving() {
        const PROMPT: &str = "Guess a number from 1 to 100: ";
        let language = crate::language();
        let program = Program::from_ron(include_str!("../examples/guess-the-number.scmb")).unwrap();
        assert!(program.ast(&language).is_clean(), "{:#?}", program.ast(&language).problems());
        let mut runner = runner(&language);
        runner.start(&program, None, &program.ast(&language));
        wait_for_input(&mut runner);
        // What the game says to `line`, up to its next prompt or the end.
        let mut answer = |line: &str| {
            runner.console_input(&TabId::from("console"), line);
            let before = runner.console().len();
            let start = Instant::now();
            while !runner.console().ends_with(PROMPT) || runner.console().len() == before {
                if runner.status() == RunStatus::Idle {
                    break;
                }
                assert!(start.elapsed() < Duration::from_secs(20), "no answer to {line}");
                runner.poll();
                std::thread::sleep(Duration::from_millis(5));
            }
            runner.console()[before..].trim_end_matches(PROMPT).trim().to_owned()
        };
        assert_eq!(answer("lots"), "That's not a number.");
        let (mut low, mut high) = (1, 100);
        for tries in 1..=7 {
            let middle = (low + high) / 2;
            match answer(&middle.to_string()).as_str() {
                "Higher!" => low = middle + 1,
                "Lower!" => high = middle - 1,
                said => {
                    assert_eq!(said, format!("Got it! Guesses: {tries}"));
                    return;
                }
            }
        }
        panic!("halving finds it in seven");
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
