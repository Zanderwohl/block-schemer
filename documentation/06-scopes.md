# Scopes

A block with a `scope` declares names and says where they may be used. The
user types a name into a declaring slot, then drags a reference to it out of
the chip around it: the field and a grip (≡), outlined and beveled like a
block in the reference's shape. Pressing anywhere on the chip but the field
drags out a new reference; the slot keeps its name. The chip shows while
the name is blank too, palette included, to mark the slot as declaring, and
so the field does not shift as its first letter is typed; it drags nothing
until there is a name. A reference is a block
like any other, except that its name is not editable and it takes the
declaring block's color: renaming the declaration renames every reference to
it.

```ron
(
    id: "lambda",
    kind: Reporter("datum"),
    spec: "lambda {formals:symbol*} {body:datum+}",
    scope: (declares: ["formals"], over: ["body"], reference: "variable"),
),
(id: "variable", kind: Reporter("datum"), spec: "{variable:symbol}"),
```

- `declares`: inputs and lists whose literals are names. Each literal item of
  a list is a name of its own.
- `over`: inputs, lists and branches where the names may be used, at any
  depth. A declaring slot is never in its own scope.
- `global`: of `declares`, those whose names may be used anywhere in the
  program, as a top-level definition's. Block Schemer's `define` and the
  procedure name of `define … _taking_` are global; the formals are not.
- `reference`: the block a grip drags out. A reporter with one input, of the
  declaring slots' type, and no list. The same block, taken from the palette,
  is an ordinary editable one; Block Schemer's `variable` is both.

A declaring slot may also hold a block. If that block's scope has neither
`over` nor `global`, its names are handed to the block it is plugged into.
That is how a `let` gets names from its `binding`s, and why a binding's
`init` cannot see them:

```ron
(id: "let", spec: "let {bindings:binding*} {body:datum+}",
    scope: (declares: ["bindings"], over: ["body"])),
(id: "binding", kind: Reporter("binding"), spec: "{variable:symbol} _be_ {init:datum}",
    scope: (declares: ["variable"], reference: "variable")),
```

## In the program

A reference is a `Block` with `refers: Some(Declaration { block, slot })`,
the declaring block's id and the slot holding the name (a list item by
index). Its one input holds a copy of the name, so a file stays readable and
a consumer that ignores `refers` still sees a plain block with a literal.
`Program::set_literal` keeps the copies in step, finding them by
declaration, never by name; loading does the same, for files edited by hand.
`set_literal` on a reference's name is refused; a procedure reference's
arguments are typed into as any list's (`documentation/07-calls.md`).

`Program::reference` makes one, out of the program, like `instantiate`, and
only for a declaring slot holding a non-blank literal. `Program::duplicate`
points references inside the copy at the copy's declarations; those to
declarations outside it keep theirs.

## Blank names

Emptying a declaring list item makes it a hole, and one at the end is
trimmed, but references keep its index: naming the slot again reconnects
them. Meanwhile they show "Unnamed {hint} {n}" (the list's hint and the
item's place from 1; just the hint for a single input), so Block Schemer's
formals, hinted "parameter", read "Unnamed parameter 2".

No language takes an empty name, so block-parse refuses one: a declaring slot
whose text is blank, and a reference to a blank or trimmed declaration, are
`Unnamed` problems.

## Out of scope

Nothing stops a reference being dropped anywhere its type fits, or a scope
moving away from its references. The AST builder carries the scopes it is
in as it descends, and a reference to one that is not is an
`OutOfScope` problem in its place, with nothing recovered: to the consumer
it is an empty slot, and the script will not run. The same holds when the
declaring block is gone, with a message that says so.

`script_at` builds from the block run, so running part of a scope (a body
expression with references in it) finds them out of scope too.

A node built from an in-scope reference carries `refers`, so a consumer can
compile by declaration rather than by name.

## Not yet

- An internal definition's reach: a `global` name is global wherever its
  block is, though Scheme scopes a `define` inside a body to that body.
- Showing a reference's scope while dragging it, or refusing snaps outside
  it; today the drop lands and the problem shows.
- A declaration with the same name twice in one scope is not flagged; the
  back end sees what the user typed.
