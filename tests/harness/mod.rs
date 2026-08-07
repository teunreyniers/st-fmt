//! The fixture harness.
//!
//! A fixture is a pair of files:
//!
//! ```text
//! tests/fixtures/<concept>/<case>.st            input, deliberately ugly
//! tests/fixtures/<concept>/<case>.expected.st   the agreed output
//! ```
//!
//! Every fixture is checked four ways. Only the first is about the agreed
//! style; the other three are invariants that must hold for *any* input, and
//! they are where most of the safety comes from:
//!
//! 1. `format(input) == expected`
//! 2. idempotence — `format(expected) == expected`
//! 3. semantic preservation — the parse tree of the output matches the input's
//! 4. comment conservation — no comment is dropped, duplicated or altered
//!
//! Run `UPDATE_EXPECT=1 cargo test` to regenerate `.expected.st` files after
//! agreeing a style change. Always read the resulting diff.

#![allow(dead_code)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

pub fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn update_mode() -> bool {
    std::env::var_os("UPDATE_EXPECT").is_some_and(|v| v != "0" && !v.is_empty())
}

/// Every `<case>.st` under `tests/fixtures/`, recursively.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(root, &mut found);
    found.sort();
    found
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if is_input_fixture(&path) {
            out.push(path);
        }
    }
}

fn is_input_fixture(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "st") && !path.to_string_lossy().ends_with(".expected.st")
}

fn expected_path(input: &Path) -> PathBuf {
    input.with_extension("expected.st")
}

/// A failure report for one fixture. Collected rather than panicked so a single
/// run reports every broken fixture at once.
pub struct Failure {
    pub fixture: String,
    pub detail: String,
}

/// Runs all four checks against one fixture.
pub fn check_fixture(input_path: &Path) -> Result<(), Failure> {
    let name = input_path
        .strip_prefix(fixtures_root())
        .unwrap_or(input_path)
        .display()
        .to_string();
    let fail = |detail: String| Failure {
        fixture: name.clone(),
        detail,
    };

    let source =
        std::fs::read_to_string(input_path).map_err(|e| fail(format!("cannot read input: {e}")))?;

    let actual =
        st_fmt::format_source(&source).map_err(|e| fail(format!("input does not parse: {e}")))?;

    let expected_path = expected_path(input_path);

    if update_mode() {
        std::fs::write(&expected_path, &actual)
            .map_err(|e| fail(format!("cannot write expected file: {e}")))?;
    }

    let expected = std::fs::read_to_string(&expected_path).map_err(|e| {
        fail(format!(
            "cannot read {}: {e}\nrun `UPDATE_EXPECT=1 cargo test` to create it",
            expected_path.display()
        ))
    })?;

    // 1. The agreed output.
    if actual != expected {
        return Err(fail(diff(&expected, &actual)));
    }

    // 2. Idempotence: formatting the output again must change nothing.
    let twice = st_fmt::format_source(&actual)
        .map_err(|e| fail(format!("formatted output no longer parses: {e}")))?;
    if twice != actual {
        return Err(fail(format!(
            "not idempotent — a second pass changed the output\n{}",
            diff(&actual, &twice)
        )));
    }

    // 3. Semantic preservation.
    if let Err(detail) = assert_same_tree(&source, &actual) {
        return Err(fail(detail));
    }

    // 4. Comment conservation.
    if let Err(detail) = assert_comments_conserved(&source, &actual) {
        return Err(fail(detail));
    }

    Ok(())
}

/// Checks the invariants only (2, 3, 4) — no expected file involved.
///
/// Used by the corpus smoke test, where there is no agreed output to compare
/// against but the invariants must still hold.
pub fn check_invariants(source: &str) -> Result<String, String> {
    let once = st_fmt::format_source(source).map_err(|e| format!("does not format: {e}"))?;
    let twice =
        st_fmt::format_source(&once).map_err(|e| format!("output no longer parses: {e}"))?;
    if once != twice {
        return Err(format!("not idempotent\n{}", diff(&once, &twice)));
    }
    assert_same_tree(source, &once)?;
    assert_comments_conserved(source, &once)?;
    Ok(once)
}

/// Compares the parse trees of two sources, ignoring positions and comments.
///
/// This is the check that catches a formatter bug turning `a AND b OR c` into
/// something that reassociates, or dropping a clause entirely.
pub fn assert_same_tree(before: &str, after: &str) -> Result<(), String> {
    let mut parser = st_fmt::parse::parser();
    let before_tree = parser.parse(before, None).ok_or("input did not parse")?;
    let after_tree = parser.parse(after, None).ok_or("output did not parse")?;

    let before_sexp = shape(before_tree.root_node());
    let after_sexp = shape(after_tree.root_node());

    if before_sexp != after_sexp {
        return Err(format!(
            "formatting changed the parse tree\n{}",
            diff(&before_sexp, &after_sexp)
        ));
    }
    Ok(())
}

/// A normalized S-expression of the tree: named nodes and field names only,
/// with comments and all positions stripped.
///
/// Two things are normalized away because the formatter is allowed to
/// introduce them:
///
/// - `comment` nodes, which are `extras` and are re-emitted from a cursor
///   rather than from tree position (they get their own check instead)
/// - `noop` nodes and blocks that contain nothing else, because the formatter
///   writes an explicit `;` where the grammar allowed an absent body. That
///   turns `IF c THEN END_IF` into `IF c THEN ; END_IF`, which adds a `block`
///   and a `noop` without changing what the code does.
///
/// Dropping a *real* statement is still caught: only empty statements vanish.
fn shape(root: Node<'_>) -> String {
    let mut out = String::new();
    write_shape(root, None, 0, &mut out);
    out
}

/// Writes `node`'s shape, returning false if it collapsed to nothing.
fn write_shape(node: Node<'_>, field: Option<&str>, depth: usize, out: &mut String) -> bool {
    if node.kind() == "comment" || node.kind() == "noop" {
        return false;
    }

    // Render the children first so an emptied block can be dropped entirely.
    let mut children_out = String::new();
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    let mut wrote_child = false;
    for (idx, child) in children.into_iter().enumerate() {
        // Field names are indexed over all named children, so comments must be
        // skipped while writing rather than filtered out of the iteration.
        let field = node.field_name_for_named_child(idx as u32);
        wrote_child |= write_shape(child, field, depth + 1, &mut children_out);
    }

    if node.kind() == "block" && !wrote_child {
        return false;
    }

    for _ in 0..depth {
        out.push_str("  ");
    }
    if let Some(field) = field {
        let _ = write!(out, "{field}: ");
    }
    let _ = writeln!(out, "{}", node.kind());
    out.push_str(&children_out);
    true
}

/// Checks that the output holds exactly the comments the input did.
///
/// Comments are `extras` in this grammar and are re-emitted by hand from a
/// cursor, so losing one is a realistic failure mode that the tree comparison
/// above deliberately ignores.
pub fn assert_comments_conserved(before: &str, after: &str) -> Result<(), String> {
    let before_comments = comment_texts(before)?;
    let after_comments = comment_texts(after)?;

    if before_comments != after_comments {
        return Err(format!(
            "comments were not preserved\ninput had {} comment(s), output has {}\n{}",
            before_comments.len(),
            after_comments.len(),
            diff(&before_comments.join("\n"), &after_comments.join("\n"))
        ));
    }
    Ok(())
}

fn comment_texts(source: &str) -> Result<Vec<String>, String> {
    let mut parser = st_fmt::parse::parser();
    let tree = parser.parse(source, None).ok_or("source did not parse")?;
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    let mut cursor = tree.root_node().walk();
    while let Some(node) = stack.pop() {
        if node.kind() == "comment" {
            out.push(normalize_comment(&source[node.byte_range()]));
            continue;
        }
        stack.extend(node.children(&mut cursor));
    }
    out.sort();
    Ok(out)
}

/// Strips trailing whitespace from each line of a comment before comparing.
///
/// The renderer removes trailing whitespace from every output line, which can
/// reach inside a comment the author left a stray space in. That is a wanted
/// normalization and not a lost comment, so it must not fail this check.
fn normalize_comment(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A compact line-oriented diff, good enough to read a fixture failure.
pub fn diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let mut out = String::from("\n--- expected\n+++ actual\n");

    let max = expected_lines.len().max(actual_lines.len());
    let mut shown = 0;
    for i in 0..max {
        let e = expected_lines.get(i);
        let a = actual_lines.get(i);
        if e == a {
            continue;
        }
        if shown >= 30 {
            let _ = writeln!(out, "  … and more");
            break;
        }
        shown += 1;
        let _ = writeln!(out, "line {}:", i + 1);
        match e {
            Some(e) => {
                let _ = writeln!(out, "  - {e:?}");
            }
            None => {
                let _ = writeln!(out, "  - <missing>");
            }
        }
        match a {
            Some(a) => {
                let _ = writeln!(out, "  + {a:?}");
            }
            None => {
                let _ = writeln!(out, "  + <missing>");
            }
        }
    }
    out
}

/// Reports every failure at once rather than stopping at the first.
pub fn report(failures: Vec<Failure>) {
    if failures.is_empty() {
        return;
    }
    let mut msg = format!("\n{} fixture(s) failed:\n", failures.len());
    for f in &failures {
        let _ = writeln!(msg, "\n=== {} ===\n{}", f.fixture, f.detail);
    }
    if !update_mode() {
        msg.push_str("\nIf these outputs are the agreed style, re-run with UPDATE_EXPECT=1.\n");
    }
    panic!("{msg}");
}
