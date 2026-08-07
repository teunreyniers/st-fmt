//! A Wadler/Prettier-style document IR and its renderer.
//!
//! The formatter never emits strings directly. It builds a [`Doc`] describing
//! where a line *may* break, and the renderer decides which of those breaks to
//! take so that lines fit within [`crate::style::MAX_WIDTH`].
//!
//! The one thing this IR deliberately cannot express is column alignment
//! (lining up `:` across a run of VAR declarations). That is a measure-then-emit
//! pass built on [`Doc::flat_width`], not a rendering mode.

use std::borrow::Cow;

use crate::style::INDENT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    /// Renders to nothing.
    Nil,
    Text(Cow<'static, str>),
    Concat(Vec<Doc>),
    /// A space when the enclosing group is flat, a newline when it is broken.
    Line,
    /// Nothing when the enclosing group is flat, a newline when it is broken.
    SoftLine,
    /// Always a newline. Forces every enclosing group to break.
    HardLine,
    /// A newline plus one empty line. Forces every enclosing group to break.
    BlankLine,
    /// A newline plus two empty lines. Forces every enclosing group to break.
    ///
    /// Used only between top-level declarations, which are set further apart
    /// than anything inside one.
    DoubleBlankLine,
    /// Indents every line break inside by one level.
    Indent(Box<Doc>),
    /// A break-together unit: rendered flat if it fits, otherwise broken.
    Group(Box<Doc>),
    /// Picks a rendering based on whether the enclosing group broke. Used for
    /// things like a trailing comma that only appears in the broken form.
    IfBreak {
        broken: Box<Doc>,
        flat: Box<Doc>,
    },
    /// Paragraph fill: pack as many items onto each line as fit, breaking only
    /// where needed. Unlike a group, which is all-or-nothing, each gap is
    /// decided independently.
    ///
    /// Used for array initializers, where a numeric table reads far better
    /// packed than one value per line.
    Fill(Vec<Doc>),
}

impl Doc {
    pub fn text(s: impl Into<Cow<'static, str>>) -> Doc {
        Doc::Text(s.into())
    }

    pub fn concat(parts: impl IntoIterator<Item = Doc>) -> Doc {
        let parts: Vec<Doc> = parts.into_iter().filter(|d| !d.is_nil()).collect();
        match parts.len() {
            0 => Doc::Nil,
            1 => parts.into_iter().next().unwrap(),
            _ => Doc::Concat(parts),
        }
    }

    /// Concatenates `parts`, placing `sep` between each pair.
    pub fn join(sep: Doc, parts: impl IntoIterator<Item = Doc>) -> Doc {
        let mut out = Vec::new();
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                out.push(sep.clone());
            }
            out.push(part);
        }
        Doc::concat(out)
    }

    pub fn group(self) -> Doc {
        Doc::Group(Box::new(self))
    }

    pub fn indent(self) -> Doc {
        Doc::Indent(Box::new(self))
    }

    pub fn if_break(broken: Doc, flat: Doc) -> Doc {
        Doc::IfBreak {
            broken: Box::new(broken),
            flat: Box::new(flat),
        }
    }

    /// Packs `items` onto as few lines as possible, separated by a space or a
    /// newline as each one fits.
    pub fn fill(items: impl IntoIterator<Item = Doc>) -> Doc {
        Doc::Fill(items.into_iter().collect())
    }

    pub fn space() -> Doc {
        Doc::text(" ")
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Doc::Nil)
    }

    /// True if this document contains a break that cannot be flattened, which
    /// means any group containing it must render broken.
    fn has_forced_break(&self) -> bool {
        match self {
            Doc::HardLine | Doc::BlankLine | Doc::DoubleBlankLine => true,
            Doc::Concat(parts) | Doc::Fill(parts) => parts.iter().any(Doc::has_forced_break),
            Doc::Indent(inner) | Doc::Group(inner) => inner.has_forced_break(),
            // A group that breaks internally does not force its parent to
            // break, but a hard line inside an `IfBreak` arm we might select
            // does. Being conservative here only costs an extra line break.
            Doc::IfBreak { broken, flat } => broken.has_forced_break() || flat.has_forced_break(),
            _ => false,
        }
    }

    /// The width this document would occupy rendered entirely flat. Returns
    /// `None` if it cannot be rendered flat at all.
    ///
    /// This is what the alignment pass measures declaration names with.
    pub fn flat_width(&self) -> Option<usize> {
        match self {
            Doc::Nil | Doc::SoftLine => Some(0),
            Doc::Text(s) => Some(s.chars().count()),
            Doc::Line => Some(1),
            Doc::HardLine | Doc::BlankLine | Doc::DoubleBlankLine => None,
            Doc::Concat(parts) => parts
                .iter()
                .try_fold(0, |acc, p| Some(acc + p.flat_width()?)),
            // Flat, a fill's items are separated by single spaces.
            Doc::Fill(parts) => parts
                .iter()
                .try_fold(0, |acc, p| Some(acc + p.flat_width()?))
                .map(|w| w + parts.len().saturating_sub(1)),
            Doc::Indent(inner) | Doc::Group(inner) => inner.flat_width(),
            Doc::IfBreak { flat, .. } => flat.flat_width(),
        }
    }
}

impl FromIterator<Doc> for Doc {
    fn from_iter<T: IntoIterator<Item = Doc>>(iter: T) -> Doc {
        Doc::concat(iter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Break,
}

/// Renders `doc` to a string, breaking groups that do not fit within `width`.
///
/// The output has no trailing whitespace on any line and always ends with
/// exactly one newline.
/// One unit of pending work in the renderer.
enum Frame<'a> {
    Doc(usize, Mode, &'a Doc),
    /// The remaining items of a fill. Popped only after the previous item has
    /// been written, so the column is known when deciding the next gap.
    FillTail(usize, Mode, &'a [Doc]),
}

pub fn render(doc: &Doc, width: usize) -> String {
    let mut out = String::new();
    // Rendering is a depth-first walk over a work stack; `col` tracks the
    // current output column so `fits` knows how much room is left.
    let mut stack: Vec<Frame<'_>> = vec![Frame::Doc(0, Mode::Break, doc)];
    let mut col = 0usize;

    while let Some(frame) = stack.pop() {
        let (ind, mode, doc) = match frame {
            Frame::Doc(ind, mode, doc) => (ind, mode, doc),
            Frame::FillTail(ind, mode, items) => {
                let Some((next, rest)) = items.split_first() else {
                    continue;
                };
                // Decide this gap on its own: keep the space if the next item
                // still fits, otherwise wrap here and carry on packing.
                let flat = next.flat_width();
                let fits_here = flat.is_some_and(|w| col + 1 + w <= width);
                if fits_here {
                    out.push(' ');
                    col += 1;
                } else {
                    col = newline(&mut out, ind, 1);
                }
                stack.push(Frame::FillTail(ind, mode, rest));
                stack.push(Frame::Doc(ind, mode, next));
                continue;
            }
        };

        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                col += s.chars().count();
            }
            Doc::Concat(parts) => {
                for part in parts.iter().rev() {
                    stack.push(Frame::Doc(ind, mode, part));
                }
            }
            Doc::Fill(items) => {
                // Write the first item unconditionally, then let FillTail
                // decide each following gap once the column is known.
                if let Some((first, rest)) = items.split_first() {
                    stack.push(Frame::FillTail(ind, mode, rest));
                    stack.push(Frame::Doc(ind, mode, first));
                }
            }
            Doc::Indent(inner) => stack.push(Frame::Doc(ind + 1, mode, inner)),
            Doc::Group(inner) => {
                let group_mode = if !inner.has_forced_break()
                    && fits(width.saturating_sub(col), inner, &stack)
                {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                stack.push(Frame::Doc(ind, group_mode, inner));
            }
            Doc::IfBreak { broken, flat } => {
                let chosen = if mode == Mode::Break { broken } else { flat };
                stack.push(Frame::Doc(ind, mode, chosen));
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    out.push(' ');
                    col += 1;
                }
                Mode::Break => col = newline(&mut out, ind, 1),
            },
            Doc::SoftLine => match mode {
                Mode::Flat => {}
                Mode::Break => col = newline(&mut out, ind, 1),
            },
            Doc::HardLine => col = newline(&mut out, ind, 1),
            Doc::BlankLine => col = newline(&mut out, ind, 2),
            Doc::DoubleBlankLine => col = newline(&mut out, ind, 3),
        }
    }

    trim_trailing_whitespace(&mut out);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Emits `count` newlines followed by `ind` levels of indentation, trimming any
/// whitespace left dangling at the end of the previous line. Returns the new
/// column.
fn newline(out: &mut String, ind: usize, count: usize) -> usize {
    while out.ends_with(' ') || out.ends_with('\t') {
        out.pop();
    }
    for _ in 0..count {
        out.push('\n');
    }
    let pad = ind * INDENT;
    for _ in 0..pad {
        out.push(' ');
    }
    pad
}

/// Decides whether `doc` — plus whatever already-queued work follows it on the
/// same line — fits in `remaining` columns.
///
/// Looking at the enclosing stack is what stops a group from being flattened
/// when the text that trails it would push the line over the limit.
fn fits(remaining: usize, doc: &Doc, rest: &[Frame<'_>]) -> bool {
    let mut budget = remaining as isize;
    // The candidate group is measured flat; everything after it keeps the mode
    // it was queued with, so a pending hard break correctly ends the line.
    let mut queue: Vec<(Mode, &Doc)> = vec![(Mode::Flat, doc)];
    let mut rest_iter = rest.iter().rev();

    loop {
        let (mode, doc) = match queue.pop() {
            Some(item) => item,
            None => match rest_iter.next() {
                Some(Frame::Doc(_, mode, doc)) => (*mode, *doc),
                // A fill's next gap may break, so the line can end there.
                Some(Frame::FillTail(..)) => return true,
                None => return true,
            },
        };

        match doc {
            Doc::Nil | Doc::SoftLine if mode == Mode::Flat => {}
            Doc::Nil => {}
            Doc::Text(s) => {
                budget -= s.chars().count() as isize;
                if budget < 0 {
                    return false;
                }
            }
            Doc::Concat(parts) => {
                for part in parts.iter().rev() {
                    queue.push((mode, part));
                }
            }
            Doc::Fill(parts) => {
                // Measured flat, a fill is its items joined by single spaces.
                for (i, part) in parts.iter().enumerate().rev() {
                    queue.push((mode, part));
                    if i > 0 {
                        budget -= 1;
                    }
                }
                if budget < 0 {
                    return false;
                }
            }
            Doc::Indent(inner) => queue.push((mode, inner)),
            // A nested group inside a candidate is measured flat too; if it
            // turns out not to fit, the outer group breaks and the nested one
            // is re-decided on its own terms.
            Doc::Group(inner) => queue.push((mode, inner)),
            Doc::IfBreak { broken, flat } => {
                queue.push((mode, if mode == Mode::Break { broken } else { flat }));
            }
            Doc::Line if mode == Mode::Flat => {
                budget -= 1;
                if budget < 0 {
                    return false;
                }
            }
            // Any break in Break mode ends the line, so everything fit.
            Doc::Line | Doc::SoftLine | Doc::HardLine | Doc::BlankLine | Doc::DoubleBlankLine => {
                return true;
            }
        }
    }
}

fn trim_trailing_whitespace(out: &mut String) {
    if !out.lines().any(|l| l.ends_with(' ') || l.ends_with('\t')) {
        return;
    }
    let trimmed: Vec<&str> = out.lines().map(|l| l.trim_end()).collect();
    let had_final_newline = out.ends_with('\n');
    *out = trimmed.join("\n");
    if had_final_newline {
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(parts: impl IntoIterator<Item = Doc>) -> Doc {
        Doc::concat(parts)
    }

    #[test]
    fn flat_group_stays_on_one_line() {
        let doc = d([
            Doc::text("a("),
            Doc::SoftLine,
            Doc::text("b"),
            Doc::SoftLine,
            Doc::text(")"),
        ])
        .group();
        assert_eq!(render(&doc, 80), "a(b)\n");
    }

    #[test]
    fn group_breaks_when_too_wide() {
        let inner = d([
            Doc::SoftLine,
            Doc::text("aaaa,"),
            Doc::Line,
            Doc::text("bbbb"),
        ])
        .indent();
        let doc = d([Doc::text("f("), inner, Doc::SoftLine, Doc::text(")")]).group();
        assert_eq!(render(&doc, 10), "f(\n    aaaa,\n    bbbb\n)\n");
    }

    #[test]
    fn hardline_forces_enclosing_group_to_break() {
        let doc = d([Doc::text("a"), Doc::HardLine, Doc::text("b")]).group();
        assert_eq!(render(&doc, 80), "a\nb\n");
    }

    #[test]
    fn if_break_selects_by_mode() {
        let doc = |w| {
            let inner = d([
                Doc::SoftLine,
                Doc::text("x"),
                Doc::if_break(Doc::text(","), Doc::Nil),
            ])
            .indent();
            render(
                &d([Doc::text("("), inner, Doc::SoftLine, Doc::text(")")]).group(),
                w,
            )
        };
        assert_eq!(doc(80), "(x)\n");
        assert_eq!(doc(2), "(\n    x,\n)\n");
    }

    #[test]
    fn blank_line_emits_exactly_one_empty_line() {
        let doc = d([Doc::text("a"), Doc::BlankLine, Doc::text("b")]);
        assert_eq!(render(&doc, 80), "a\n\nb\n");
    }

    #[test]
    fn double_blank_line_emits_exactly_two_empty_lines() {
        let doc = d([Doc::text("a"), Doc::DoubleBlankLine, Doc::text("b")]);
        assert_eq!(render(&doc, 80), "a\n\n\nb\n");
    }

    #[test]
    fn indentation_applies_to_breaks_only() {
        let doc = d([
            Doc::text("VAR"),
            d([Doc::HardLine, Doc::text("x")]).indent(),
            Doc::HardLine,
            Doc::text("END_VAR"),
        ]);
        assert_eq!(render(&doc, 80), "VAR\n    x\nEND_VAR\n");
    }

    #[test]
    fn trailing_text_is_accounted_for_when_measuring() {
        // The group `(x)` fits in isolation but not once `;` and the long tail
        // that follows are counted, so it must break.
        let group = d([
            Doc::text("("),
            d([Doc::SoftLine, Doc::text("x")]).indent(),
            Doc::SoftLine,
            Doc::text(")"),
        ])
        .group();
        let doc = d([Doc::text("aaaaaaaa"), group, Doc::text(";")]);
        assert_eq!(render(&doc, 10), "aaaaaaaa(\n    x\n);\n");
    }

    #[test]
    fn flat_width_measures_flat_rendering() {
        assert_eq!(Doc::text("abc").flat_width(), Some(3));
        assert_eq!(
            d([Doc::text("a"), Doc::Line, Doc::text("b")]).flat_width(),
            Some(3)
        );
        assert_eq!(
            d([Doc::text("a"), Doc::SoftLine, Doc::text("b")]).flat_width(),
            Some(2)
        );
        assert_eq!(d([Doc::text("a"), Doc::HardLine]).flat_width(), None);
    }

    #[test]
    fn fill_packs_items_up_to_the_width() {
        let items = (1..=8).map(|n| Doc::text(format!("{n}000,")));
        let doc = Doc::fill(items);
        // 4 items of 5 chars plus 3 separating spaces is 23; a 5th would need 29.
        assert_eq!(
            render(&doc, 25),
            "1000, 2000, 3000, 4000,\n5000, 6000, 7000, 8000,\n"
        );
    }

    #[test]
    fn fill_stays_on_one_line_when_everything_fits() {
        let doc = Doc::fill([Doc::text("a,"), Doc::text("b,"), Doc::text("c")]);
        assert_eq!(render(&doc, 80), "a, b, c\n");
    }

    #[test]
    fn fill_breaks_only_where_needed() {
        // A single over-long item forces a break before it, but the items
        // around it still pack.
        let doc = Doc::fill([Doc::text("a,"), Doc::text("bbbbbbbbbb,"), Doc::text("c")]);
        assert_eq!(render(&doc, 12), "a,\nbbbbbbbbbb,\nc\n");
    }

    #[test]
    fn render_always_ends_with_single_newline() {
        assert_eq!(render(&Doc::text("x"), 80), "x\n");
        assert_eq!(render(&d([Doc::text("x"), Doc::HardLine]), 80), "x\n");
    }
}
