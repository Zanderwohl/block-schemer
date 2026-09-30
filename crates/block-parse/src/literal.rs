//! Checking what the user types into a slot.
//!
//! Literals are stored as typed and parsed only when needed, so an invalid one
//! stays in the program to be fixed and is shown as a problem, rather than
//! being refused or quietly rewritten. Numeric kinds ignore surrounding
//! whitespace.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use crate::language::LiteralKind;
use crate::value::Value;

/// Consumer-supplied parsing for a `LiteralKind::Custom` type. Called whenever
/// the UI chooses to validate, so it should be cheap.
pub trait LiteralValidator: Debug + Send + Sync {
    /// `Err` is a short message for the tag under the slot.
    fn validate(&self, text: &str) -> Result<Value, String>;
}

/// Validators by the name `LiteralKind::Custom` refers to them by.
#[derive(Debug, Clone, Default)]
pub struct Validators(HashMap<String, Arc<dyn LiteralValidator>>);

impl Validators {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, validator: Arc<dyn LiteralValidator>) {
        self.0.insert(name.into(), validator);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn LiteralValidator>> {
        self.0.get(name)
    }
}

pub fn parse(kind: &LiteralKind, text: &str, validators: &Validators) -> Result<Value, String> {
    match kind {
        LiteralKind::None => Err("takes no typed value".into()),
        LiteralKind::Float => float(text.trim()).map(Value::Float),
        LiteralKind::Integer => integer(text.trim()).map(Value::Integer),
        LiteralKind::Number => number(text.trim()),
        LiteralKind::Currency => currency(text.trim()).map(Value::Currency),
        LiteralKind::Binary => {
            let text = text.trim();
            let digits = text
                .strip_prefix("0b")
                .or_else(|| text.strip_prefix("0B"))
                .unwrap_or(text);
            radix(digits, 2, "binary").map(Value::Unsigned)
        }
        LiteralKind::Hex => {
            let text = text.trim();
            let digits = ["0x", "0X", "#"]
                .iter()
                .find_map(|prefix| text.strip_prefix(prefix))
                .unwrap_or(text);
            radix(digits, 16, "hex").map(Value::Unsigned)
        }
        LiteralKind::Text => Ok(Value::Text(text.to_owned())),
        LiteralKind::Bool => match text {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("expected true or false".into()),
        },
        LiteralKind::Choice(options) => {
            if options.iter().any(|option| option == text) {
                Ok(Value::Text(text.to_owned()))
            } else {
                Err(format!("expected one of: {}", options.join(", ")))
            }
        }
        LiteralKind::Custom(name) => match validators.get(name) {
            Some(validator) => validator.validate(text),
            None => Err(format!("no validator named `{name}`")),
        },
    }
}

/// What an untouched slot of this kind holds when the spec gives no default.
pub fn blank(kind: &LiteralKind) -> Option<String> {
    match kind {
        LiteralKind::None => None,
        // A checkbox has no empty state.
        LiteralKind::Bool => Some("false".into()),
        LiteralKind::Choice(options) => Some(options.first().cloned().unwrap_or_default()),
        _ => Some(String::new()),
    }
}

fn float(text: &str) -> Result<f64, String> {
    if text.is_empty() {
        return Err("enter a number".into());
    }
    // Rust accepts `inf` and `NaN` and overflows `1e999` to infinity; all
    // three are refused by the finiteness check.
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(value),
        Ok(_) => Err("number is out of range".into()),
        Err(_) => Err("not a number".into()),
    }
}

fn integer(text: &str) -> Result<i64, String> {
    if text.is_empty() {
        return Err("enter a whole number".into());
    }
    text.parse::<i64>().map_err(|error| match error.kind() {
        std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
            "whole number is out of range".into()
        }
        _ => "not a whole number".into(),
    })
}

fn number(text: &str) -> Result<Value, String> {
    if !text.contains(['.', 'e', 'E'])
        && let Ok(value) = text.parse::<i64>()
    {
        return Ok(Value::Integer(value));
    }
    float(text).map(Value::Float)
}

fn currency(text: &str) -> Result<i64, String> {
    let invalid = || "expected an amount like 12 or 12.30".to_string();
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, cents) = match unsigned.split_once('.') {
        Some((whole, cents)) if cents.len() == 2 => (whole, cents),
        Some(_) => return Err("use no decimal places or exactly two".into()),
        None => (unsigned, "00"),
    };
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    if !cents.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let minor = whole
        .parse::<i64>()
        .ok()
        .and_then(|whole| whole.checked_mul(100))
        .and_then(|whole| whole.checked_add(cents.parse::<i64>().ok()?))
        .ok_or("amount is out of range")?;
    Ok(if negative { -minor } else { minor })
}

fn radix(digits: &str, radix: u32, name: &str) -> Result<u64, String> {
    if digits.is_empty() {
        return Err(format!("enter a {name} number"));
    }
    // `from_str_radix` accepts a leading `+`, which is not a digit.
    if digits.starts_with('+') {
        return Err(format!("not a {name} number"));
    }
    u64::from_str_radix(digits, radix).map_err(|error| match error.kind() {
        std::num::IntErrorKind::PosOverflow => format!("{name} number is over 64 bits"),
        _ => format!("not a {name} number"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(kind: LiteralKind, text: &str) -> Value {
        parse(&kind, text, &Validators::new()).unwrap_or_else(|e| panic!("{text:?}: {e}"))
    }

    fn bad(kind: LiteralKind, text: &str) {
        assert!(parse(&kind, text, &Validators::new()).is_err(), "{text:?} was accepted");
    }

    #[test]
    fn floats_take_e_notation_and_refuse_non_finite() {
        assert_eq!(ok(LiteralKind::Float, "1e3"), Value::Float(1000.0));
        assert_eq!(ok(LiteralKind::Float, " .5 "), Value::Float(0.5));
        assert_eq!(ok(LiteralKind::Float, "7"), Value::Float(7.0));
        for text in ["", "inf", "NaN", "1e999", "abc", "0x10"] {
            bad(LiteralKind::Float, text);
        }
    }

    #[test]
    fn numbers_keep_integers_integral() {
        assert_eq!(ok(LiteralKind::Number, "42"), Value::Integer(42));
        assert_eq!(ok(LiteralKind::Number, "-3"), Value::Integer(-3));
        assert_eq!(ok(LiteralKind::Number, "42.0"), Value::Float(42.0));
        assert_eq!(ok(LiteralKind::Number, "1e3"), Value::Float(1000.0));
        // Too big for i64, so it falls through to a float.
        assert_eq!(
            ok(LiteralKind::Number, "99999999999999999999"),
            Value::Float(1e20)
        );
        for text in ["", "Infinity", "0x10"] {
            bad(LiteralKind::Number, text);
        }
    }

    #[test]
    fn currency_takes_no_decimals_or_exactly_two() {
        assert_eq!(ok(LiteralKind::Currency, "12"), Value::Currency(1200));
        assert_eq!(ok(LiteralKind::Currency, "12.30"), Value::Currency(1230));
        assert_eq!(ok(LiteralKind::Currency, "-0.05"), Value::Currency(-5));
        for text in ["12.3", "12.345", ".50", "", "1,000", "12.x0"] {
            bad(LiteralKind::Currency, text);
        }
    }

    #[test]
    fn binary_and_hex_accept_their_prefixes() {
        assert_eq!(ok(LiteralKind::Binary, "101"), Value::Unsigned(5));
        assert_eq!(ok(LiteralKind::Binary, "0b101"), Value::Unsigned(5));
        assert_eq!(ok(LiteralKind::Hex, "ff"), Value::Unsigned(255));
        assert_eq!(ok(LiteralKind::Hex, "0xFF"), Value::Unsigned(255));
        assert_eq!(ok(LiteralKind::Hex, "#ff"), Value::Unsigned(255));
        assert_eq!(
            ok(LiteralKind::Hex, "ffffffffffffffff"),
            Value::Unsigned(u64::MAX)
        );
        for text in ["102", "0b", "+1", ""] {
            bad(LiteralKind::Binary, text);
        }
        for text in ["fg", "0x", "#", "1ffffffffffffffff"] {
            bad(LiteralKind::Hex, text);
        }
    }

    #[test]
    fn text_is_kept_exactly() {
        assert_eq!(ok(LiteralKind::Text, "  hi "), Value::Text("  hi ".into()));
    }

    #[test]
    fn custom_kinds_use_the_registered_validator() {
        #[derive(Debug)]
        struct Upper;
        impl LiteralValidator for Upper {
            fn validate(&self, text: &str) -> Result<Value, String> {
                if text.chars().all(|c| c.is_ascii_uppercase()) {
                    Ok(Value::Text(text.into()))
                } else {
                    Err("capitals only".into())
                }
            }
        }
        let mut validators = Validators::new();
        validators.insert("upper", Arc::new(Upper));
        let kind = LiteralKind::Custom("upper".into());

        assert!(parse(&kind, "ABC", &validators).is_ok());
        assert_eq!(parse(&kind, "abc", &validators), Err("capitals only".into()));
        assert!(parse(&kind, "ABC", &Validators::new()).is_err());
    }
}
