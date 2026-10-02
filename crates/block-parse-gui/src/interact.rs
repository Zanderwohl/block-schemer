//! Pointer and keyboard state between frames.

use block_parse::Language;
use block_parse::edit::{Fragment, Target};
use block_parse::language::{Fit, Shape};
use block_parse::program::{BlockId, Declaration, Program, Reach, Slot};
use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::layout::{Layout, Run, SNAP_RADIUS, Scene};
use crate::paint::Transform;

/// Screen pixels the pointer must travel before a press becomes a drag, so a
/// click never takes a block out of its stack.
pub const DRAG_THRESHOLD: f32 = 4.0;

#[derive(Debug, Clone, Default)]
pub enum Gesture {
    #[default]
    Idle,
    /// Down on something draggable, not yet moved past `DRAG_THRESHOLD`.
    Pressed(Press),
    Dragging(Drag),
    Panning,
    /// Moving a panel's edge; `grab` is the pointer's x minus the edge's at
    /// the press, so the edge does not jump.
    Resizing { edge: Edge, grab: f32 },
    /// Moving a callable block's right end.
    Reaching(Reaching),
}

/// Canvas units at a callable reporter's right end that grab it.
pub const REACH_GRIP: f32 = 6.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Reaching {
    pub block: BlockId,
    /// The block's left edge, canvas units; it stays put.
    pub left: f32,
    /// Narrowest first.
    pub stops: Vec<(Option<Reach>, f32)>,
    /// The pointer's x minus the right end's at the press.
    pub grab: f32,
    /// Changed the program, so the gesture settles as one step.
    pub moved: bool,
}

impl Reaching {
    /// True if the program changed.
    pub fn follow(&mut self, language: &Language, program: &mut Program, x: f32) -> bool {
        let distance = |width: f32| (self.left + width - (x - self.grab)).abs();
        let nearest = self.stops.iter().min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)));
        let changed = nearest.is_some_and(|&(reach, _)| {
            program.find(self.block).is_some_and(|block| block.reach != reach)
                && program.set_reach(language, self.block, reach)
        });
        self.moved |= changed;
        changed
    }
}

/// The right end or ⏴|⏵ of a callable block at `point`, canvas units, if
/// it has more than one stop.
pub fn reach_at(layout: &Layout, scene: &Scene, program: &Program, point: Pos2) -> Option<Reaching> {
    let on_marker = |rect: Rect| rect.expand(REACH_GRIP / 2.0).contains(point);
    let hit = scene
        .hit(point)
        .filter(|hit| point.x >= hit.rect.max.x - REACH_GRIP || hit.reach.is_some_and(on_marker))?;
    let stops = layout.stops(program.find(hit.id)?);
    (stops.len() > 1).then_some(Reaching {
        block: hit.id,
        left: hit.rect.min.x,
        stops,
        grab: point.x - hit.rect.max.x,
        moved: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    Palette,
    Side,
}

/// Screen pixels either side of a panel's edge that grab it.
pub const DIVIDER_GRIP: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Press {
    /// Screen position.
    pub at: Pos2,
    pub on: Pressed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pressed {
    /// `top_left` in screen coordinates; the palette does not zoom.
    Palette { opcode: String, top_left: Pos2 },
    /// `top_left` in canvas units.
    Block { id: BlockId, top_left: Pos2 },
    /// A declaring slot's grip. `top_left` in canvas units, where the
    /// reference starts.
    Handle { declaration: Declaration, top_left: Pos2 },
    /// A callable block's right end: moving takes the edge, and not moving
    /// is a click on the block.
    Reach(Reaching),
}

impl Pressed {
    /// The block a release without moving clicks.
    pub fn clicked(&self) -> Option<BlockId> {
        match self {
            Self::Block { id, .. } => Some(*id),
            Self::Reach(reaching) => Some(reaching.block),
            Self::Palette { .. } | Self::Handle { .. } => None,
        }
    }
}

/// A canvas run stays in the program until dropped, so the program is always
/// whole and canceling needs nothing from it.
#[derive(Debug, Clone)]
pub struct Drag {
    /// A copy of the run in hand, for drawing and snapping.
    pub fragment: Fragment,
    pub source: DragSource,
    /// Pointer minus head top-left at the grab, canvas units, so the run does
    /// not jump.
    pub grab_offset: Vec2,
    /// Head top-left, canvas units.
    pub head: Pos2,
    /// Computed once per frame so the highlight and the drop agree.
    pub snap: Option<(Target, SnapMark)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DragSource {
    /// Instantiated afresh on drop, so its ids come from the program it lands in.
    Palette { opcode: String },
    /// Dropping it on the palette deletes it.
    Canvas { head: BlockId },
    /// Made afresh on drop, like a palette block, if the name is still there.
    Reference(Declaration),
}

/// What the snap highlight draws, canvas units.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapMark {
    /// A notched top edge where the run's head would land.
    Seam { at: Pos2, width: f32 },
    Slot { rect: Rect, shape: Shape },
}

/// The literal with keyboard focus. Edits write straight into the program,
/// valid or not. Keyed by id so it survives other blocks moving.
#[derive(Debug, Clone, PartialEq)]
pub struct LiteralEdit {
    pub block: BlockId,
    pub slot: Slot,
}

/// The choice literal whose menu is open. Keyed by id, like [`LiteralEdit`].
#[derive(Debug, Clone, PartialEq)]
pub struct OpenChoice {
    pub block: BlockId,
    pub slot: Slot,
    /// Screen pixels, when the menu is cut short to fit.
    pub scroll: f32,
}

impl OpenChoice {
    pub fn is(&self, block: BlockId, slot: &Slot) -> bool {
        self.block == block && self.slot == *slot
    }
}

pub fn find_snap(
    language: &Language,
    program: &Program,
    scene: &Scene,
    fragment: &Fragment,
    run: &Run,
) -> Option<(Target, SnapMark)> {
    let head = fragment.blocks.first()?;
    let def = language.block(&head.opcode)?;
    let head_rect = run.scene.blocks.first()?.rect;
    let mut best: Option<(f32, Target, SnapMark)> = None;
    let mut consider = |distance: f32, target: Target, mark: SnapMark| {
        if distance <= SNAP_RADIUS
            && best.as_ref().is_none_or(|(nearest, ..)| distance < *nearest)
            // A palette block's fresh id is in no program: `can_attach` for it.
            && program.can_move(language, fragment, &target).is_ok()
        {
            best = Some((distance, target, mark));
        }
    };

    if let Some(output) = def.kind.output() {
        let probe = pos2(head_rect.min.x, head_rect.center().y);
        for slot in scene.slots() {
            if language.fit(output, &slot.ty) == Fit::No {
                continue;
            }
            let distance = probe.distance(pos2(slot.rect.min.x, slot.rect.center().y));
            let target = Target::Input {
                parent: slot.parent,
                slot: slot.slot.clone(),
            };
            let mark = SnapMark::Slot {
                rect: slot.rect,
                shape: slot.shape,
            };
            consider(distance, target, mark);
        }
    } else {
        let probe = head_rect.min;
        for seam in &scene.seams {
            let mark = SnapMark::Seam {
                at: seam.at,
                width: head_rect.width(),
            };
            consider(probe.distance(seam.at), seam.target.clone(), mark);
        }
        for stack in scene.heads.iter().filter(|stack| !stack.is_hat) {
            let at = stack.top_left - vec2(0.0, run.size.y);
            let target = Target::Above {
                head: stack.block,
                pos: [at.x, at.y],
            };
            let mark = SnapMark::Seam {
                at: stack.top_left,
                width: stack.width,
            };
            consider(probe.distance(at), target, mark);
        }
    }
    best.map(|(_, target, mark)| (target, mark))
}

/// Offsets are taken from the press, so the run does not jump by the
/// threshold.
pub fn start_drag(press: Press, language: &Language, program: &mut Program, t: Transform, read_only: bool) -> Gesture {
    match press.on {
        Pressed::Reach(reaching) => Gesture::Reaching(reaching),
        Pressed::Palette { opcode, top_left } => {
            let Some(block) = program.instantiate(language, &opcode) else {
                return Gesture::Idle;
            };
            let grab_offset = (press.at - top_left) / t.zoom;
            Gesture::Dragging(Drag {
                fragment: Fragment { blocks: vec![block] },
                source: DragSource::Palette { opcode },
                grab_offset,
                head: t.canvas(press.at) - grab_offset,
                snap: None,
            })
        }
        Pressed::Handle { declaration, top_left } => match program.reference(language, &declaration) {
            Some(block) => Gesture::Dragging(Drag {
                fragment: Fragment { blocks: vec![block] },
                source: DragSource::Reference(declaration),
                grab_offset: t.canvas(press.at) - top_left,
                head: top_left,
                snap: None,
            }),
            None => Gesture::Idle,
        },
        // Read-only blocks cannot move, so dragging one pans instead.
        Pressed::Block { .. } if read_only => Gesture::Panning,
        Pressed::Block { id, top_left } => match program.run_at(id) {
            Some(fragment) => Gesture::Dragging(Drag {
                fragment,
                source: DragSource::Canvas { head: id },
                grab_offset: t.canvas(press.at) - top_left,
                head: top_left,
                snap: None,
            }),
            None => Gesture::Idle,
        },
    }
}

/// A reporter pushed out of a slot lands just below it.
/// Delete drops the run instead of placing it. A canvas run that no longer
/// matches the program, as when the host switched programs mid-drag, is left
/// alone. True if the program changed.
pub fn drop_run(language: &Language, program: &mut Program, drag: Drag, delete: bool) -> bool {
    let mut next = program.clone();
    let fragment = match &drag.source {
        DragSource::Canvas { head } => match next.detach(*head) {
            Some(fragment) if fragment == drag.fragment => fragment,
            _ => return false,
        },
        _ if delete => return false,
        DragSource::Palette { opcode } => match next.instantiate(language, opcode) {
            Some(block) => Fragment { blocks: vec![block] },
            None => return false,
        },
        DragSource::Reference(declaration) => match next.reference(language, declaration) {
            Some(block) => Fragment { blocks: vec![block] },
            None => return false,
        },
    };
    if !delete {
        let head = [drag.head.x, drag.head.y];
        let (target, eject) = match drag.snap {
            Some((target, SnapMark::Slot { rect, .. })) => (target, rect.min + vec2(16.0, 40.0)),
            Some((target, SnapMark::Seam { .. })) => (target, drag.head),
            None => (Target::Free { pos: head }, drag.head),
        };
        match next.attach(language, fragment, target) {
            Ok(Some(ejected)) => {
                let _ = next.attach(language, ejected, Target::Free { pos: [eject.x, eject.y] });
            }
            Ok(None) => {}
            Err((_, fragment)) => {
                let _ = next.attach(language, fragment, Target::Free { pos: head });
            }
        }
    }
    *program = next;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_parse::program::Stack;
    use block_parse::Validators;

    #[test]
    fn a_reference_whose_name_was_blanked_mid_drag_drops_nothing() {
        let language = Language::from_ron(
            r#"Language(
                name: "scoped",
                file: (extension: "s"),
                types: { "name": (literal: Text), "value": (literal: Text) },
                blocks: [
                    (id: "get", name: "Get", kind: Reporter("value"), spec: "{name:name}"),
                    (
                        id: "fn", name: "Fn", kind: Reporter("value"), spec: "fn {param:name} {body:value}",
                        scope: (declares: ["param"], over: ["body"], reference: "get"),
                    ),
                ],
            )"#,
            &Validators::new(),
        )
        .unwrap();
        let mut program = Program::new(&language);
        let function = program.instantiate(&language, "fn").unwrap();
        let id = function.id;
        program.stacks.push(Stack {
            pos: [0.0, 0.0],
            blocks: vec![function],
        });
        program.set_literal(id, &Slot::input("param"), "x".into());
        let declaration = Declaration {
            block: id,
            slot: Slot::input("param"),
        };
        let held = Drag {
            fragment: Fragment {
                blocks: vec![program.clone().reference(&language, &declaration).unwrap()],
            },
            source: DragSource::Reference(declaration),
            grab_offset: Vec2::ZERO,
            head: pos2(0.0, 200.0),
            snap: None,
        };

        let mut named = program.clone();
        assert!(drop_run(&language, &mut named, held.clone(), false));
        assert_eq!(named.stacks.len(), 2, "dropped free");

        program.set_literal(id, &Slot::input("param"), String::new());
        let before = program.to_ron();
        assert!(!drop_run(&language, &mut program, held, false));
        assert_eq!(program.to_ron(), before);
    }
}
