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
- `literals`: validators for the `datum` type (a number, boolean, character,
  string in quotes or symbol) and the `symbol` type (an identifier). The
  editor shows text they refuse as a problem, and the code generator emits
  text they accept as typed.
- `codegen`: AST to Scheme text, the layer between untrusted blocks and the
  interpreter. A script with any error-level problem is not generated. Only
  opcodes in the language reach it, so offering a user fewer blocks, such as
  a smaller language file for non-admins, limits what they can run. The text
  of a `string` block is escaped here; control characters other than
  newline, tab and return are refused.
- `scheme`: the `Scheme` trait, `run(source) -> Answer { output, value }`,
  and `Steel`, its implementation on Steel's sandboxed engine. Definitions
  last for the session, as in a REPL. `display` writes to a string port
  (`__out`), which is why names starting `__` are refused.
- `runner`: `SchemerRunner`, the `Runner` the editor calls on a double-click.
  The bubble shows what was displayed, then the value, `ok` for no value, or
  the reason it could not run.

## Special forms

| Block | Generates |
| --- | --- |
| `program` | its one expression |
| `string` | a string literal |
| `variable` | the name |
| `call` | `(procedure arguments…)` |
| `binding` | `(name value)`, for `let` |
| `define_procedure` | `(define (name parameters…) body…)` |
| `lambda` | `(lambda (parameters…) body…)` |
| `let` | `(let (bindings…) body…)` |
| `display` | `(display value __out)` |

## Known gaps

- Steel's `=` takes exactly two arguments, unlike R7RS.
- A run happens on the UI thread with no step limit, so a loop that never
  ends freezes the window.
- Steel will be replaced by a Scheme that runs in WASM, for a web version
  with no file system and a smaller library. Only `Scheme` needs a new
  implementation; the rest does not touch Steel.
