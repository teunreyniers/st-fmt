//! Literals and identifiers.
//!
//! Structured Text is case-insensitive, so every literal below has several
//! spellings that mean the same thing. The house style is:
//!
//! - based integers keep their radix and separators, hex digits uppercased
//! - floats pad a bare decimal point and uppercase the exponent
//! - duration literals get an uppercase prefix and lowercase unit letters
//! - date and time-of-day literals get an uppercase prefix, body untouched
//! - typed-literal prefixes are elementary types and are always uppercased
//! - strings and identifiers are never touched

use tree_sitter::Node;

use super::Formatter;
use crate::doc::Doc;

/// A leaf copied exactly as written: identifiers, strings, and anything whose
/// text carries meaning the formatter must not disturb.
pub fn verbatim_leaf(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(f.text(node).to_owned())
}

/// `TRUE` / `FALSE`. The grammar has distinct `true` and `false` nodes, so the
/// canonical spelling comes from the node kind rather than the source text.
pub fn boolean(_f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(node.kind().to_ascii_uppercase())
}

/// An integer, plain or based.
///
/// The radix prefixes are the fixed tokens `2#`, `8#` and `16#`, so only the
/// digits after `#` can vary in case. Uppercasing them is a no-op for binary
/// and octal and canonicalizes hex.
pub fn integer(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(normalize_integer(f.text(node)))
}

fn normalize_integer(text: &str) -> String {
    match text.split_once('#') {
        Some((radix, digits)) => format!("{radix}#{}", digits.to_ascii_uppercase()),
        None => text.to_owned(),
    }
}

/// A real literal.
pub fn float(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(normalize_float(f.text(node)))
}

/// Pads a bare decimal point on either side and uppercases the exponent marker
/// and any size suffix.
///
/// `1.` becomes `1.0` and `.5` becomes `0.5`; digit-group separators and the
/// exponent's sign are left exactly as written.
fn normalize_float(text: &str) -> String {
    // A trailing `L`/`J` size suffix is not part of the number.
    let (number, suffix) = match text.chars().last() {
        Some(c) if c.eq_ignore_ascii_case(&'l') || c.eq_ignore_ascii_case(&'j') => {
            (&text[..text.len() - 1], c.to_ascii_uppercase().to_string())
        }
        _ => (text, String::new()),
    };

    let (mantissa, exponent) = match number.find(['e', 'E']) {
        Some(i) => (&number[..i], format!("E{}", &number[i + 1..])),
        None => (number, String::new()),
    };

    let mut mantissa = mantissa.to_owned();
    if mantissa.starts_with('.') {
        mantissa.insert(0, '0');
    }
    if mantissa.ends_with('.') {
        mantissa.push('0');
    }

    format!("{mantissa}{exponent}{suffix}")
}

/// A duration literal such as `T#1d2h3m4s500ms`.
///
/// Everything up to and including `#` is a keyword-like prefix and is
/// uppercased. Everything after it is digits, decimal points and unit letters,
/// so lowercasing the whole tail canonicalizes the units without touching the
/// value.
pub fn duration(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let text = f.text(node);
    Doc::text(match text.split_once('#') {
        Some((prefix, value)) => format!(
            "{}#{}",
            prefix.to_ascii_uppercase(),
            value.to_ascii_lowercase()
        ),
        None => text.to_owned(),
    })
}

/// A date, time-of-day or date-and-time literal.
///
/// Only the prefix can vary in case — the body is digits, `-`, `:` and `.` —
/// so the body is copied through untouched.
pub fn date_like(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let text = f.text(node);
    Doc::text(match text.split_once('#') {
        Some((prefix, value)) => format!("{}#{value}", prefix.to_ascii_uppercase()),
        None => text.to_owned(),
    })
}

/// A typed literal such as `DINT#0` or `WORD#16#FFF0`.
///
/// The grammar restricts `literal_type` to the elementary types, so the prefix
/// is always a builtin and always uppercased — there is no user-defined type to
/// preserve here.
pub fn typed(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let ty = node.child_by_field_name("type");
    let value = node.child_by_field_name("value");

    match (ty, value) {
        (Some(ty), Some(value)) => {
            let prefix = f.text(ty).to_ascii_uppercase();
            let value_doc = f.node(value);
            Doc::concat([Doc::text(prefix), value_doc])
        }
        _ => f.verbatim(node),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_digits_are_uppercased_and_separators_kept() {
        assert_eq!(normalize_integer("16#20fd"), "16#20FD");
        assert_eq!(normalize_integer("16#FF_ff"), "16#FF_FF");
        assert_eq!(normalize_integer("2#1010_1001"), "2#1010_1001");
        assert_eq!(normalize_integer("8#563"), "8#563");
    }

    #[test]
    fn plain_integers_are_untouched() {
        assert_eq!(normalize_integer("42"), "42");
        assert_eq!(normalize_integer("1_000_000"), "1_000_000");
    }

    #[test]
    fn bare_decimal_points_are_padded() {
        assert_eq!(normalize_float("1."), "1.0");
        assert_eq!(normalize_float(".5"), "0.5");
        assert_eq!(normalize_float("1.0"), "1.0");
    }

    #[test]
    fn exponents_are_uppercased_with_their_sign_kept() {
        assert_eq!(normalize_float("1e10"), "1E10");
        assert_eq!(normalize_float("1.5e-3"), "1.5E-3");
        assert_eq!(normalize_float("2.5E+7"), "2.5E+7");
    }

    #[test]
    fn float_separators_and_suffixes_survive() {
        assert_eq!(normalize_float("1_000.25"), "1_000.25");
        assert_eq!(normalize_float("1.0l"), "1.0L");
        // A bare point plus a suffix still gets padded.
        assert_eq!(normalize_float("1.L"), "1.0L");
    }

    #[test]
    fn float_normalization_is_idempotent() {
        for input in ["1.", ".5", "1e10", "1.5e-3", "2.5E+7", "1_000.25", "1.L"] {
            let once = normalize_float(input);
            assert_eq!(normalize_float(&once), once, "unstable for {input:?}");
        }
    }
}
