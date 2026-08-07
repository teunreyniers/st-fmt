//! Expressions: operators, calls, and access chains.
//!
//! Two things here are more than plain concatenation.
//!
//! **Operator chains are flattened.** `a AND b AND c` parses left-nested as
//! `(a AND b) AND c`. Formatting that nesting directly would indent each
//! operand one level deeper than the last. Chains of equal precedence are
//! therefore collected into a flat list so a broken chain sits at one indent
//! with the operator leading each line.
//!
//! **Argument lists align their `:=` and `=>`.** Alignment is a width the
//! Wadler IR cannot express, so the labels are measured up front and the padded
//! form is selected only in the broken arm of an `IfBreak`.

use tree_sitter::Node;

use super::Formatter;
use crate::doc::Doc;
use crate::style;

/// A named argument list breaks when it has more than this many named
/// parameters, even if it would fit on one line — a long `:=` list is far
/// easier to read one per line.
const NAMED_PARAM_BREAK_THRESHOLD: usize = 2;

/// Formats an expression node.
pub fn expression(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    f.node(node)
}

/// A single `name := value` or `name => target` outside a call's alignment
/// pass — as used by a `PROGRAM … WITH … : Type(x := 1)` configuration.
pub fn param_assignment(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let param = split_param(f, node);
    param_doc(f, param, 0)
}

/// A binary, boolean, comparison or equality operator chain.
///
/// All four grammar rules share the same `left` / `operator` / `right` shape.
pub fn operator_chain(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    chain_parts(f, node).group()
}

/// The same chain, but without its own group.
///
/// A construct that must break *with* the chain — `IF <cond> THEN`, where
/// `THEN` drops to its own line only when the condition breaks — has to put
/// the chain's line breaks in the same group as its own. Wrapping the chain in
/// a group first would let it re-flatten inside a broken parent, leaving
/// `THEN` stranded on a line by itself after an unbroken condition.
pub fn condition(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    match node.kind() {
        "binary_operator" | "boolean_operator" | "comparison_operator" | "equality_operator" => {
            chain_parts(f, node)
        }
        _ => f.node(node),
    }
}

fn chain_parts(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let (operands, operators) = flatten_chain(node);

    if operands.len() < 2 {
        return f.verbatim(node);
    }

    let mut iter = operands.into_iter();
    let first = iter.next().expect("a chain has at least two operands");
    let first_doc = f.node(first);

    let mut tail = Vec::new();
    for (operand, op) in iter.zip(operators) {
        // The operator leads its continuation line, so the break goes before
        // it rather than after.
        tail.push(Doc::Line);
        tail.push(Doc::text(op));
        tail.push(Doc::space());
        tail.push(f.node(operand));
    }

    Doc::concat([first_doc, Doc::concat(tail).indent()])
}

/// Collects a run of equal-precedence operators into operands and operator
/// texts, so `a + b - c` yields three operands and `["+", "-"]`.
///
/// Only left-associative chains are flattened by descending the `left` spine;
/// `**` is right-associative and is left nested, which is both correct and
/// rare enough not to matter.
fn flatten_chain<'t>(node: Node<'t>) -> (Vec<Node<'t>>, Vec<String>) {
    let Some(level) = precedence(node) else {
        return (vec![node], vec![]);
    };

    let mut operands = Vec::new();
    let mut operators = Vec::new();
    let mut current = node;

    loop {
        let (Some(left), Some(op), Some(right)) = (
            current.child_by_field_name("left"),
            current.child_by_field_name("operator"),
            current.child_by_field_name("right"),
        ) else {
            operands.push(current);
            break;
        };

        operands.push(right);
        operators.push(operator_text(op));

        // Keep descending only while the left operand is the same precedence,
        // so `a * b + c` stops at the `+` and leaves `a * b` as one operand.
        if precedence(left) == Some(level) && left.kind() == current.kind() {
            current = left;
        } else {
            operands.push(left);
            break;
        }
    }

    operands.reverse();
    operators.reverse();
    (operands, operators)
}

/// The precedence level of an operator node, mirroring the grammar's `PREC`
/// table. `None` for anything that is not an operator.
fn precedence(node: Node<'_>) -> Option<u8> {
    let op = node.child_by_field_name("operator")?;
    Some(match node.kind() {
        "boolean_operator" => match op.kind() {
            "or" => 0,
            "xor" => 1,
            _ => 2,
        },
        "equality_operator" => 10,
        "comparison_operator" => 11,
        "binary_operator" => match op.kind() {
            "+" | "-" => 20,
            "**" => 22,
            _ => 21,
        },
        _ => return None,
    })
}

/// The canonical spelling of an operator token.
///
/// Keyword operators are aliased to lowercase by the grammar regardless of
/// source casing, so uppercasing the node kind canonicalizes `mod`, `and`,
/// `or` and `xor`, and leaves symbolic operators untouched.
fn operator_text(op: Node<'_>) -> String {
    style::keyword(op.kind())
}

/// `NOT x` and `-x`.
pub fn unary(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let Some(op) = node.child(0) else {
        return f.verbatim(node);
    };
    let Some(operand) = node.named_child(0) else {
        return f.verbatim(node);
    };

    // `NOT` is a word and needs separating; `-` binds tight to its operand.
    let (text, space) = match op.kind() {
        "not" => ("NOT", true),
        other => (other, false),
    };

    let operand_doc = f.node(operand);
    Doc::concat([
        Doc::text(text.to_owned()),
        if space { Doc::space() } else { Doc::Nil },
        operand_doc,
    ])
}

/// `( expr )` — always preserved, never added or removed, spacing normalized.
pub fn parenthesized(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let Some(inner) = node.named_child(0) else {
        return f.verbatim(node);
    };
    let inner_doc = f.node(inner);
    Doc::concat([Doc::text("("), inner_doc, Doc::text(")")])
}

/// A function call or function-block invocation. Both have the same shape:
/// a callable, then a parenthesized parameter list.
pub fn call(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let children = super::source::named_children(node);
    let mut params: Vec<Node<'_>> = Vec::new();
    let mut name: Option<Node<'_>> = None;

    for child in children {
        if child.kind() == "param_assignment" {
            params.push(child);
        } else if name.is_none() {
            name = Some(child);
        }
    }

    let Some(name) = name else {
        return f.verbatim(node);
    };
    let name_doc = f.node(name);

    if params.is_empty() {
        return Doc::concat([name_doc, Doc::text("()")]);
    }

    let parsed: Vec<Param<'_>> = params.iter().map(|p| split_param(f, *p)).collect();

    // Alignment column: the widest label among the named parameters.
    let label_width = parsed
        .iter()
        .filter_map(|p| p.label.as_ref())
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);

    let named_count = parsed.iter().filter(|p| p.label.is_some()).count();
    let force = named_count > NAMED_PARAM_BREAK_THRESHOLD;

    let mut items = Vec::new();
    for (i, param) in parsed.into_iter().enumerate() {
        if i > 0 {
            items.push(Doc::text(","));
            items.push(if force { Doc::HardLine } else { Doc::Line });
        }
        items.push(param_doc(f, param, label_width));
    }
    // The grammar allows a trailing comma, and adding one keeps a later
    // argument from showing up as a two-line diff.
    items.push(Doc::if_break(Doc::text(","), Doc::Nil));

    let open = if force { Doc::HardLine } else { Doc::SoftLine };
    let close = if force { Doc::HardLine } else { Doc::SoftLine };

    Doc::concat([
        name_doc,
        Doc::text("("),
        Doc::concat([open, Doc::concat(items)]).indent(),
        close,
        Doc::text(")"),
    ])
    .group()
}

/// A parameter split into its optional `name :=` / `name =>` label and value.
struct Param<'t> {
    label: Option<String>,
    operator: &'static str,
    value: Option<Node<'t>>,
    /// Set when the parameter could not be decomposed and must be copied.
    raw: Option<Node<'t>>,
}

/// Splits a `param_assignment` into label, operator and value.
///
/// The grammar gives two shapes: `[name :=] expr`, and `[NOT] name => target`.
/// Only the `=>` form's target is a field, so the operator token is located by
/// scanning the children.
fn split_param<'t>(f: &Formatter<'_>, node: Node<'t>) -> Param<'t> {
    let mut cursor = node.walk();
    let children: Vec<Node<'t>> = node.children(&mut cursor).collect();

    let op_index = children
        .iter()
        .position(|c| matches!(c.kind(), ":=" | "=>"));

    let Some(op_index) = op_index else {
        // Positional: the whole parameter is one expression.
        return Param {
            label: None,
            operator: "",
            value: node.named_child(0),
            raw: node.named_child(0).is_none().then_some(node),
        };
    };

    let operator = if children[op_index].kind() == ":=" {
        ":="
    } else {
        "=>"
    };

    // Everything before the operator is the label: an identifier, optionally
    // preceded by `NOT`.
    let label: String = children[..op_index]
        .iter()
        .map(|c| match c.kind() {
            "not" => "NOT".to_owned(),
            _ => f.text(*c).to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ");

    Param {
        label: Some(label),
        operator,
        value: children.get(op_index + 1).copied(),
        raw: None,
    }
}

fn param_doc(f: &mut Formatter<'_>, param: Param<'_>, label_width: usize) -> Doc {
    if let Some(raw) = param.raw {
        return f.verbatim(raw);
    }

    let value_doc = match param.value {
        Some(v) => f.node(v),
        None => return Doc::Nil,
    };

    let Some(label) = param.label else {
        return value_doc;
    };

    // The padded spelling is used only when the list breaks; on one line the
    // parameters are separated by single spaces.
    let pad = label_width.saturating_sub(label.chars().count());
    let broken = format!("{label}{} {} ", " ".repeat(pad), param.operator);
    let flat = format!("{label} {} ", param.operator);

    Doc::concat([Doc::if_break(Doc::text(broken), Doc::text(flat)), value_doc])
}

/// `buffer[i]` and `m[i, j]`.
pub fn index(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let children = super::source::named_children(node);
    let Some((array, indices)) = children.split_first() else {
        return f.verbatim(node);
    };

    let array_doc = f.node(*array);
    let index_docs: Vec<Doc> = indices.iter().map(|i| f.node(*i)).collect();

    Doc::concat([
        array_doc,
        Doc::text("["),
        Doc::join(Doc::text(", "), index_docs),
        Doc::text("]"),
    ])
}

/// `p^`.
pub fn deref(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let Some(operand) = node.named_child(0) else {
        return f.verbatim(node);
    };
    let operand_doc = f.node(operand);
    Doc::concat([operand_doc, Doc::text("^")])
}

/// `motor.speed`, `items[i].value`, `input.0`.
pub fn qualified(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    let parts = super::source::named_children(node);
    if parts.is_empty() {
        return f.verbatim(node);
    }
    let docs: Vec<Doc> = parts.into_iter().map(|p| f.node(p)).collect();
    Doc::join(Doc::text("."), docs)
}

/// `%IX0.0`, `%MW100`, `%I*` — a single token, uppercased.
pub fn direct_address(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(f.text(node).to_ascii_uppercase())
}

/// `THIS` and `SUPER`, whose canonical spelling is their node kind.
pub fn this_or_super(_f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
    Doc::text(node.kind().to_ascii_uppercase())
}
