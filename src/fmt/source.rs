//! File level and statement blocks.
//!
//! `source` is `choice(repeat1($._top_level_declaration), $.block)`: a file
//! holds either declarations or a bare statement list, never both.
//!
//! The `;` is owned by `block`, not by the statement — a statement node's byte
//! range stops before its terminator — so blocks emit terminators themselves.

use tree_sitter::Node;

use super::{Formatter, is_compound_statement};
use crate::doc::Doc;
use crate::trivia::blank_line_between;

impl Formatter<'_> {
    /// Formats the root node.
    pub fn format_source_node(&mut self, root: Node<'_>) -> Doc {
        let items = named_children(root);

        if items.is_empty() {
            // A file with nothing but comments still has to keep them.
            return self.leading_comments(usize::MAX, true);
        }

        let mut parts = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let prev_end = i.checked_sub(1).map(|p| items[p].end_byte());
            let next_start = items.get(i + 1).map(Node::start_byte);

            parts.push(self.separator_before(*item, prev_end, i == 0));
            parts.push(match item.kind() {
                // A file that is a bare statement list has one `block` child;
                // its statements take terminators inside `block`. Nothing
                // follows it, so its bound is the end of the file.
                "block" => self.block(*item, next_start.unwrap_or(usize::MAX)),
                // Top-level declarations are *not* statements: `;` belongs to
                // `block`, and END_FUNCTION_BLOCK never takes one.
                _ => {
                    let doc = self.node(*item);
                    let trailing = self.trailing_comments(
                        item.end_position().row,
                        next_start.unwrap_or(usize::MAX),
                    );
                    Doc::concat([doc, trailing])
                }
            });
        }

        // Whatever is left is a trailing comment at end of file.
        parts.push(self.leading_comments(usize::MAX, false));
        Doc::concat(parts)
    }

    /// A statement block: each item on its own line, terminators applied, blank
    /// lines between items preserved.
    ///
    /// `bound` is the byte offset of the token that follows the block — the
    /// enclosing `END_IF`, `ELSE`, `END_WHILE` and so on, or the end of the
    /// file at top level. The block's extent stops at its last `;`, so without
    /// this the formatter cannot tell a comment trailing the final statement
    /// from one belonging to the construct that closes the block.
    pub fn block(&mut self, node: Node<'_>, bound: usize) -> Doc {
        let items = named_children(node);
        let mut parts = Vec::new();

        for (i, item) in items.iter().enumerate() {
            let prev_end = i.checked_sub(1).map(|p| items[p].end_byte());
            // A trailing comment may only be claimed from the gap before the
            // next item, so two statements sharing a line cannot steal each
            // other's comment.
            let next_start = items.get(i + 1).map_or(bound, Node::start_byte);

            parts.push(self.separator_before(*item, prev_end, i == 0));
            parts.push(self.statement(*item, next_start));
        }

        // Own-line comments between the last statement and the closing keyword
        // belong to this block, not to what follows it. This is the case
        // grammar.js calls out: a comment before END_VAR attaches to the
        // section node rather than to any declaration.
        parts.push(self.leading_comments(bound, items.is_empty()));
        Doc::concat(parts)
    }

    /// One statement plus its terminator and any comment trailing it.
    fn statement(&mut self, node: Node<'_>, next_start: usize) -> Doc {
        let doc = self.node(node);
        let terminator = self.terminator_for(node);
        // The terminator sits between the statement and its trailing comment,
        // so the comment is looked up from the row the statement ended on.
        let trailing = self.trailing_comments(node.end_position().row, next_start);
        Doc::concat([doc, terminator, trailing])
    }

    /// The `;` after a statement.
    ///
    /// Simple statements require one. Compound statements (`END_IF` and
    /// friends) may take an optional one, which is normalized away. `noop` is a
    /// bare `;` and formats as exactly that.
    fn terminator_for(&self, node: Node<'_>) -> Doc {
        match node.kind() {
            "noop" => Doc::text(";"),
            "pragma" => Doc::Nil,
            k if is_compound_statement(k) => Doc::Nil,
            _ => Doc::text(";"),
        }
    }

    /// The line break preceding an item: a blank line if the author left one,
    /// otherwise a plain newline. Emits nothing before the first item.
    ///
    /// Comments in the gap are drained here so they land above the item at the
    /// right indentation, and so the blank-line decision is made against the
    /// nearest neighbour rather than across the comment.
    pub(crate) fn separator_before(
        &mut self,
        item: Node<'_>,
        prev_end: Option<usize>,
        first: bool,
    ) -> Doc {
        let comments = self.leading_comments(item.start_byte(), first);

        // With a comment in the gap, the separator above the item is measured
        // from that comment, not from the previous statement.
        let gap_start = if comments.is_nil() {
            prev_end
        } else {
            self.last_comment_end.or(prev_end)
        };

        let separator = match gap_start {
            _ if first && comments.is_nil() => Doc::Nil,
            Some(start) if blank_line_between(self.source, start, item.start_byte()) => {
                Doc::BlankLine
            }
            _ => Doc::HardLine,
        };

        Doc::concat([comments, separator])
    }
}

/// The named children of a node, with comments filtered out.
///
/// Comments are `extras`, so they show up as ordinary named children at
/// unpredictable positions. They are handled entirely by the trivia cursor and
/// must never be walked as structure.
pub fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect()
}
