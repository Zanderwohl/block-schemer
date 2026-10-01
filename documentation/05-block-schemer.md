# Block Schemer

`crates/block-schemer` is a consumer of block-parse: a block editor for a
small subset of R7RS Scheme whose blocks run when double-clicked. It is the
template for a host: a language file, validators, a `Runner`, and a `main`
that hands them to `block_parse_gui::app::run`.

```sh
cargo schemer                                              # a new program
cargo schemer crates/block-schemer/examples/sum-of-squares.scmb
```

![Runs answered by Steel](images/04-schemer-run.png)

## Pieces

- `scheme.ron`: the language, embedded with `include_str!` so the binary
  needs no files beside it. Programs are `.scmb`. Opcodes are Scheme's own
  names, so most blocks generate `(opcode arguments…)` in spec order.
  Categories are R7RS's section titles and inputs its argument names
  (`proc`, `formals`, `z`), except `cons`, whose inputs are `car` and `cdr`.
  Connecting words that are not Scheme, such as `if`'s `then` and `else`,
  are faint. `fold` is SRFI 1's, with its names, filed under Pairs and lists;
  Steel's takes one list.
- `literals`: validators for the `datum` type (a number, boolean, character,
  string in quotes or symbol) and the `symbol` type (an identifier). The
  editor shows text they refuse as a problem, and the code generator emits
  text they accept as typed.
- `codegen`: AST to Scheme text, the layer between untrusted blocks and the
  interpreter. `script`, what runs, refuses a script with any error-level
  problem. `pretty`, for Inspect and never run, writes each missing or
  faulty input as its name in angle brackets (`<test>`), a faulty block as
  far as it parsed, and leaves out a statement with nothing to recover. Only
  opcodes in the language reach it, so offering a user fewer blocks, such as
  a smaller language file for non-admins, limits what they can run. The text
  of a `string` block is escaped here; control characters other than
  newline, tab and return are refused.
- `form`: generated code as a tree of atoms and lists, printed on one line
  to run or laid out to read. Forms that fit stay on one line; `define`,
  `lambda`, `let` and the like keep their first argument on the head's line
  and indent their body two, one body form to a line when there are several;
  other calls line their arguments up under the first, or indent two under
  a name longer than ten characters.
- `scheme`: the `Scheme` trait, `run(source) -> Answer { output, value }`,
  and `Steel`, its implementation on Steel's sandboxed engine. Definitions
  last for the session, as in a REPL. `display` writes to a string port
  (`__out`), which is why names starting `__` are refused. A prelude evens
  out where Steel differs from R7RS: its `=` takes exactly two arguments, so
  `=` is redefined to take any number.
- `runner`: `SchemerRunner`, the `Runner` the editor calls on a double-click.
  The bubble shows what was displayed, then the value, `ok` for no value, or
  the reason it could not run. Its `inspect` gives `codegen::pretty` at 48
  columns, which its tab wraps if it is narrower. Lines typed into the
  console are echoed until programs can read them.

## Playing the program

Play (`RunCommand::Start`), or double-clicking the `program` block, runs the
canvas as if it were one file:

- Every stack headed by `define` or `define … taking`, in reading order (top
  to bottom, then left to right), then the `program` block's expression.
  Other loose blocks are scratch and left out, so their side effects do not
  run on every Play.
- All of it as one source. Steel refuses a name it has not yet seen within a
  run, so definitions run one at a time could not refer to later ones; in
  one source they can. They stay at top level, not in a body, so they are
  in scope for double-clicks afterwards.
- In a fresh session, so the result never depends on what was run before.
- A definition with a problem stops the run and is named in the console, as
  skipping it would only turn up later as an undefined name.
- With no `program` block, or several, nothing runs and the console says why.

What it displayed, then its value, goes to the Console tab, never a bubble,
after `> block-schemer <file name>`, or `untitled.scmb` until the program is
saved: the command that will one day run the file from a shell, which this
line should then match. A double-click on any other block answers in a
bubble and also writes `> ` and the expression, on one line, then what it
said, to the console.

## The harness

What runs is not quite what was entered: `display` writes to the `__out`
port so the console can show it. Code shown to the user, in Inspect and in
the console's echo, leaves that out, unless the "Schemer Harness" checkbox
at the left of the actions bar, a `Toggle` the runner offers, is on. It is
off at launch and not saved. Flipping it regenerates open inspections; lines
already in the console stay as they were written.
Inspect on the `program` block shows the same file, pretty-printed. An error
from Steel does not say which definition it came from.

## Special forms

| Block | Generates |
| --- | --- |
| `program` | its one expression |
| `string` | a string literal |
| `variable` | the name |
| `call` | `(operator operands…)` |
| `binding` | `(variable init)`, for `let` |
| `define_procedure` | `(define (variable formals…) body…)` |
| `lambda` | `(lambda (formals…) body…)` |
| `let` | `(let (bindings…) body…)` |
| `display` | `(display obj __out)`, shown as `(display obj)` unless the harness is |

## Dispatch

No run happens on the UI thread. The runner hands generated source to a
`dispatch::Dispatch` and takes answers back in `Runner::poll`, which the app
calls every frame:

- Jobs run one at a time, in the order sent, in one session; a Play's job
  asks for a fresh one first. Every job gets exactly one answer.
- Stop ends the running job and drops the queued ones, each answering
  "Stopped.", and the next job starts a fresh session. A Web Worker can only
  be stopped by terminating it, so losing the session is the rule for every
  implementation.
- `dispatch::native` is one thread that builds and owns the Scheme, so the
  Scheme need not be `Send`. Stop sets Steel's interrupt, through
  `Scheme::interrupter`, which Steel checks as it runs: an endless loop of
  calls stops within milliseconds. The interrupt stays set until the worker
  clears it before the next job, so one that lands just before a run starts
  still stops it. The worker remembers which stop its session dates from,
  so a stop while idle also loses it. A worker that dies answers its queue
  with an error and is replaced.

While a job runs, Play is disabled and Stop enabled. A double-click's echo
and answer reach the console together when it is answered; a Play's
`> block-schemer` line goes there at once. Only the latest double-click's
answer becomes a bubble; an earlier one still answering goes to the
console alone.

## Known gaps

- Steel will be replaced by a Scheme that runs in WASM, for a web version
  with no file system and a smaller library. Only `Scheme` needs a new
  implementation; the rest does not touch Steel.
