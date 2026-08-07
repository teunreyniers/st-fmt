//! Program organization units, their members, and TYPE declarations.
//!
//! Layout follows one rule: **containers stay flat, leaf contents indent.**
//! `VAR`, `STRUCT`, `METHOD` and the POU keywords all sit at their parent's
//! column; only the things that are not themselves containers — variable
//! declarations, struct fields, statements — move in a level. That keeps deeply
//! nested ST from marching off the right margin, and matches how vendor tools
//! export these files. `PROPERTY` and `ACTION` are the exception and indent
//! their contents; see [`indents_contents`].
//!
//! Blank lines are forced before a POU's statement body, between members, and
//! above a top-level POU's closing keyword. Everywhere else the author's
//! spacing is preserved.

use tree_sitter::Node;

use super::{Formatter, source::named_children};
use crate::doc::Doc;
use crate::style;
use crate::trivia::blank_line_between;

/// An enumeration breaks and aligns once more than this many members carry an
/// explicit value, matching the rule for named call parameters.
const ENUM_VALUE_BREAK_THRESHOLD: usize = 2;

/// The child kinds that make up a POU's contents, in source order.
fn is_var_section(kind: &str) -> bool {
    matches!(
        kind,
        "var"
            | "var_input"
            | "var_output"
            | "var_in_out"
            | "var_temp"
            | "var_static"
            | "var_global"
            | "var_external"
            | "var_inst"
    )
}

fn is_member(kind: &str) -> bool {
    matches!(
        kind,
        "method_declaration"
            | "property_declaration"
            | "action_declaration"
            | "get_accessor"
            | "set_accessor"
    )
}

/// Members that a blank line is forced between.
///
/// The GET and SET accessors of a property are excluded: they are two halves of
/// one declaration rather than independent members, and separating them reads
/// as though the property had ended.
fn is_separated_member(kind: &str) -> bool {
    is_member(kind) && !matches!(kind, "get_accessor" | "set_accessor")
}

/// The POUs that stand alone in a file, as opposed to being a member of one.
///
/// Their closing keyword is set off by a blank line: a file-level POU is long
/// enough that `END_FUNCTION_BLOCK` reads as a boundary rather than as the next
/// line of the body.
fn is_top_level_pou(kind: &str) -> bool {
    matches!(
        kind,
        "function_block_declaration"
            | "test_function_block_declaration"
            | "function_declaration"
            | "program_declaration"
            | "class_declaration"
            | "interface_declaration"
    )
}

/// The POUs whose contents indent instead of staying flat.
///
/// These are the exception to the containers-stay-flat rule. A PROPERTY is a
/// wrapper around its accessors rather than a unit of its own, and an ACTION's
/// body is a fragment of the enclosing POU's code, so in both cases the
/// indentation is what shows where the enclosing construct resumes.
fn indents_contents(kind: &str) -> bool {
    matches!(
        kind,
        "property_declaration" | "action_declaration" | "get_accessor" | "set_accessor"
    )
}

impl Formatter<'_> {
    /// Any POU: FUNCTION_BLOCK, FUNCTION, PROGRAM, CLASS, INTERFACE,
    /// TEST_FUNCTION_BLOCK, METHOD, ACTION, PROPERTY and the GET/SET accessors.
    ///
    /// They differ only in their keywords and which optional clauses they
    /// allow, so one routine covers them all by reading the tree.
    pub fn pou(&mut self, node: Node<'_>, keyword: &str, end_keyword: &str) -> Doc {
        let end = token(node, &end_keyword.to_ascii_lowercase());
        let bound = end.map_or(node.end_byte(), |t| t.start_byte());

        let contents = self.pou_contents(node, bound);
        let empty = contents.iter().all(Doc::is_nil);

        let mut parts = vec![self.pou_header(node, keyword)];
        if indents_contents(node.kind()) {
            parts.push(Doc::concat(contents).indent());
        } else {
            parts.extend(contents);
        }

        // A METHOD's END_METHOD is optional in the grammar. Whether the author
        // wrote one is preserved rather than normalized, so a file that omits
        // them keeps parsing the same way.
        if end.is_some() {
            // An empty POU keeps its keywords together: a blank line there
            // would separate nothing from nothing.
            parts.push(if is_top_level_pou(node.kind()) && !empty {
                Doc::BlankLine
            } else {
                Doc::HardLine
            });
            parts.push(Doc::text(end_keyword.to_owned()));
        }
        Doc::concat(parts)
    }

    /// `<KEYWORD> [ACCESS] <name> [: type] [EXTENDS …] [IMPLEMENTS …]`
    fn pou_header(&mut self, node: Node<'_>, keyword: &str) -> Doc {
        let mut parts = vec![Doc::text(keyword.to_owned())];

        if let Some(access) = node.child_by_field_name("access") {
            parts.push(Doc::space());
            parts.push(self.keyword_list(access));
        }

        if let Some(name) = node.child_by_field_name("name") {
            parts.push(Doc::space());
            parts.push(Doc::text(self.text(name).to_owned()));
        }

        // FUNCTION and METHOD carry a return type; PROPERTY carries a type.
        let ty = node
            .child_by_field_name("return_type")
            .or_else(|| node.child_by_field_name("type"));
        if let Some(ty) = ty {
            parts.push(Doc::text(" : "));
            parts.push(self.type_name(ty));
        }

        for clause in named_children(node) {
            match clause.kind() {
                "extends_clause" => {
                    parts.push(Doc::text(" EXTENDS "));
                    parts.push(self.identifier_list(clause));
                }
                "implements_clause" => {
                    parts.push(Doc::text(" IMPLEMENTS "));
                    parts.push(self.identifier_list(clause));
                }
                _ => {}
            }
        }

        Doc::concat(parts)
    }

    /// The variable sections, statement body and members of a POU.
    fn pou_contents(&mut self, node: Node<'_>, bound: usize) -> Vec<Doc> {
        let items: Vec<Node<'_>> = named_children(node)
            .into_iter()
            .filter(|c| is_var_section(c.kind()) || is_member(c.kind()) || c.kind() == "block")
            .collect();

        let mut parts = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let prev_end = i.checked_sub(1).map(|p| items[p].end_byte());
            let next_start = items.get(i + 1).map_or(bound, Node::start_byte);

            // A blank line is forced above the statement body and between
            // members; between variable sections the author's spacing stands.
            // Never above the first item, where it would only push the POU's
            // contents away from its header.
            let force_blank =
                prev_end.is_some() && (item.kind() == "block" || is_separated_member(item.kind()));
            parts.push(self.section_separator(*item, prev_end, force_blank));

            parts.push(match item.kind() {
                "block" => self.block(*item, next_start),
                _ => self.node(*item),
            });
        }

        parts.push(self.leading_comments(bound, items.is_empty()));
        parts
    }

    /// The line break above a POU section, optionally forced to a blank line.
    fn section_separator(
        &mut self,
        item: Node<'_>,
        prev_end: Option<usize>,
        force_blank: bool,
    ) -> Doc {
        // `first_in_block` is set so the comment block does not emit its own
        // leading break — this routine supplies it below, and both would stack
        // into a double blank line.
        let comments = self.leading_comments(item.start_byte(), true);
        let gap_start = if comments.is_nil() {
            prev_end
        } else {
            self.last_comment_end.or(prev_end)
        };

        let authored_blank = gap_start
            .is_some_and(|start| blank_line_between(self.source, start, item.start_byte()));

        // The forced blank applies above the comment block, so a note stays
        // attached to the section it documents.
        let leading = if force_blank || (comments.is_nil() && authored_blank) {
            Doc::BlankLine
        } else {
            Doc::HardLine
        };

        if comments.is_nil() {
            Doc::concat([leading])
        } else {
            let inner = if authored_blank {
                Doc::BlankLine
            } else {
                Doc::HardLine
            };
            Doc::concat([leading, comments, inner])
        }
    }

    /// `TYPE … END_TYPE`
    pub fn type_declaration(&mut self, node: Node<'_>) -> Doc {
        let end = token(node, "end_type");
        let bound = end.map_or(node.end_byte(), |t| t.start_byte());
        let items = named_children(node);

        let mut parts = vec![Doc::text("TYPE")];
        for (i, item) in items.iter().enumerate() {
            let prev_end = i.checked_sub(1).map(|p| items[p].end_byte());
            parts.push(self.separator_before(*item, prev_end, false));
            parts.push(self.node(*item));
        }
        parts.push(self.leading_comments(bound, items.is_empty()));
        parts.push(Doc::HardLine);
        parts.push(Doc::text("END_TYPE"));
        Doc::concat(parts)
    }

    /// `name : <definition> [:= init];`
    pub fn type_definition(&mut self, node: Node<'_>) -> Doc {
        let Some(name) = node.child_by_field_name("name") else {
            return self.verbatim(node);
        };
        let Some(definition) = node.child_by_field_name("definition") else {
            return self.verbatim(node);
        };

        let mut parts = vec![
            Doc::text(self.text(name).to_owned()),
            Doc::text(" : "),
            self.node(definition),
        ];

        if let Some(initial) = node.child_by_field_name("initial_value") {
            parts.push(Doc::text(" := "));
            parts.push(self.node(initial));
        }

        // Vendors omit the `;` after END_STRUCT but write it after an
        // enumeration or an alias; that convention is kept.
        if definition.kind() != "struct_definition" {
            parts.push(Doc::text(";"));
        }
        Doc::concat(parts)
    }

    /// `STRUCT … END_STRUCT`, with its fields aligned like a VAR section.
    pub fn struct_definition(&mut self, node: Node<'_>) -> Doc {
        let end = token(node, "end_struct");
        let bound = end.map_or(node.end_byte(), |t| t.start_byte());
        let fields = named_children(node);
        let body = self.aligned_declarations(&fields, bound);

        Doc::concat([
            Doc::text("STRUCT"),
            Doc::concat([Doc::HardLine, body]).indent(),
            Doc::HardLine,
            Doc::text("END_STRUCT"),
        ])
    }

    /// `(Red, Green, Blue)` and `(Idle := 0, Running := 10)`.
    pub fn enum_definition(&mut self, node: Node<'_>) -> Doc {
        let members = named_children(node);
        if members.is_empty() {
            return Doc::text("()");
        }

        let names: Vec<String> = members
            .iter()
            .map(|m| {
                m.child_by_field_name("name")
                    .map_or_else(String::new, |n| self.text(n).to_owned())
            })
            .collect();
        let width = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);

        let valued = members
            .iter()
            .filter(|m| m.child_by_field_name("value").is_some())
            .count();
        let force = valued > ENUM_VALUE_BREAK_THRESHOLD;

        let mut items = Vec::new();
        for (i, member) in members.iter().enumerate() {
            if i > 0 {
                items.push(Doc::text(","));
                items.push(if force { Doc::HardLine } else { Doc::Line });
            }
            match member.child_by_field_name("value") {
                Some(value) => {
                    let value_doc = self.node(value);
                    let name = &names[i];
                    items.push(Doc::if_break(
                        Doc::text(format!("{} := ", pad(name, width))),
                        Doc::text(format!("{name} := ")),
                    ));
                    items.push(value_doc);
                }
                None => items.push(Doc::text(names[i].clone())),
            }
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

    /// `CONFIGURATION … END_CONFIGURATION` and `RESOURCE … ON … END_RESOURCE`.
    ///
    /// Both hold variable sections and resource members, laid out flat like a
    /// POU's contents.
    pub fn configuration(&mut self, node: Node<'_>, keyword: &str, end_keyword: &str) -> Doc {
        let end = token(node, &end_keyword.to_ascii_lowercase());
        let bound = end.map_or(node.end_byte(), |t| t.start_byte());

        let mut header = vec![Doc::text(keyword.to_owned())];
        if let Some(name) = node.child_by_field_name("name") {
            header.push(Doc::space());
            header.push(Doc::text(self.text(name).to_owned()));
        }
        // A RESOURCE names the CPU it runs on.
        if let Some(ty) = node.child_by_field_name("type") {
            header.push(Doc::text(" ON "));
            header.push(Doc::text(self.text(ty).to_owned()));
        }

        let items: Vec<Node<'_>> = named_children(node)
            .into_iter()
            .filter(|c| Some(c.id()) != node.child_by_field_name("name").map(|n| n.id()))
            .filter(|c| Some(c.id()) != node.child_by_field_name("type").map(|n| n.id()))
            .collect();

        let mut parts = vec![Doc::concat(header)];
        for (i, item) in items.iter().enumerate() {
            let prev_end = i.checked_sub(1).map(|p| items[p].end_byte());
            parts.push(self.separator_before(*item, prev_end, false));
            parts.push(self.node(*item));
        }
        parts.push(self.leading_comments(bound, items.is_empty()));
        parts.push(Doc::HardLine);
        parts.push(Doc::text(end_keyword.to_owned()));
        Doc::concat(parts)
    }

    /// `TASK Fast(interval := T#10ms, priority := 1);`
    pub fn task_declaration(&mut self, node: Node<'_>) -> Doc {
        let mut parts = vec![Doc::text("TASK ")];
        if let Some(name) = node.child_by_field_name("name") {
            parts.push(Doc::text(self.text(name).to_owned()));
        }
        let params: Vec<Doc> = named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "task_parameter")
            .map(|p| self.task_parameter(p))
            .collect();

        parts.push(Doc::text("("));
        parts.push(Doc::join(Doc::text(", "), params));
        parts.push(Doc::text(");"));
        Doc::concat(parts)
    }

    fn task_parameter(&mut self, node: Node<'_>) -> Doc {
        let name = node
            .child_by_field_name("name")
            .map_or_else(String::new, |n| self.text(n).to_owned());
        match node.child_by_field_name("value") {
            Some(value) => {
                let value_doc = self.node(value);
                Doc::concat([Doc::text(format!("{name} := ")), value_doc])
            }
            None => Doc::text(name),
        }
    }

    /// `PROGRAM Main WITH Fast : MainProg(nInput := 1);`
    pub fn program_configuration(&mut self, node: Node<'_>) -> Doc {
        let mut parts = vec![Doc::text("PROGRAM ")];
        if let Some(name) = node.child_by_field_name("name") {
            parts.push(Doc::text(self.text(name).to_owned()));
        }
        if let Some(task) = node.child_by_field_name("task") {
            parts.push(Doc::text(" WITH "));
            parts.push(Doc::text(self.text(task).to_owned()));
        }
        if let Some(ty) = node.child_by_field_name("type") {
            parts.push(Doc::text(" : "));
            parts.push(Doc::text(self.text(ty).to_owned()));
        }

        let params: Vec<Node<'_>> = named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "param_assignment")
            .collect();
        if !params.is_empty() {
            let docs: Vec<Doc> = params.into_iter().map(|p| self.node(p)).collect();
            parts.push(Doc::text("("));
            parts.push(Doc::join(Doc::text(", "), docs));
            parts.push(Doc::text(")"));
        }

        parts.push(Doc::text(";"));
        Doc::concat(parts)
    }

    /// A run of keywords such as `PUBLIC FINAL` or `CONSTANT RETAIN`.
    fn keyword_list(&mut self, node: Node<'_>) -> Doc {
        let mut cursor = node.walk();
        let words: Vec<Doc> = node
            .children(&mut cursor)
            .map(|c| Doc::text(style::keyword(c.kind())))
            .collect();
        Doc::join(Doc::space(), words)
    }

    /// The comma-separated names of an EXTENDS or IMPLEMENTS clause.
    fn identifier_list(&mut self, node: Node<'_>) -> Doc {
        let names: Vec<Doc> = named_children(node)
            .into_iter()
            .map(|n| Doc::text(self.text(n).to_owned()))
            .collect();
        Doc::join(Doc::text(", "), names)
    }
}

fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    format!("{text}{}", " ".repeat(width.saturating_sub(len)))
}

fn token<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|n| n.kind() == kind)
}
