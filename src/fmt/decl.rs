//! Variable sections, declarations, type names and initializers.
//!
//! The distinctive work here is **column alignment**. A run of declarations is
//! laid out so its `AT`, `:` and `:=` line up:
//!
//! ```text
//! bStart AT %IX0.0 : BOOL  := TRUE;
//! nCount           : INT;
//! rSetpoint        : REAL  := 20.5;
//! ```
//!
//! That is a width the Wadler IR cannot express, so it is a measure-then-emit
//! pass: each declaration's parts are built as documents, measured flat with
//! [`Doc::flat_width`], and only then padded. A run is broken by a blank line —
//! comments and pragmas stay inside the group so a note never silently shifts
//! the column beneath it.

use tree_sitter::Node;

use super::{Formatter, pad, source::named_children};
use crate::doc::Doc;
use crate::style;
use crate::trivia::blank_line_between;

/// A structure initializer breaks and aligns once it has more than this many
/// elements, matching the rule for named call parameters.
const STRUCT_ELEMENT_BREAK_THRESHOLD: usize = 2;

impl Formatter<'_> {
    /// A `VAR` … `END_VAR` section of any flavour.
    pub fn var_section(&mut self, node: Node<'_>) -> Doc {
        let keyword = section_keyword(node);
        let mut header = vec![Doc::text(keyword)];

        if let Some(qualifier) = node.child_by_field_name("qualifier") {
            header.push(Doc::space());
            header.push(self.var_qualifier(qualifier));
        }

        let end = token(node, "end_var");
        let bound = end.map_or(node.end_byte(), |t| t.start_byte());
        let items: Vec<Node<'_>> = named_children(node)
            .into_iter()
            .filter(|c| c.kind() != "var_qualifier")
            .collect();

        let body = self.aligned_declarations(&items, bound);

        Doc::concat([
            Doc::concat(header),
            Doc::concat([Doc::HardLine, body]).indent(),
            Doc::HardLine,
            Doc::text("END_VAR"),
        ])
    }

    /// `CONSTANT`, `RETAIN PERSISTENT`, … — one or more qualifier keywords.
    fn var_qualifier(&mut self, node: Node<'_>) -> Doc {
        let mut cursor = node.walk();
        let words: Vec<Doc> = node
            .children(&mut cursor)
            .map(|c| Doc::text(style::keyword(c.kind())))
            .collect();
        Doc::join(Doc::space(), words)
    }

    /// Lays out a run of declarations with their columns aligned.
    ///
    /// Alignment groups are delimited by blank lines only. Comments and
    /// pragmas belong to the group they sit in, so adding a note never shifts
    /// the column of the declarations under it.
    pub fn aligned_declarations(&mut self, items: &[Node<'_>], bound: usize) -> Doc {
        let groups = alignment_groups(self.source, items);
        let mut parts = Vec::new();

        for (i, item) in items.iter().enumerate() {
            let prev_end = i.checked_sub(1).map(|p| items[p].end_byte());
            let next_start = items.get(i + 1).map_or(bound, Node::start_byte);

            parts.push(self.separator_before(*item, prev_end, i == 0));

            let doc = match item.kind() {
                k if is_aligned_declaration(k) => {
                    let widths = groups[i];
                    self.variable_declaration(*item, widths)
                }
                _ => self.node(*item),
            };

            let trailing = self.trailing_comments(item.end_position().row, next_start);
            parts.push(Doc::concat([doc, trailing]));
        }

        parts.push(self.leading_comments(bound, items.is_empty()));
        Doc::concat(parts)
    }

    /// One declaration: `[pragmas] name [AT loc] : type [:= init];`
    ///
    /// `widths` carries the alignment columns measured across the whole group.
    fn variable_declaration(&mut self, node: Node<'_>, widths: Widths) -> Doc {
        let Some(name) = node.child_by_field_name("name") else {
            return self.verbatim(node);
        };
        let Some(ty) = node.child_by_field_name("type") else {
            return self.verbatim(node);
        };

        let mut parts = Vec::new();

        // Pragmas sit on their own lines above the declaration.
        for pragma in named_children(node) {
            if pragma.kind() != "pragma" {
                continue;
            }
            parts.push(Doc::text(self.text(pragma).to_owned()));
            parts.push(Doc::HardLine);
        }

        // A doc comment written between a pragma and the name it documents
        // falls *inside* this declaration node, because `repeat($.pragma)` is
        // part of the rule. Draining here keeps it above the name where the
        // author put it, instead of letting it slide below the declaration.
        let after_pragmas = self.leading_comments(name.start_byte(), true);
        if !after_pragmas.is_nil() {
            parts.push(after_pragmas);
            parts.push(Doc::HardLine);
        }

        let name_text = self.text(name).to_owned();
        let location = node.child_by_field_name("location");

        // Name column, then the AT column, then `:`.
        parts.push(Doc::text(pad(&name_text, widths.name)));
        match location {
            Some(loc) => {
                let loc_text = self.text(loc).to_ascii_uppercase();
                parts.push(Doc::text(format!(
                    " AT {}",
                    pad(&loc_text, widths.location)
                )));
            }
            // Keep the `:` column even where a declaration has no location.
            None if widths.location > 0 => {
                parts.push(Doc::text(" ".repeat(widths.location + 4)));
            }
            None => {}
        }

        parts.push(Doc::text(" : "));

        let type_doc = self.type_name(ty);
        let initial = node.child_by_field_name("initial_value");

        if let Some(initial) = initial {
            let type_width = type_doc.flat_width().unwrap_or(0);
            parts.push(type_doc);
            parts.push(Doc::text(pad("", widths.ty.saturating_sub(type_width))));
            parts.push(Doc::text(" := "));
            parts.push(self.initial_value(initial));
        } else {
            parts.push(type_doc);
        }

        parts.push(Doc::text(";"));
        Doc::concat(parts)
    }

    /// A type in declaration position.
    pub fn type_name(&mut self, node: Node<'_>) -> Doc {
        match node.kind() {
            // `type_name` is a thin wrapper around the real type.
            "type_name" => match node.named_child(0) {
                Some(inner) => self.type_name(inner),
                None => self.verbatim(node),
            },
            // A bare identifier here is a type: uppercase it when it names one
            // of the elementary types, otherwise leave the author's spelling.
            "identifier" => Doc::text(style::type_name_case(self.text(node))),
            "qualified_type_name" => {
                let parts: Vec<Doc> = named_children(node)
                    .into_iter()
                    .map(|p| Doc::text(self.text(p).to_owned()))
                    .collect();
                Doc::join(Doc::text("."), parts)
            }
            "array_type" => self.array_type(node),
            "sized_type" => self.sized_type(node),
            "pointer_type" => self.prefixed_type(node, "POINTER TO "),
            "reference_type" => self.prefixed_type(node, "REFERENCE TO "),
            _ => self.node(node),
        }
    }

    /// `ARRAY [0..9, 1..2] OF <type>`
    fn array_type(&mut self, node: Node<'_>) -> Doc {
        let ranges: Vec<Node<'_>> = named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "array_range")
            .collect();
        let range_docs: Vec<Doc> = ranges.into_iter().map(|r| self.array_range(r)).collect();

        let element = match node.child_by_field_name("element_type") {
            Some(e) => self.type_name(e),
            None => return self.verbatim(node),
        };

        Doc::concat([
            Doc::text("ARRAY ["),
            Doc::join(Doc::text(", "), range_docs),
            Doc::text("] OF "),
            element,
        ])
    }

    /// `0..9`, including bounds that are expressions.
    pub fn array_range(&mut self, node: Node<'_>) -> Doc {
        let bounds: Vec<Doc> = named_children(node)
            .into_iter()
            .map(|b| self.node(b))
            .collect();
        Doc::join(Doc::text(".."), bounds)
    }

    /// An arithmetic array bound such as `MAX_DEVICES - 1` or `(MAX * 2) - 1`.
    ///
    /// A separate grammar rule from `binary_operator` with the same three
    /// shapes: binary, unary negation, and parenthesized. Bounds never wrap, so
    /// there are no groups here.
    pub fn array_bound_expression(&mut self, node: Node<'_>) -> Doc {
        // Binary: `left op right`.
        let (left, op, right) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("operator"),
            node.child_by_field_name("right"),
        );
        if let (Some(left), Some(op), Some(right)) = (left, op, right) {
            let left_doc = self.node(left);
            let op_text = style::keyword(op.kind());
            let right_doc = self.node(right);
            return Doc::concat([left_doc, Doc::text(format!(" {op_text} ")), right_doc]);
        }

        // Unary negation: `-bound`.
        if let Some(operand) = node.child_by_field_name("operand") {
            let doc = self.node(operand);
            return Doc::concat([Doc::text("-"), doc]);
        }

        // Parenthesized: `(bound)`.
        match node.named_child(0) {
            Some(inner) => {
                let doc = self.node(inner);
                Doc::concat([Doc::text("("), doc, Doc::text(")")])
            }
            None => self.verbatim(node),
        }
    }

    /// `STRING[80]` and `WSTRING(80)`.
    ///
    /// Both spellings are legal IEC and the author's choice is preserved; only
    /// the spacing is normalized.
    fn sized_type(&mut self, node: Node<'_>) -> Doc {
        let Some(name) = node.child_by_field_name("name") else {
            return self.verbatim(node);
        };
        let Some(size) = node.child_by_field_name("size") else {
            return self.verbatim(node);
        };

        let parenthesized = token(node, "(").is_some();
        // A parenthesized *range* is a subrange type — `INT (0..100)` — which
        // conventionally takes a space, unlike a size — `STRING(80)`.
        let subrange = parenthesized && size.kind() == "array_range";
        let (open, close) = match (parenthesized, subrange) {
            (true, true) => (" (", ")"),
            (true, false) => ("(", ")"),
            (false, _) => ("[", "]"),
        };

        let name_doc = Doc::text(style::type_name_case(self.text(name)));
        let size_doc = self.node(size);
        Doc::concat([name_doc, Doc::text(open), size_doc, Doc::text(close)])
    }

    /// `POINTER TO <type>` and `REFERENCE TO <type>`.
    fn prefixed_type(&mut self, node: Node<'_>, prefix: &'static str) -> Doc {
        match node.child_by_field_name("type") {
            Some(inner) => {
                let doc = self.type_name(inner);
                Doc::concat([Doc::text(prefix), doc])
            }
            None => self.verbatim(node),
        }
    }

    /// An initial value: a plain expression, an array initializer or a
    /// structure initializer.
    fn initial_value(&mut self, node: Node<'_>) -> Doc {
        match node.kind() {
            "array_initializer" => self.array_initializer(node),
            "structure_initializer" => self.structure_initializer(node),
            _ => self.node(node),
        }
    }

    /// `[1, 2, 3]` — a value table, packed as many per line as fit.
    pub fn array_initializer(&mut self, node: Node<'_>) -> Doc {
        let items = named_children(node);
        if items.is_empty() {
            return Doc::text("[]");
        }

        let last = items.len() - 1;
        let filled: Vec<Doc> = items
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                let doc = self.initial_value(item);
                // The values are positional, so the list never ends on a
                // trailing comma, broken or not.
                let comma = if i == last { Doc::Nil } else { Doc::text(",") };
                Doc::concat([doc, comma])
            })
            .collect();

        Doc::concat([
            Doc::text("["),
            Doc::concat([Doc::SoftLine, Doc::fill(filled)]).indent(),
            Doc::SoftLine,
            Doc::text("]"),
        ])
        .group()
    }

    /// `10(0)` — a repeated element inside an array initializer.
    pub fn array_initializer_repetition(&mut self, node: Node<'_>) -> Doc {
        let Some(count) = node.child_by_field_name("count") else {
            return self.verbatim(node);
        };
        let count_doc = self.node(count);
        let values: Vec<Doc> = named_children(node)
            .into_iter()
            .skip(1)
            .map(|v| self.initial_value(v))
            .collect();

        Doc::concat([
            count_doc,
            Doc::text("("),
            Doc::join(Doc::text(", "), values),
            Doc::text(")"),
        ])
    }

    /// `(x := 1, y := 2)` — laid out like a named parameter list, with the
    /// `:=` aligned once it breaks.
    pub fn structure_initializer(&mut self, node: Node<'_>) -> Doc {
        let elements = named_children(node);
        if elements.is_empty() {
            return Doc::text("()");
        }

        let names: Vec<String> = elements
            .iter()
            .map(|e| {
                e.child_by_field_name("name")
                    .map_or_else(String::new, |n| self.text(n).to_owned())
            })
            .collect();
        let width = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);

        let force = elements.len() > STRUCT_ELEMENT_BREAK_THRESHOLD;
        let mut items = Vec::new();

        for (i, element) in elements.iter().enumerate() {
            if i > 0 {
                items.push(Doc::text(","));
                items.push(if force { Doc::HardLine } else { Doc::Line });
            }
            let value = match element.child_by_field_name("value") {
                Some(v) => self.initial_value(v),
                None => self.verbatim(*element),
            };
            let name = &names[i];
            items.push(Doc::if_break(
                Doc::text(format!("{} := ", pad(name, width))),
                Doc::text(format!("{name} := ")),
            ));
            items.push(value);
        }
        items.push(Doc::if_break(Doc::text(","), Doc::Nil));

        let edge = if force { Doc::HardLine } else { Doc::SoftLine };
        Doc::concat([
            Doc::text("("),
            Doc::concat([edge.clone(), Doc::concat(items)]).indent(),
            edge,
            Doc::text(")"),
        ])
        .group()
    }
}

/// The alignment columns for one declaration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Widths {
    /// Width of the name column.
    name: usize,
    /// Width of the `AT` location column; zero when the group has no locations.
    location: usize,
    /// Width of the type column; zero when no declaration in the group has an
    /// initial value, in which case the type needs no padding.
    ty: usize,
}

/// Computes per-declaration alignment widths, one entry per item.
///
/// Items are partitioned into runs separated by blank lines, and each run is
/// measured independently.
fn alignment_groups(source: &str, items: &[Node<'_>]) -> Vec<Widths> {
    let mut widths = vec![Widths::default(); items.len()];
    let mut start = 0;

    while start < items.len() {
        let mut end = start + 1;
        while end < items.len()
            && !blank_line_between(source, items[end - 1].end_byte(), items[end].start_byte())
        {
            end += 1;
        }

        let group = measure(source, &items[start..end]);
        for slot in widths.iter_mut().take(end).skip(start) {
            *slot = group;
        }
        start = end;
    }

    widths
}

/// Measures one alignment run.
fn measure(source: &str, group: &[Node<'_>]) -> Widths {
    let mut widths = Widths::default();
    let mut any_initial = false;

    for item in group {
        if !is_aligned_declaration(item.kind()) {
            continue;
        }
        if let Some(name) = item.child_by_field_name("name") {
            widths.name = widths.name.max(source[name.byte_range()].chars().count());
        }
        if let Some(loc) = item.child_by_field_name("location") {
            widths.location = widths
                .location
                .max(source[loc.byte_range()].chars().count());
        }
        if item.child_by_field_name("initial_value").is_some() {
            any_initial = true;
        }
    }

    // The type column only needs padding when something is aligned after it.
    if any_initial {
        for item in group {
            if !is_aligned_declaration(item.kind()) {
                continue;
            }
            if let Some(ty) = item.child_by_field_name("type") {
                // Measured from the source text, which is the same width as the
                // formatted type for everything that fits on one line.
                let text = source[ty.byte_range()]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                widths.ty = widths.ty.max(text.chars().count());
            }
        }
    }

    widths
}

/// The canonical spelling of a section keyword, taken from the node kind.
fn section_keyword(node: Node<'_>) -> String {
    style::keyword(node.kind())
}

fn token<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|n| n.kind() == kind)
}

/// The declaration kinds that take part in column alignment.
///
/// `variable_declaration` and `struct_field` have identical shapes — optional
/// pragmas, a name, an optional `AT` location, a type and an optional initial
/// value — so both are laid out by the same pass.
fn is_aligned_declaration(kind: &str) -> bool {
    matches!(kind, "variable_declaration" | "struct_field")
}
