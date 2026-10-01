//! What may be typed into a slot. Text that passes is emitted as typed, so
//! these are the code generator's guard as much as the user's help.

use block_parse::{LiteralValidator, Value};

/// A number, boolean, character, string in double quotes, or symbol.
#[derive(Debug)]
pub struct Datum;

/// An identifier, as a variable or parameter name.
#[derive(Debug)]
pub struct Symbol;

/// Names the generated code uses for itself.
pub const RESERVED_PREFIX: &str = "__";

impl LiteralValidator for Datum {
    fn validate(&self, text: &str) -> Result<Value, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("enter a value".into());
        }
        let ok = is_boolean(text) || is_character(text) || is_string(text) || is_number(text) || is_symbol(text);
        if ok {
            Ok(Value::Text(text.to_owned()))
        } else {
            Err("not a number, boolean, character, string or symbol".into())
        }
    }

    fn normalize(&self, text: &str) -> String {
        text.trim().to_owned()
    }
}

impl LiteralValidator for Symbol {
    fn validate(&self, text: &str) -> Result<Value, String> {
        let text = text.trim();
        if is_symbol(text) {
            Ok(Value::Text(text.to_owned()))
        } else if text.starts_with(RESERVED_PREFIX) {
            Err(format!("names starting `{RESERVED_PREFIX}` are reserved"))
        } else {
            Err("not a name".into())
        }
    }

    fn normalize(&self, text: &str) -> String {
        text.trim().to_owned()
    }
}

fn is_boolean(text: &str) -> bool {
    matches!(text, "#t" | "#f" | "#true" | "#false")
}

const CHARACTER_NAMES: [&str; 6] = ["space", "newline", "tab", "return", "null", "alarm"];

fn is_character(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("#\\") else {
        return false;
    };
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => !c.is_control(),
        _ => CHARACTER_NAMES.contains(&rest),
    }
}

/// Only the escapes `codegen::string` writes, so nothing else can slip out.
fn is_string(text: &str) -> bool {
    let Some(inner) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) else {
        return false;
    };
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !matches!(chars.next(), Some('\\' | '"' | 'n' | 't' | 'r')) => return false,
            '"' => return false,
            c if c.is_control() => return false,
            _ => {}
        }
    }
    true
}

/// Decimal with optional fraction and exponent, a ratio, or `#x`/`#b`/`#o`
/// digits, optionally after `#e` or `#i`.
fn is_number(text: &str) -> bool {
    let text = text
        .strip_prefix("#e")
        .or_else(|| text.strip_prefix("#i"))
        .unwrap_or(text);
    for (prefix, radix) in [("#x", 16), ("#b", 2), ("#o", 8), ("#d", 10)] {
        if let Some(digits) = text.strip_prefix(prefix) {
            let digits = digits.strip_prefix(['+', '-']).unwrap_or(digits);
            return !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix));
        }
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some((numerator, denominator)) = unsigned.split_once('/') {
        return digits(numerator) && digits(denominator);
    }
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            (whole.is_empty() || digits(whole)) && (fraction.is_empty() || digits(fraction)) && mantissa != "."
        }
        None => digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|exponent| digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent)));
    mantissa_ok && exponent_ok
}

/// R7RS identifiers without `|…|` quoting: no delimiters, `#`, quotes or
/// backslashes can appear, and the reserved prefix is refused.
fn is_symbol(text: &str) -> bool {
    const SPECIAL: &str = "!$%&*/:<=>?^_~+-.@";
    let Some(first) = text.chars().next() else {
        return false;
    };
    if text.starts_with(RESERVED_PREFIX) || is_number(text) {
        return false;
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || SPECIAL.contains(c);
    if !text.chars().all(allowed) || first.is_ascii_digit() {
        return false;
    }
    // `.` starts only `...`, and a sign is followed by no digit.
    match first {
        '.' => text == "...",
        '+' | '-' => !text[1..].starts_with(|c: char| c.is_ascii_digit()),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_are_numbers_booleans_characters_strings_or_symbols() {
        for good in [
            "42", "-7", "+3", "1.5", ".5", "1.", "1e10", "-2.5E-3", "1/3", "#x1F", "#b101", "#e1.5", "#t", "#false",
            "#\\a", "#\\space", "\"hi\"", "\"say \\\"hi\\\"\\n\"", "x", "list->vector", "+", "-", "...", "<=?", "a.b",
        ] {
            assert!(Datum.validate(good).is_ok(), "{good}");
        }
        for bad in [
            "", "(", "a b", "(+ 1 2)", "'x", "\"open", "\"a\"b\"", "\"\\q\"", "#\\", "#\\ab", "1a", ".a", "#x1G",
            "x;comment", "|odd|", "a\"b", "__secret", "\"tab\there\"",
        ] {
            assert!(Datum.validate(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn symbols_are_names_only() {
        assert_eq!(Symbol.validate(" sum-of-squares "), Ok(Value::Text("sum-of-squares".into())));
        for bad in ["1", "\"x\"", "#t", "a b", "__out", ""] {
            assert!(Symbol.validate(bad).is_err(), "{bad:?}");
        }
        assert!(Symbol.validate("__out").unwrap_err().contains("reserved"));
    }
}
