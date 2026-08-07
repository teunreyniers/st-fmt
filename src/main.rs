//! The st-fmt command line interface.
//!
//! ```text
//! st-fmt <PATH>...       format each file in place; walk each directory
//! st-fmt --check <P>...  exit 1 if any file would change; write nothing
//! st-fmt -               read stdin, write formatted source to stdout
//! ```
//!
//! Exit codes: 0 clean, 1 would-change (`--check` only), 2 parse or I/O error.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
st-fmt — a formatter for IEC 61131-3 Structured Text

USAGE:
    st-fmt <PATH>...       format each file in place; a directory is walked
    st-fmt --check <P>...  report files that would change; write nothing
    st-fmt -               read stdin, write formatted source to stdout

A directory argument is searched recursively for .st, .iec and .scl files.
Hidden entries and symlinks are skipped. A file named directly is always
formatted, whatever it is called.

OPTIONS:
    --check       do not write; exit 1 if any file is not already formatted
    -h, --help    show this help
    -V, --version show the version
";

/// The extensions a directory walk picks up. Matched case-insensitively, since
/// vendor exports are as likely to write `.ST` as `.st`.
const ST_EXTENSIONS: [&str; 3] = ["st", "iec", "scl"];

/// Exit code 2 is reserved for a refusal: a file that does not parse, or an I/O
/// failure. It is distinct from 1 so `--check` in CI can tell "needs
/// formatting" apart from "is broken".
const EXIT_FAILURE: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("st-fmt: {msg}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut check = false;
    let mut stdin = false;
    let mut paths: Vec<PathBuf> = Vec::new();

    for arg in &args {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            "-V" | "--version" => {
                println!("st-fmt {}", env!("CARGO_PKG_VERSION"));
                return Ok(ExitCode::SUCCESS);
            }
            "--check" => check = true,
            "-" => stdin = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown option `{other}`\n\n{USAGE}"));
            }
            other => paths.push(PathBuf::from(other)),
        }
    }

    if stdin && !paths.is_empty() {
        return Err("cannot mix `-` with file arguments".to_owned());
    }
    if !stdin && paths.is_empty() {
        eprint!("{USAGE}");
        return Ok(ExitCode::from(EXIT_FAILURE));
    }

    if stdin {
        return format_stdin();
    }

    let (files, walked_cleanly) = collect_targets(&paths);
    if files.is_empty() {
        // Only a directory argument can come back empty, and a silent success
        // there reads as "everything is formatted" when nothing was looked at.
        eprintln!("st-fmt: no Structured Text files found");
    }

    let code = format_files(&files, check)?;
    if !walked_cleanly {
        return Ok(ExitCode::from(EXIT_FAILURE));
    }
    Ok(code)
}

/// Expands the command line into the list of files to format: a file argument
/// stands for itself, a directory for every Structured Text file beneath it.
///
/// Returns `false` alongside the files if any directory could not be read, so
/// that a partial walk cannot pass for a clean one.
fn collect_targets(paths: &[PathBuf]) -> (Vec<PathBuf>, bool) {
    let mut files = Vec::new();
    let mut ok = true;

    for path in paths {
        if !path.is_dir() {
            files.push(path.clone());
            continue;
        }
        // Sort what this argument contributed: `read_dir` order is arbitrary,
        // and `--check` output should not shuffle between runs. Each argument
        // is sorted on its own, so the order they were given in survives.
        let start = files.len();
        ok &= walk(path, &mut files);
        files[start..].sort();
    }

    (files, ok)
}

/// Collects the Structured Text files under `dir`, recursively.
///
/// Hidden entries are skipped — `.git` holds no source — and so are symlinks:
/// formatting is in place, and writing through a link would rewrite a file
/// outside the tree the user pointed at. Reading the entry's own type rather
/// than following it also means a symlink cycle cannot trap the walk.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("st-fmt: {}: {e}", dir.display());
            return false;
        }
    };

    let mut ok = true;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("st-fmt: {}: {e}", dir.display());
                ok = false;
                continue;
            }
        };

        if entry.file_name().as_encoded_bytes().starts_with(b".") {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(e) => {
                eprintln!("st-fmt: {}: {e}", entry.path().display());
                ok = false;
                continue;
            }
        };

        let path = entry.path();
        if file_type.is_dir() {
            ok &= walk(&path, out);
        } else if file_type.is_file() && is_st_file(&path) {
            out.push(path);
        }
    }
    ok
}

fn is_st_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ST_EXTENSIONS
            .iter()
            .any(|known| e.eq_ignore_ascii_case(known))
    })
}

fn format_stdin() -> Result<ExitCode, String> {
    let mut source = String::new();
    std::io::stdin()
        .read_to_string(&mut source)
        .map_err(|e| format!("reading stdin: {e}"))?;

    let formatted = st_fmt::format_source(&source).map_err(|e| format!("<stdin>:{e}"))?;

    std::io::stdout()
        .write_all(formatted.as_bytes())
        .map_err(|e| format!("writing stdout: {e}"))?;
    Ok(ExitCode::SUCCESS)
}

fn format_files(paths: &[PathBuf], check: bool) -> Result<ExitCode, String> {
    let mut would_change: Vec<&Path> = Vec::new();
    let mut failed = false;

    for path in paths {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("st-fmt: {}: {e}", path.display());
                failed = true;
                continue;
            }
        };

        let formatted = match st_fmt::format_source(&source) {
            Ok(f) => f,
            Err(e) => {
                // A refusal names the file and the fault, and changes nothing.
                eprintln!("st-fmt: {}:{e}", path.display());
                failed = true;
                continue;
            }
        };

        if formatted == source {
            continue;
        }

        if check {
            would_change.push(path);
        } else if let Err(e) = std::fs::write(path, &formatted) {
            eprintln!("st-fmt: {}: {e}", path.display());
            failed = true;
        }
    }

    if failed {
        return Ok(ExitCode::from(EXIT_FAILURE));
    }
    if check && !would_change.is_empty() {
        for path in &would_change {
            println!("{}", path.display());
        }
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}
