//! Control flow: IF, CASE, FOR, WHILE and REPEAT.
//!
//! Two rules shape everything here.
//!
//! **A body is always non-empty.** The grammar makes every body
//! `optional($.block)`, so `IF c THEN END_IF` parses with no body node at all.
//! Rather than emit two bare keywords, the formatter writes an explicit empty
//! statement, which is what an author would have typed.
//!
//! **The trailing keyword breaks with its condition.** `THEN`, `DO` and `OF`
//! share a group with the condition before them, so they drop to their own line
//! exactly when the condition wraps — never on their own.

use tree_sitter::Node;

use super::{Formatter, expr};
use crate::doc::Doc;

impl Formatter<'_> {
    /// `IF … THEN … ELSIF … ELSE … END_IF`
    pub fn if_statement(&mut self, node: Node<'_>) -> Doc {
        let mut parts = vec![self.clause_header(node, "IF", "condition", "THEN")];

        let consequence = node.child_by_field_name("consequence");
        let anchor = consequence.or_else(|| token(node, "then"));
        parts.push(self.body(consequence, bound_after(anchor, node)));

        // `elsif_clause` and `else_clause` share the field name `alternative`.
        for alt in super::source::named_children(node) {
            if matches!(alt.kind(), "elsif_clause" | "else_clause") {
                parts.push(Doc::HardLine);
                parts.push(self.node(alt));
            }
        }

        parts.push(Doc::HardLine);
        parts.push(Doc::text("END_IF"));
        Doc::concat(parts)
    }

    /// `ELSIF … THEN …`
    pub fn elsif_clause(&mut self, node: Node<'_>) -> Doc {
        let header = self.clause_header(node, "ELSIF", "condition", "THEN");
        let consequence = node.child_by_field_name("consequence");
        let anchor = consequence.or_else(|| token(node, "then"));
        Doc::concat([header, self.body(consequence, bound_after(anchor, node))])
    }

    /// `ELSE …`
    pub fn else_clause(&mut self, node: Node<'_>) -> Doc {
        let body = node.child_by_field_name("body");
        let anchor = body.or_else(|| token(node, "else"));
        Doc::concat([
            Doc::text("ELSE"),
            self.body(body, bound_after(anchor, node)),
        ])
    }

    /// `CASE … OF … ELSE … END_CASE`
    ///
    /// The `ELSE` here is not an `else_clause` node — the grammar inlines it as
    /// an anonymous `seq(else, optional(block))` under the field `else`, so it
    /// has no wrapper to dispatch on and is handled directly.
    pub fn case_statement(&mut self, node: Node<'_>) -> Doc {
        let mut parts = vec![self.clause_header(node, "CASE", "value", "OF")];

        let else_token = token(node, "else");
        let end_token = token(node, "end_case");
        // Everything after the last branch belongs to ELSE if there is one,
        // otherwise to END_CASE.
        let items_bound = else_token
            .or(end_token)
            .map_or(node.end_byte(), |t| t.start_byte());

        if let Some(body) = node.child_by_field_name("body") {
            let items = super::source::named_children(body);
            let mut branches = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let bound = items.get(i + 1).map_or(items_bound, Node::start_byte);
                branches.push(self.separator_before(*item, prev_end(&items, i), i == 0));
                branches.push(self.case_item(*item, bound));
            }
            branches.push(self.leading_comments(items_bound, items.is_empty()));
            parts.push(Doc::concat([Doc::HardLine, Doc::concat(branches)]).indent());
        }

        if let Some(else_token) = else_token {
            // The ELSE body is the block sibling that follows the ELSE token.
            let else_body = next_non_comment(else_token).filter(|n| n.kind() == "block");
            let anchor = else_body.or(Some(else_token));
            parts.push(Doc::HardLine);
            parts.push(Doc::text("ELSE"));
            parts.push(self.body(else_body, bound_after(anchor, node)));
        }

        parts.push(Doc::HardLine);
        parts.push(Doc::text("END_CASE"));
        Doc::concat(parts)
    }

    /// One `label: body` branch of a CASE.
    fn case_item(&mut self, node: Node<'_>, bound: usize) -> Doc {
        let label = match node.child_by_field_name("label") {
            Some(l) => self.case_label(l),
            None => return self.verbatim(node),
        };
        let body = node.child_by_field_name("body");
        let anchor = body.or_else(|| token(node, ":"));
        Doc::concat([
            label,
            Doc::text(":"),
            self.body(body, bound_after(anchor, node).min(bound)),
        ])
    }

    /// A branch label: one or more singles and ranges, comma separated.
    fn case_label(&mut self, node: Node<'_>) -> Doc {
        let parts = super::source::named_children(node);
        let docs: Vec<Doc> = parts.into_iter().map(|p| self.node(p)).collect();
        Doc::join(Doc::text(", "), docs)
    }

    /// `1..5`
    pub fn case_label_range(&mut self, node: Node<'_>) -> Doc {
        let parts = super::source::named_children(node);
        let docs: Vec<Doc> = parts.into_iter().map(|p| self.node(p)).collect();
        Doc::join(Doc::text(".."), docs)
    }

    /// `-1` as a CASE label.
    pub fn negative_integer(&mut self, node: Node<'_>) -> Doc {
        match node.named_child(0) {
            Some(value) => {
                let doc = self.node(value);
                Doc::concat([Doc::text("-"), doc])
            }
            None => self.verbatim(node),
        }
    }

    /// `FOR i := start TO end [BY step] DO … END_FOR`
    pub fn for_statement(&mut self, node: Node<'_>) -> Doc {
        let mut header = vec![Doc::text("FOR ")];

        for (field, prefix) in [
            ("variable", ""),
            ("start", " := "),
            ("end", " TO "),
            ("by", " BY "),
        ] {
            if let Some(child) = node.child_by_field_name(field) {
                if !prefix.is_empty() {
                    header.push(Doc::text(prefix));
                }
                let doc = self.node(child);
                header.push(doc);
            }
        }

        header.push(Doc::Line);
        header.push(Doc::text("DO"));

        let body = node.child_by_field_name("body");
        let anchor = body.or_else(|| token(node, "do"));
        Doc::concat([
            Doc::concat(header).group(),
            self.body(body, bound_after(anchor, node)),
            Doc::HardLine,
            Doc::text("END_FOR"),
        ])
    }

    /// `WHILE … DO … END_WHILE`
    pub fn while_statement(&mut self, node: Node<'_>) -> Doc {
        let header = self.clause_header(node, "WHILE", "condition", "DO");
        let body = node.child_by_field_name("body");
        let anchor = body.or_else(|| token(node, "do"));
        Doc::concat([
            header,
            self.body(body, bound_after(anchor, node)),
            Doc::HardLine,
            Doc::text("END_WHILE"),
        ])
    }

    /// `REPEAT … UNTIL c END_REPEAT`
    pub fn repeat_statement(&mut self, node: Node<'_>) -> Doc {
        let body = node.child_by_field_name("body");
        let anchor = body.or_else(|| token(node, "repeat"));
        let mut parts = vec![
            Doc::text("REPEAT"),
            self.body(body, bound_after(anchor, node)),
            Doc::HardLine,
        ];

        match node.child_by_field_name("condition") {
            Some(cond) => {
                let cond_doc = expr::condition(self, cond);
                parts.push(Doc::concat([Doc::text("UNTIL "), cond_doc]).group());
            }
            None => parts.push(Doc::text("UNTIL")),
        }

        parts.push(Doc::HardLine);
        parts.push(Doc::text("END_REPEAT"));
        Doc::concat(parts)
    }

    /// `<KEYWORD> <expression> <TRAILER>`, breaking as one unit.
    ///
    /// The trailer (`THEN`, `DO`, `OF`) shares the condition's group, so it
    /// moves to its own line precisely when the condition wraps.
    fn clause_header(
        &mut self,
        node: Node<'_>,
        keyword: &'static str,
        field: &str,
        trailer: &'static str,
    ) -> Doc {
        let Some(cond) = node.child_by_field_name(field) else {
            return Doc::text(format!("{keyword} {trailer}"));
        };
        let cond_doc = expr::condition(self, cond);
        Doc::concat([
            Doc::text(format!("{keyword} ")),
            cond_doc,
            Doc::Line,
            Doc::text(trailer),
        ])
        .group()
    }

    /// An indented body.
    ///
    /// When the grammar produced no block — `IF c THEN END_IF` is legal — an
    /// explicit `;` is written instead of leaving the keywords bare. Any
    /// comment in the gap still belongs inside the body and is emitted above
    /// it.
    fn body(&mut self, block: Option<Node<'_>>, bound: usize) -> Doc {
        let inner = match block {
            Some(block) => self.block(block, bound),
            None => {
                let comments = self.leading_comments(bound, true);
                let separator = if comments.is_nil() {
                    Doc::Nil
                } else {
                    Doc::HardLine
                };
                Doc::concat([comments, separator, Doc::text(";")])
            }
        };
        Doc::concat([Doc::HardLine, inner]).indent()
    }
}

fn prev_end(items: &[Node<'_>], i: usize) -> Option<usize> {
    i.checked_sub(1).map(|p| items[p].end_byte())
}

/// The first child with the given kind, named or anonymous.
fn token<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|n| n.kind() == kind)
}

/// The next sibling that is not a comment.
fn next_non_comment<'t>(node: Node<'t>) -> Option<Node<'t>> {
    let mut next = node.next_sibling();
    while let Some(n) = next {
        if n.kind() != "comment" {
            return Some(n);
        }
        next = n.next_sibling();
    }
    None
}

/// Where a body's trivia stops: the start of the next real token after it.
///
/// Comment siblings are skipped deliberately. A comment sitting between the
/// last statement and `END_IF` belongs to the body, so the bound must reach
/// past it to the closing keyword.
fn bound_after(anchor: Option<Node<'_>>, parent: Node<'_>) -> usize {
    anchor
        .and_then(next_non_comment)
        .map_or(parent.end_byte(), |n| n.start_byte())
}
