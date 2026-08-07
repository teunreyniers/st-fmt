//! The st-fmt command line interface.
//!
//! ```text
//! st-fmt <FILE>...       format each file in place
//! st-fmt --check <F>...  exit 1 if any file would change; write nothing
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
    st-fmt <FILE>...       format each file in place
    st-fmt --check <F>...  report files that would change; write nothing
    st-fmt -               read stdin, write formatted source to stdout

OPTIONS:
    --check       do not write; exit 1 if any file is not already formatted
    -h, --help    show this help
    -V, --version show the version
";

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
    format_files(&paths, check)
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
