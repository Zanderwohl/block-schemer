//! AST to Scheme text: the layer between untrusted blocks and the
//! interpreter. Only blocks the language offers get this far, as anything
//! else is a problem in the AST. Strings are escaped here; datum and symbol
//! literals were checked by [`literals`](crate::literals) and go out as typed.

use block_parse::ast::{Expr, Node, Script, Stmt};
use block_parse::language::{BlockDef, Part};
use block_parse::{Language, Value};

use crate::form::Form;

/// Blocks whose opcode is taken by another, by the Scheme name they call.
const RENAMED: [(&str, &str); 1] = [("string_chars", "string")];

/// Where `display` writes, so the runner can show it. Code generated for
/// reading names it only when asked to show this harness.
pub const OUTPUT_PORT: &str = "__out";

/// One expression per statement of `script`, a line each. `Err` holds the
/// first problem, as a script with any error-level problem is not run.
pub fn script(language: &Language, script: &Script) -> Result<String, String> {
    let forms = forms(language, script, false, true)?;
    Ok(forms.iter().map(Form::to_string).collect::<Vec<_>>().join("\n"))
}

/// As [`script`] laid out to `width` columns, for reading only: a faulty
/// input becomes `<name>`, a faulty block what parsed of it, and a statement
/// with nothing recovered is left out. `harness` keeps [`OUTPUT_PORT`] in.
pub fn pretty(language: &Language, script: &Script, width: usize, harness: bool) -> Result<String, String> {
    let forms = forms(language, script, true, harness)?;
    Ok(forms.iter().map(|form| form.pretty(width)).collect::<Vec<_>>().join("\n\n"))
}

/// As [`pretty`], a line per statement however long, for echoing what ran.
pub fn flat(language: &Language, script: &Script, harness: bool) -> Result<String, String> {
    let forms = forms(language, script, true, harness)?;
    Ok(forms.iter().map(Form::to_string).collect::<Vec<_>>().join("\n"))
}

fn forms(language: &Language, script: &Script, holes: bool, harness: bool) -> Result<Vec<Form>, String> {
    let mut forms = Vec::new();
    for statement in &script.body {
        let node = match statement {
            Stmt::Node(node) => node,
            Stmt::Problem(problem) if !holes => return Err(problem.message.clone()),
            Stmt::Problem(problem) => match &problem.recovered {
                Some(node) => node,
                None => continue,
            },
        };
        forms.push(Generator { language, holes, harness }.node(node)?);
    }
    Ok(forms)
}

struct Generator<'a> {
    language: &'a Language,
    /// Write `<name>` for a faulty input instead of failing.
    holes: bool,
    harness: bool,
}

impl Generator<'_> {
    fn node(&self, node: &Node) -> Result<Form, String> {
        let def = self
            .language
            .block(&node.opcode)
            .ok_or_else(|| format!("no block `{}`", node.opcode))?;
        let one = |name: &str| self.arg(def, node, name);
        let many = |name: &str| self.list(def, node, name);
        let wrap = Form::List;
        let atom = Form::atom;
        if node.named {
            return match node.opcode.as_str() {
                "procedure_call" => one("variable"),
                opcode => Ok(atom(scheme_name(opcode))),
            };
        }
        Ok(match node.opcode.as_str() {
            "program" => one("main")?,
            "string" => Form::Atom(self.text(node, "text")?),
            "nil" => atom("'()"),
            "variable" => one("variable")?,
            "call" => wrap([vec![one("operator")?], many("operands")?].concat()),
            "procedure_call" => wrap([vec![one("variable")?], many("arguments")?].concat()),
            "binding" => wrap(vec![one("variable")?, one("init")?]),
            "define_procedure" => {
                let head = wrap([vec![one("variable")?], many("formals")?].concat());
                wrap([vec![atom("define"), head], many("body")?].concat())
            }
            "lambda" => wrap([vec![atom("lambda"), wrap(many("formals")?)], many("body")?].concat()),
            "let" | "letrec" | "letrec*" => {
                wrap([vec![atom(&node.opcode), wrap(many("bindings")?)], many("body")?].concat())
            }
            "clause" => wrap([vec![one("test")?], many("expressions")?].concat()),
            "arrow_clause" => wrap(vec![one("test")?, atom("=>"), one("receiver")?]),
            "else_clause" => wrap([vec![atom("else")], many("expressions")?].concat()),
            "display" if self.harness && node.list("port").is_none_or(<[Expr]>::is_empty) => {
                wrap(vec![atom("display"), one("obj")?, atom(OUTPUT_PORT)])
            }
            opcode => {
                let mut parts = vec![atom(scheme_name(opcode))];
                // What the call hides is not in the node.
                for part in &def.parts {
                    match part {
                        Part::Input(input) if node.arg(&input.name).is_some() => parts.push(one(&input.name)?),
                        Part::List(list) if node.list(&list.name).is_some() => parts.extend(many(&list.name)?),
                        Part::Input(_) | Part::List(_) | Part::Label(_) | Part::Branch(_) => {}
                    }
                }
                wrap(parts)
            }
        })
    }

    fn arg(&self, def: &BlockDef, node: &Node, name: &str) -> Result<Form, String> {
        let Some(expr) = node.arg(name) else {
            return match self.holes {
                true => Ok(hole(name)),
                false => Err(format!("`{}` has no `{name}`", def.name)),
            };
        };
        let ty = def.input(name).map(|input| input.ty.as_str());
        self.expr(expr, ty, name)
    }

    fn list(&self, def: &BlockDef, node: &Node, name: &str) -> Result<Vec<Form>, String> {
        let ty = def.list(name).map(|list| list.ty.as_str());
        let items = node.list(name).unwrap_or_default();
        let mut forms: Vec<Form> = items.iter().map(|item| self.expr(item, ty, name)).collect::<Result<_, _>>()?;
        let min = def.list(name).map_or(0, |list| list.min);
        if self.holes && forms.len() < min {
            forms.resize(min, hole(name));
        }
        Ok(forms)
    }

    fn text(&self, node: &Node, name: &str) -> Result<String, String> {
        match node.arg(name) {
            Some(Expr::Literal(Value::Text(text))) => Ok(string(text)?),
            _ if self.holes => Ok(hole(name).to_string()),
            _ => Err(format!("`{}` needs text in `{name}`", node.opcode)),
        }
    }

    /// `name` is the input or list `expr` fills, for its hole.
    fn expr(&self, expr: &Expr, ty: Option<&str>, name: &str) -> Result<Form, String> {
        match expr {
            Expr::Literal(value) => literal(value, ty),
            Expr::Node(node) => self.node(node),
            Expr::Convert { value, .. } => self.expr(value, ty, name),
            Expr::Problem(problem) if self.holes => match &problem.recovered {
                Some(node) => self.node(node),
                None => Ok(hole(name)),
            },
            Expr::Problem(problem) => Err(problem.message.clone()),
        }
    }

}

pub(crate) fn scheme_name(opcode: &str) -> &str {
    RENAMED.iter().find(|(id, _)| *id == opcode).map_or(opcode, |(_, name)| name)
}

fn literal(value: &Value, ty: Option<&str>) -> Result<Form, String> {
    Ok(Form::Atom(match value {
        Value::Text(text) if ty == Some("string") => string(text)?,
        Value::Text(text) => text.clone(),
        Value::Bool(true) => "#t".into(),
        Value::Bool(false) => "#f".into(),
        Value::Integer(n) | Value::Currency(n) => n.to_string(),
        Value::Unsigned(n) => n.to_string(),
        Value::Float(x) => format!("{x:?}"),
    }))
}

fn hole(name: &str) -> Form {
    Form::Atom(format!("<{name}>"))
}

/// A string literal. Control characters other than newline, tab and return
/// are refused rather than escaped, as interpreters disagree on how.
pub fn string(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => return Err(format!("text holds control character {:?}", c)),
            c => out.push(c),
        }
    }
    out.push('"');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_parse::program::{Input, Program, Reach, Slot, Stack};
    use block_parse::Block;

    struct Builder {
        language: Language,
        program: Program,
    }

    impl Builder {
        fn new() -> Self {
            let language = crate::language();
            let program = Program::new(&language);
            Self { language, program }
        }

        fn block(&mut self, opcode: &str) -> Block {
            self.program.instantiate(&self.language, opcode).unwrap()
        }

        fn code(&mut self, block: Block) -> Result<String, String> {
            let run = self.stack(block);
            script(&self.language, &run)
        }

        fn pretty(&mut self, block: Block) -> Result<String, String> {
            let run = self.stack(block);
            pretty(&self.language, &run, 80, false)
        }

        fn stack(&mut self, block: Block) -> Script {
            let id = block.id;
            self.program.stacks.push(Stack {
                pos: [0.0, 0.0],
                blocks: vec![block],
            });
            self.program.script_at(&self.language, id).unwrap()
        }
    }

    fn set(block: &mut Block, slot: Slot, input: Input) {
        match slot.item {
            None => {
                block.inputs.insert(slot.input, input);
            }
            Some(_) => block.lists.entry(slot.input).or_default().push(input),
        }
    }

    fn text(text: &str) -> Input {
        Input {
            literal: Some(text.into()),
            block: None,
        }
    }

    fn plug(block: Block) -> Input {
        Input {
            literal: None,
            block: Some(Box::new(block)),
        }
    }

    fn item(list: &str) -> Slot {
        Slot::item(list, 0)
    }

    #[test]
    fn plain_blocks_are_their_opcode_applied_in_spec_order() {
        let mut b = Builder::new();
        let mut add = b.block("+");
        set(&mut add, item("z"), text("1"));
        set(&mut add, item("z"), text("2.5"));
        let mut fold = b.block("fold");
        set(&mut fold, Slot::input("kons"), text("+"));
        set(&mut fold, Slot::input("knil"), text("0"));
        set(&mut fold, Slot::input("clist"), plug(add));
        assert_eq!(b.code(fold), Ok("(fold + 0 (+ 1 2.5))".into()));
    }

    #[test]
    fn special_forms_get_their_own_shape() {
        let mut b = Builder::new();
        let mut times = b.block("*");
        set(&mut times, item("z"), text("x"));
        set(&mut times, item("z"), text("x"));
        let mut define = b.block("define_procedure");
        set(&mut define, Slot::input("variable"), text("square"));
        set(&mut define, item("formals"), text("x"));
        set(&mut define, item("body"), plug(times));
        assert_eq!(b.code(define), Ok("(define (square x) (* x x))".into()));

        let mut binding = b.block("binding");
        set(&mut binding, Slot::input("variable"), text("a"));
        set(&mut binding, Slot::input("init"), text("3"));
        let mut display = b.block("display");
        set(&mut display, Slot::input("obj"), text("a"));
        let mut scope = b.block("let");
        set(&mut scope, item("bindings"), plug(binding));
        set(&mut scope, item("body"), plug(display));
        assert_eq!(b.code(scope), Ok(format!("(let ((a 3)) (display a {OUTPUT_PORT}))")));

        let mut call = b.block("call");
        set(&mut call, Slot::input("operator"), text("f"));
        assert_eq!(b.code(call), Ok("(f)".into()));

        let mut pair = b.block("cons");
        set(&mut pair, Slot::input("car"), text("1"));
        let nil = b.block("nil");
        set(&mut pair, Slot::input("cdr"), plug(nil));
        assert_eq!(b.code(pair), Ok("(cons 1 '())".into()));
    }

    #[test]
    fn cond_clauses_are_lists_and_else_goes_out_as_else() {
        let mut b = Builder::new();
        let mut negative = b.block("clause");
        let mut is_negative = b.block("negative?");
        set(&mut is_negative, Slot::input("x"), text("x"));
        set(&mut negative, Slot::input("test"), plug(is_negative));
        set(&mut negative, item("expressions"), text("-1"));
        let mut found = b.block("clause");
        set(&mut found, Slot::input("test"), text("x"));
        let mut passed = b.block("arrow_clause");
        set(&mut passed, Slot::input("test"), text("x"));
        set(&mut passed, Slot::input("receiver"), text("abs"));
        let mut otherwise = b.block("else_clause");
        set(&mut otherwise, item("expressions"), text("0"));
        let mut cond = b.block("cond");
        for clause in [negative, found, passed, otherwise] {
            set(&mut cond, item("clauses"), plug(clause));
        }
        assert_eq!(b.code(cond), Ok("(cond ((negative? x) -1) (x) (x => abs) (else 0))".into()));
    }

    #[test]
    fn letrec_bindings_see_each_other() {
        use block_parse::edit::{Fragment, Target};
        use block_parse::Declaration;

        for opcode in ["letrec", "letrec*"] {
            let mut b = Builder::new();
            let mut scope = b.block(opcode);
            let mut ids = Vec::new();
            for name in ["even", "odd"] {
                let mut binding = b.block("binding");
                set(&mut binding, Slot::input("variable"), text(name));
                ids.push(binding.id);
                set(&mut scope, item("bindings"), plug(binding));
            }
            let scope_id = scope.id;
            b.program.stacks.push(Stack {
                pos: [0.0, 0.0],
                blocks: vec![scope],
            });
            let odd = Declaration {
                block: ids[1],
                slot: Slot::input("variable"),
            };
            for (parent, slot) in [(ids[0], Slot::input("init")), (scope_id, item("body"))] {
                let reference = b.program.reference(&b.language, &odd).unwrap();
                let target = Target::Input { parent, slot };
                b.program.attach(&b.language, Fragment { blocks: vec![reference] }, target).unwrap();
            }
            b.program.set_literal(ids[1], &Slot::input("init"), "1".into());
            let run = b.program.script_at(&b.language, scope_id).unwrap();
            assert_eq!(script(&b.language, &run), Ok(format!("({opcode} ((even odd) (odd 1)) odd)")));
        }
    }

    #[test]
    fn a_reference_goes_out_as_the_name_it_was_given_last() {
        use block_parse::{Declaration, Fragment};
        use block_parse::edit::Target;

        let mut b = Builder::new();
        let mut lambda = b.block("lambda");
        set(&mut lambda, item("formals"), text("x"));
        let (lambda_id, times) = (lambda.id, b.block("*"));
        let times_id = times.id;
        set(&mut lambda, item("body"), plug(times));
        b.program.stacks.push(Stack {
            pos: [0.0, 0.0],
            blocks: vec![lambda],
        });
        let x = Declaration {
            block: lambda_id,
            slot: item("formals"),
        };
        for index in 0..2 {
            let reference = b.program.reference(&b.language, &x).unwrap();
            let target = Target::Input {
                parent: times_id,
                slot: Slot::item("z", index),
            };
            b.program.attach(&b.language, Fragment { blocks: vec![reference] }, target).unwrap();
        }
        assert!(b.program.set_literal(lambda_id, &item("formals"), "n".into()));
        let run = b.program.script_at(&b.language, lambda_id).unwrap();
        assert_eq!(script(&b.language, &run), Ok("(lambda (n) (* n n))".into()));

        let outside = b.program.script_at(&b.language, times_id).unwrap();
        let refused = script(&b.language, &outside).unwrap_err();
        assert!(refused.contains("outside"), "{refused}");
    }

    #[test]
    fn the_output_port_is_shown_only_with_the_harness() {
        let mut b = Builder::new();
        let mut display = b.block("display");
        set(&mut display, Slot::input("obj"), text("a"));
        let run = b.stack(display);
        assert_eq!(script(&b.language, &run), Ok(format!("(display a {OUTPUT_PORT})")), "it always runs with it");
        assert_eq!(flat(&b.language, &run, false), Ok("(display a)".into()));
        assert_eq!(pretty(&b.language, &run, 80, false), Ok("(display a)".into()));
        assert_eq!(pretty(&b.language, &run, 80, true), Ok(format!("(display a {OUTPUT_PORT})")));
    }

    #[test]
    fn renamed_blocks_call_their_scheme_name() {
        let mut b = Builder::new();
        let mut chars = b.block("string_chars");
        set(&mut chars, item("char"), text("#\\a"));
        assert_eq!(b.code(chars), Ok("(string #\\a)".into()));

        let mut map = b.block("string-map");
        let by_name = named(b.block("string_chars"));
        set(&mut map, Slot::input("proc"), plug(by_name));
        set(&mut map, item("strings"), text("s"));
        assert_eq!(b.code(map), Ok("(string-map string s)".into()));
    }

    #[test]
    fn display_to_a_port_keeps_it() {
        let mut b = Builder::new();
        let mut display = b.block("display");
        set(&mut display, Slot::input("obj"), text("a"));
        set(&mut display, item("port"), text("p"));
        assert_eq!(b.code(display), Ok("(display a p)".into()));
    }

    #[test]
    fn string_blocks_are_escaped_whatever_they_hold() {
        let mut b = Builder::new();
        let mut quoted = b.block("string");
        set(&mut quoted, Slot::input("text"), text("say \"hi\") (exit"));
        assert_eq!(b.code(quoted), Ok(r#""say \"hi\") (exit""#.into()));
        assert!(string("bell\u{7}").is_err());
    }

    #[test]
    fn a_script_with_an_error_is_not_generated() {
        let mut b = Builder::new();
        let mut add = b.block("+");
        set(&mut add, item("z"), text("(exit)"));
        let message = b.code(add).unwrap_err();
        assert!(message.contains("item 1 of `z`"), "{message}");

        let minus = b.block("-");
        assert!(b.code(minus).is_err(), "`-` needs an operand");
    }

    fn named(mut block: Block) -> Block {
        block.reach = Some(Reach::Name);
        block
    }

    #[test]
    fn named_blocks_go_out_by_name() {
        let mut b = Builder::new();
        let mut items = b.block("list");
        set(&mut items, item("obj"), text("1"));
        set(&mut items, item("obj"), text("2"));
        let mut fold = b.block("fold");
        let add = named(b.block("+"));
        set(&mut fold, Slot::input("kons"), plug(add));
        let empty = b.block("list");
        set(&mut fold, Slot::input("knil"), plug(empty));
        set(&mut fold, Slot::input("clist"), plug(items));
        assert_eq!(b.code(fold), Ok("(fold + (list) (list 1 2))".into()), "a round block is called");

        let mut call = b.block("call");
        let minus = named(b.block("-"));
        set(&mut call, Slot::input("operator"), plug(minus));
        set(&mut call, item("operands"), text("5"));
        assert_eq!(b.code(call), Ok("(- 5)".into()), "a name needs no items");

        let mut items = b.block("list");
        let car = named(b.block("car"));
        set(&mut items, item("obj"), plug(car));
        assert_eq!(b.code(items), Ok("(list car)".into()), "in any slot");
    }

    #[test]
    fn an_empty_procedure_dropped_in_a_procedure_slot_is_named() {
        use block_parse::edit::{Fragment, Target};

        let mut b = Builder::new();
        let map = b.block("map");
        let map_id = map.id;
        b.program.stacks.push(Stack {
            pos: [0.0, 0.0],
            blocks: vec![map],
        });
        let car = b.block("car");
        let target = Target::Input {
            parent: map_id,
            slot: Slot::input("proc"),
        };
        b.program.attach(&b.language, Fragment { blocks: vec![car] }, target).unwrap();
        b.program.set_literal(map_id, &item("lists"), "xs".into());
        let run = b.program.script_at(&b.language, map_id).unwrap();
        assert_eq!(script(&b.language, &run), Ok("(map car xs)".into()));
    }

    #[test]
    fn a_round_block_in_a_procedure_slot_is_called_for_its_procedure() {
        let mut b = Builder::new();
        let mut adder = b.block("+");
        set(&mut adder, item("z"), text("1"));
        let mut call = b.block("call");
        set(&mut call, Slot::input("operator"), plug(adder));
        assert_eq!(b.code(call), Ok("((+ 1))".into()));

        let mut apply = b.block("apply");
        let lambda = b.block("lambda");
        set(&mut apply, Slot::input("proc"), plug(lambda));
        set(&mut apply, item("args"), text("xs"));
        assert!(b.code(apply).is_err(), "an empty lambda is no name");
    }

    #[test]
    fn a_call_hiding_a_parameter_is_refused_but_inspected() {
        let mut b = Builder::new();
        let mut fold = b.block("fold");
        set(&mut fold, Slot::input("kons"), text("+"));
        fold.reach = Some(Reach::Call(1));
        let refused = b.code(fold.clone()).unwrap_err();
        assert!(refused.contains("without `knil`"), "{refused}");
        assert_eq!(b.pretty(fold), Ok("(fold +)".into()));
    }

    #[test]
    fn a_procedure_reference_calls_its_procedure_with_an_argument_per_parameter() {
        let mut b = Builder::new();
        let mut define = b.block("define_procedure");
        set(&mut define, Slot::input("variable"), text("square"));
        set(&mut define, item("formals"), text("x"));
        set(&mut define, item("body"), text("x"));
        let define_id = define.id;
        b.program.stacks.push(Stack {
            pos: [0.0, 0.0],
            blocks: vec![define],
        });
        let name = block_parse::Declaration {
            block: define_id,
            slot: Slot::input("variable"),
        };
        let mut square = |reach: Option<Reach>, argument: Option<&str>| {
            let mut square = b.program.reference(&b.language, &name).unwrap();
            assert_eq!(square.opcode, "procedure_call");
            square.reach = reach;
            if let Some(argument) = argument {
                set(&mut square, item("arguments"), text(argument));
            }
            b.pretty(square)
        };
        assert_eq!(square(None, Some("3")), Ok("(square 3)".into()));
        assert_eq!(square(None, None), Ok("(square <arguments>)".into()), "an argument per parameter");
        assert_eq!(square(Some(Reach::Call(0)), None), Ok("(square)".into()), "refused when run, as Scheme would");
        assert_eq!(square(Some(Reach::Name), None), Ok("square".into()));
    }

    #[test]
    fn inspecting_writes_faulty_inputs_as_their_names() {
        let mut b = Builder::new();
        let mut choose = b.block("if");
        set(&mut choose, Slot::input("consequent"), text("yes"));
        let minus = b.block("-");
        set(&mut choose, Slot::input("alternate"), plug(minus));
        assert_eq!(b.pretty(choose), Ok("(if <test> yes (- <z>))".into()));

        let mut add = b.block("+");
        set(&mut add, item("z"), text("(exit)"));
        assert_eq!(b.pretty(add.clone()), Ok("(+ <z>)".into()));
        assert!(b.code(add).is_err(), "running still refuses it");
    }
}
