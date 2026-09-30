# block-parse

A generic front end for block-based (Scratch-like) programming. A consumer
defines its language in a config file, the user builds programs from blocks,
and the consumer asks for an AST to compile or interpret. Started from the
block editor in `~/rust/jellycell/src/coder/`, which is one hard-coded case.

## Crates

- `crates/block-parse` — core. serde + ron only, no geometry, no GUI.
  - `language`: `LanguageConfig` (as written, strings kept raw) compiles into a
    validated `Language`. Types give slot/reporter shape and literal kind;
    blocks are Hat / Statement / Cap / Reporter(type); a spec string like
    `"repeat {times:number=10} [body]"` gives inputs and C-block branches.
    `file.extension` names the language's program files (RON inside,
    whatever the extension) so consumers can bind file types. No types are
    built in. A reporter fits a slot on an exact type match, or when the
    slot's type `accepts` it or the reporter's type `fits` the slot; the last
    two appear in the AST as `Expr::Convert`, and converting is the consumer's
    job.
  - `literal`: literals are stored as typed and parsed when the AST is built,
    so invalid text stays in the program and shows as a problem. Built-in
    kinds (Float with e-notation, Integer, Number as an i64|f64 union,
    Currency in minor units, Binary, Hex, Text, Bool, Choice) plus
    consumer-registered `LiteralValidator`s for `Custom(name)`.
  - `program`: the saved document. Stacks with canvas positions; blocks keyed
    by stable `BlockId`, inputs/branches by name. Block positions inside a
    stack are derived, never stored. Loading is tolerant.
  - `edit`: tree operations by id (`detach`, `can_attach`, `attach`). All
    connection rules live here so GUI and headless tools agree.
  - `ast`: always a whole tree. Faults become `Problem` nodes in place (in
    `Stmt` or `Expr`), keeping what could be parsed in `recovered`. Only
    unreadable RON is fatal.
  - `debug`: `RunCommand`, `RunStatus`, `DebugView`, `trait Runner`. In core so
    interpreters need not depend on egui.
- `crates/block-parse-gui` — egui component `BlockEditor`. Feature `app` (off
  by default) adds eframe and the `block-parse-editor` binary
  (`cargo editor <language> [program]`).
- `examples/languages/` — sample language definitions: `tiny.ron` (loose,
  Scratch-style typing) and `strict_tiny.ron` (the same language with exact
  types and explicit conversions).

## Principles

- `BlockId` is the only handle the outside world has on a block: AST nodes,
  problems, breakpoints and pauses all use it.
- The editor runs nothing and stores no run state. Status, breakpoints, pauses
  and annotations come from the consumer each frame. Commands go out either as
  a polled list in `EditorOutput` or through a `Runner`. Breakpoints are
  requests; the consumer owns them and their persistence.
- Layout is pure given a `Measure`, computed at zoom 1 and scaled when drawn.
  Drawing, hit-testing and snapping all read one `Scene`.
- Opcodes, type names, input and branch names are ASCII identifiers.
- Nesting is capped at `MAX_DEPTH` (120): deeper blocks load as `TooDeep`
  problems and attaching checks the combined depth. The loader raises RON's
  own recursion limit (128 by default) to reach it.
- The editor never opens documentation links; it emits
  `EditorEvent::OpenDocumentation`. Untrusted language files are the user's
  risk, and escaping text for code generation is the back end's job.
- Fills are unions of convex pieces (epaint fans closed paths).
- Colors: a category gives an OKLCH hue, optionally chroma and lightness.
  Core only carries that. The GUI resolves it with `palette` into a `Swatch`
  (fill, edge, shadow, highlight, muted, ink) by stepping lightness and chroma,
  so every category sits at the same perceived lightness; displayed as sRGB.
  Other consumers choose their own scheme.

## Deferred

Runtime-supplied dropdowns (variables, procedures).

## Spelling

American spelling everywhere: code, identifiers, serde field names, comments
and docs (`color`, `center`, `behavior`).

## Comments

Comments are a cost. Keep only the why that the code cannot say: constraints,
discarded alternatives, invariants, units. One line is usually enough. No
restatements, history, banners or commented-out code. Prefer a better name or
type over a comment. The `decimate-comments` skill enforces this.
