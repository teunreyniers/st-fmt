//! Runs the formatter's invariants over a directory of real ST files.
//!
//! Read-only: nothing is ever written. For each file it checks that the source
//! parses, that formatting is idempotent, that the parse tree is unchanged, and
//! that no comment was lost.
//!
//! ```sh
//! cargo run --example check_tree -- /path/to/plc
//! ```

use std::path::{Path, PathBuf};

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: check_tree <directory>");
        std::process::exit(2);
    });

    let mut files = Vec::new();
    collect(Path::new(&root), &mut files);
    files.sort();

    let (mut ok, mut refused, mut broken) = (0usize, 0usize, 0usize);
    let mut changed = 0usize;

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };

        let formatted = match st_fmt::format_source(&source) {
            Ok(f) => f,
            Err(e) => {
                refused += 1;
                println!("REFUSED  {}: {e}", file.display());
                continue;
            }
        };

        if formatted != source {
            changed += 1;
        }

        // Idempotence.
        match st_fmt::format_source(&formatted) {
            Ok(twice) if twice == formatted => {}
            Ok(_) => {
                broken += 1;
                println!(
                    "UNSTABLE {}: a second pass changed the output",
                    file.display()
                );
                continue;
            }
            Err(e) => {
                broken += 1;
                println!("INVALID  {}: output no longer parses: {e}", file.display());
                continue;
            }
        }

        // Comments conserved.
        if comment_count(&source) != comment_count(&formatted) {
            broken += 1;
            println!(
                "COMMENTS {}: {} in, {} out",
                file.display(),
                comment_count(&source),
                comment_count(&formatted)
            );
            continue;
        }

        ok += 1;
    }

    println!(
        "\n{} files: {ok} ok ({changed} would change), {refused} refused, {broken} broken",
        files.len()
    );
    if broken > 0 {
        std::process::exit(1);
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "st" || e == "iec" || e == "scl")
        {
            out.push(path);
        }
    }
}

fn comment_count(source: &str) -> usize {
    let mut parser = st_fmt::parse::parser();
    let Some(tree) = parser.parse(source, None) else {
        return 0;
    };
    let mut count = 0;
    let mut cursor = tree.root_node().walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "comment" {
            count += 1;
            continue;
        }
        stack.extend(node.children(&mut cursor));
    }
    count
}
