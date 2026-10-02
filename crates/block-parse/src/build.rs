//! Program to [`Ast`]. Never fails: each fault becomes a [`Problem`] in the
//! place it occurs, and parsing carries on around it.

use std::collections::{HashMap, HashSet};

use crate::ast::{Arg, Ast, Branch, Expr, List, Node, Problem, ProblemCode, Script, Severity, Stmt};
use crate::language::{BlockDef, BlockKind, Callable, Extent, Fit, Language, LiteralKind, Part, is_blank};
use crate::program::{Block, BlockId, Declaration, Input, MAX_DEPTH, Program, Slot};
use crate::value::Value;

impl Program {
    /// One script per stack. A stack that is a single reporter is a loose
    /// expression, not a problem.
    pub fn ast(&self, language: &Language) -> Ast {
        let mut builder = Builder::new(language, self);
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

    /// What running from `id` covers, as a script of its own: the block and
    /// those below it (a hat's whole script), or a lone expression for a
    /// reporter, even one in a slot.
    pub fn script_at(&self, language: &Language, id: BlockId) -> Option<Script> {
        let run = self.run_at(id)?;
        let mut builder = Builder::new(language, self);
        Some(Script {
            body: builder.stack(&run.blocks),
        })
    }
}

struct Builder<'a> {
    language: &'a Language,
    program: &'a Program,
    seen: HashSet<BlockId>,
    /// Blocks whose local names are in scope where the builder is.
    visible: Vec<BlockId>,
    /// Every block in the program that has a scope.
    scopes: HashMap<BlockId, &'a Block>,
}

impl<'a> Builder<'a> {
    fn new(language: &'a Language, program: &'a Program) -> Self {
        let mut scopes = HashMap::new();
        program.each_block(|block| {
            if language.block(&block.opcode).is_some_and(|def| def.scope.is_some()) {
                scopes.insert(block.id, block);
            }
        });
        Self {
            language,
            program,
            seen: HashSet::new(),
            visible: Vec::new(),
            scopes,
        }
    }

    /// Runs `work` with the local names of `blocks` in scope.
    fn within<T>(&mut self, blocks: &[BlockId], work: impl FnOnce(&mut Self) -> T) -> T {
        let mark = self.visible.len();
        self.visible.extend_from_slice(blocks);
        let result = work(self);
        self.visible.truncate(mark);
        result
    }

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
            // Before parsing, so a block that fails still takes the first place
            // after a cap, and a cap that fails still ends the run.
            let first_after_cap = std::mem::take(&mut after_cap);
            if kind.as_ref().is_some_and(BlockKind::is_cap) {
                after_cap = true;
            }
            let node = match self.block(block, depth) {
                Ok(node) => node,
                Err(problem) => {
                    body.push(Stmt::Problem(problem));
                    continue;
                }
            };
            let name = self.name(block);
            // Only the first: the rest are unreachable for the same reason.
            let fault = if first_after_cap {
                Some((ProblemCode::AfterCap, format!("nothing can follow a cap, but `{name}` does")))
            } else {
                match &kind {
                    Some(BlockKind::Reporter(_)) => Some((
                        ProblemCode::ReporterAsStatement,
                        format!("`{name}` reports a value and cannot stand in a stack"),
                    )),
                    Some(kind) if kind.is_hat() && !(top_of_stack && index == 0) => Some((
                        ProblemCode::HatNotAtTop,
                        format!("`{name}` can only start a stack"),
                    )),
                    _ => None,
                }
            };
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
        if let Some(declaration) = &block.refers
            && let Some((code, message)) = self.reference_fault(block, declaration)
        {
            self.seen.insert(block.id);
            return Err(problem(code, message, None));
        }
        let duplicate = !self.seen.insert(block.id);
        let arity = self.program.arity(self.language, block);
        let (node, def, extent) = match self.language.block(&block.opcode) {
            Some(def) => {
                let extent = def.extent(block, arity);
                (self.node(block, def, extent, depth), def, extent)
            }
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
        for list in def.lists().filter(|list| node.list(&list.name).is_some()) {
            let count = block.lists.get(&list.name).map_or(0, Vec::len);
            if count < list.min {
                return Err(problem(
                    ProblemCode::TooFewItems,
                    format!("`{}` needs at least {} in `{}`", def.name, list.min, list.name),
                    Some(node),
                ));
            }
        }
        if let Some(extent) = extent.filter(|extent| !extent.named && !self.language.curried())
            && let Some(message) = arity_fault(block, def, &node, extent, arity)
        {
            return Err(problem(ProblemCode::Arity, message, Some(node)));
        }
        Ok(node)
    }

    fn node(&mut self, block: &Block, def: &BlockDef, extent: Option<Extent>, depth: usize) -> Node {
        let named = extent.is_some_and(|extent| extent.named);
        let shown = extent.map_or(&def.parts[..], |extent| def.shown_parts(extent.shown));
        let shows = |name: &str| {
            shown.iter().any(|part| match part {
                Part::Input(input) => input.name == name,
                Part::List(list) => list.name == name,
                Part::Label(_) | Part::Branch(_) => false,
            })
        };
        let arguments = match &def.callable {
            Some(Callable::Arguments(list)) => extent.map(|extent| (list.as_str(), extent.shown)),
            Some(Callable::Parts) | None => None,
        };
        let scope_blocks = match &def.scope {
            Some(scope) if !scope.over.is_empty() => block.scope_blocks(self.language),
            _ => Vec::new(),
        };
        let scoped = |name: &str| if def.scopes_over(name) { scope_blocks.as_slice() } else { &[] };
        let mut args: Vec<Arg> = def
            .inputs()
            .filter(|input| shows(&input.name))
            .map(|input| Arg {
                name: input.name.clone(),
                value: self.within(scoped(&input.name), |this| {
                    this.value(
                        block,
                        def,
                        Slot::input(input.name.clone()),
                        &input.ty,
                        block.inputs.get(&input.name),
                        input.default.as_deref(),
                        depth,
                    )
                }),
            })
            .collect();
        for (name, stored) in &block.inputs {
            if def.input(name).is_none() {
                let value = self.unknown(block, def, Slot::input(name.clone()), stored, depth);
                args.push(Arg { name: name.clone(), value });
            }
        }

        let mut lists: Vec<List> = def
            .lists()
            .filter(|list| shows(&list.name) && !(named && arguments.is_some()))
            .map(|list| {
                let stored = block.lists.get(&list.name).map(Vec::as_slice).unwrap_or(&[]);
                let count = match arguments {
                    Some((name, shown)) if name == list.name => shown,
                    _ => stored.len(),
                };
                let items = self.within(scoped(&list.name), |this| {
                    (0..count)
                        .map(|index| {
                            let slot = Slot::item(list.name.clone(), index);
                            this.value(block, def, slot, &list.ty, stored.get(index), None, depth)
                        })
                        .collect()
                });
                List {
                    name: list.name.clone(),
                    items,
                }
            })
            .collect();
        for (name, stored) in &block.lists {
            if def.list(name).is_some() {
                continue;
            }
            let items = stored
                .iter()
                .enumerate()
                .map(|(index, item)| self.unknown(block, def, Slot::item(name.clone(), index), item, depth))
                .collect();
            lists.push(List {
                name: name.clone(),
                items,
            });
        }

        let mut branches: Vec<Branch> = def
            .branches()
            .map(|name| Branch {
                name: name.to_owned(),
                body: self.within(scoped(name), |this| {
                    this.sequence(block.branches.get(name).map_or(&[][..], Vec::as_slice), depth + 1, false)
                }),
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
            lists,
            branches,
            refers: block.refers.clone(),
            named,
        }
    }

    fn reference_fault(&self, block: &Block, declaration: &Declaration) -> Option<(ProblemCode, String)> {
        let def = self
            .scopes
            .get(&declaration.block)
            .and_then(|owner| self.language.block(&owner.opcode))
            .filter(|def| def.declares(&declaration.slot.input));
        let Some(def) = def else {
            let name = block.inputs.values().find_map(|input| input.literal.as_deref()).unwrap_or_default();
            return Some((ProblemCode::OutOfScope, format!("`{name}` refers to a declaration that is gone")));
        };
        let name = self.program.declared_name(declaration).filter(|name| !is_blank(name));
        let shown = name.map_or_else(|| def.unnamed(&declaration.slot), |name| format!("`{name}`"));
        if !def.is_global(&declaration.slot.input) && !self.visible.contains(&declaration.block) {
            return Some((ProblemCode::OutOfScope, format!("{shown} is used outside the block that declares it")));
        }
        name.is_none().then(|| (ProblemCode::Unnamed, format!("{shown} has no name")))
    }

    /// What a slot the block does not define held, kept under a warning.
    fn unknown(&mut self, block: &Block, def: &BlockDef, slot: Slot, stored: &Input, depth: usize) -> Expr {
        let recovered = stored.block.as_deref().and_then(|inner| self.recover(inner, depth + 1));
        let literal = match &stored.literal {
            Some(text) => format!(" (it held {text:?})"),
            None => String::new(),
        };
        let what = if slot.item.is_some() { "list" } else { "input" };
        let message = format!("`{}` has no {what} `{}`{literal}", def.name, slot.input);
        Expr::Problem(Box::new(Problem {
            block: None,
            slot: Some((block.id, slot)),
            code: ProblemCode::UnknownInput,
            severity: Severity::Warning,
            message,
            recovered: recovered.map(Box::new),
        }))
    }

    /// A single input or list item of type `ty`. `default` stands in for a
    /// single input the file lacks; a list item holding nothing is a hole.
    #[allow(clippy::too_many_arguments)]
    fn value(
        &mut self,
        block: &Block,
        def: &BlockDef,
        slot: Slot,
        ty: &str,
        stored: Option<&Input>,
        default: Option<&str>,
        depth: usize,
    ) -> Expr {
        let label = match slot.item {
            Some(index) => format!("item {} of `{}`", index + 1, slot.input),
            None => format!("`{}`", slot.input),
        };
        let hole = slot.item.is_some() && stored.is_none_or(Input::is_hole);
        let declares = def.declares(&slot.input);
        let slot = Some((block.id, slot));
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
                    format!("`{inner_name}` does not report a value, so it cannot fill {label}"),
                    Some(inner.id),
                    Some(node),
                );
            };
            return match self.language.fit(&output, ty) {
                Fit::Exact => Expr::Node(Box::new(node)),
                Fit::Convert => Expr::Convert {
                    from: output,
                    to: ty.to_owned(),
                    value: Box::new(Expr::Node(Box::new(node))),
                },
                Fit::No => problem(
                    ProblemCode::TypeMismatch,
                    format!("`{inner_name}` reports {output}, but {label} of `{}` takes {ty}", def.name),
                    Some(inner.id),
                    Some(node),
                ),
            };
        }

        let takes_literal = self
            .language
            .ty(ty)
            .is_some_and(|ty| ty.literal != LiteralKind::None);
        if hole && declares {
            return problem(ProblemCode::Unnamed, format!("{label} of `{}` needs a name", def.name), None, None);
        }
        if hole {
            return problem(
                ProblemCode::MissingInput,
                format!("{label} of `{}` is empty", def.name),
                None,
                None,
            );
        }
        if !takes_literal {
            return problem(
                ProblemCode::MissingInput,
                format!("`{}` needs a block in {label}", def.name),
                None,
                None,
            );
        }
        // An input the file lacks, say one the language added since, reads as
        // its default.
        let text = stored
            .and_then(|stored| stored.literal.as_deref())
            .or(default)
            .unwrap_or_default();
        if declares && is_blank(text) {
            return problem(ProblemCode::Unnamed, format!("{label} of `{}` needs a name", def.name), None, None);
        }
        match self.language.parse_literal(ty, text) {
            Ok(value) => Expr::Literal(value),
            Err(message) => problem(
                ProblemCode::InvalidLiteral,
                format!("{label} of `{}`: {message}", def.name),
                None,
                None,
            ),
        }
    }

    /// A block whose opcode the language lacks, kept as written: literals as
    /// text, plugged blocks and branches parsed.
    fn raw(&mut self, block: &Block, depth: usize) -> Node {
        let mut raw = |stored: &Input| match (&stored.block, &stored.literal) {
            (Some(inner), _) => match self.block(inner, depth + 1) {
                Ok(node) => Expr::Node(Box::new(node)),
                Err(problem) => Expr::Problem(Box::new(problem)),
            },
            (None, literal) => Expr::Literal(Value::Text(literal.clone().unwrap_or_default())),
        };
        let args = block
            .inputs
            .iter()
            .map(|(name, stored)| Arg {
                name: name.clone(),
                value: raw(stored),
            })
            .collect();
        let lists = block
            .lists
            .iter()
            .map(|(name, items)| List {
                name: name.clone(),
                items: items.iter().map(&mut raw).collect(),
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
            lists,
            branches,
            refers: block.refers.clone(),
            named: false,
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

/// Why a call shows the wrong number of parameters, if it does. A hidden
/// list that may be empty is no fault.
fn arity_fault(block: &Block, def: &BlockDef, node: &Node, extent: Extent, arity: Option<usize>) -> Option<String> {
    match def.callable.as_ref()? {
        Callable::Parts => {
            let input = def.inputs().find(|input| node.arg(&input.name).is_none()).map(|input| &input.name);
            let list = def.lists().find(|list| list.min > 0 && node.list(&list.name).is_none()).map(|list| &list.name);
            let missing = input.or(list)?;
            Some(format!("`{}` is called without `{missing}`", def.name))
        }
        Callable::Arguments(_) => {
            let arity = arity.filter(|&arity| arity != extent.shown)?;
            let called = block.inputs.values().find_map(|input| input.literal.as_deref()).unwrap_or(&def.name);
            let plural = |n: usize| if n == 1 { "" } else { "s" };
            Some(format!(
                "`{called}` takes {arity} argument{}, but is given {}",
                plural(arity),
                extent.shown
            ))
        }
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
            (id: "sum", name: "Sum", kind: Reporter("number"), spec: "sum {xs:number+}"),
            (id: "all", name: "All", kind: Reporter("bool"), spec: "all {cs:bool*}"),
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

    fn literal(text: &str) -> Input {
        Input {
            literal: Some(text.into()),
            block: None,
        }
    }

    fn reporter(block: Block) -> Input {
        Input {
            literal: None,
            block: Some(Box::new(block)),
        }
    }

    #[test]
    fn list_items_parse_like_inputs_and_holes_are_missing() {
        let language = language();
        let mut scratch = Program::new(&language);
        let mut sum = block(&mut scratch, &language, "sum");
        let inner = block(&mut scratch, &language, "add");
        let sum_id = sum.id;
        sum.lists.insert(
            "xs".into(),
            vec![literal("1.5"), Input::default(), reporter(inner), literal("x")],
        );
        let mut say = block(&mut scratch, &language, "say");
        plug(&mut say, "text", sum);

        let ast = one_stack(vec![say]).ast(&language);
        let Stmt::Node(say) = &ast.scripts[0].body[0] else { panic!() };
        let Some(Expr::Convert { value, .. }) = say.arg("text") else { panic!("{say:#?}") };
        let Expr::Node(sum) = value.as_ref() else { panic!() };
        let xs = sum.list("xs").unwrap();
        assert_eq!(xs.len(), 4, "indices match the program's");
        assert_eq!(xs[0], Expr::Literal(Value::Float(1.5)));
        assert!(matches!(&xs[2], Expr::Node(node) if node.opcode == "add"));
        let problems = ast.problems();
        assert_eq!(codes(&ast), [ProblemCode::MissingInput, ProblemCode::InvalidLiteral]);
        assert_eq!(problems[0].slot, Some((sum_id, Slot::item("xs", 1))));
        assert!(problems[0].message.contains("item 2 of `xs`"), "{}", problems[0].message);
        assert_eq!(problems[1].slot, Some((sum_id, Slot::item("xs", 3))));
    }

    #[test]
    fn a_plus_list_with_no_items_is_too_few() {
        let language = language();
        let mut scratch = Program::new(&language);
        let sum = block(&mut scratch, &language, "sum");
        let all = block(&mut scratch, &language, "all");
        let ast = one_stack(vec![sum]).ast(&language);
        assert_eq!(codes(&ast), [ProblemCode::TooFewItems]);
        assert!(ast.problems()[0].recovered.is_some());
        assert!(one_stack(vec![all]).ast(&language).is_clean(), "`*` may be empty");
    }

    #[test]
    fn lists_the_block_lacks_are_kept_under_warnings() {
        let language = language();
        let mut scratch = Program::new(&language);
        let mut all = block(&mut scratch, &language, "all");
        let yes = block(&mut scratch, &language, "yes");
        all.lists.insert("old".into(), vec![reporter(yes)]);
        all.inputs.insert("cs".into(), literal("true"));

        let ast = one_stack(vec![all]).ast(&language);
        assert!(ast.is_clean());
        assert_eq!(codes(&ast), [ProblemCode::UnknownInput, ProblemCode::UnknownInput], "a list as an input too");
        let Stmt::Node(all) = &ast.scripts[0].body[0] else { panic!() };
        let Some([Expr::Problem(problem)]) = all.list("old") else { panic!("{all:#?}") };
        assert!(problem.recovered.as_ref().is_some_and(|node| node.opcode == "yes"));
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
        assert_eq!(problems[0].slot, Some((empty_id, Slot::input("c"))));
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
    fn a_block_that_fails_after_a_cap_still_takes_the_first_place() {
        let language = language();
        let mut scratch = Program::new(&language);
        let mystery = Block {
            opcode: "teleport".into(),
            ..block(&mut scratch, &language, "wait")
        };
        let blocks = vec![block(&mut scratch, &language, "end"), mystery, block(&mut scratch, &language, "wait")];

        let ast = one_stack(blocks).ast(&language);
        assert_eq!(codes(&ast), [ProblemCode::UnknownOpcode], "the wait is not the first after the cap");

        // A cap that fails parsing still ends the run.
        let mut end = block(&mut scratch, &language, "end");
        let wait = block(&mut scratch, &language, "wait");
        end.id = wait.id;
        let after = block(&mut scratch, &language, "say");
        let ast = one_stack(vec![wait, end, after]).ast(&language);
        assert_eq!(codes(&ast), [ProblemCode::DuplicateId, ProblemCode::AfterCap]);
    }

    #[test]
    fn a_lone_reporter_is_a_loose_expression() {
        let language = language();
        let mut scratch = Program::new(&language);
        let ast = one_stack(vec![block(&mut scratch, &language, "add")]).ast(&language);
        assert!(ast.is_clean());
    }

    #[test]
    fn a_script_at_a_block_runs_from_it_and_a_plugged_reporter_alone() {
        let language = language();
        let mut scratch = Program::new(&language);
        let go = block(&mut scratch, &language, "go");
        let mut wait = block(&mut scratch, &language, "wait");
        let add = block(&mut scratch, &language, "add");
        let (go_id, wait_id, add_id) = (go.id, wait.id, add.id);
        plug(&mut wait, "s", add);
        let program = one_stack(vec![go, wait, block(&mut scratch, &language, "end")]);

        assert_eq!(program.script_at(&language, go_id).unwrap(), program.ast(&language).scripts[0]);
        let from_wait = program.script_at(&language, wait_id).unwrap();
        assert_eq!(from_wait.body.len(), 2);
        let reporter = program.script_at(&language, add_id).unwrap();
        assert!(matches!(&reporter.body[..], [Stmt::Node(node)] if node.id == add_id));
        assert!(program.script_at(&language, BlockId(999)).is_none());
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
            lists: BTreeMap::new(),
            branches: BTreeMap::from([("then".to_owned(), vec![inner])]),
            refers: None,
            reach: None,
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

    /// Calls, names and procedure references. See `documentation/07-calls.md`.
    mod calls {
        use super::*;
        use crate::edit::{Fragment, Target};
        use crate::language::{Callable, LanguageError};
        use crate::program::Reach;

        const CALLS: &str = r#"Language(
            name: "c",
            file: (extension: "c"),
            callable: true,
            types: {
                "value": (literal: Text),
                "name": (literal: Text),
                "procedure": (shape: Square, literal: Text, accepts: Types(["value"]), by_name: true),
            },
            blocks: [
                (id: "fold", name: "Fold", kind: Reporter("value"), spec: "fold {kons:procedure} {knil:value} {xs:value}"),
                (id: "add", name: "Add", kind: Reporter("value"), spec: "add {zs:value+}"),
                (id: "member", name: "Member", kind: Reporter("value"), spec: "member {x:value} {compare:procedure*}"),
                (id: "quote", name: "Quote", kind: Reporter("value"), spec: "quote {x:value}", callable: false),
                (id: "get", name: "Get", kind: Reporter("value"), spec: "{name:name}"),
                (id: "call", name: "Call", kind: Reporter("value"), spec: "{name:name} {args:value*}"),
                (
                    id: "define", name: "Define", kind: Reporter("value"), callable: false,
                    spec: "define {name:name} _taking_ {params:name*} {body:value}",
                    hints: {"params": "parameter"},
                    scope: (
                        declares: ["name", "params"], over: ["body"], global: ["name"], reference: "get",
                        signature: (name: "name", parameters: "params", reference: "call"),
                    ),
                ),
            ],
        )"#;

        fn calls() -> Language {
            Language::from_ron(CALLS, &Validators::new()).unwrap()
        }

        fn problems(text: &str) -> Vec<String> {
            match Language::from_ron(text, &Validators::new()) {
                Err(LanguageError::Invalid(problems)) => problems.into_iter().map(|p| p.message).collect(),
                other => panic!("{other:?}"),
            }
        }

        /// The one expression a lone reporter makes, problem or not.
        fn expression(language: &Language, program: &Program, id: BlockId) -> Stmt {
            program.script_at(language, id).unwrap().body.remove(0)
        }

        fn on_canvas(program: &mut Program, block: Block) -> BlockId {
            let id = block.id;
            program.stacks.push(Stack {
                pos: [0.0, 0.0],
                blocks: vec![block],
            });
            id
        }

        /// `define f taking x y`, and a fresh reference to `f`.
        fn procedure(language: &Language, program: &mut Program) -> (BlockId, Block) {
            let define = block(program, language, "define");
            let id = on_canvas(program, define);
            program.set_literal(id, &Slot::input("name"), "f".into());
            program.set_literal(id, &Slot::item("params", 0), "x".into());
            program.set_literal(id, &Slot::item("params", 1), "y".into());
            let name = Declaration {
                block: id,
                slot: Slot::input("name"),
            };
            (id, program.reference(language, &name).unwrap())
        }

        #[test]
        fn reporters_named_by_a_label_are_callable_unless_they_say_not() {
            let language = calls();
            let callable = |opcode: &str| language.block(opcode).unwrap().callable.clone();
            assert_eq!(callable("fold"), Some(Callable::Parts));
            assert_eq!(callable("quote"), None);
            assert_eq!(callable("get"), None, "nothing names it");
            assert_eq!(callable("call"), Some(Callable::Arguments("args".into())), "a signature's reference");

            let go = r#"blocks: [(id: "go", name: "Go", spec: "go", callable: true),"#;
            let statement = CALLS.replace("blocks: [", go);
            assert_eq!(problems(&statement), ["only reporters are callable"]);
            let unnamed = CALLS.replace(r#"spec: "{name:name}")"#, r#"spec: "{name:name}", callable: true)"#);
            assert!(problems(&unnamed)[0].contains("starts with a label"));
            let wrong = CALLS.replace(r#"reference: "call")"#, r#"reference: "get")"#);
            assert!(problems(&wrong)[0].contains("one input and one list"), "{:?}", problems(&wrong));
        }

        #[test]
        fn the_edge_stops_at_the_last_parameter_holding_something() {
            let language = calls();
            let mut program = Program::new(&language);
            let fold = language.block("fold").unwrap();
            let mut block = block(&mut program, &language, "fold");
            let extent = fold.extent(&block, None).unwrap();
            assert_eq!((extent.parameters, extent.filled, extent.shown, extent.named), (3, 0, 3, false));
            let all = [Some(Reach::Name), Some(Reach::Call(0)), Some(Reach::Call(1)), Some(Reach::Call(2)), None];
            assert_eq!(extent.stops(), all);

            block.reach = Some(Reach::Call(1));
            let labels = |parts: &[Part]| parts.iter().filter(|part| matches!(part, Part::Input(_))).count();
            assert_eq!(labels(fold.shown_parts(fold.extent(&block, None).unwrap().shown)), 1);

            block.inputs.get_mut("knil").unwrap().literal = Some("0".into());
            let extent = fold.extent(&block, None).unwrap();
            assert_eq!(extent.shown, 2, "a filled parameter shows whatever the reach says");
            assert_eq!(extent.stops(), [Some(Reach::Call(2)), None]);
        }

        #[test]
        fn a_named_block_has_no_arguments_and_a_partial_call_is_an_arity_problem() {
            let language = calls();
            let mut program = Program::new(&language);
            let mut add = block(&mut program, &language, "add");
            add.reach = Some(Reach::Name);
            let id = on_canvas(&mut program, add);
            let Stmt::Node(node) = expression(&language, &program, id) else { panic!("no TooFewItems by name") };
            assert!(node.named && node.args.is_empty() && node.lists.is_empty(), "{node:#?}");

            let mut fold = block(&mut program, &language, "fold");
            fold.reach = Some(Reach::Call(1));
            let id = on_canvas(&mut program, fold);
            let Stmt::Problem(problem) = expression(&language, &program, id) else { panic!() };
            assert_eq!(problem.code, ProblemCode::Arity);
            assert!(problem.message.contains("`knil`"), "{}", problem.message);
            let recovered = problem.recovered.unwrap();
            assert_eq!(recovered.args.len(), 1, "the hidden parameters are left out");

            let curried = CALLS.replace("callable: true,", "callable: true, curried: true,");
            let curried = Language::from_ron(&curried, &Validators::new()).unwrap();
            let called = expression(&curried, &program, id);
            assert!(matches!(called, Stmt::Node(node) if !node.named && node.args.len() == 1));

            let mut member = block(&mut program, &language, "member");
            member.reach = Some(Reach::Call(1));
            let id = on_canvas(&mut program, member);
            assert!(matches!(expression(&language, &program, id), Stmt::Node(_)), "a list that may be empty can hide");
        }

        #[test]
        fn a_procedure_reference_shows_an_argument_per_parameter() {
            let language = calls();
            let mut program = Program::new(&language);
            let (define, mut call) = procedure(&language, &mut program);
            assert_eq!(call.opcode, "call");
            let name = call.refers.clone().unwrap();
            assert_eq!(program.parameters(&language, &name), Some(vec!["x".into(), "y".into()]));
            assert_eq!(program.arity(&language, &call), Some(2));

            call.lists.insert("args".into(), vec![literal("1")]);
            let id = call.id;
            plug(program.find_mut(define).unwrap(), "body", call);
            let Stmt::Node(define_node) = expression(&language, &program, define) else { panic!() };
            let Some(Expr::Node(node)) = define_node.arg("body") else { panic!() };
            let args = node.list("args").unwrap();
            assert_eq!(args.len(), 2, "one per parameter");
            assert!(matches!(&args[1], Expr::Problem(problem) if problem.code == ProblemCode::MissingInput));

            assert!(!program.set_reach(&language, id, Some(Reach::Name)), "an argument is filled in");
            assert!(program.set_reach(&language, id, Some(Reach::Call(1))));
            let ast = program.ast(&language);
            assert_eq!(codes(&ast), [ProblemCode::Arity]);
            assert!(ast.problems()[0].message.contains("`f` takes 2 arguments, but is given 1"));

            program.find_mut(id).unwrap().lists.clear();
            assert!(program.set_reach(&language, id, Some(Reach::Name)));
            let ast = program.ast(&language);
            assert!(ast.is_clean(), "{:?}", ast.problems());
            program.set_literal(define, &Slot::item("params", 2), "z".into());
            assert!(program.set_reach(&language, id, Some(Reach::Call(3))));
            assert_eq!(program.find(id).unwrap().reach, None, "showing every parameter follows the procedure");
        }

        #[test]
        fn any_argument_a_reference_shows_takes_text_or_a_drop_but_its_name_does_not() {
            let language = calls();
            let mut program = Program::new(&language);
            let (define, call) = procedure(&language, &mut program);
            let id = call.id;
            let body = Target::Input {
                parent: define,
                slot: Slot::input("body"),
            };
            program.attach(&language, Fragment { blocks: vec![call] }, body).unwrap();

            assert!(!program.set_literal(id, &Slot::input("name"), "g".into()), "the name is the declaration's");
            assert!(program.set_literal(id, &Slot::item("args", 1), "2".into()), "the second before the first");
            assert!(program.find(id).unwrap().lists["args"][0].is_hole());
            assert!(program.set_literal(id, &Slot::item("args", 1), String::new()));
            assert!(!program.find(id).unwrap().lists.contains_key("args"), "emptied, it trims");

            let quote = block(&mut program, &language, "quote");
            let second = Target::Input {
                parent: id,
                slot: Slot::item("args", 1),
            };
            program.attach(&language, Fragment { blocks: vec![quote] }, second).unwrap();
            assert!(program.find(id).unwrap().lists["args"][1].block.is_some());
            let third = Target::Input {
                parent: id,
                slot: Slot::item("args", 2),
            };
            let quote = Fragment {
                blocks: vec![block(&mut program, &language, "quote")],
            };
            assert!(program.can_attach(&language, &quote, &third).is_err(), "f takes two");
        }

        #[test]
        fn a_signatures_reference_may_say_it_is_callable() {
            let text = CALLS.replace(r#"spec: "{name:name} {args:value*}")"#, r#"spec: "{name:name} {args:value*}", callable: true)"#);
            let language = Language::from_ron(&text, &Validators::new()).unwrap();
            assert_eq!(language.block("call").unwrap().callable, Some(Callable::Arguments("args".into())));
        }

        #[test]
        fn a_version_3_file_names_the_bare_procedures_it_meant_by_name() {
            let language = calls();
            let mut old = Program::new(&language);
            old.version = 3;
            let mut fold = block(&mut old, &language, "fold");
            let (bare, called) = (block(&mut old, &language, "add"), block(&mut old, &language, "add"));
            let (bare_id, called_id) = (bare.id, called.id);
            plug(&mut fold, "kons", bare);
            plug(&mut fold, "knil", called);
            on_canvas(&mut old, fold);
            let path = std::env::temp_dir().join(format!("calls-{}.c", std::process::id()));
            old.save(&path).unwrap();
            let (loaded, warnings) = Program::load(&path, &language).unwrap();
            std::fs::remove_file(&path).unwrap();

            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(loaded.find(bare_id).unwrap().reach, Some(Reach::Name));
            assert_eq!(loaded.find(called_id).unwrap().reach, None, "only in by_name slots");
            assert_eq!(loaded.version, crate::program::FORMAT_VERSION, "so it is not named again");
        }

        #[test]
        fn blank_parameters_are_unnamed() {
            let language = calls();
            let mut program = Program::new(&language);
            let (define, call) = procedure(&language, &mut program);
            program.set_literal(define, &Slot::item("params", 0), String::new());
            let name = call.refers.unwrap();
            assert_eq!(program.parameters(&language, &name), Some(vec!["Unnamed parameter 1".into(), "y".into()]));
        }

        #[test]
        fn an_empty_callable_dropped_in_a_by_name_slot_becomes_its_name() {
            let language = calls();
            let mut program = Program::new(&language);
            let fold = block(&mut program, &language, "fold");
            let fold = on_canvas(&mut program, fold);
            let drop_in = |program: &mut Program, block: Block, input: &str| {
                let id = block.id;
                let target = Target::Input {
                    parent: fold,
                    slot: Slot::input(input),
                };
                program.attach(&language, Fragment { blocks: vec![block] }, target).unwrap();
                program.find(id).unwrap().reach
            };
            let add = block(&mut program, &language, "add");
            let named = add.id;
            assert_eq!(drop_in(&mut program, add, "kons"), Some(Reach::Name));
            let saved = Program::from_ron(&program.to_ron()).unwrap();
            assert_eq!(saved.find(named).unwrap().reach, Some(Reach::Name), "saved");

            let mut filled = block(&mut program, &language, "add");
            filled.lists.insert("zs".into(), vec![literal("1")]);
            assert_eq!(drop_in(&mut program, filled, "kons"), None, "a call returning the procedure");
            let add = block(&mut program, &language, "add");
            assert_eq!(drop_in(&mut program, add, "knil"), None, "only by_name slots");
        }
    }
}
