//! Block spec strings: `{name:type=default}` is an input, `[name]` a branch,
//! everything else label text.

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SpecPart {
    Label(String),
    Input {
        name: String,
        ty: String,
        default: Option<String>,
    },
    Branch(String),
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
            Some((ty, default)) => (ty, Some(default.trim().to_owned())),
            None => (rest, None),
        };
        parts.push(SpecPart::Input {
            name: name.trim().to_owned(),
            ty: ty.trim().to_owned(),
            default,
        });
    }
    push_label(&mut parts, &mut label);
    Ok(parts)
}

/// Collapses whitespace so labels lay out the same however the spec is spaced.
fn push_label(parts: &mut Vec<SpecPart>, label: &mut String) {
    let words: Vec<&str> = label.split_whitespace().collect();
    if !words.is_empty() {
        parts.push(SpecPart::Label(words.join(" ")));
    }
    label.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, ty: &str, default: Option<&str>) -> SpecPart {
        SpecPart::Input {
            name: name.into(),
            ty: ty.into(),
            default: default.map(Into::into),
        }
    }

    #[test]
    fn splits_labels_inputs_and_branches() {
        assert_eq!(
            parse("if {cond:bool} then [then]  else [else]").unwrap(),
            vec![
                SpecPart::Label("if".into()),
                input("cond", "bool", None),
                SpecPart::Label("then".into()),
                SpecPart::Branch("then".into()),
                SpecPart::Label("else".into()),
                SpecPart::Branch("else".into()),
            ]
        );
    }

    #[test]
    fn defaults_may_hold_commas_and_spaces() {
        assert_eq!(
            parse("print {value:text=Hello, world!}").unwrap(),
            vec![
                SpecPart::Label("print".into()),
                input("value", "text", Some("Hello, world!")),
            ]
        );
    }

    #[test]
    fn a_colon_after_the_first_stays_in_the_type() {
        // So `{a:b:c}` is refused later as a bad type name, not misread.
        assert_eq!(parse("{a:b:c}").unwrap(), vec![input("a", "b:c", None)]);
    }

    #[test]
    fn malformed_specs_are_refused() {
        for spec in ["{a:number", "[body", "a}", "{a}", "{a:{b:c}}", "[a[b]]"] {
            assert!(parse(spec).is_err(), "{spec:?}");
        }
    }
}
