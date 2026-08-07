//! Statements.
//!
//! Phases 3 and 4 fill this in. Phase 0 implements assignment only, which is
//! enough to prove the pipeline end to end.

use tree_sitter::Node;

use super::{Formatter, expr};
use crate::doc::Doc;

/// `target := value` and `target REF= value`.
///
/// The operator token is anonymous, so it is read from the tree rather than
/// assumed, which keeps `REF=` working without a special case.
pub fn assignment(f: &mut Formatter<'_>, node: Node<'_>) -> Doc {
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

    Doc::concat([target_doc, Doc::text(format!(" {op} ")), value_doc])
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
