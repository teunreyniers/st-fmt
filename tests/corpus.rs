//! Runs the formatter over every source in the grammar's own test corpus.
//!
//! The grammar ships 34 corpus files covering real CODESYS/TwinCAT syntax. They
//! carry no agreed formatting, but the three input-independent invariants must
//! still hold for every one of them: idempotence, an unchanged parse tree, and
//! conserved comments.
//!
//! This is the broadest regression net in the suite, and it costs nothing to
//! maintain — it grows automatically as the grammar does.

mod harness;

use std::path::{Path, PathBuf};

/// The grammar repo, resolved relative to this crate.
///
/// The corpus is test data, not part of the published crate, so it does not
/// come along with the git dependency. These tests need the grammar repo
/// checked out as a sibling and skip themselves when it is absent.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tree-sitter-structured-text/test/corpus")
}

/// One `==== name ====` section of a corpus file.
struct Case {
    name: String,
    source: String,
}

/// Splits a tree-sitter corpus file into its cases.
///
/// The format is a `=`-underlined name, a blank line, the source, then a `---`
/// separator and the expected S-expression, which is not needed here.
fn parse_corpus(text: &str) -> Vec<Case> {
    let lines: Vec<&str> = text.lines().collect();
    let is_rule = |l: &str| l.len() >= 3 && l.chars().all(|c| c == '=');

    let mut cases = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !is_rule(lines[i]) {
            i += 1;
            continue;
        }
        // A header is: rule, name, rule.
        let Some(name) = lines.get(i + 1) else { break };
        if !lines.get(i + 2).is_some_and(|l| is_rule(l)) {
            i += 1;
            continue;
        }

        let body_start = i + 3;
        let body_end = (body_start..lines.len())
            .find(|&j| lines[j].trim_end() == "---")
            .unwrap_or(lines.len());

        cases.push(Case {
            name: (*name).to_owned(),
            source: lines[body_start..body_end].join("\n").trim().to_owned(),
        });

        // Skip past the expected-tree section to the next header.
        i = (body_end..lines.len())
            .find(|&j| is_rule(lines[j]))
            .unwrap_or(lines.len());
    }
    cases
}

#[test]
fn grammar_corpus_satisfies_the_invariants() {
    let dir = corpus_dir();
    if !dir.exists() {
        eprintln!(
            "skipping: grammar corpus not found at {}\n\
             (the corpus is not shipped with the crate; check the grammar repo out alongside this one)",
            dir.display()
        );
        return;
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read corpus dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();

    assert!(!files.is_empty(), "no corpus files in {}", dir.display());

    let mut failures = Vec::new();
    let mut checked = 0usize;

    for file in &files {
        let text = std::fs::read_to_string(file).expect("read corpus file");
        let stem = file
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        for case in parse_corpus(&text) {
            if case.source.trim().is_empty() {
                continue;
            }
            checked += 1;
            if let Err(detail) = harness::check_invariants(&case.source) {
                failures.push(harness::Failure {
                    fixture: format!("{stem}: {}", case.name),
                    detail: format!("{detail}\n--- source ---\n{}", case.source),
                });
            }
        }
    }

    assert!(
        checked > 100,
        "expected the corpus to yield many cases, got {checked}"
    );
    eprintln!(
        "corpus: checked {checked} cases across {} files",
        files.len()
    );
    harness::report(failures);
}

/// Every node kind in the corpus must be formatted deliberately.
///
/// The formatter has a verbatim fallback that copies a node's source text
/// unchanged. That was the scaffolding each phase built against; now that the
/// whole grammar is covered, anything reaching it is either a construct nobody
/// implemented or a dispatch entry that got lost. Both should fail loudly.
#[test]
fn report_unhandled_node_kinds() {
    let dir = corpus_dir();
    if !dir.exists() {
        return;
    }

    let mut unhandled: std::collections::BTreeMap<String, usize> = Default::default();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read corpus dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();

    for file in &files {
        let text = std::fs::read_to_string(file).expect("read corpus file");
        for case in parse_corpus(&text) {
            if let Ok((_, kinds)) = st_fmt::format_source_reporting(&case.source) {
                for kind in kinds {
                    *unhandled.entry(kind).or_default() += 1;
                }
            }
        }
    }

    if unhandled.is_empty() {
        return;
    }

    let mut by_count: Vec<_> = unhandled.into_iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let listing: Vec<String> = by_count
        .iter()
        .map(|(kind, count)| format!("  {count:4}  {kind}"))
        .collect();

    panic!(
        "{} node kind(s) fell through to the verbatim fallback:\n{}\n\n\
         Add a dispatch arm in src/fmt/mod.rs for each.",
        by_count.len(),
        listing.join("\n")
    );
}
