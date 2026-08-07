//! Prints the parse tree of a Structured Text source, with field names.
//!
//! The development tool for every formatter phase: it shows the exact node
//! kinds and fields a construct produces, which is what the formatter
//! dispatches on.
//!
//! ```sh
//! cargo run --example sexp -- file.st
//! echo 'x := a + b;' | cargo run --example sexp
//! ```

use std::io::Read;

use tree_sitter::Node;

fn main() {
    let arg = std::env::args().nth(1);
    let source = match arg {
        Some(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(2);
        }),
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .expect("read stdin");
            buf
        }
    };

    let mut parser = st_fmt::parse::parser();
    let tree = parser.parse(&source, None).expect("parse");
    print(tree.root_node(), None, 0, &source);

    if tree.root_node().has_error() {
        eprintln!("\nnote: the tree contains errors — st-fmt would refuse this file");
    }
}

fn print(node: Node<'_>, field: Option<&str>, depth: usize, source: &str) {
    let indent = "  ".repeat(depth);
    let label = field.map_or(String::new(), |f| format!("{f}: "));
    let text = &source[node.byte_range()];
    let preview = if node.child_count() == 0 && !text.contains('\n') {
        format!("  {text:?}")
    } else {
        String::new()
    };
    let named = if node.is_named() { "" } else { " (anon)" };
    println!("{indent}{label}{}{named}{preview}", node.kind());

    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    for child in children {
        // Field names are indexed over named children only.
        let field = if child.is_named() {
            let mut c = node.walk();
            node.named_children(&mut c)
                .position(|n| n.id() == child.id())
                .and_then(|i| node.field_name_for_named_child(i as u32))
        } else {
            None
        };
        print(child, field, depth + 1, source);
    }
}
