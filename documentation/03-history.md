# Undo and redo

`block_parse::History` is a line of program states and a cursor. Step `k`
turns state `k` into state `k + 1`, so every state is the after of one step and
the before of the next: undo moves the cursor back and restores the state
there, redo moves it forward. Nothing is popped until a new step is recorded
while undone, which drops every state after the cursor.

The line starts at the program as opened and lives only in memory.

## Who owns it

The host, beside the program, for the same reason the program is the host's:
the editor holds only view and interaction state. A headless tool can keep one
too.

```rust
let output = editor.show_with(ui, &language, &mut program, &overlay);
if output.settled {
    history.record(&program);
}
```

## What makes a step

Every edit the editor makes ends a step: a drop (move, snap, unsnap, a new
block from the palette, a delete onto the palette), Duplicate and Delete in the
context menu, a checkbox, a choice. Typing is the exception: each keystroke
writes into the program, but `settled` stays false until the field loses focus,
so the whole entry, normalizing included, is one step.

`record` does nothing when the program matches the current state, so a field
clicked and left without typing keeps the redo line.

States hold only stacks. Restoring one leaves the program's id counter where
it is, so a block that was undone away never has its id reissued to a new one,
and host state keyed by `BlockId` (breakpoints, bubbles) never lands on the
wrong block.

## Undoing mid-edit

Changes not yet recorded, such as text still being typed, count as a step:
`can_undo` is true for them, `undo` records them before stepping back, and
`can_redo` is false, since they will end the line once recorded. The standalone
editor calls `BlockEditor::commit_edit` before undoing or redoing, so the field
lets go and a later focus loss does not record over the redo line. Its
shortcuts are taken before the editor draws, so a focused field's own text undo
never sees them.

## Saved state

`mark_saved` notes the current state as written to disk and `is_saved` says
whether the program is back at it, so undoing to what was saved clears the
unsaved-changes marker. A new step recorded while undone past it forgets it.
