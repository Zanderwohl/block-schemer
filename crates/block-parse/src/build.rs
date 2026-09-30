//! Program to [`Ast`]. Never fails: each fault becomes a [`Problem`] in the
//! place it occurs, and parsing carries on around it.

use std::collections::HashSet;

use crate::ast::{Arg, Ast, Branch, Expr, Node, Problem, ProblemCode, Script, Severity, Stmt};
use crate::language::{BlockDef, BlockKind, Fit, InputDef, Language, LiteralKind};
use crate::program::{Block, BlockId, MAX_DEPTH, Program};
use crate::value::Value;

impl Program {
    /// One script per stack. A stack that is a single reporter is a loose
    /// expression, not a problem.
    pub fn ast(&self, language: &Language) -> Ast {
        let mut builder = Builder {
            language,
            seen: HashSet::new(),
        };
        Ast {
            scripts: self
                .stacks
                .iter()
                .map(|stack| Script {
                    body: builder.stack(&stack.blocks),
                })
                .collect(),
        }
    }
}

struct Builder<'a> {
    language: &'a Language,
    seen: HashSet<BlockId>,
}

impl Builder<'_> {
    fn stack(&mut self, blocks: &[Block]) -> Vec<Stmt> {
        if let [only] = blocks
            && self.kind(only).is_some_and(|kind| kind.output().is_some())
        {
            return vec![match self.block(only, 1) {
                Ok(node) => Stmt::Node(node),
                Err(problem) => Stmt::Problem(problem),
            }];
        }
        self.sequence(blocks, 1, true)
    }

    fn sequence(&mut self, blocks: &[Block], depth: usize, top_of_stack: bool) -> Vec<Stmt> {
        let mut after_cap = false;
        let mut body = Vec::with_capacity(blocks.len());
        for (index, block) in blocks.iter().enumerate() {
            let kind = self.kind(block).cloned();
            let node = match self.block(block, depth) {
                Ok(node) => node,
                Err(problem) => {
                    body.push(Stmt::Problem(problem));
                    continue;
                }
            };
            let name = self.name(block);
            let fault = if after_cap {
                // Only the first: the rest are unreachable for the same reason.
                after_cap = false;
                Some((ProblemCode::AfterCap, format!("nothing can follow a cap, but `{name}` does")))
            } else {
                match &kind {
                    Some(BlockKind::Reporter(_)) => Some((
                        ProblemCode::ReporterAsStatement,
                        format!("`{name}` reports a value and cannot stand in a stack"),
                    )),
                    Some(BlockKind::Hat) if !(top_of_stack && index == 0) => Some((
                        ProblemCode::HatNotAtTop,
                        format!("`{name}` can only start a stack"),
                    )),
                    _ => None,
                }
            };
            if kind == Some(BlockKind::Cap) {
                after_cap = true;
            }
            body.push(match fault {
                Some((code, message)) => Stmt::Problem(Problem {
                    block: Some(block.id),
                    slot: None,
                    code,
                    severity: Severity::Error,
                    message,
                    recovered: Some(Box::new(node)),
                }),
                None => Stmt::Node(node),
            });
        }
        body
    }

    /// Checks shared by every position: duplicate ids, depth, opcode.
    fn block(&mut self, block: &Block, depth: usize) -> Result<Node, Problem> {
        let problem = |code, message: String, recovered: Option<Node>| Problem {
            block: Some(block.id),
            slot: None,
            code,
            severity: Severity::Error,
            message,
            recovered: recovered.map(Box::new),
        };
        if depth > MAX_DEPTH {
            return Err(problem(
                ProblemCode::TooDeep,
                format!("nested deeper than {MAX_DEPTH} levels"),
                None,
            ));
        }
        let duplicate = !self.seen.insert(block.id);
        let node = match self.language.block(&block.opcode) {
            Some(def) => self.node(block, def, depth),
            None => {
                let raw = self.raw(block, depth);
                return Err(problem(
                    ProblemCode::UnknownOpcode,
                    format!("the language has no block `{}`", block.opcode),
                    Some(raw),
                ));
            }
        };
        if duplicate {
            return Err(problem(
                ProblemCode::DuplicateId,
                format!("block id {} is used twice", block.id.0),
                Some(node),
            ));
        }
        Ok(node)
    }

    fn node(&mut self, block: &Block, def: &BlockDef, depth: usize) -> Node {
        let mut args: Vec<Arg> = def
            .inputs()
            .map(|input| Arg {
                name: input.name.clone(),
                value: self.arg(block, def, input, depth),
            })
            .collect();
        for (name, stored) in &block.inputs {
            if def.input(name).is_some() {
                continue;
            }
            let recovered = stored.block.as_deref().and_then(|inner| self.recover(inner, depth + 1));
            args.push(Arg {
                name: name.clone(),
                value: Expr::Problem(Box::new(Problem {
                    block: None,
                    slot: Some((block.id, name.clone())),
                    code: ProblemCode::UnknownInput,
                    severity: Severity::Warning,
                    message: format!("`{}` has no input `{name}`", def.name),
                    recovered: recovered.map(Box::new),
                })),
            });
        }

        let mut branches: Vec<Branch> = def
            .branches()
            .map(|name| Branch {
                name: name.to_owned(),
                body: self.sequence(
                    block.branches.get(name).map_or(&[][..], Vec::as_slice),
                    depth + 1,
                    false,
                ),
            })
            .collect();
        for (name, children) in &block.branches {
            if def.has_branch(name) {
                continue;
            }
            let mut body = vec![Stmt::Problem(Problem {
                block: Some(block.id),
                slot: None,
                code: ProblemCode::UnknownBranch,
                severity: Severity::Warning,
                message: format!("`{}` has no branch `{name}`", def.name),
                recovered: None,
            })];
            body.extend(self.sequence(children, depth + 1, false));
            branches.push(Branch {
                name: name.clone(),
                body,
            });
        }

        Node {
            id: block.id,
            opcode: block.opcode.clone(),
            args,
            branches,
        }
    }

    fn arg(&mut self, block: &Block, def: &BlockDef, input: &InputDef, depth: usize) -> Expr {
        let stored = block.inputs.get(&input.name);
        let slot = Some((block.id, input.name.clone()));
        let problem = |code, message: String, at: Option<BlockId>, recovered: Option<Node>| {
            Expr::Problem(Box::new(Problem {
                block: at,
                slot: slot.clone(),
                code,
                severity: Severity::Error,
                message,
                recovered: recovered.map(Box::new),
            }))
        };

        if let Some(inner) = stored.and_then(|stored| stored.block.as_deref()) {
            let node = match self.block(inner, depth + 1) {
                Ok(node) => node,
                Err(problem) => return Expr::Problem(Box::new(Problem { slot, ..problem })),
            };
            let inner_name = self.name(inner);
            let Some(output) = self.kind(inner).and_then(BlockKind::output).map(str::to_owned) else {
                return problem(
                    ProblemCode::StatementAsInput,
                    format!("`{inner_name}` does not report a value, so it cannot fill `{}`", input.name),
                    Some(inner.id),
                    Some(node),
                );
            };
            return match self.language.fit(&output, &input.ty) {
                Fit::Exact => Expr::Node(Box::new(node)),
                Fit::Convert => Expr::Convert {
                    from: output,
                    to: input.ty.clone(),
                    value: Box::new(Expr::Node(Box::new(node))),
                },
                Fit::No => problem(
                    ProblemCode::TypeMismatch,
                    format!(
                        "`{inner_name}` reports {output}, but `{}` of `{}` takes {}",
                        input.name, def.name, input.ty
                    ),
                    Some(inner.id),
                    Some(node),
                ),
            };
        }

        let takes_literal = self
            .language
            .ty(&input.ty)
            .is_some_and(|ty| ty.literal != LiteralKind::None);
        if !takes_literal {
            return problem(
                ProblemCode::MissingInput,
                format!("`{}` needs a block in `{}`", def.name, input.name),
                None,
                None,
            );
        }
        // An input the file lacks, say one the language added since, reads as
        // its default.
        let text = stored
            .and_then(|stored| stored.literal.as_deref())
            .or(input.default.as_deref())
            .unwrap_or_default();
        match self.language.parse_literal(&input.ty, text) {
            Ok(value) => Expr::Literal(value),
            Err(message) => problem(
                ProblemCode::InvalidLiteral,
                format!("`{}` of `{}`: {message}", input.name, def.name),
                None,
                None,
            ),
        }
    }

    /// A block whose opcode the language lacks, kept as written: literals as
    /// text, plugged blocks and branches parsed.
    fn raw(&mut self, block: &Block, depth: usize) -> Node {
        let args = block
            .inputs
            .iter()
            .map(|(name, stored)| {
                let value = match (&stored.block, &stored.literal) {
                    (Some(inner), _) => match self.block(inner, depth + 1) {
                        Ok(node) => Expr::Node(Box::new(node)),
                        Err(problem) => Expr::Problem(Box::new(problem)),
                    },
                    (None, literal) => Expr::Literal(Value::Text(literal.clone().unwrap_or_default())),
                };
                Arg {
                    name: name.clone(),
                    value,
                }
            })
            .collect();
        let branches = block
            .branches
            .iter()
            .map(|(name, children)| Branch {
                name: name.clone(),
                body: self.sequence(children, depth + 1, false),
            })
            .collect();
        Node {
            id: block.id,
            opcode: block.opcode.clone(),
            args,
            branches,
        }
    }

    /// Whatever can be made of a block, for keeping under a problem.
    fn recover(&mut self, block: &Block, depth: usize) -> Option<Node> {
        match self.block(block, depth) {
            Ok(node) => Some(node),
            Err(problem) => problem.recovered.map(|node| *node),
        }
    }

    fn kind(&self, block: &Block) -> Option<&BlockKind> {
        self.language.block(&block.opcode).map(|def| &def.kind)
    }

    fn name(&self, block: &Block) -> String {
        self.language
            .block(&block.opcode)
            .map_or_else(|| block.opcode.clone(), |def| def.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::literal::Validators;
    use crate::program::{Input, Stack};
    use std::collections::BTreeMap;

    const LANGUAGE: &str = r#"Language(
        name: "t",
        file: (extension: "t"),
        types: {
            "number": (literal: Float),
            "text": (literal: Text, accepts: All),
            "bool": (shape: Hexagon),
        },
        blocks: [
            (id: "go", name: "Go", kind: Hat, spec: "go"),
            (id: "if", name: "If", spec: "if {c:bool} [then]"),
            (id: "say", name: "Say", spec: "say {text:text}"),
            (id: "wait", name: "Wait", spec: "wait {s:number=1}"),
            (id: "end", name: "End", kind: Cap, spec: "end"),
            (id: "add", name: "Add", kind: Reporter("number"), spec: "{a:number=1} + {b:number=2}"),
            (id: "yes", name: "Yes", kind: Reporter("bool"), spec: "yes"),
        ],
    )"#;

    fn language() -> Language {
        Language::from_ron(LANGUAGE, &Validators::new()).unwrap()
    }

    fn block(program: &mut Program, language: &Language, opcode: &str) -> Block {
        program.instantiate(language, opcode).unwrap()
    }

    fn plug(into: &mut Block, input: &str, reporter: Block) {
        into.inputs.entry(input.into()).or_default().block = Some(Box::new(reporter));
    }

    fn codes(ast: &Ast) -> Vec<ProblemCode> {
        ast.problems().iter().map(|problem| problem.code).collect()
    }

    fn one_stack(blocks: Vec<Block>) -> Program {
        let mut program = Program::default();
        program.stacks.push(Stack {
            pos: [0.0, 0.0],
            blocks,
        });
        program
    }

    #[test]
    fn a_sound_program_is_clean_and_literals_are_values() {
        let language = language();
        let mut scratch = Program::new(&language);
        let mut wait = block(&mut scratch, &language, "wait");
        wait.inputs.get_mut("s").unwrap().literal = Some("2.5".into());
        let mut conditional = block(&mut scratch, &language, "if");
        plug(&mut conditional, "c", block(&mut scratch, &language, "yes"));
        let program = one_stack(vec![block(&mut scratch, &language, "go"), wait, conditional]);

        let ast = program.ast(&language);
        assert!(ast.is_clean(), "{:#?}", ast.problems());
        let Stmt::Node(wait) = &ast.scripts[0].body[1] else { panic!() };
        assert_eq!(wait.arg("s"), Some(&Expr::Literal(Value::Float(2.5))));
        let Stmt::Node(conditional) = &ast.scripts[0].body[2] else { panic!() };
        assert!(matches!(conditional.arg("c"), Some(Expr::Node(_))));
        assert_eq!(conditional.branch("then"), Some(&[][..]));
    }

    #[test]
    fn a_looser_type_is_marked_as_a_conversion() {
        let language = language();
        let mut scratch = Program::new(&language);
        let mut say = block(&mut scratch, &language, "say");
        plug(&mut say, "text", block(&mut scratch, &language, "add"));

        let ast = one_stack(vec![say]).ast(&language);
        let Stmt::Node(say) = &ast.scripts[0].body[0] else { panic!() };
        let Some(Expr::Convert { from, to, .. }) = say.arg("text") else {
            panic!("{say:#?}")
        };
        assert_eq!((from.as_str(), to.as_str()), ("number", "text"));
    }

    #[test]
    fn slot_problems_name_the_slot_and_keep_what_parsed() {
        let language = language();
        let mut scratch = Program::new(&language);
        let empty = block(&mut scratch, &language, "if");
        let empty_id = empty.id;
        let mut bad = block(&mut scratch, &language, "wait");
        bad.inputs.get_mut("s").unwrap().literal = Some("soon".into());
        let mut mismatch = block(&mut scratch, &language, "if");
        let add = block(&mut scratch, &language, "add");
        let add_id = add.id;
        plug(&mut mismatch, "c", add);

        let ast = one_stack(vec![empty, bad, mismatch]).ast(&language);
        assert_eq!(
            codes(&ast),
            [ProblemCode::MissingInput, ProblemCode::InvalidLiteral, ProblemCode::TypeMismatch]
        );
        let problems = ast.problems();
        assert_eq!(problems[0].slot, Some((empty_id, "c".to_owned())));
        assert_eq!(problems[2].block, Some(add_id));
        assert!(problems[2].recovered.is_some(), "the misplaced reporter still parses");
        assert!(!ast.is_clean());
    }

    #[test]
    fn misplaced_blocks_are_problems_that_keep_their_node() {
        let language = language();
        let mut scratch = Program::new(&language);
        let blocks = ["say", "go", "end", "wait", "wait", "add"]
            .map(|opcode| block(&mut scratch, &language, opcode))
            .to_vec();

        let ast = one_stack(blocks).ast(&language);
        assert_eq!(
            codes(&ast),
            [ProblemCode::HatNotAtTop, ProblemCode::AfterCap, ProblemCode::ReporterAsStatement]
        );
        assert!(ast.problems().iter().all(|problem| problem.recovered.is_some()));
    }

    #[test]
    fn a_lone_reporter_is_a_loose_expression() {
        let language = language();
        let mut scratch = Program::new(&language);
        let ast = one_stack(vec![block(&mut scratch, &language, "add")]).ast(&language);
        assert!(ast.is_clean());
    }

    #[test]
    fn a_statement_in_a_slot_is_refused_but_kept() {
        let language = language();
        let mut scratch = Program::new(&language);
        let mut say = block(&mut scratch, &language, "say");
        plug(&mut say, "text", block(&mut scratch, &language, "wait"));

        let ast = one_stack(vec![say]).ast(&language);
        assert_eq!(codes(&ast), [ProblemCode::StatementAsInput]);
    }

    #[test]
    fn unknown_blocks_keep_the_valid_blocks_inside_them() {
        let language = language();
        let mut scratch = Program::new(&language);
        let inner = block(&mut scratch, &language, "wait");
        let inner_id = inner.id;
        let mystery = Block {
            id: scratch.fresh_id(),
            opcode: "teleport".into(),
            inputs: BTreeMap::from([(
                "where".to_owned(),
                Input {
                    literal: Some("moon".into()),
                    block: None,
                },
            )]),
            branches: BTreeMap::from([("then".to_owned(), vec![inner])]),
        };

        let ast = one_stack(vec![mystery]).ast(&language);
        let problems = ast.problems();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, ProblemCode::UnknownOpcode);
        let raw = problems[0].recovered.as_ref().unwrap();
        assert_eq!(raw.arg("where"), Some(&Expr::Literal(Value::Text("moon".into()))));
        let Some([Stmt::Node(kept)]) = raw.branch("then") else { panic!("{raw:#?}") };
        assert_eq!(kept.id, inner_id);
    }

    #[test]
    fn inputs_and_branches_the_language_lacks_are_warnings() {
        let language = language();
        let mut scratch = Program::new(&language);
        let mut wait = block(&mut scratch, &language, "wait");
        wait.inputs.insert("speed".into(), Input::default());
        wait.branches.insert("else".into(), vec![block(&mut scratch, &language, "say")]);

        let ast = one_stack(vec![wait]).ast(&language);
        assert_eq!(codes(&ast), [ProblemCode::UnknownInput, ProblemCode::UnknownBranch]);
        assert!(ast.is_clean(), "warnings do not make a program unrunnable");
    }

    #[test]
    fn a_reused_id_is_reported_on_its_second_use() {
        let language = language();
        let mut scratch = Program::new(&language);
        let first = block(&mut scratch, &language, "wait");
        let mut second = block(&mut scratch, &language, "say");
        second.id = first.id;

        let ast = one_stack(vec![first, second]).ast(&language);
        assert_eq!(codes(&ast), [ProblemCode::DuplicateId]);
        let Stmt::Problem(problem) = &ast.scripts[0].body[1] else { panic!() };
        assert_eq!(problem.recovered.as_ref().unwrap().opcode, "say");
    }

    /// `levels` nested ifs around a wait; each if's condition sits one deeper.
    fn nested(language: &Language, levels: usize) -> Program {
        let mut scratch = Program::new(language);
        let mut innermost = block(&mut scratch, language, "wait");
        for _ in 0..levels {
            let mut outer = block(&mut scratch, language, "if");
            plug(&mut outer, "c", block(&mut scratch, language, "yes"));
            outer.branches.insert("then".into(), vec![innermost]);
            innermost = outer;
        }
        one_stack(vec![innermost])
    }

    #[test]
    fn blocks_past_the_depth_limit_are_problems_and_the_limit_itself_is_fine() {
        let language = language();
        assert!(nested(&language, MAX_DEPTH - 1).ast(&language).is_clean());
        // The last if's condition and body both land one past the limit.
        assert_eq!(
            codes(&nested(&language, MAX_DEPTH).ast(&language)),
            [ProblemCode::TooDeep, ProblemCode::TooDeep]
        );
    }
}
