//! The command line's own behaviour: which paths a run picks up, and what it
//! reports. Formatting itself is covered by the fixtures.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_st-fmt");

/// Deliberately unformatted: lowercase keywords and no space around `:=`.
const UGLY: &str = "program Main\nx:=1;\nend_program\n";

#[test]
fn a_directory_argument_is_walked() {
    let tree = TempTree::new("walk");
    tree.write("top.st", UGLY);
    tree.write("nested/deep/inner.st", UGLY);
    tree.write("nested/upper.ST", UGLY);
    tree.write("nested/other.iec", UGLY);
    tree.write("notes.txt", UGLY);
    tree.write(".hidden/skipped.st", UGLY);
    tree.write("nested/.also-hidden.st", UGLY);

    let out = tree.run(&["--check", "."]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));

    // Sorted within the directory argument, so the listing is reproducible.
    assert_eq!(
        listed(&out),
        vec![
            "./nested/deep/inner.st",
            "./nested/other.iec",
            "./nested/upper.ST",
            "./top.st",
        ]
    );
}

#[test]
fn a_walk_formats_what_it_finds_and_leaves_the_rest() {
    let tree = TempTree::new("format");
    tree.write("top.st", UGLY);
    tree.write("nested/inner.st", UGLY);
    tree.write("notes.txt", UGLY);
    tree.write(".hidden/skipped.st", UGLY);

    let out = tree.run(&["."]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    assert_eq!(
        tree.read("top.st"),
        "PROGRAM Main\nx := 1;\n\nEND_PROGRAM\n"
    );
    assert_ne!(tree.read("nested/inner.st"), UGLY);
    assert_eq!(tree.read("notes.txt"), UGLY);
    assert_eq!(tree.read(".hidden/skipped.st"), UGLY);

    // Idempotent at the CLI level too: a second run has nothing left to do.
    let again = tree.run(&["--check", "."]);
    assert_eq!(again.status.code(), Some(0), "{}", stderr(&again));
}

#[test]
fn a_file_argument_is_formatted_whatever_it_is_called() {
    let tree = TempTree::new("named");
    tree.write("odd_extension.plcst", UGLY);

    let out = tree.run(&["odd_extension.plcst"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_ne!(tree.read("odd_extension.plcst"), UGLY);
}

#[test]
fn a_directory_holding_nothing_to_format_says_so() {
    let tree = TempTree::new("empty");
    tree.write("notes.txt", UGLY);

    let out = tree.run(&["."]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("no Structured Text files found"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_missing_path_fails() {
    let tree = TempTree::new("missing");

    let out = tree.run(&["nope.st"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

/// A directory under the target directory, removed when the test ends.
struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp tree");
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("create directory");
        std::fs::write(path, contents).expect("write file");
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.root.join(relative)).expect("read file")
    }

    /// Runs st-fmt with the tree as its working directory, so the arguments and
    /// the reported paths are both relative to it.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("run st-fmt")
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn listed(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| line.replace('\\', "/"))
        .collect()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}
