# Calls

A callable block is a procedure the user can call with all of its
parameters, some of them, or none, or leave uncalled as its name. A marker
at the end of its first row says which ways it can go: ⏴ where it can show
fewer parameters, ⏵ where more, as `⏴|⏵`, `⏴|` or `|⏵`. A block with one
stop has none. Dragging the marker or the block's right end (the cursor
turns ↔) moves it through those stops, and the block's left edge stays
put. The edge is taken once the pointer moves; a click there is a click on
the block, and a second one runs it:

- **Name**: square, just the labels that name it. The procedure itself, not
  called: `+`.
- **Call(0)**: round, the same labels: `(newline)`.
- **Call(n)**: round, the first `n` parameters and the labels before each.
- **Every parameter**, stored as no reach at all, so a reference keeps up
  with its procedure as parameters are added.

The edge never hides a parameter that holds something, so nothing is ever
out of sight; a block with a filled parameter cannot be named. An input
holding its spec's default counts as empty.

```ron
callable: true,   // every reporter whose spec starts with a label
curried: false,   // a call short of its parameters is a problem
types: {
    "procedure": (shape: Square, literal: Custom("symbol"), accepts: Types(["datum"]), by_name: true),
},
blocks: [
    (id: "if", …, callable: false),
    (id: "display", spec: "display {obj:datum} {port:datum*}", shows: 1),
],
```

- `callable` on the language makes every reporter whose spec starts with a
  label callable; a block's own `callable` overrides it. Syntax that is no
  procedure (`if`, `lambda`) says `false`. One saying `true` must be a
  reporter whose spec starts with a label.
- `curried`: a call may show fewer parameters than its procedure has, or
  more, the rest going to the procedure it returns. Without it, either is
  an `Arity` problem on the block, so Block Schemer refuses `(fold +)` with
  a message naming the missing parameter rather than leaving it to Scheme.
  A hidden list that may be empty (`*`) is no fault; that is how optional
  arguments are left off.
- `shows` on a block: how many parameters a fresh one shows, from the
  palette or anywhere else it is made; all of them otherwise. Only a
  callable block takes it. Block Schemer hides the optional `port` of
  every input and output procedure this way.
- `by_name` on a type: a callable block dropped into its slots with nothing
  filled in becomes its name. One with something filled in stays a call,
  whose result is the procedure, so `((make-adder 1) 2)` is a round
  `make-adder 1` in `call`'s operator. The shape tells which: square is
  passed by name, round is computed. Widening a name in such a slot makes it
  a call again; nothing reshapes a block behind the user's back.

## Procedure references

A scope can say that one of its names is a procedure's and which list holds
its parameters:

```ron
(
    id: "define_procedure",
    spec: "define {variable:symbol} _taking_ {formals:symbol*} {body:datum+}",
    scope: (
        declares: ["variable", "formals"], over: ["body"], global: ["variable"], reference: "variable",
        signature: (name: "variable", parameters: "formals", reference: "procedure_call"),
    ),
),
(id: "procedure_call", kind: Reporter("datum"), spec: "{variable:symbol} {arguments:datum*}"),
```

The name's grip drags out the signature's `reference` instead of the
scope's: a reporter with one input, for the name, and one list, for the
arguments. It is callable whatever `callable` says. It shows an argument
slot per parameter, each hinted with the parameter's name ("Unnamed
parameter 2" while that one is blank), and no empty slot to grow it, since
its length is the procedure's. Each takes text or a drop in any order, the
items before it kept as holes; its name is the declaration's and takes
neither. The same block taken from the palette names
no procedure, so its list grows as any other.

`Program::parameters` gives a declaration's parameter names and
`Program::arity` a reference's count. A reference given more arguments than
there are parameters, as when one is removed, still shows them all, and is
an `Arity` problem unless `curried`.

## In the program and the AST

`Block::reach` is `None` (every parameter), `Some(Name)` or
`Some(Call(n))`. `Program::set_reach` refuses a reach that would hide a
filled parameter, and stores `Call(n)` for all `n` parameters as `None`.
`Program::attach` names a block dropped empty into a `by_name` slot.
`BlockDef::extent` says how a block shows now, and `Extent::stops` where its
edge can go.

`reach` came in with format version 4. Before it, an empty procedure block
in a `by_name` slot was passed by name without saying so; `Program::load`
names every such block in an older file and marks it version 4, so a call
widened later is not named again. `Program::from_ron` knows no language and
does not.

A `Node` built from a named block has `named` set and no args or lists, but
a procedure reference keeps its name input. A call leaves out the
parameters it hides, so a consumer emits what is there: Block Schemer writes
`+` for a named Add and `(fold +)` for a fold showing one parameter, though
that one is refused before it runs.

## Not yet

- A `define` whose expression is a `lambda` has no signature, so its
  references are plain variables.
- Rest parameters (`(define (f a . rest) …)`), which would let a reference
  grow past its procedure's parameters.
- Inspect writes a missing argument as its list's name, `(square
  <arguments>)`, not the parameter's that the editor hints, `<x>`: the AST
  does not carry parameter names.
