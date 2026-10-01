//! Block spec strings: `{name:type=default}` is an input, `{name:type*}` or
//! `{name:type+}` a list, `[name]` a branch, everything else label text, faint
//! for words wrapped in underscores.

use crate::language::Label;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SpecPart {
    Label(Label),
    Input {
        name: String,
        ty: String,
        default: Option<String>,
        list: Option<Arity>,
    },
    Branch(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arity {
    /// `*`
    Any,
    /// `+`
    AtLeastOne,
}

pub(crate) fn parse(spec: &str) -> Result<Vec<SpecPart>, String> {
    let mut parts = Vec::new();
    let mut label = String::new();
    let mut chars = spec.chars();

    while let Some(c) = chars.next() {
        let close = match c {
            '{' => '}',
            '[' => ']',
            '}' | ']' => return Err(format!("unmatched `{c}`")),
            _ => {
                label.push(c);
                continue;
            }
        };
        push_label(&mut parts, &mut label);

        let mut inner = String::new();
        loop {
            match chars.next() {
                Some(end) if end == close => break,
                Some('{' | '[') => return Err(format!("`{c}` opened inside `{c}`")),
                Some(other) => inner.push(other),
                None => return Err(format!("`{c}` is never closed")),
            }
        }

        if c == '[' {
            parts.push(SpecPart::Branch(inner.trim().to_owned()));
            continue;
        }
        let Some((name, rest)) = inner.split_once(':') else {
            return Err(format!("input `{{{inner}}}` needs a type, as `{{name:type}}`"));
        };
        let (ty, default) = match rest.split_once('=') {
            Some((ty, default)) => (ty.trim(), Some(default.trim().to_owned())),
            None => (rest.trim(), None),
        };
        let (ty, list) = if let Some(ty) = ty.strip_suffix('*') {
            (ty, Some(Arity::Any))
        } else if let Some(ty) = ty.strip_suffix('+') {
            (ty, Some(Arity::AtLeastOne))
        } else {
            (ty, None)
        };
        parts.push(SpecPart::Input {
            name: name.trim().to_owned(),
            ty: ty.trim().to_owned(),
            default,
            list,
        });
    }
    push_label(&mut parts, &mut label);
    Ok(parts)
}

/// One label per run of plain or faint words, whitespace collapsed so labels
/// lay out the same however the spec is spaced.
fn push_label(parts: &mut Vec<SpecPart>, label: &mut String) {
    for word in label.split_whitespace() {
        let (text, faint) = match word.strip_prefix('_').and_then(|word| word.strip_suffix('_')) {
            Some(inner) if !inner.is_empty() => (inner, true),
            _ => (word, false),
        };
        match parts.last_mut() {
            Some(SpecPart::Label(last)) if last.faint == faint => {
                last.text.push(' ');
                last.text.push_str(text);
            }
            _ => parts.push(SpecPart::Label(Label {
                text: text.to_owned(),
                faint,
            })),
        }
    }
    label.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> SpecPart {
        SpecPart::Label(Label {
            text: text.into(),
            faint: false,
        })
    }

    fn faint(text: &str) -> SpecPart {
        SpecPart::Label(Label {
            text: text.into(),
            faint: true,
        })
    }

    fn input(name: &str, ty: &str, default: Option<&str>) -> SpecPart {
        SpecPart::Input {
            name: name.into(),
            ty: ty.into(),
            default: default.map(Into::into),
            list: None,
        }
    }

    fn list(name: &str, ty: &str, list: Arity) -> SpecPart {
        SpecPart::Input {
            name: name.into(),
            ty: ty.into(),
            default: None,
            list: Some(list),
        }
    }

    #[test]
    fn splits_labels_inputs_and_branches() {
        assert_eq!(
            parse("if {cond:bool} then [then]  else [else]").unwrap(),
            vec![
                plain("if"),
                input("cond", "bool", None),
                plain("then"),
                SpecPart::Branch("then".into()),
                plain("else"),
                SpecPart::Branch("else".into()),
            ]
        );
    }

    #[test]
    fn words_in_underscores_are_faint() {
        assert_eq!(
            parse("define {name:symbol} _taking_ {x:symbol} _and_ _then_ x_ _ __").unwrap(),
            vec![
                plain("define"),
                input("name", "symbol", None),
                faint("taking"),
                input("x", "symbol", None),
                faint("and then"),
                plain("x_ _ __"),
            ]
        );
    }

    #[test]
    fn defaults_may_hold_commas_and_spaces() {
        assert_eq!(
            parse("print {value:text=Hello, world!}").unwrap(),
            vec![
                plain("print"),
                input("value", "text", Some("Hello, world!")),
            ]
        );
    }

    #[test]
    fn a_colon_after_the_first_stays_in_the_type() {
        // So `{a:b:c}` is refused later as an unknown type, not misread.
        assert_eq!(parse("{a:b:c}").unwrap(), vec![input("a", "b:c", None)]);
    }

    #[test]
    fn a_star_or_plus_after_the_type_makes_a_list() {
        assert_eq!(
            parse("{f:datum} {args:datum*} {rest : datum + }").unwrap(),
            vec![
                input("f", "datum", None),
                list("args", "datum", Arity::Any),
                list("rest", "datum", Arity::AtLeastOne),
            ]
        );
    }

    #[test]
    fn malformed_specs_are_refused() {
        for spec in ["{a:number", "[body", "a}", "{a}", "{a:{b:c}}", "[a[b]]"] {
            assert!(parse(spec).is_err(), "{spec:?}");
        }
    }
}
