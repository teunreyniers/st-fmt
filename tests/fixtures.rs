//! Runs every fixture under `tests/fixtures/`.
//!
//! One test covers all fixtures so a single run reports every failure at once
//! instead of stopping at the first.

mod harness;

#[test]
fn all_fixtures() {
    let root = harness::fixtures_root();
    let inputs = harness::discover(&root);

    assert!(
        !inputs.is_empty(),
        "no fixtures found under {}",
        root.display()
    );

    let failures: Vec<harness::Failure> = inputs
        .iter()
        .filter_map(|path| harness::check_fixture(path).err())
        .collect();

    harness::report(failures);
}

/// Every `.expected.st` must be a fixed point of the formatter. This is the
/// same guarantee `st-fmt --check` gives users.
#[test]
fn expected_files_are_already_formatted() {
    let root = harness::fixtures_root();
    let mut failures = Vec::new();

    for input in harness::discover(&root) {
        let expected_path = input.with_extension("expected.st");
        let Ok(expected) = std::fs::read_to_string(&expected_path) else {
            continue;
        };
        let name = expected_path
            .strip_prefix(&root)
            .unwrap_or(&expected_path)
            .display()
            .to_string();

        match st_fmt::format_source(&expected) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(harness::Failure {
                fixture: name,
                detail: format!(
                    "expected file is not a fixed point{}",
                    harness::diff(&expected, &actual)
                ),
            }),
            Err(e) => failures.push(harness::Failure {
                fixture: name,
                detail: format!("expected file does not parse: {e}"),
            }),
        }
    }

    harness::report(failures);
}
