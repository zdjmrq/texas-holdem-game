//! JavaScript value-coercion helpers.
//!
//! The reference server validates and normalises untrusted JSON with JavaScript
//! operators such as `Number(value)`, `!!value` and `String(value)`. The room
//! configuration whitelist and the protocol-version check must behave exactly
//! like those operators, so the corners of JavaScript's coercion rules that can
//! show up in a JSON payload are reproduced here.

use serde_json::Value;

/// JavaScript `Number(value)`.
///
/// Returns `None` for NaN. Arrays follow `ToPrimitive`: `[]` becomes `""`,
/// `[x]` becomes `String(x)` and longer arrays stringify with commas. Plain
/// objects stringify to `"[object Object]"` and therefore become NaN.
pub(crate) fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Null => Some(0.0),
        Value::Bool(false) => Some(0.0),
        Value::Bool(true) => Some(1.0),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => parse_number(text),
        Value::Array(items) => {
            if items.is_empty() {
                Some(0.0)
            } else if items.len() == 1 {
                parse_number(&string(Some(&items[0])))
            } else {
                None
            }
        }
        Value::Object(_) => None,
    }
}

fn parse_number(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(is_js_whitespace);
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if trimmed == "Infinity" || trimmed == "+Infinity" {
        return Some(f64::INFINITY);
    }
    if trimmed == "-Infinity" {
        return Some(f64::NEG_INFINITY);
    }
    let (sign, unsigned) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1.0, &trimmed[1..]),
        Some(b'+') => (1.0, &trimmed[1..]),
        _ => (1.0, trimmed),
    };
    if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        return i64::from_str_radix(hex, 16)
            .ok()
            .map(|value| sign * value as f64);
    }
    if let Some(binary) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        return i64::from_str_radix(binary, 2)
            .ok()
            .map(|value| sign * value as f64);
    }
    if let Some(octal) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        return i64::from_str_radix(octal, 8)
            .ok()
            .map(|value| sign * value as f64);
    }
    // Rust's f64 parser accepts "inf"/"infinity"/"nan" spellings which
    // JavaScript rejects, so reject any non-decimal spelling first.
    let mut has_digit = false;
    let mut has_exponent = false;
    let mut previous = '\0';
    for character in unsigned.chars() {
        match character {
            '0'..='9' => has_digit = true,
            '.' if !has_exponent => {}
            'e' | 'E' if has_digit && !has_exponent && previous != 'e' && previous != 'E' => {
                has_exponent = true;
            }
            '+' | '-' if previous == 'e' || previous == 'E' => {}
            _ => return None,
        }
        previous = character;
    }
    if !has_digit {
        return None;
    }
    unsigned.parse::<f64>().ok().map(|value| value * sign)
}

/// JavaScript truthiness (`!!value`).
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(boolean) => *boolean,
        Value::Number(number) => number
            .as_f64()
            .is_some_and(|value| value != 0.0 && !value.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// JavaScript `String(value)` for the primitive/array shapes that can reach a
/// player-name field. Objects stringify to `"[object Object]"`.
pub(crate) fn string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(boolean)) => boolean.to_string(),
        Some(Value::Number(number)) => number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_owned(),
    }
}

fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    if !value.is_finite() {
        return if value.is_sign_positive() {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_owned();
    }
    if value.fract() == 0.0 && value.abs() < 1e21 {
        return format!("{}", value as i64);
    }
    let mut text = format!("{value}");
    if let Some((_mantissa, exponent)) = text.split_once('e') {
        if !exponent.starts_with('-') && !exponent.starts_with('+') {
            text = format!(
                "{}e+{}",
                &text[..text.find('e').unwrap_or(text.len())],
                exponent
            );
        }
    }
    text
}

/// JavaScript `String.prototype.trim` whitespace set (WhiteSpace + LineTerminator).
/// Rust's `char::is_whitespace` follows the Unicode White_Space property, which
/// additionally covers U+FEFF (BOM) in JavaScript but not in Rust.
pub(crate) fn is_js_whitespace(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}
