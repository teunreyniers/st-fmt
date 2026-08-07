//! Parsing and the validity gate.
//!
//! st-fmt formats valid files only. A file that does not parse cleanly is
//! refused outright — there is no best-effort mode, because rewriting a tree
//! that contains ERROR nodes reliably loses source text.

use std::fmt;

use tree_sitter::{Node, Parser, Tree};

pub fn parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_structured_text::LANGUAGE.into())
        .expect("the bundled structured-text grammar is ABI-compatible");
    parser
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The source does not parse. `line` and `column` are 1-based.
    Parse {
        line: usize,
        column: usize,
        kind: ParseFault,
        snippet: String,
    },
    /// tree-sitter declined to produce a tree at all (timeout or cancellation).
    NoTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseFault {
    /// An ERROR node: text the grammar could not fit anywhere.
    Unexpected,
    /// A MISSING node: the parser inserted a token to recover, e.g. an absent
    /// `END_IF`.
    Missing,
    /// The root node stops short of the end of the input.
    Truncated,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Parse {
                line,
                column,
                kind,
                snippet,
            } => {
                let what = match kind {
                    ParseFault::Unexpected => "syntax error",
                    ParseFault::Missing => "incomplete construct",
                    ParseFault::Truncated => "unparsed trailing input",
                };
                write!(f, "{line}:{column}: {what}: {snippet}")
            }
            FormatError::NoTree => write!(f, "the parser did not return a tree"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Parses `source` and rejects it unless the tree is completely clean.
///
/// Three separate conditions have to hold. `has_error()` alone is not enough:
/// it does not flag every MISSING node, and it says nothing about whether the
/// root actually spans the input.
pub fn parse_valid(source: &str) -> Result<Tree, FormatError> {
    let tree = parser().parse(source, None).ok_or(FormatError::NoTree)?;
    let root = tree.root_node();

    if let Some(fault) = first_fault(root) {
        return Err(fault_error(source, fault.0, fault.1));
    }

    // A clean tree that stops early means trailing text was silently dropped.
    let consumed = root.end_byte();
    if source[consumed..].trim() != "" {
        let mut offset = consumed;
        while offset < source.len() && source.as_bytes()[offset].is_ascii_whitespace() {
            offset += 1;
        }
        let (line, column) = line_col(source, offset);
        return Err(FormatError::Parse {
            line,
            column,
            kind: ParseFault::Truncated,
            snippet: snippet_at(source, offset),
        });
    }

    Ok(tree)
}

/// A comment found by [`scan_trivial`], with whether a blank line preceded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrivialComment {
    pub text: String,
    pub blank_before: bool,
}

/// Recognizes a source made up of nothing but whitespace and comments.
///
/// The grammar cannot parse such a file — `source` is
/// `choice(repeat1($._top_level_declaration), $.block)`, so it needs at least
/// one declaration or statement, and a comment is an `extra` that cannot stand
/// alone. But empty and header-only `.st` files exist in real projects, and
/// refusing them would make `st-fmt *.st` fail on a placeholder. They are
/// therefore detected here and formatted without the parser.
///
/// Returns `None` for anything containing code, and for an unterminated block
/// comment — that is a genuine syntax error and must still be refused.
pub fn scan_trivial(source: &str) -> Option<Vec<TrivialComment>> {
    let bytes = source.as_bytes();
    let mut comments = Vec::new();
    let mut i = 0;
    // Newlines seen since the last comment ended; two or more means the next
    // comment had a blank line above it.
    let mut newlines = 0usize;
    let mut seen_any = false;

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                newlines += 1;
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = source[i..].find('\n').map_or(source.len(), |n| i + n);
                comments.push(TrivialComment {
                    text: source[i..end].trim_end().to_owned(),
                    blank_before: seen_any && newlines > 1,
                });
                seen_any = true;
                newlines = 0;
                i = end;
            }
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                // An unterminated block comment is a syntax error, not trivia.
                let end = source[i + 2..].find("*)").map(|n| i + 2 + n + 2)?;
                comments.push(TrivialComment {
                    text: source[i..end].to_owned(),
                    blank_before: seen_any && newlines > 1,
                });
                seen_any = true;
                newlines = 0;
                i = end;
            }
            // Anything else is code: this is a normal file.
            _ => return None,
        }
    }

    Some(comments)
}

/// Walks the whole tree and returns the first faulty node, in source order.
fn first_fault(root: Node<'_>) -> Option<(Node<'_>, ParseFault)> {
    let mut cursor = root.walk();
    let mut stack = vec![root];
    let mut best: Option<(Node<'_>, ParseFault)> = None;

    while let Some(node) = stack.pop() {
        let fault = if node.is_missing() {
            Some(ParseFault::Missing)
        } else if node.is_error() {
            Some(ParseFault::Unexpected)
        } else {
            None
        };

        if let Some(fault) = fault {
            let better = best.is_none_or(|(b, _)| node.start_byte() < b.start_byte());
            if better {
                best = Some((node, fault));
            }
            // No need to descend into a faulty subtree.
            continue;
        }

        // `has_error` is true for any ancestor of a fault, so it prunes the
        // walk to just the branches that can contain one.
        if node.has_error() {
            stack.extend(node.children(&mut cursor));
        }
    }

    best
}

fn fault_error(source: &str, node: Node<'_>, kind: ParseFault) -> FormatError {
    let offset = node.start_byte();
    let (line, column) = line_col(source, offset);
    let snippet = if kind == ParseFault::Missing {
        format!("expected {}", node.kind())
    } else {
        snippet_at(source, offset)
    };
    FormatError::Parse {
        line,
        column,
        kind,
        snippet,
    }
}

/// Converts a byte offset into a 1-based line and column.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

/// The rest of the line at `offset`, truncated so error output stays readable.
fn snippet_at(source: &str, offset: usize) -> String {
    let rest = &source[offset.min(source.len())..];
    let line = rest.lines().next().unwrap_or("").trim_end();
    if line.chars().count() > 40 {
        let truncated: String = line.chars().take(40).collect();
        format!("{truncated}…")
    } else {
        line.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_simple_statement_list() {
        assert!(parse_valid("x := 1;").is_ok());
    }

    #[test]
    fn accepts_a_full_pou() {
        let src =
            "FUNCTION_BLOCK FB_Motor\nVAR\n  n : INT;\nEND_VAR\n  n := 1;\nEND_FUNCTION_BLOCK";
        assert!(parse_valid(src).is_ok(), "should parse: {src}");
    }

    #[test]
    fn empty_input_is_trivial_not_parseable() {
        // `source` requires at least one declaration or statement, so the
        // grammar rejects an empty file. It is handled ahead of the parser.
        assert!(parse_valid("").is_err());
        assert_eq!(scan_trivial(""), Some(vec![]));
        assert_eq!(scan_trivial("   \n\n"), Some(vec![]));
    }

    #[test]
    fn scan_trivial_collects_comment_only_files() {
        let found = scan_trivial("// one\n\n// two\n").expect("comments only");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].text, "// one");
        assert!(!found[0].blank_before);
        assert_eq!(found[1].text, "// two");
        assert!(found[1].blank_before, "a blank line separated the two");
    }

    #[test]
    fn scan_trivial_handles_block_comments() {
        let found = scan_trivial("(* a\n   b *)\n").expect("comments only");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "(* a\n   b *)");
    }

    #[test]
    fn scan_trivial_declines_files_containing_code() {
        assert_eq!(scan_trivial("// note\nx := 1;\n"), None);
        assert_eq!(scan_trivial("x := 1;"), None);
    }

    #[test]
    fn scan_trivial_declines_an_unterminated_block_comment() {
        // This is a genuine syntax error and must reach the parser's refusal
        // path rather than being silently treated as trivia.
        assert_eq!(scan_trivial("(* never closed\n"), None);
    }

    #[test]
    fn rejects_unterminated_if() {
        let err = parse_valid("IF x THEN").unwrap_err();
        assert!(matches!(err, FormatError::Parse { .. }), "got {err:?}");
    }

    #[test]
    fn rejects_garbage() {
        let err = parse_valid("x := ;;; @@@ 1;").unwrap_err();
        assert!(matches!(err, FormatError::Parse { .. }), "got {err:?}");
    }

    #[test]
    fn rejects_unterminated_pou() {
        // The README calls this out: a declaration-only export with no
        // END_FUNCTION_BLOCK is malformed.
        let err = parse_valid("FUNCTION_BLOCK FB\nVAR\nEND_VAR").unwrap_err();
        assert!(matches!(err, FormatError::Parse { .. }), "got {err:?}");
    }

    #[test]
    fn error_reports_a_useful_position() {
        let src = "x := 1;\ny := @;\n";
        let err = parse_valid(src).unwrap_err();
        let FormatError::Parse { line, .. } = err else {
            panic!("expected a parse error, got {err:?}");
        };
        assert_eq!(line, 2, "the fault is on line 2");
    }

    #[test]
    fn line_col_is_one_based() {
        assert_eq!(line_col("abc", 0), (1, 1));
        assert_eq!(line_col("abc", 2), (1, 3));
        assert_eq!(line_col("ab\ncd", 3), (2, 1));
    }
}
