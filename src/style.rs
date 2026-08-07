//! The single place where style constants live.
//!
//! st-fmt is deliberately zero-config: there is no config file and no flags.
//! Changing the house style means changing a constant here and re-running
//! `UPDATE_EXPECT=1 cargo test`.

/// Lines are broken to keep them at or under this many columns where possible.
pub const MAX_WIDTH: usize = 100;

/// Spaces per indentation level.
pub const INDENT: usize = 4;

/// The builtin IEC 61131-3 elementary types, plus the vendor-ubiquitous `ANY_*`
/// families. Matched case-insensitively; anything not in this list is treated as
/// a user-defined type and its casing is preserved exactly as written.
const BUILTIN_TYPES: &[&str] = &[
    // Bit strings
    "BOOL",
    "BYTE",
    "WORD",
    "DWORD",
    "LWORD", //
    // Signed integers
    "SINT",
    "INT",
    "DINT",
    "LINT", //
    // Unsigned integers
    "USINT",
    "UINT",
    "UDINT",
    "ULINT", //
    // Reals
    "REAL",
    "LREAL", //
    // Duration
    "TIME",
    "LTIME", //
    // Date and time
    "DATE",
    "LDATE",
    "TIME_OF_DAY",
    "TOD",
    "LTOD",
    "LTIME_OF_DAY",
    "DATE_AND_TIME",
    "DT",
    "LDT",
    "LDATE_AND_TIME", //
    // Character
    "STRING",
    "WSTRING",
    "CHAR",
    "WCHAR", //
    // Generic
    "ANY",
    "ANY_DERIVED",
    "ANY_ELEMENTARY",
    "ANY_MAGNITUDE",
    "ANY_NUM",
    "ANY_REAL",
    "ANY_INT",
    "ANY_BIT",
    "ANY_STRING",
    "ANY_DATE",
];

/// Returns the canonical uppercase spelling if `name` is a builtin type,
/// otherwise `None` — meaning the identifier is user-defined and must be left
/// exactly as the author wrote it.
pub fn builtin_type(name: &str) -> Option<&'static str> {
    BUILTIN_TYPES
        .iter()
        .find(|t| t.eq_ignore_ascii_case(name))
        .copied()
}

/// Canonicalizes an identifier that appears in type position.
pub fn type_name_case(name: &str) -> String {
    builtin_type(name)
        .map(str::to_owned)
        .unwrap_or_else(|| name.to_owned())
}

/// Canonicalizes a keyword. The grammar aliases every keyword token to its
/// lowercase spelling regardless of how it was written in the source, so the
/// node's kind is already a clean lowercase key and uppercasing it is enough.
pub fn keyword(kind: &str) -> String {
    kind.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_types_are_recognized_case_insensitively() {
        assert_eq!(builtin_type("bool"), Some("BOOL"));
        assert_eq!(builtin_type("Bool"), Some("BOOL"));
        assert_eq!(builtin_type("BOOL"), Some("BOOL"));
        assert_eq!(builtin_type("time_of_day"), Some("TIME_OF_DAY"));
    }

    #[test]
    fn user_types_are_not_builtins() {
        assert_eq!(builtin_type("FB_Motor"), None);
        assert_eq!(builtin_type("TON"), None);
        // A user type whose name merely starts with a builtin is untouched.
        assert_eq!(builtin_type("INTERLOCK"), None);
    }

    #[test]
    fn type_name_case_preserves_user_types() {
        assert_eq!(type_name_case("int"), "INT");
        assert_eq!(type_name_case("FB_Motor"), "FB_Motor");
        assert_eq!(type_name_case("analog_event_udt"), "analog_event_udt");
    }

    #[test]
    fn keyword_uppercases_the_aliased_kind() {
        assert_eq!(keyword("end_function_block"), "END_FUNCTION_BLOCK");
        assert_eq!(keyword("if"), "IF");
        assert_eq!(keyword("non_retain"), "NON_RETAIN");
    }
}
