//! Comment and blank-line recovery.
//!
//! The grammar declares `extras: [/\s/, $.comment]`, which means comments float
//! to wherever the parser happened to be when it read them. A comment written
//! just before `END_VAR` becomes a sibling of the `var` node; a comment written
//! between `:=` and its right-hand side lands *inside* the `assignment`. No
//! comment is ever a field of anything.
//!
//! So comment placement cannot be read off the tree shape. Instead every
//! comment is collected once, sorted by byte offset, and handed out by a cursor
//! that the formatter drains as it walks the source in order. Classification is
//! by source geometry: what else is on the comment's own line.
//!
//! Whitespace is invisible to the tree too, so blank lines are recovered here
//! by comparing row numbers.

use tree_sitter::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub text: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_row: usize,
    pub end_row: usize,
    /// True when only whitespace precedes the comment on its opening line.
    /// Own-line comments become leading trivia; the rest trail the code before
    /// them.
    pub own_line: bool,
    /// True when a blank line separates this comment from whatever precedes it.
    pub blank_before: bool,
}

impl Comment {
    pub fn is_block(&self) -> bool {
        self.text.starts_with("(*")
    }

    /// True if the comment body spans more than one source line.
    pub fn is_multiline(&self) -> bool {
        self.end_row > self.start_row
    }
}

/// Every comment in a file, in source order, plus a consumed cursor.
#[derive(Debug)]
pub struct Trivia {
    comments: Vec<Comment>,
    /// Index of the first comment not yet emitted.
    cursor: usize,
}

impl Trivia {
    /// Collects every `comment` node in the tree.
    ///
    /// Comments can nest arbitrarily deep because they are extras, so this
    /// walks the entire tree rather than any particular level.
    pub fn collect(root: Node<'_>, source: &str) -> Trivia {
        let mut comments = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];

        while let Some(node) = stack.pop() {
            if node.kind() == "comment" {
                comments.push(build_comment(node, source));
                continue;
            }
            stack.extend(node.children(&mut cursor));
        }

        comments.sort_by_key(|c| c.start_byte);
        Trivia {
            comments,
            cursor: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.comments.is_empty()
    }

    pub fn total(&self) -> usize {
        self.comments.len()
    }

    /// True once every comment has been handed out.
    pub fn fully_drained(&self) -> bool {
        self.cursor >= self.comments.len()
    }

    /// The comments not yet emitted, for diagnostics.
    pub fn remaining(&self) -> &[Comment] {
        &self.comments[self.cursor.min(self.comments.len())..]
    }

    /// Removes and returns every unemitted comment starting before `byte`.
    ///
    /// This is the workhorse: the formatter calls it immediately before
    /// emitting any construct, and again before each closing keyword, so a
    /// comment can never be silently dropped.
    pub fn take_before(&mut self, byte: usize) -> Vec<Comment> {
        let start = self.cursor;
        while self.cursor < self.comments.len() && self.comments[self.cursor].start_byte < byte {
            self.cursor += 1;
        }
        self.comments[start..self.cursor].to_vec()
    }

    /// Removes and returns unemitted comments that both start before `byte` and
    /// sit on `row` — i.e. trailing comments on the line just emitted.
    pub fn take_trailing_on_row(&mut self, row: usize, byte: usize) -> Vec<Comment> {
        let mut taken = Vec::new();
        while self.cursor < self.comments.len() {
            let c = &self.comments[self.cursor];
            if c.start_byte < byte && c.start_row == row && !c.own_line {
                taken.push(c.clone());
                self.cursor += 1;
            } else {
                break;
            }
        }
        taken
    }

    /// Peeks at the next unemitted comment.
    pub fn peek(&self) -> Option<&Comment> {
        self.comments.get(self.cursor)
    }

    /// The cursor position, for a build that has to be undone.
    ///
    /// The comment list itself never changes after collection, so this one
    /// index is the whole of the cursor's state.
    pub fn checkpoint(&self) -> usize {
        self.cursor
    }

    /// Rewinds to a [`Trivia::checkpoint`]: every comment handed out since is
    /// handed out again.
    pub fn restore(&mut self, checkpoint: usize) {
        self.cursor = checkpoint;
    }
}

fn build_comment(node: Node<'_>, source: &str) -> Comment {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let line_start = source[..start_byte].rfind('\n').map_or(0, |i| i + 1);
    let before = &source[line_start..start_byte];

    Comment {
        text: source[start_byte..end_byte].to_owned(),
        start_byte,
        end_byte,
        start_row: node.start_position().row,
        end_row: node.end_position().row,
        own_line: before.trim().is_empty(),
        blank_before: has_blank_line_before(source, line_start),
    }
}

/// True if the line before the one starting at `line_start` is blank.
fn has_blank_line_before(source: &str, line_start: usize) -> bool {
    if line_start == 0 {
        return false;
    }
    let before = &source[..line_start - 1];
    let prev_line_start = before.rfind('\n').map_or(0, |i| i + 1);
    before[prev_line_start..].trim().is_empty()
}

/// True if at least one wholly empty line separates the two byte offsets.
///
/// Counting newlines is not enough. The gap between two declarations that have
/// a comment between them holds two newlines without containing a blank line,
/// and treating that as a blank would split alignment groups wherever an author
/// wrote a note. Only the *complete* lines inside the gap are examined: the
/// first fragment is the tail of the line the previous node ended on, and the
/// last is the indentation before the next node, so neither counts.
pub fn blank_line_between(source: &str, end_byte: usize, start_byte: usize) -> bool {
    if end_byte >= start_byte || start_byte > source.len() {
        return false;
    }
    let gap: Vec<&str> = source[end_byte..start_byte].split('\n').collect();
    if gap.len() < 3 {
        return false;
    }
    gap[1..gap.len() - 1]
        .iter()
        .any(|line| line.trim().is_empty())
}

/// The number of source rows spanned by the gap, used to decide whether two
/// constructs were written on the same line.
pub fn same_line(a: Node<'_>, b: Node<'_>) -> bool {
    a.end_position().row == b.start_position().row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_valid;

    fn trivia_of(src: &str) -> Trivia {
        let tree = parse_valid(src).expect("test source must parse");
        Trivia::collect(tree.root_node(), src)
    }

    #[test]
    fn collects_line_and_block_comments() {
        let t = trivia_of("// one\nx := 1; (* two *)\n");
        assert_eq!(t.total(), 2);
        assert_eq!(t.comments[0].text, "// one");
        assert_eq!(t.comments[1].text, "(* two *)");
    }

    #[test]
    fn a_restored_cursor_hands_the_same_comments_out_again() {
        // What makes the speculative build the alignment pass relies on exact:
        // measuring a document drains trivia, and rewinding must put every
        // comment back within reach.
        let mut t = trivia_of("// one\nx := 1;\n// two\ny := 2;\n");
        let checkpoint = t.checkpoint();

        let taken = t.take_before(usize::MAX);
        assert_eq!(taken.len(), 2);
        assert!(t.fully_drained());

        t.restore(checkpoint);
        assert!(!t.fully_drained());
        let again = t.take_before(usize::MAX);
        assert_eq!(again.len(), 2);
        assert_eq!(again[0].text, "// one");
        assert_eq!(again[1].text, "// two");
    }

    #[test]
    fn classifies_own_line_versus_trailing() {
        let t = trivia_of("// leading\nx := 1; // trailing\n");
        assert!(
            t.comments[0].own_line,
            "a comment alone on its line is own-line"
        );
        assert!(!t.comments[1].own_line, "a comment after code trails it");
    }

    #[test]
    fn finds_comments_nested_inside_expressions() {
        // The grammar puts this comment inside the `assignment` node, between
        // `:=` and the right-hand side. A shallow scan would miss it.
        let t = trivia_of("x := (* inline *) y;");
        assert_eq!(t.total(), 1);
        assert_eq!(t.comments[0].text, "(* inline *)");
    }

    #[test]
    fn finds_the_comment_before_end_var() {
        // grammar.js calls this case out explicitly: the comment attaches to
        // the `var` node as a sibling, not to any declaration.
        let src = "FUNCTION_BLOCK FB\nVAR\n  n : INT;\n  // last word\nEND_VAR\nEND_FUNCTION_BLOCK";
        let t = trivia_of(src);
        assert_eq!(t.total(), 1);
        assert_eq!(t.comments[0].text, "// last word");
    }

    #[test]
    fn comments_come_back_in_source_order() {
        let t = trivia_of("// a\nx := 1; // b\n// c\ny := 2;\n");
        let texts: Vec<&str> = t.comments.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["// a", "// b", "// c"]);
    }

    #[test]
    fn take_before_drains_up_to_the_offset() {
        let src = "// a\n// b\nx := 1;\n";
        let mut t = trivia_of(src);
        let x_offset = src.find("x :=").unwrap();
        let taken = t.take_before(x_offset);
        assert_eq!(taken.len(), 2);
        assert!(t.fully_drained());
    }

    #[test]
    fn take_before_leaves_later_comments_alone() {
        let src = "// a\nx := 1;\n// b\n";
        let mut t = trivia_of(src);
        let x_offset = src.find("x :=").unwrap();
        assert_eq!(t.take_before(x_offset).len(), 1);
        assert!(!t.fully_drained());
        assert_eq!(t.remaining().len(), 1);
        assert_eq!(t.remaining()[0].text, "// b");
    }

    #[test]
    fn take_trailing_on_row_only_takes_same_row_trailers() {
        let src = "x := 1; // here\n// next line\ny := 2;\n";
        let mut t = trivia_of(src);
        let y_offset = src.find("y :=").unwrap();
        let trailing = t.take_trailing_on_row(0, y_offset);
        assert_eq!(trailing.len(), 1);
        assert_eq!(trailing[0].text, "// here");
    }

    #[test]
    fn detects_blank_lines_before_a_comment() {
        let t = trivia_of("x := 1;\n\n// spaced\ny := 2;\n");
        assert!(t.comments[0].blank_before);

        let t = trivia_of("x := 1;\n// tight\ny := 2;\n");
        assert!(!t.comments[0].blank_before);
    }

    #[test]
    fn blank_line_between_scans_actual_text() {
        let src = "a\n\nb";
        assert!(blank_line_between(src, 1, 3));
        let src = "a\nb";
        assert!(!blank_line_between(src, 1, 2));
    }

    #[test]
    fn a_comment_in_the_gap_is_not_a_blank_line() {
        // Two newlines, but no empty line — this must not split an alignment
        // group or turn into a blank separator.
        let src = "a;\n// note\nb;";
        let gap_start = src.find(';').unwrap() + 1;
        let gap_end = src.rfind('b').unwrap();
        assert!(!blank_line_between(src, gap_start, gap_end));
    }

    #[test]
    fn a_blank_line_beside_a_comment_still_counts() {
        let src = "a;\n\n// note\nb;";
        let gap_start = src.find(';').unwrap() + 1;
        let gap_end = src.rfind('b').unwrap();
        assert!(blank_line_between(src, gap_start, gap_end));
    }

    #[test]
    fn a_whitespace_only_line_counts_as_blank() {
        let src = "a;\n   \nb;";
        let gap_start = src.find(';').unwrap() + 1;
        let gap_end = src.rfind('b').unwrap();
        assert!(blank_line_between(src, gap_start, gap_end));
    }

    #[test]
    fn multiline_block_comments_are_flagged() {
        let t = trivia_of("(* line one\n   line two *)\nx := 1;");
        assert!(t.comments[0].is_block());
        assert!(t.comments[0].is_multiline());
    }
}
