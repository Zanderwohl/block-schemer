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
    consumer-registered `LiteralValidator`s for `Custom(name)`, which may also
    `normalize` text when a field loses focus (never while typing).
  - `program`: the saved document. Stacks with canvas positions; blocks keyed
    by stable `BlockId`, inputs/branches by name. Block positions inside a
    stack are derived, never stored. Loading is tolerant.
  - `edit`: tree operations by id (`detach`, `can_attach`, `attach`). All
    connection rules live here so GUI and headless tools agree.
  - `ast`: always a whole tree. Faults become `Problem` nodes in place (in
    `Stmt` or `Expr`), keeping what could be parsed in `recovered`. Only
    unreadable RON is fatal.
  - `host`: `Overlay` (breakpoints, highlights, annotations, muted blocks),
    `RunCommand`, `RunStatus`, `trait Runner`. In core so interpreters need not
    depend on egui.
- `crates/block-parse-gui` — egui component `BlockEditor`. Feature `app` (off
  by default) adds eframe, clap, rfd, winit (macOS only) and the
  `block-parse-editor` binary (`cargo editor -l <language> [program]`). Its
  File/Edit menus are egui for
  now; `--no-menu-bar` hides them for when native menus arrive. On macOS it
  turns off winit's default menu, whose Quit would skip the save prompt.
- `examples/languages/` — sample language definitions: `tiny.ron` (loose,
  Scratch-style typing) and `strict_tiny.ron` (the same language with exact
  types and explicit conversions).

## Principles

- `BlockId` is the only handle the outside world has on a block: AST nodes,
  problems, breakpoints and pauses all use it.
- The editor runs nothing and stores no host state. Breakpoints, highlights
  (semantic styles the theme colors; pauses are `Active`), annotations and
  muted blocks come from the host each frame as an `Overlay`. Commands and
  events go out either as a polled list in `EditorOutput` or through a
  `Runner`. Breakpoints are requests; the host owns them and their
  persistence.
- Layout is pure given a `Measure`, computed at zoom 1 and scaled when drawn.
  Drawing, hit-testing and snapping all read one `Scene`.
- Opcodes, type names, input and branch names are printable ASCII without
  spaces; names used in specs also exclude `{ } [ ] : =`.
- Nesting is capped at `MAX_DEPTH` (120): deeper blocks load as `TooDeep`
  problems and attaching checks the combined depth. Program loads raise RON's
  recursion limit to `RON_RECURSION_LIMIT` (RON spends 6–7 levels per block);
  only nesting past that is a fatal syntax error.
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

- AST building (`Program::ast`), including `TooDeep` problems on load.
- `RunToolbar`, `Runner` dispatch and `EditorOptions::toolbar`.
- Other ways for the editor binary to choose a language than `--language`.
- Undo and redo (the Edit menu items are there, disabled).
- Native OS menus.
- Runtime-supplied dropdowns (variables, procedures).

## Spelling

American spelling everywhere: code, identifiers, serde field names, comments
and docs (`color`, `center`, `behavior`).

## Comments

Comments are a cost. Keep only the why that the code cannot say: constraints,
discarded alternatives, invariants, units. One line is usually enough. No
restatements, history, banners or commented-out code. Prefer a better name or
type over a comment. The `decimate-comments` skill enforces this.
