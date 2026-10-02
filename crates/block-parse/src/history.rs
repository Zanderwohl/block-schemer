//! Undo and redo as a line of program states with a cursor. Step `k` turns
//! state `k` into state `k + 1`, so each state is both the after of one step
//! and the before of the next.

use crate::program::{Program, Stack};

#[derive(Debug, Clone)]
pub struct History {
    /// Only stacks: restoring leaves the program's id counter alone, so an
    /// undone block's id is never reissued to a new one.
    states: Vec<Vec<Stack>>,
    cursor: usize,
    /// The state last written to disk, if still in the line.
    saved: Option<usize>,
}

impl History {
    /// `program` as just opened, which counts as saved.
    pub fn new(program: &Program) -> Self {
        Self {
            states: vec![program.stacks.clone()],
            cursor: 0,
            saved: Some(0),
        }
    }

    /// A `program` matching the current state is not a step, so the redo line
    /// survives it. True if a step was added.
    pub fn record(&mut self, program: &Program) -> bool {
        if !self.is_pending(program) {
            return false;
        }
        self.states.truncate(self.cursor + 1);
        if self.saved.is_some_and(|saved| saved > self.cursor) {
            self.saved = None;
        }
        self.states.push(program.stacks.clone());
        self.cursor += 1;
        true
    }

    /// Changes in `program` not yet recorded count as a step to undo.
    pub fn can_undo(&self, program: &Program) -> bool {
        self.cursor > 0 || self.is_pending(program)
    }

    /// False while `program` has unrecorded changes: they would end the line.
    pub fn can_redo(&self, program: &Program) -> bool {
        self.cursor + 1 < self.states.len() && !self.is_pending(program)
    }

    /// Records any pending change first, so it is what gets undone. True if
    /// `program` changed.
    pub fn undo(&mut self, program: &mut Program) -> bool {
        self.record(program);
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        program.stacks = self.states[self.cursor].clone();
        true
    }

    /// True if `program` changed.
    pub fn redo(&mut self, program: &mut Program) -> bool {
        if !self.can_redo(program) {
            return false;
        }
        self.cursor += 1;
        program.stacks = self.states[self.cursor].clone();
        true
    }

    pub fn mark_saved(&mut self, program: &Program) {
        self.record(program);
        self.saved = Some(self.cursor);
    }

    /// True if `program` matches what was last written.
    pub fn is_saved(&self, program: &Program) -> bool {
        self.saved == Some(self.cursor) && !self.is_pending(program)
    }

    fn is_pending(&self, program: &Program) -> bool {
        program.stacks != self.states[self.cursor]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::{Block, BlockId};
    use std::collections::BTreeMap;

    fn moved(program: &mut Program, x: f32) {
        program.stacks[0].pos = [x, 0.0];
    }

    fn start() -> Program {
        let mut program = Program::default();
        program.stacks.push(Stack {
            pos: [0.0, 0.0],
            blocks: vec![Block {
                id: BlockId(1),
                opcode: "say".into(),
                inputs: BTreeMap::new(),
                lists: BTreeMap::new(),
                branches: BTreeMap::new(),
                refers: None,
            }],
        });
        program
    }

    fn x(program: &Program) -> f32 {
        program.stacks[0].pos[0]
    }

    #[test]
    fn nothing_to_undo_or_redo_when_just_opened() {
        let mut program = start();
        let mut history = History::new(&program);
        assert!(!history.can_undo(&program));
        assert!(!history.can_redo(&program));
        assert!(!history.undo(&mut program));
        assert!(!history.redo(&mut program));
    }

    #[test]
    fn undo_and_redo_walk_the_line_without_losing_it() {
        let mut program = start();
        let mut history = History::new(&program);
        for step in 1..=3 {
            moved(&mut program, step as f32);
            assert!(history.record(&program));
        }
        assert!(history.undo(&mut program));
        assert!(history.undo(&mut program));
        assert_eq!(x(&program), 1.0);
        assert!(history.redo(&mut program));
        assert!(history.redo(&mut program));
        assert_eq!(x(&program), 3.0);
        assert!(!history.can_redo(&program));
        while history.undo(&mut program) {}
        assert_eq!(x(&program), 0.0);
    }

    #[test]
    fn a_new_step_after_undoing_drops_the_future() {
        let mut program = start();
        let mut history = History::new(&program);
        moved(&mut program, 1.0);
        history.record(&program);
        moved(&mut program, 2.0);
        history.record(&program);
        history.undo(&mut program);
        moved(&mut program, 5.0);
        history.record(&program);
        assert!(!history.can_redo(&program));
        history.undo(&mut program);
        assert_eq!(x(&program), 1.0);
    }

    #[test]
    fn recording_no_change_keeps_the_future() {
        let mut program = start();
        let mut history = History::new(&program);
        moved(&mut program, 1.0);
        history.record(&program);
        history.undo(&mut program);
        assert!(!history.record(&program));
        assert!(history.can_redo(&program));
    }

    #[test]
    fn an_unrecorded_change_is_undone_first_and_blocks_redo() {
        let mut program = start();
        let mut history = History::new(&program);
        moved(&mut program, 1.0);
        history.record(&program);
        history.undo(&mut program);
        moved(&mut program, 7.0);
        assert!(!history.can_redo(&program));
        assert!(history.can_undo(&program));
        history.undo(&mut program);
        assert_eq!(x(&program), 0.0);
        history.redo(&mut program);
        assert_eq!(x(&program), 7.0);
        assert!(!history.can_redo(&program));
    }

    #[test]
    fn undoing_keeps_ids_of_undone_blocks_from_being_reissued() {
        let mut program = start();
        let mut history = History::new(&program);
        let id = program.fresh_id();
        program.stacks[0].blocks[0].id = id;
        history.record(&program);
        history.undo(&mut program);
        assert_ne!(program.fresh_id(), id);
    }

    #[test]
    fn undoing_back_to_the_saved_state_is_saved() {
        let mut program = start();
        let mut history = History::new(&program);
        moved(&mut program, 1.0);
        history.record(&program);
        assert!(!history.is_saved(&program));
        history.mark_saved(&program);
        history.undo(&mut program);
        assert!(!history.is_saved(&program));
        history.redo(&mut program);
        assert!(history.is_saved(&program));
        history.undo(&mut program);
        moved(&mut program, 4.0);
        history.record(&program);
        history.undo(&mut program);
        history.redo(&mut program);
        assert!(!history.is_saved(&program));
    }
}
