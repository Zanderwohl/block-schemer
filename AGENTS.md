# block-parse

A generic front end for block-based (Scratch-like) programming. A consumer
defines its language in a config file, the user builds programs from blocks,
and the consumer asks for an AST to compile or interpret. Started from the
block editor in `~/rust/jellycell/src/coder/`, which is one hard-coded case.

## Crates

- `crates/block-parse` — core. serde + ron only, no geometry, no GUI.
  - `language`: `LanguageConfig` (as written, strings kept raw) compiles into a
    validated `Language`. Types give slot/reporter shape and literal kind;
    blocks are Hat / Statement / Cap / HatCap (a whole script in one block)
    / Reporter(type); a spec string like
    `"repeat {times:number=10} [body]"` gives inputs and C-block branches,
    `{args:datum*}` (or `+`, at least one) a list of inputs, and a word
    wrapped in underscores (`_then_`) a faint label, smaller and dimmer. A
    block's `layout` is `Inline` or `Body(n)`: the first `n` inputs on the
    first row, later ones and later lists' items on indented rows of their own
    (`documentation/03-variadic.md`). A blank slot shows its input's name as a
    faded hint, or the block's `hints` text for it, and no validation error
    until there is text to check; a list's empty slot shows it
    with `…`. A `Slot` addresses an input, or a list item by index; the
    index one past the last item appends.
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
  - `edit`: tree operations by id (`detach`, `run_at`, `can_attach`,
    `can_move`, `attach`). All connection rules live here so GUI and
    headless tools agree.
  - `ast` (built by `Program::ast`): always a whole tree. Faults become
    `Problem` nodes in place (in `Stmt` or `Expr`), keeping what could be
    parsed in `recovered`. Only unreadable RON is fatal. A stack of one
    reporter is a loose expression, not a problem; warnings (unknown inputs
    and branches) leave `is_clean` true. `Program::script_at` builds the same
    for what running one block covers.
  - `history`: `History`, undo and redo as a line of program states (stacks
    only, so ids are never reissued) with a cursor; recording while undone
    drops the future, recording no change is a no-op, and unrecorded changes
    are recorded before undoing. The host owns it beside the program and
    records when `EditorOutput::settled`, which a literal being typed holds
    back until its field lets go (`documentation/04-history.md`).
  - `host`: `Overlay` (breakpoints, highlights, annotations, muted blocks,
    switch states, speech bubbles, the side panel's `Tab`s: each `Text` or a
    `Console`, closable or not), `RunCommand`, `RunStatus`, `trait Runner`. In core so interpreters need not
    depend on egui. `Runner` needs only `overlay` and `run_block`; the
    debugger methods and `console_input` default to doing nothing, and
    `inspect` (a block's script as the back end's text, shown in a side
    panel tab; it gets the program, as `run_block` does) to `None`. `start`
    and `run_block` also get the program's path, `None` until saved. A
    runner may offer on/off `Toggle`s (`toggles`, `set_toggle`), its own
    settings, which the host draws. A runner that works off the UI thread
    answers later through `poll`, which the host calls every frame.
- `crates/block-parse-gui` — egui component `BlockEditor`: the palette, the
  canvas and, right of it, the side panel, which starts collapsed. Dragging a
  panel's edge sets `EditorOptions::palette_width` or `side_width`, and a
  button half its width inside the canvas collapses or restores it at that
  width (`palette_collapsed`, `side_collapsed`). A run dropped over the side
  panel goes back where it came from. The side panel shows the host's tabs in
  the editor's order (`tab_order`, `active_tab`): new tabs open after the
  active one, dragging a tab reorders it, and a closed active tab hands over
  to its right-hand neighbor. Closing is a request (`EditorEvent::CloseTab`);
  a `Console` tab is monospace output over a line whose Enter sends
  `EditorEvent::ConsoleInput`, never a shell. Feature `app`
  (off by default) adds eframe, rfd, and on macOS winit and muda, the window
  as a library (`app::run` with an `AppConfig`: name, language, program path,
  `Menus`, an optional `Runner` and an optional window icon). With a runner,
  an actions bar under the menus holds a `RunToolbar`: Play (⏵) and Stop
  (⏹), drawn disabled where the runner does not `support` them; ▶ is not
  in egui's default fonts. The buttons sit at the bar's right and the
  runner's toggles at its left, as checkboxes that are not saved; flipping
  one regenerates the open inspections. Feature `cli` adds clap and the `block-parse-editor` binary (`cargo editor -l <language>
  [program]`). Its File/Edit menus (Undo Cmd/Ctrl+Z; Redo Cmd+Shift+Z on
  macOS, Ctrl+Y or Ctrl+Shift+Z elsewhere) are one `Command` list drawn as
  `Menus::Native` (the macOS menu bar through muda, with app and Window menus;
  egui elsewhere), `Egui` or `Hidden` (`--menus`); shortcuts work in all
  three. Native menus take their own shortcuts, so egui only swallows the ones
  a disabled item lets through. The app menu's Quit is a custom item and
  winit's default menu is off, since `terminate:` would skip the save prompt.
  Feature `snapshot` (off by default) renders programs to images offscreen
  through egui_kittest's wgpu renderer; with it, `--command snapshot` (`cargo
  snapshot -l <language> <out.png>`) writes `Layout::grid`, every block in a
  column per category, instead of opening the window.
- `crates/block-schemer` — Block Schemer, a consumer: R7RS's standard
  procedures (short of files, process, eval and mutating pairs and strings)
  and a few of its forms, whose blocks run in Steel when double-clicked;
  tagged `basics` for the most used and with their R7RS library. Play, or double-clicking the one
  `program` block, runs the canvas as one file in a fresh session: every
  `define` stack in reading order, then the program, into a Console tab
  after `> block-schemer <file>` (`untitled.scmb` until saved), the command
  that will one day do the same; loose expressions are scratch and left out.
  Other double-clicks answer in a bubble and echo `> <expression>` and what
  it said into the console. Code shown in the console or Inspect is as
  entered, unless the "Schemer Harness" toggle (off by default) shows the
  `__out` port that carries `display` to the console (`cargo schemer
  [program.scmb]`,
  `documentation/05-block-schemer.md`). Its language is embedded; its
  `codegen` turns its blocks into Scheme, refusing a script with problems,
  and escapes strings before Scheme sees them, and writes a procedure
  block with nothing filled in by name in a square `procedure` slot
  (`call`'s operator, `fold`, `map`, `apply`); Inspect shows the same code
  pretty-printed, with `<name>` for each missing or faulty input. Steel sits
  behind the `Scheme` trait so a WASM Scheme can replace it; `prelude.scm`
  evens out where Steel differs from R7RS, and every port without one
  given is the console's. Every run goes
  through a `dispatch::Dispatch`, which spawns, tracks and kills the workers
  that run Scheme off the UI thread: `dispatch::native` is one thread owning
  the session, replaced if it dies; Stop interrupts the run, drops the queue
  and loses the session, as terminating a Web Worker would.
- `documentation/` — design notes, numbered.
- `examples/languages/` — sample language definitions: `tiny.ron` (loose,
  Scratch-style typing), `strict_tiny.ron` (the same language with exact
  types and explicit conversions) and `scheme.ron` (an R7RS subset built on
  lists and `Body` layout).

## Principles

- `BlockId` is the only handle the outside world has on a block: AST nodes,
  problems, breakpoints and pauses all use it.
- The editor runs nothing and stores no host state. Breakpoints, highlights
  (semantic styles the theme colors; pauses are `Active`, runs awaiting
  or showing an answer `Dispatched`), annotations and
  muted blocks and switch states come from the host each frame as an
  `Overlay`. A block with `switch: true` shows a checkbox whose state is the
  host's, never saved; clicking requests `EditorEvent::Switched`, even in
  read-only mode, and a switch with no state is drawn disabled; pressing
  one does nothing rather than grabbing its block, and hovering one shows
  the host's `switch_hint`, if it gives one. A second click on the same
  block within the double-click delay requests `EditorEvent::Run` with
  `script_at` that block, even in read-only mode; the host answers in the
  overlay's `bubbles`, which the editor places beside the block where they
  cover least (`documentation/02-bubbles.md`). Inspect in a block's context
  menu requests `EditorEvent::Inspect` with the same script, also in
  read-only mode; the host answers in the overlay's `tabs` and sets
  `active_tab`. `app::run` asks the runner's `inspect`, else shows the AST,
  in one closable tab per block, refreshed by inspecting it again, beside
  the runner's own tabs, which it asks for after each call to the runner and
  each `poll` that took in answers, repainting while the runner is not idle.
  A console a run or Play writes to as it is sent comes to the front,
  unless that run answered in a bubble; later answers never bring it
  forward. Commands and events go out either as a polled list in `EditorOutput` or through a
  `Runner`. Breakpoints are requests; the host owns them and their
  persistence.
- A run in hand stays in the program until dropped, so the program is always
  whole and saving mid-drag is safe. Layout (`Layout::lifted`) and snapping
  (`can_move`) skip it rather than copy the program. A drop lands in
  whichever program is shown on release if it still holds the run unchanged;
  a host switching programs mid-drag calls `cancel_drag`. Going read-only
  mid-drag lets the run go without dropping it.
- Layout is pure given a `Measure`, computed at zoom 1 and scaled when drawn.
  Drawing, hit-testing and snapping all read one `Scene`. Fields and
  switches share the canvas's layer and draw right after their block, and
  error tags and the host's markers after their stack, so a later stack
  covers them all; a field or switch it overlaps is `covered`, drawn static
  and not live, so presses there go to the stack on top.
- Opcodes, type names, input and branch names are printable ASCII without
  spaces; names used in specs also exclude `{ } [ ] : = * +`.
- Nesting is capped at `MAX_DEPTH` (120): deeper blocks load as `TooDeep`
  problems and attaching checks the combined depth. Program loads raise RON's
  recursion limit to `RON_RECURSION_LIMIT` (RON spends 6–9 levels per block);
  only nesting past that is a fatal syntax error.
- The editor never opens documentation links; it emits
  `EditorEvent::OpenDocumentation`. Untrusted language files are the user's
  risk, and escaping text for code generation is the back end's job.
- Fills are unions of convex pieces (epaint fans closed paths).
- Colors: a category gives an OKLCH hue, optionally chroma and lightness. A
  block may give its own `color`, which replaces its category's for that block
  alone; it stays under its category in the palette.
  Core only carries that. The GUI resolves it with `palette` into a `Swatch`
  (fill, edge, shadow, highlight, muted and muted versions of the steps, ink)
  by stepping lightness and chroma, so every category sits at the same
  perceived lightness; displayed as sRGB. Highlight and shadow shade a thin
  chamfer inside each block's outline, lit from the top left.
  Other consumers choose their own scheme.

## Seeing the blocks

To check a drawing change without opening a window, render every block of a
language and look at the image:
`cargo snapshot -l examples/languages/tiny.ron <scratch>/tiny.png`
(`--scale` sets pixels per canvas unit, default 2). For a particular program,
call `block_parse_gui::snapshot::program`.

## Deferred

- `EditorOptions::toolbar`, and `Runner` dispatch beyond `run_block`,
  `start` and `stop`: `app::run` shows only a runner's bubbles and tabs, not
  its highlights, breakpoints or annotations.
- Other ways for the editor binary to choose a language than `--language`.
- Native menus off macOS (muda can attach to a Windows window; on Linux
  it needs GTK, which winit does not use), and Cut/Copy/Paste items, whose
  predefined selectors winit's view does not answer.
- A cap on undo history; each step holds a whole copy of the stacks.
- A cap on a console's output, which grows for the session and is cloned
  on each `Runner::overlay` call and laid out every frame.
- Tabs past the strip's width: they shrink to `MIN_TAB_WIDTH`, then are
  clipped and cannot be reached until others close. A scrolling strip or a
  menu of hidden tabs would fix it.
- Runtime-supplied dropdowns (variables, procedures).
- Keyboard navigation (arrows, Enter) and accessibility roles for choice
  menus; Escape closes one.
- Ticking or choosing into a `Bool` or `Choice` list's empty slot to append;
  it takes drops only.
- Inserting between list items, and closing holes
  (`documentation/03-variadic.md`).
- Refreshing an inspection when its blocks change; it keeps what Inspect
  last gave back until the next Inspect of that block, New or Open.
- Block Schemer's console: `read-line` fed from `ConsoleInput` to a worker
  waiting on it; what Play does with several `program` blocks; a tab per
  run, if runs become concurrent.
- A test that the context menu's Inspect item sends `EditorEvent::Inspect`;
  driving an egui context menu headless needs the button's position.
- Tests of `app::run`'s tab and run bookkeeping (which console was written
  to, bringing it forward only without a bubble, `refresh_inspections`,
  `CloseTab` removing only inspections; an outline kept while a run is
  pending, a late bubble only for a block still outlined, an outline with
  no bubble dropped once the runner is idle); it lives in `App`, which
  needs a window. Moving it out, or a stub `Runner`, would make it
  testable.
- `dispatch::native` replaces a dead worker inside `poll`, on the UI
  thread: building Steel's prelude blocks a frame, and a `make` that panics
  takes the UI down with it.
- Block Schemer: the rest of R7RS's syntax (`cond`, `and`, `let*`, `do`,
  `guard`, …), and a maximum for a list of optional arguments.
- Block Schemer: opening `.scm` files as blocks, auto-formatted; a web build
  once a WASM Scheme replaces Steel, with a Web Worker `Dispatch`. A native
  run stuck outside Steel's safepoints cannot be interrupted; its worker
  would have to be abandoned.

## Spelling

American spelling everywhere: code, identifiers, serde field names, comments
and docs (`color`, `center`, `behavior`).

## Comments

Comments are a cost. Keep only the why that the code cannot say: constraints,
discarded alternatives, invariants, units. One line is usually enough. No
restatements, history, banners or commented-out code. Prefer a better name or
type over a comment. The `decimate-comments` skill enforces this.
