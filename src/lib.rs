//! st-fmt — an opinionated formatter for IEC 61131-3 Structured Text.
//!
//! ```no_run
//! let out = st_fmt::format_source("if a then x:=1; end_if;").unwrap();
//! ```
//!
//! The pipeline is: parse and reject anything invalid ([`parse`]), collect
//! comments and blank lines that the tree does not carry ([`trivia`]), build a
//! [`doc::Doc`] describing the legal line breaks, then render it at
//! [`style::MAX_WIDTH`].

pub mod doc;
pub mod fmt;
pub mod parse;
pub mod style;
pub mod trivia;

pub use parse::{FormatError, ParseFault};

/// Formats a Structured Text source file.
///
/// Returns [`FormatError`] without producing any output if the source does not
/// parse cleanly — st-fmt never rewrites a file it does not fully understand.
pub fn format_source(source: &str) -> Result<String, FormatError> {
    if let Some(comments) = parse::scan_trivial(source) {
        return Ok(format_trivial(&comments));
    }
    let tree = parse::parse_valid(source)?;
    Ok(fmt::format_tree(&tree, source))
}

/// Formats a file holding only whitespace and comments, which the grammar
/// cannot parse. Each comment goes on its own line, authored blank lines are
/// kept but collapsed to one, and an empty file stays empty.
fn format_trivial(comments: &[parse::TrivialComment]) -> String {
    if comments.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for comment in comments {
        if !out.is_empty() {
            out.push('\n');
            if comment.blank_before {
                out.push('\n');
            }
        }
        out.push_str(&comment.text);
    }
    out.push('\n');
    out
}

/// Formats `source` and reports which node kinds fell through to the verbatim
/// fallback. Used by the test suite to track formatter coverage as phases land.
pub fn format_source_reporting(source: &str) -> Result<(String, Vec<String>), FormatError> {
    if let Some(comments) = parse::scan_trivial(source) {
        return Ok((format_trivial(&comments), Vec::new()));
    }
    let tree = parse::parse_valid(source)?;
    let (out, unhandled) = fmt::format_tree_reporting(&tree, source);
    Ok((out, unhandled))
}
