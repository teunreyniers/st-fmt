//! The validity gate: st-fmt formats valid files only.
//!
//! Every source here must be *refused*. A formatter that best-effort rewrites a
//! tree containing ERROR nodes loses source text, so refusing is the feature.

use st_fmt::{FormatError, format_source};

fn assert_refused(label: &str, source: &str) {
    match format_source(source) {
        Err(FormatError::Parse { .. }) => {}
        Err(other) => panic!("{label}: expected a parse error, got {other:?}"),
        Ok(out) => panic!("{label}: expected a refusal, but it formatted to:\n{out}"),
    }
}

#[test]
fn refuses_unterminated_if() {
    assert_refused("unterminated IF", "IF x THEN\n  y := 1;\n");
}

#[test]
fn refuses_unterminated_pou() {
    // The grammar README calls this out: a declaration-only export with no
    // END_FUNCTION_BLOCK is malformed, and deliberately so.
    assert_refused(
        "declaration-only export",
        "FUNCTION_BLOCK FB_Thing\nVAR\n  n : INT;\nEND_VAR\n",
    );
}

#[test]
fn refuses_missing_semicolon() {
    assert_refused("missing terminator", "x := 1\ny := 2;\n");
}

#[test]
fn refuses_stray_tokens() {
    assert_refused("stray tokens", "x := @@ 1;\n");
}

#[test]
fn refuses_unclosed_block_comment() {
    assert_refused("unclosed block comment", "x := 1;\n(* never closed\n");
}

#[test]
fn refuses_unbalanced_parentheses() {
    assert_refused("unbalanced parens", "x := (1 + 2;\n");
}

#[test]
fn refuses_case_without_end() {
    assert_refused("unterminated CASE", "CASE n OF\n  1: x := 1;\n");
}

#[test]
fn accepts_an_empty_file() {
    // The grammar cannot parse an empty file, but refusing one would make
    // `st-fmt *.st` fail on a placeholder POU. An empty file stays empty.
    assert_eq!(format_source("").expect("an empty file is valid"), "");
    assert_eq!(
        format_source("  \n\n").expect("whitespace only is valid"),
        ""
    );
}

#[test]
fn accepts_a_comment_only_file() {
    let out = format_source("// just a note\n").expect("a comment-only file is valid");
    assert_eq!(out, "// just a note\n");
}

#[test]
fn a_refusal_reports_line_and_column() {
    let err = format_source("x := 1;\ny := @;\n").unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.starts_with("2:"),
        "error should name line 2, got {rendered:?}"
    );
}
