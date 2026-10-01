//! AST to Scheme text: the layer between untrusted blocks and the
//! interpreter. Only blocks the language offers get this far, as anything
//! else is a problem in the AST. Strings are escaped here; datum and symbol
//! literals were checked by [`literals`](crate::literals) and go out as typed.

use block_parse::ast::{Ast, Expr, Node, Script, Severity, Stmt};
use block_parse::language::{BlockDef, Part};
use block_parse::{Language, Value};

/// Where `display` writes, so the runner can show it.
pub const OUTPUT_PORT: &str = "__out";

/// One expression per statement of `script`. `Err` holds the first problem,
/// as a script with any error-level problem is not run.
pub fn script(language: &Language, script: &Script) -> Result<String, String> {
    let ast = Ast {
        scripts: vec![script.clone()],
    };
    if let Some(problem) = ast.problems().into_iter().find(|problem| problem.severity == Severity::Error) {
        return Err(problem.message.clone());
    }
    let mut forms = Vec::new();
    for statement in &script.body {
        let node = match statement {
            Stmt::Node(node) => node,
            // Errors were refused above; a warning keeps what it recovered.
            Stmt::Problem(problem) => match &problem.recovered {
                Some(node) => node,
                None => continue,
            },
        };
        forms.push(Generator { language }.node(node)?);
    }
    Ok(forms.join("\n"))
}

struct Generator<'a> {
    language: &'a Language,
}

impl Generator<'_> {
    fn node(&self, node: &Node) -> Result<String, String> {
        let def = self
            .language
            .block(&node.opcode)
            .ok_or_else(|| format!("no block `{}`", node.opcode))?;
        let one = |name: &str| self.arg(def, node, name);
        let many = |name: &str| self.list(def, node, name);
        let wrap = |parts: Vec<String>| format!("({})", parts.join(" "));
        Ok(match node.opcode.as_str() {
            "program" => one("main")?,
            "string" => string(&self.text(node, "text")?)?,
            "variable" => one("name")?,
            "call" => wrap([vec![one("procedure")?], many("args")?].concat()),
            "binding" => wrap(vec![one("name")?, one("value")?]),
            "define_procedure" => {
                let head = wrap([vec![one("name")?], many("params")?].concat());
                wrap([vec!["define".into(), head], many("body")?].concat())
            }
            "lambda" => wrap([vec!["lambda".into(), wrap(many("params")?)], many("body")?].concat()),
            "let" => wrap([vec!["let".into(), wrap(many("bindings")?)], many("body")?].concat()),
            "display" => wrap(vec!["display".into(), one("value")?, OUTPUT_PORT.into()]),
            opcode => {
                let mut parts = vec![opcode.to_owned()];
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

    fn arg(&self, def: &BlockDef, node: &Node, name: &str) -> Result<String, String> {
        let expr = node.arg(name).ok_or_else(|| format!("`{}` has no `{name}`", def.name))?;
        let ty = def.input(name).map(|input| input.ty.as_str());
        self.expr(expr, ty)
    }

    fn list(&self, def: &BlockDef, node: &Node, name: &str) -> Result<Vec<String>, String> {
        let ty = def.list(name).map(|list| list.ty.as_str());
        let items = node.list(name).unwrap_or_default();
        items.iter().map(|item| self.expr(item, ty)).collect()
    }

    fn text(&self, node: &Node, name: &str) -> Result<String, String> {
        match node.arg(name) {
            Some(Expr::Literal(Value::Text(text))) => Ok(text.clone()),
            _ => Err(format!("`{}` needs text in `{name}`", node.opcode)),
        }
    }

    fn expr(&self, expr: &Expr, ty: Option<&str>) -> Result<String, String> {
        match expr {
            Expr::Literal(value) => literal(value, ty),
            Expr::Node(node) => self.node(node),
            Expr::Convert { value, .. } => self.expr(value, ty),
            Expr::Problem(problem) => Err(problem.message.clone()),
        }
    }
}

fn literal(value: &Value, ty: Option<&str>) -> Result<String, String> {
    Ok(match value {
        Value::Text(text) if ty == Some("string") => string(text)?,
        Value::Text(text) => text.clone(),
        Value::Bool(true) => "#t".into(),
        Value::Bool(false) => "#f".into(),
        Value::Integer(n) | Value::Currency(n) => n.to_string(),
        Value::Unsigned(n) => n.to_string(),
        Value::Float(x) => format!("{x:?}"),
    })
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
            let id = block.id;
            self.program.stacks.push(Stack {
                pos: [0.0, 0.0],
                blocks: vec![block],
            });
            let run = self.program.script_at(&self.language, id).unwrap();
            script(&self.language, &run)
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
        set(&mut add, item("args"), text("1"));
        set(&mut add, item("args"), text("2.5"));
        let mut reduce = b.block("reduce");
        set(&mut reduce, Slot::input("procedure"), text("+"));
        set(&mut reduce, Slot::input("initial"), text("0"));
        set(&mut reduce, Slot::input("list"), plug(add));
        assert_eq!(b.code(reduce), Ok("(reduce + 0 (+ 1 2.5))".into()));
    }

    #[test]
    fn special_forms_get_their_own_shape() {
        let mut b = Builder::new();
        let mut times = b.block("*");
        set(&mut times, item("args"), text("x"));
        set(&mut times, item("args"), text("x"));
        let mut define = b.block("define_procedure");
        set(&mut define, Slot::input("name"), text("square"));
        set(&mut define, item("params"), text("x"));
        set(&mut define, item("body"), plug(times));
        assert_eq!(b.code(define), Ok("(define (square x) (* x x))".into()));

        let mut binding = b.block("binding");
        set(&mut binding, Slot::input("name"), text("a"));
        set(&mut binding, Slot::input("value"), text("3"));
        let mut display = b.block("display");
        set(&mut display, Slot::input("value"), text("a"));
        let mut scope = b.block("let");
        set(&mut scope, item("bindings"), plug(binding));
        set(&mut scope, item("body"), plug(display));
        assert_eq!(b.code(scope), Ok(format!("(let ((a 3)) (display a {OUTPUT_PORT}))")));

        let mut call = b.block("call");
        set(&mut call, Slot::input("procedure"), text("f"));
        assert_eq!(b.code(call), Ok("(f)".into()));
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
        set(&mut add, item("args"), text("(exit)"));
        let message = b.code(add).unwrap_err();
        assert!(message.contains("item 1 of `args`"), "{message}");

        let minus = b.block("-");
        assert!(b.code(minus).is_err(), "`-` needs an operand");
    }
}
