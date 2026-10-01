//! Generated Scheme as a tree, so it can be printed on one line to run or
//! laid out to read.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Form {
    /// Printed as is: a symbol, a number, or an already escaped string.
    Atom(String),
    List(Vec<Form>),
}

impl Form {
    pub fn atom(text: &str) -> Self {
        Self::Atom(text.to_owned())
    }

    /// Lines of at most `width` columns where breaking can manage it.
    pub fn pretty(&self, width: usize) -> String {
        let mut out = String::new();
        self.layout(width, 0, &mut out);
        out
    }

    /// Lays out at the end of `out`. `closers` is how many parentheses will
    /// follow on the same line, which count against the width too.
    fn layout(&self, width: usize, closers: usize, out: &mut String) {
        let column = last_line_len(out);
        let flat = self.to_string();
        let Self::List(items) = self else {
            out.push_str(&flat);
            return;
        };
        if column + flat.chars().count() + closers <= width && !self.always_breaks() {
            out.push_str(&flat);
            return;
        }
        let Some((head, args)) = items.split_first() else {
            out.push_str("()");
            return;
        };
        // Arguments past `same_line` start lines at `under`.
        let (same_line, under) = match (self.body_start(), head) {
            // `(define (f x)` then the body indented two.
            (Some(start), _) => (start, column + 2),
            // `(f a` with the rest under `a`, while the head is short enough
            // that this does not push everything to the right.
            (None, Self::Atom(name)) if name.chars().count() <= ALIGN_UNDER_FIRST_UP_TO => {
                (1, column + name.chars().count() + 2)
            }
            (None, Self::Atom(_)) => (0, column + 2),
            // A list of lists, such as `let`'s bindings.
            (None, Self::List(_)) => (0, column + 1),
        };
        let closing = |index: usize| if index + 1 == args.len() { closers + 1 } else { 0 };
        out.push('(');
        head.layout(width, if args.is_empty() { closers + 1 } else { 0 }, out);
        for (index, form) in args.iter().enumerate() {
            if index < same_line {
                out.push(' ');
            } else {
                newline(out, under);
            }
            form.layout(width, closing(index), out);
        }
        out.push(')');
    }

    /// For forms with a body, how many arguments stay on the head's line.
    fn body_start(&self) -> Option<usize> {
        let Self::List(items) = self else { return None };
        match items.first() {
            Some(Self::Atom(head)) => match head.as_str() {
                "define" | "lambda" | "let" | "let*" | "letrec" | "when" | "unless" => Some(1),
                "begin" => Some(0),
                _ => None,
            },
            _ => None,
        }
    }

    fn always_breaks(&self) -> bool {
        match (self, self.body_start()) {
            (Self::List(items), Some(start)) => items.len() > start + 2,
            _ => false,
        }
    }
}

/// Head atoms longer than this put their arguments on lines of their own.
const ALIGN_UNDER_FIRST_UP_TO: usize = 10;

impl fmt::Display for Form {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Atom(text) => f.write_str(text),
            Self::List(items) => {
                f.write_str("(")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str(")")
            }
        }
    }
}

fn newline(out: &mut String, indent: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n(' ', indent));
}

fn last_line_len(out: &str) -> usize {
    out.rsplit('\n').next().map_or(0, |line| line.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Form {
        fn list(tokens: &mut std::iter::Peekable<std::vec::IntoIter<String>>) -> Form {
            let mut items = Vec::new();
            while let Some(token) = tokens.next() {
                match token.as_str() {
                    "(" => items.push(list(tokens)),
                    ")" => break,
                    _ => items.push(Form::Atom(token)),
                }
            }
            Form::List(items)
        }
        let spaced = text.replace('(', " ( ").replace(')', " ) ");
        let tokens: Vec<String> = spaced.split_whitespace().map(str::to_owned).collect();
        let mut tokens = tokens.into_iter().peekable();
        tokens.next();
        list(&mut tokens)
    }

    #[test]
    fn what_fits_stays_on_one_line() {
        let form = read("(define (square x) (* x x))");
        assert_eq!(form.pretty(60), "(define (square x) (* x x))");
    }

    #[test]
    fn several_body_forms_go_one_to_a_line() {
        let form = read("(let ((a 3) (b 4)) (display a) (+ a b))");
        assert_eq!(form.pretty(60), "(let ((a 3) (b 4))\n  (display a)\n  (+ a b))");
    }

    #[test]
    fn calls_too_wide_align_under_their_first_argument() {
        let form = read("(define (sum-of-squares xs) (fold + 0 (map (lambda (x) (* x x)) xs)))");
        assert_eq!(
            form.pretty(38),
            "(define (sum-of-squares xs)\n  (fold +\n        0\n        \
             (map (lambda (x) (* x x))\n             xs)))"
        );
    }

    #[test]
    fn bindings_too_wide_stack_under_each_other() {
        let form = read("(let ((first-number 3) (second-number 4)) (+ first-number second-number))");
        assert_eq!(
            form.pretty(30),
            "(let ((first-number 3)\n      (second-number 4))\n  (+ first-number\n     second-number))"
        );
    }

    #[test]
    fn long_names_put_their_arguments_on_lines_of_their_own() {
        let form = read("(call-with-current-continuation (lambda (k) (k 1)))");
        assert_eq!(form.pretty(30), "(call-with-current-continuation
  (lambda (k) (k 1)))");
    }
}
