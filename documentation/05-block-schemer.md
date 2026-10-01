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
  columns, which the inspector wraps if it is narrower.

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
| `display` | `(display obj __out)` |

## Known gaps

- A run happens on the UI thread with no step limit, so a loop that never
  ends freezes the window.
- Steel will be replaced by a Scheme that runs in WASM, for a web version
  with no file system and a smaller library. Only `Scheme` needs a new
  implementation; the rest does not touch Steel.
