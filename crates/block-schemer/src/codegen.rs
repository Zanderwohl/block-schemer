//! AST to Scheme text: the layer between untrusted blocks and the
//! interpreter. Only blocks the language offers get this far, as anything
//! else is a problem in the AST. Strings are escaped here; datum and symbol
//! literals were checked by [`literals`](crate::literals) and go out as typed.

use block_parse::ast::{Expr, Node, ProblemCode, Script, Stmt};
use block_parse::language::{BlockDef, Part};
use block_parse::{Language, Value};

use crate::form::Form;

/// The slot type whose bare procedure blocks are passed by name.
const PROCEDURE: &str = "procedure";

/// Blocks that are syntax, not procedures, so never passed by name.
const SYNTAX: [&str; 12] = [
    "program", "define", "define_procedure", "variable", "quote", "string", "call", "lambda", "if", "let", "binding",
    "begin",
];

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

/// Problems are refused as generation reaches them rather than up front, as
/// a procedure passed by name is a problem to the AST (`-` with no operand).
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
        Ok(match node.opcode.as_str() {
            "program" => one("main")?,
            "string" => Form::Atom(self.text(node, "text")?),
            "variable" => one("variable")?,
            "call" => wrap([vec![one("operator")?], many("operands")?].concat()),
            "binding" => wrap(vec![one("variable")?, one("init")?]),
            "define_procedure" => {
                let head = wrap([vec![one("variable")?], many("formals")?].concat());
                wrap([vec![atom("define"), head], many("body")?].concat())
            }
            "lambda" => wrap([vec![atom("lambda"), wrap(many("formals")?)], many("body")?].concat()),
            "let" => wrap([vec![atom("let"), wrap(many("bindings")?)], many("body")?].concat()),
            "display" if self.harness => wrap(vec![atom("display"), one("obj")?, atom(OUTPUT_PORT)]),
            "display" => wrap(vec![atom("display"), one("obj")?]),
            opcode => {
                let mut parts = vec![atom(opcode)];
                for part in &def.parts {
                    match part {
                        Part::Input(input) => parts.push(one(&input.name)?),
                        Part::List(list) => parts.extend(many(&list.name)?),
                        Part::Label(_) | Part::Branch(_) => {}
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
        let by_name = ty == Some(PROCEDURE);
        match expr {
            Expr::Literal(value) => literal(value, ty),
            Expr::Node(node) if by_name && let Some(form) = self.reference(node) => Ok(form),
            Expr::Problem(problem)
                if by_name
                    && problem.code == ProblemCode::TooFewItems
                    && let Some(form) = problem.recovered.as_deref().and_then(|node| self.reference(node)) =>
            {
                Ok(form)
            }
            Expr::Node(node) => self.node(node),
            Expr::Convert { value, .. } => self.expr(value, ty, name),
            Expr::Problem(problem) if self.holes => match &problem.recovered {
                Some(node) => self.node(node),
                None => Ok(hole(name)),
            },
            Expr::Problem(problem) => Err(problem.message.clone()),
        }
    }

    /// The procedure's name, if `node` is a procedure block with every input
    /// blank and every list empty.
    fn reference(&self, node: &Node) -> Option<Form> {
        let def = self.language.block(&node.opcode)?;
        let blank = |expr: &Expr| {
            matches!(expr, Expr::Problem(problem) if problem.block.is_none() && problem.recovered.is_none())
        };
        let bare = !SYNTAX.contains(&node.opcode.as_str())
            && def.inputs().all(|input| node.arg(&input.name).is_none_or(blank))
            && def.lists().all(|list| node.list(&list.name).is_none_or(<[Expr]>::is_empty));
        bare.then(|| Form::atom(&node.opcode))
    }
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
    use block_parse::program::{Input, Program, Slot, Stack};
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

    #[test]
    fn bare_procedure_blocks_in_procedure_slots_are_passed_by_name() {
        let mut b = Builder::new();
        let mut items = b.block("list");
        set(&mut items, item("obj"), text("1"));
        set(&mut items, item("obj"), text("2"));
        let mut fold = b.block("fold");
        let add = b.block("+");
        set(&mut fold, Slot::input("kons"), plug(add));
        let empty = b.block("list");
        set(&mut fold, Slot::input("knil"), plug(empty));
        set(&mut fold, Slot::input("clist"), plug(items));
        assert_eq!(b.code(fold), Ok("(fold + (list) (list 1 2))".into()), "only the procedure slot passes by name");

        let mut map = b.block("map");
        let car = b.block("car");
        set(&mut map, Slot::input("proc"), plug(car));
        set(&mut map, item("lists"), text("xs"));
        assert_eq!(b.code(map), Ok("(map car xs)".into()), "a blank input counts as bare");

        let mut call = b.block("call");
        let minus = b.block("-");
        set(&mut call, Slot::input("operator"), plug(minus));
        set(&mut call, item("operands"), text("5"));
        assert_eq!(b.code(call), Ok("(- 5)".into()), "too few items is no problem by name");
    }

    #[test]
    fn filled_or_syntax_blocks_in_procedure_slots_are_still_generated() {
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
        assert!(b.code(apply).is_err(), "an empty lambda is not a name");
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
