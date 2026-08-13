//! Node dispatch and the shared formatter state.
//!
//! Each construct family lives in its own submodule. Anything not yet
//! implemented falls through to [`Formatter::verbatim`], which copies the
//! source slice unchanged and records the node kind so tests can see exactly
//! how much of the language is really covered.

mod control;
mod decl;
mod expr;
mod lit;
mod pou;
pub(crate) mod source;
mod stmt;

use std::collections::BTreeSet;

use tree_sitter::{Node, Tree};

use crate::doc::{Doc, render};
use crate::style::MAX_WIDTH;
use crate::trivia::{Comment, Trivia};

pub fn format_tree(tree: &Tree, source: &str) -> String {
    format_tree_reporting(tree, source).0
}

pub fn format_tree_reporting(tree: &Tree, source: &str) -> (String, Vec<String>) {
    let mut f = Formatter::new(source, tree.root_node());
    let doc = f.format_source_node(tree.root_node());
    let out = render(&doc, MAX_WIDTH);
    (out, f.unhandled.into_iter().collect())
}

pub struct Formatter<'a> {
    pub source: &'a str,
    pub trivia: Trivia,
    /// End offset of the most recently emitted comment. Blank-line decisions
    /// measure the gap from here when a comment sits between two constructs.
    pub last_comment_end: Option<usize>,
    /// Node kinds that fell through to the verbatim fallback.
    pub unhandled: BTreeSet<String>,
}

impl<'a> Formatter<'a> {
    fn new(source: &'a str, root: Node<'_>) -> Formatter<'a> {
        Formatter {
            source,
            trivia: Trivia::collect(root, source),
            last_comment_end: None,
            unhandled: BTreeSet::new(),
        }
    }

    /// The exact source text of a node.
    pub fn text(&self, node: Node<'_>) -> &'a str {
        &self.source[node.byte_range()]
    }

    /// Formats any node, dispatching on its kind.
    pub fn node(&mut self, node: Node<'_>) -> Doc {
        match node.kind() {
            // Statements
            "assignment" => stmt::assignment(self, node),
            "return" => stmt::return_statement(self, node),
            "exit" | "continue" => stmt::bare_keyword(self, node),
            "pragma" => stmt::pragma(self, node),
            // A `noop` is a bare `;`; the terminator is emitted by the block.
            "noop" => Doc::Nil,

            // Control flow
            "if_statement" => self.if_statement(node),
            "elsif_clause" => self.elsif_clause(node),
            "else_clause" => self.else_clause(node),
            "case_statement" => self.case_statement(node),
            "case_label_range" => self.case_label_range(node),
            "case_label_single" => match node.named_child(0) {
                Some(inner) => self.node(inner),
                None => self.verbatim(node),
            },
            "negative_integer" => self.negative_integer(node),
            "for_statement" => self.for_statement(node),
            "while_statement" => self.while_statement(node),
            "repeat_statement" => self.repeat_statement(node),

            // Expressions
            "binary_operator"
            | "boolean_operator"
            | "comparison_operator"
            | "equality_operator" => expr::operator_chain(self, node),
            "unary_expression" => expr::unary(self, node),
            "parenthesized_expression" => expr::parenthesized(self, node),
            "function_call" | "fb_invocation" => expr::call(self, node),
            "param_assignment" => expr::param_assignment(self, node),
            "index_expression" => expr::index(self, node),
            "deref_expression" => expr::deref(self, node),
            "qualified_identifier" => expr::qualified(self, node),
            "direct_address" => expr::direct_address(self, node),
            "this" | "super" => expr::this_or_super(self, node),
            "bit_selector" => lit::verbatim_leaf(self, node),

            // Program organization units and their members. Each is the same
            // shape with different keywords, so one routine covers them all.
            "function_block_declaration" => self.pou(node, "FUNCTION_BLOCK", "END_FUNCTION_BLOCK"),
            "test_function_block_declaration" => {
                self.pou(node, "TEST_FUNCTION_BLOCK", "END_TEST_FUNCTION_BLOCK")
            }
            "function_declaration" => self.pou(node, "FUNCTION", "END_FUNCTION"),
            "program_declaration" => self.pou(node, "PROGRAM", "END_PROGRAM"),
            "class_declaration" => self.pou(node, "CLASS", "END_CLASS"),
            "interface_declaration" => self.pou(node, "INTERFACE", "END_INTERFACE"),
            "method_declaration" => self.pou(node, "METHOD", "END_METHOD"),
            "property_declaration" => self.pou(node, "PROPERTY", "END_PROPERTY"),
            "action_declaration" => self.pou(node, "ACTION", "END_ACTION"),
            "get_accessor" => self.pou(node, "GET", "END_GET"),
            "set_accessor" => self.pou(node, "SET", "END_SET"),

            // Configuration
            "configuration_declaration" => {
                self.configuration(node, "CONFIGURATION", "END_CONFIGURATION")
            }
            "resource_declaration" => self.configuration(node, "RESOURCE", "END_RESOURCE"),
            "task_declaration" => self.task_declaration(node),
            "program_configuration" => self.program_configuration(node),

            // Type declarations
            "type_declaration" => self.type_declaration(node),
            "type_definition" => self.type_definition(node),
            "struct_definition" => self.struct_definition(node),
            "enum_definition" => self.enum_definition(node),

            // Declarations
            "var" | "var_input" | "var_output" | "var_in_out" | "var_temp" | "var_static"
            | "var_global" | "var_external" | "var_inst" => self.var_section(node),
            "type_name" => self.type_name(node),
            "array_bound_expression" => self.array_bound_expression(node),
            // Reached directly by a subrange type such as `INT (0..100)`.
            "array_range" => self.array_range(node),
            "array_initializer" => self.array_initializer(node),
            "array_initializer_repetition" => self.array_initializer_repetition(node),
            "structure_initializer" => self.structure_initializer(node),

            // Literals and leaves
            "identifier" | "string_literal" => lit::verbatim_leaf(self, node),
            "integer_literal" => lit::integer(self, node),
            "float_literal" => lit::float(self, node),
            "time_literal" => lit::duration(self, node),
            "date_literal" | "time_of_day_literal" | "date_and_time_literal" => {
                lit::date_like(self, node)
            }
            "typed_literal" => lit::typed(self, node),
            "true" | "false" => lit::boolean(self, node),

            _ => self.verbatim(node),
        }
    }

    /// Copies a node's source text unchanged.
    ///
    /// Any comment inside the copied range comes along in the text, so the
    /// trivia cursor is advanced past it — otherwise the comment would be
    /// emitted a second time when the formatter next drains trivia.
    pub fn verbatim(&mut self, node: Node<'_>) -> Doc {
        if node.is_named() {
            self.unhandled.insert(node.kind().to_owned());
        }
        self.trivia.take_before(node.end_byte());
        Doc::text(self.text(node).to_owned())
    }

    /// Builds a document for its measurements only, undoing every effect.
    ///
    /// Column alignment has to know how wide a document is *before* the
    /// documents around it have been built, and building one hands out
    /// comments. Everything the formatter carries between nodes is a cursor, so
    /// a speculative build rewinds exactly — and the destructuring below is
    /// what makes that claim fail to compile the day a field is added.
    ///
    /// `unhandled` is deliberately not rewound: a speculative build only ever
    /// visits nodes the real build visits too, so the set it reports is the
    /// same either way.
    pub fn speculative<T>(&mut self, build: impl FnOnce(&mut Formatter<'a>) -> T) -> T {
        let Formatter {
            source: _,
            trivia,
            last_comment_end,
            unhandled: _,
        } = self;
        let checkpoint = trivia.checkpoint();
        let last_comment_end = *last_comment_end;

        let measured = build(self);

        self.trivia.restore(checkpoint);
        self.last_comment_end = last_comment_end;
        measured
    }

    /// Emits every comment starting before `byte` as leading trivia, each on
    /// its own line.
    ///
    /// Blank lines the author put above a comment are preserved (collapsed to
    /// one). `first_in_block` suppresses that leading blank so a block never
    /// opens with an empty line.
    pub fn leading_comments(&mut self, byte: usize, first_in_block: bool) -> Doc {
        let comments = self.trivia.take_before(byte);
        let mut parts = Vec::new();
        for (i, c) in comments.iter().enumerate() {
            if i > 0 || !first_in_block {
                parts.push(if c.blank_before {
                    Doc::BlankLine
                } else {
                    Doc::HardLine
                });
            }
            parts.push(self.comment_doc(c));
            self.last_comment_end = Some(c.end_byte);
        }
        Doc::concat(parts)
    }

    /// Emits comments that trail the code just written, on the same line.
    pub fn trailing_comments(&mut self, row: usize, byte: usize) -> Doc {
        let comments = self.trivia.take_trailing_on_row(row, byte);
        Doc::concat(comments.iter().map(|c| {
            self.last_comment_end = Some(c.end_byte);
            Doc::concat([Doc::text("  "), Doc::text(c.text.clone())])
        }))
    }

    /// Renders one comment.
    ///
    /// A multi-line block comment is copied byte for byte: its interior layout
    /// is the author's, and re-indenting it would corrupt ASCII diagrams and
    /// aligned tables that are common in PLC headers.
    pub fn comment_doc(&self, comment: &Comment) -> Doc {
        Doc::text(comment.text.clone())
    }
}

/// Right-pads `text` with spaces to `width` columns.
///
/// The shared primitive of every column-alignment pass: declarations, call
/// parameters, enum members and runs of assignments all measure first and pad
/// here. Measured in characters, not bytes, so a non-ASCII identifier lines up.
pub(super) fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    format!("{text}{}", " ".repeat(width.saturating_sub(len)))
}

/// True for the statement kinds that carry their own terminator keyword, and so
/// take the grammar's *optional* `;` rather than a required one.
pub fn is_compound_statement(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "case_statement"
            | "for_statement"
            | "while_statement"
            | "repeat_statement"
    )
}
