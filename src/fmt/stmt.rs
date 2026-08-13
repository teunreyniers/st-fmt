//! Statements.
//!
//! The distinctive work here is **column alignment**, the statement-side
//! counterpart of the declaration rule in [`super::decl`]. A run of consecutive
//! assignments lines its values up:
//!
//! ```text
//! pTarget REF= pSource;
//! nState  :=   1;
//! rSpeed  :=   12.5;
//! ```
//!
//! Both the target column and the operator column are padded, so a run mixing
//! `:=` and `REF=` still starts every right-hand side in one place.
//!
//! Widths cannot be read off the source text the way [`super::decl`] reads a
//! type: formatting changes a target's width, `motor . speed` losing two
//! columns and `m[i,j]` gaining one. Measuring the source would therefore move
//! the column on a second pass. The targets are *built* instead, speculatively,
//! and the build is rewound — see [`Formatter::speculative`].

use tree_sitter::Node;

use super::{Formatter, expr, pad};
use crate::doc::Doc;
use crate::trivia::blank_line_between;

/// The alignment columns shared by one run of assignments.
///
/// The default is the unaligned form: both columns zero pads to nothing, so a
/// lone assignment comes out as `target := value` with single spaces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Align {
    /// Width of the target column.
    target: usize,
    /// Width of the operator column — 4 once anything in the run is `REF=`.
    operator: usize,
}

/// How an item takes part in an alignment run.
///
/// The classification reads the tree and nothing else, so it cannot come out
/// differently on a second pass over the formatter's own output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// An assignment whose operator joins the column.
    Member,
    /// Sits inside a run without joining it: measured by nothing, padded by
    /// nothing, and does not end the run.
    Neutral,
    /// Ends the run above it and starts a fresh one below.
    Breaker,
}

/// `target := value` and `target REF= value`, unaligned.
///
/// The entry point from the dispatch table, for an assignment reached outside a
/// statement list.
pub fn assignment(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    aligned_assignment(f, node, Align::default())
}

/// `target := value` and `target REF= value`, padded into `align`'s columns.
///
/// The operator token is anonymous, so it is read from the tree rather than
/// assumed, which keeps `REF=` working without a special case.
pub fn aligned_assignment(f: &mut Formatter<'_>, node: Node<'_>, align: Align) -> Doc {
    let target = node.child_by_field_name("identifier");
    let value = node.child_by_field_name("expression");
    let op = assignment_operator(node);

    let target_doc = match target {
        Some(t) => f.node(t),
        None => return f.verbatim(node),
    };
    let value_doc = match value {
        Some(v) => expr::expression(f, v),
        None => return f.verbatim(node),
    };

    // A target that cannot be rendered flat was not measured either, so it is
    // left at its natural width rather than padded to a column it never joined.
    let target_pad = target_doc
        .flat_width()
        .map_or(0, |w| align.target.saturating_sub(w));
    let joint = format!("{} {} ", " ".repeat(target_pad), pad(&op, align.operator));

    Doc::concat([target_doc, Doc::text(joint), value_doc])
}

/// Measures the alignment columns for every item of a statement list, one entry
/// per item.
///
/// Runs are partitioned by blank line, exactly as
/// [`super::decl`] partitions declarations, with the extra rule that any
/// statement other than an assignment ends a run. Comments never appear here —
/// they are trivia, not list items — so a note can no more shift a statement
/// column than it can shift a declaration one.
pub fn alignment_runs(f: &mut Formatter<'_>, items: &[Node<'_>]) -> Vec<Align> {
    let roles: Vec<Role> = items.iter().map(|item| role(f, *item)).collect();
    let mut columns = vec![Align::default(); items.len()];
    let mut start = 0;

    while start < items.len() {
        if roles[start] == Role::Breaker {
            start += 1;
            continue;
        }

        let mut end = start + 1;
        while end < items.len()
            && roles[end] != Role::Breaker
            && !blank_line_between(f.source, items[end - 1].end_byte(), items[end].start_byte())
        {
            end += 1;
        }

        let run = measure(f, &items[start..end], &roles[start..end]);
        for slot in columns.iter_mut().take(end).skip(start) {
            *slot = run;
        }
        start = end;
    }

    columns
}

/// Measures one alignment run.
fn measure(f: &mut Formatter<'_>, run: &[Node<'_>], roles: &[Role]) -> Align {
    let mut align = Align::default();

    for (item, role) in run.iter().zip(roles) {
        if *role != Role::Member {
            continue;
        }
        let Some(target) = item.child_by_field_name("identifier") else {
            continue;
        };
        // The one place a target is built out of source order, and the only
        // reason the speculative build exists.
        let Some(width) = f.speculative(|f| f.node(target).flat_width()) else {
            continue;
        };
        align.target = align.target.max(width);
        align.operator = align
            .operator
            .max(assignment_operator(*item).chars().count());
    }

    align
}

/// How `item` takes part in the run around it.
fn role(f: &Formatter<'_>, item: Node<'_>) -> Role {
    match item.kind() {
        // An assignment missing either field is copied verbatim and has no
        // operator to align, so it sits in the run without joining it.
        "assignment" => {
            let decomposable = item.child_by_field_name("identifier").is_some()
                && item.child_by_field_name("expression").is_some();
            if decomposable {
                Role::Member
            } else {
                Role::Neutral
            }
        }
        // A region marker moves everything below it one level, so the column it
        // would share is not the same column. Every other pragma annotates the
        // code it sits among and stays inside the run.
        "pragma" => match region(f.text(item)) {
            Some(_) => Role::Breaker,
            None => Role::Neutral,
        },
        // `noop`, calls, `RETURN`, `EXIT`, `CONTINUE` and every compound
        // statement.
        _ => Role::Breaker,
    }
}

/// Finds the assignment's operator token. `:=` and `REF=` are both anonymous
/// children of `assignment`.
fn assignment_operator(node: Node<'_>) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            ":=" => return ":=".to_owned(),
            "ref=" | "REF=" => return "REF=".to_owned(),
            _ => {}
        }
    }
    ":=".to_owned()
}

/// `RETURN`, optionally with a value.
pub fn return_statement(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    match node.named_child(0) {
        Some(value) => {
            let value_doc = expr::expression(f, value);
            Doc::concat([Doc::text("RETURN "), value_doc])
        }
        None => Doc::text("RETURN"),
    }
}

/// `EXIT` and `CONTINUE`, whose canonical spelling is their node kind.
pub fn bare_keyword(_f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(node.kind().to_ascii_uppercase())
}

/// A pragma such as `{region Event logic}` or `{attribute 'qualified_only'}`.
///
/// The grammar captures the interior as one opaque token (`/[^\}]*/`), so there
/// is no structure to reformat. It is copied through byte for byte.
pub fn pragma(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(f.text(node).to_owned())
}

/// What a pragma does to region nesting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// `{region Event logic}` — everything below it indents one level.
    Open,
    /// `{endregion}` — returns to the opening pragma's column.
    Close,
}

/// Classifies a pragma as a region marker.
///
/// Only the first word counts, so a region's title is free text and
/// `{regionally 'odd'}` is not mistaken for one. `{end_region}` is accepted
/// alongside `{endregion}`: both spellings are in the wild.
pub fn region(text: &str) -> Option<Region> {
    let body = text.strip_prefix('{')?;
    let word: String = body
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();

    if word.eq_ignore_ascii_case("region") {
        Some(Region::Open)
    } else if word.eq_ignore_ascii_case("endregion") || word.eq_ignore_ascii_case("end_region") {
        Some(Region::Close)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_markers_are_recognized_case_insensitively() {
        assert_eq!(region("{region Event logic}"), Some(Region::Open));
        assert_eq!(region("{REGION \"Event logic\"}"), Some(Region::Open));
        assert_eq!(region("{ region }"), Some(Region::Open));
        assert_eq!(region("{endregion}"), Some(Region::Close));
        assert_eq!(region("{EndRegion Event logic}"), Some(Region::Close));
        assert_eq!(region("{end_region}"), Some(Region::Close));
    }

    #[test]
    fn other_pragmas_are_not_region_markers() {
        assert_eq!(region("{attribute 'qualified_only'}"), None);
        assert_eq!(region("{regionally 'odd'}"), None);
        assert_eq!(region("{}"), None);
    }
}
