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
    whatever the extension) so consumers can bind file types.
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

## Principles

- `BlockId` is the only handle the outside world has on a block: AST nodes,
  problems, breakpoints and pauses all use it.
- The editor runs nothing and stores no run state. Status, breakpoints, pauses
  and annotations come from the consumer each frame. Commands go out either as
  a polled list in `EditorOutput` or through a `Runner`. Breakpoints are
  requests; the consumer owns them and their persistence.
- Layout is pure given a `Measure`, computed at zoom 1 and scaled when drawn.
  Drawing, hit-testing and snapping all read one `Scene`.
- Fills are unions of convex pieces (epaint fans closed paths).

## Deferred

Runtime-supplied dropdowns (variables, procedures).

## Comments

Comments are a cost. Keep only the why that the code cannot say: constraints,
discarded alternatives, invariants, units. One line is usually enough. No
restatements, history, banners or commented-out code. Prefer a better name or
type over a comment. The `decimate-comments` skill enforces this.
