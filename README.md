# st-fmt

An opinionated formatter for **IEC 61131-3 Structured Text**, built on the
[tree-sitter-structured-text](https://github.com/teunreyniers/tree-sitter-structured-text)
grammar.

It formats valid files only. A file that does not parse is refused with a
position and no output, there is no best-effort mode.

```sh
st-fmt <PATH>...       # format each file in place; a directory is walked
st-fmt --check <P>...  # report files that would change; write nothing
st-fmt -               # read stdin, write formatted source to stdout
```

Exit codes: `0` clean, `1` would-change (`--check` only), `2` parse or I/O error.

A directory argument is searched recursively for `.st`, `.iec` and `.scl` files,
so `st-fmt .` formats a whole project and `st-fmt --check .` is the CI gate.
Hidden entries such as `.git` are skipped, and so are symlinks, because
formatting is in place and writing through a link would rewrite a file outside
the tree. A file named directly on the command line is formatted whatever its
extension.

## Style

See [STYLE.md](STYLE.md). Briefly: 100 columns, 4-space indent, UPPERCASE
keywords and elementary types, identifiers preserved, `:` and `:=` column-aligned
in declarations, and long expressions wrapped with the operator leading each
continuation line.

st-fmt is zero-config. The constants live in `src/style.rs`.

## Pre-commit hook

st-fmt ships hook definitions for [pre-commit](https://pre-commit.com), so a
Structured Text repository can name this one in its `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/teunreyniers/st-fmt
    rev: v0.1.0        # any tag or commit of this repository
    hooks:
      - id: st-fmt
```

`language: rust` means pre-commit builds the binary with cargo the first time
the hook is installed. That machine needs network then, to fetch the pinned
grammar, but never the tree-sitter CLI. Two hooks are defined:

| Hook | Effect |
|---|---|
| `st-fmt` | Formats the staged files in place. The commit fails reporting the files the hook modified; stage them and commit again. |
| `st-fmt-check` | Writes nothing and fails on a file that is not formatted. Use it where the commit should never be rewritten under you. |

Both match `.st`, `.iec` and `.scl` case-insensitively and skip symlinks. A
parse error exits 2, so a file that does not parse fails the commit as loudly as
an unformatted one, and is left untouched.

### Without the framework

A plain `.git/hooks/pre-commit` does the same with the staged paths:

```sh
#!/bin/sh
git diff --cached --name-only --diff-filter=ACMR -z \
    -- ':(icase)*.st' ':(icase)*.iec' ':(icase)*.scl' \
  | xargs -0 -r st-fmt --check
```

`xargs -r` matters: with no paths at all st-fmt prints its usage and exits 2,
which would fail every commit that touches no Structured Text.

## Build

```sh
cargo build
cargo test --workspace
```

`--workspace` picks up `bindings/ffi` as well; a plain `cargo build` deliberately
produces only the library and the CLI, leaving the C ABI to be built on demand.

Nothing else is needed. The grammar is a **git dependency pinned to an exact
revision**, and its generated `parser.c` is committed, so cargo fetches and
builds it without the tree-sitter CLI.

> **Not the crates.io crate.** The `tree-sitter-structured-text` package on
> crates.io is a different grammar by another author, unrelated to this project.
> st-fmt is written against the node kinds and tree shape of the grammar at
> `github.com/teunreyniers/tree-sitter-structured-text` and will not work with
> the crates.io one. Do not "fix" the dependency by pointing it at the registry.

### Developing against a local grammar checkout

To iterate on the grammar and the formatter together, copy the example override
and point it at your checkout:

```sh
cp .cargo/config.toml.example .cargo/config.toml
```

`.cargo/config.toml` is gitignored, so the override stays local while everyone
else, CI included, keeps building the pinned revision. To adopt grammar changes
for real, push them and bump `rev` in `Cargo.toml`.

## Calling it from Python

`bindings/python` packages the formatter for **CPython 2.7, CPython 3.x, and
Jython 2.7**:

```python
import st_fmt

st_fmt.format_source("if a then x:=1; end_if;")
st_fmt.is_formatted(source)
```

There is no binding generator that covers both Python majors, so the package
does not use one. `bindings/ffi` exposes the formatter as a plain C ABI, which
`ctypes` calls in-process; Jython, which has no working ctypes, spawns the CLI
instead. Nothing links against libpython, so one build serves every interpreter.

```sh
python3 bindings/python/build.py     # writes a wheel and a Jython archive
```

See [`bindings/python/README.md`](bindings/python/README.md), in particular the
note on Linux glibc floors, which decides whether the result runs on an older
host.

### Jython

A Jython host generally has no pip: a third-party library is a directory on the
Python path rather than something a package manager installs. So the build also
writes a zip of the package tree, to be unpacked onto that path.

Jython is also why the subprocess backend exists. A library that reaches native
code through a CPython extension module cannot be imported under Jython at all,
so the formatter ships as an executable the package spawns instead, and the
same `st_fmt` API works there and on a developer's CPython.

## How it works

```mermaid
flowchart LR
    source[source] --> parse[parse] --> gate[validity gate] --> trivia[trivia scan] --> doc[Doc IR] --> render[render]
```

| Module | Responsibility |
|---|---|
| `src/parse.rs` | Parsing and the refusal path: ERROR nodes, MISSING nodes, truncated input |
| `src/trivia.rs` | Comment and blank-line recovery |
| `src/doc.rs` | Wadler/Prettier document IR, its renderer, and paragraph fill |
| `src/style.rs` | Width, indent, and the casing tables |
| `src/fmt/` | One module per construct family, dispatched by node kind |

Three properties of the grammar shape the design:

1. **Comments are `extras`.** They float to arbitrary positions in the tree, and
   one can land inside an assignment between `:=` and its right-hand side.
   Comment placement is recovered from byte offsets by a cursor in `trivia.rs`,
   never from tree shape.
2. **Whitespace is invisible.** There is no newline token, so blank lines are
   recovered by comparing source positions.
3. **`;` belongs to `block`, not to the statement.** A statement node's range
   stops before its terminator, so blocks emit semicolons themselves.

## Tests

```sh
cargo test                                    # everything
UPDATE_EXPECT=1 cargo test                    # regenerate expected files, then review the diff
cargo run --example sexp -- file.st           # inspect a parse tree
cargo run --example check_tree -- /path/to/plc  # run the invariants over a real tree, read-only
```

Fixtures are plain file pairs (`<case>.st` and `<case>.expected.st`) and each
is checked four ways:

1. `format(input) == expected`
2. **idempotence**: formatting the output again changes nothing
3. **semantic preservation**: the parse tree is unchanged, ignoring comments and
   inserted empty statements
4. **comment conservation**: no comment dropped, duplicated or altered

### Continuous integration

Two workflows run on every push and pull request. `rust.yml` builds and tests
the workspace and gates `cargo fmt` and `clippy`. `bindings.yml` runs the Python
suite once per interpreter the package claims:

| Job | Interpreter | Backend exercised |
|---|---|---|
| `cpython3` | CPython 3.9 and 3.13 | `ctypes` and `subprocess` |
| `cpython27` | CPython 2.7 | `ctypes` and `subprocess` |
| `jython` | Jython 2.7 | `subprocess` |

Each job asserts which backend actually loaded before running the suite. The
tests skip rather than fail when no native build is staged, so without that
assertion a job that staged nothing would pass with everything skipped.

`cpython27` builds the Rust code inside the same container it tests in. The
shared library and the interpreter that loads it then share a glibc; a library
built on a newer host fails to load, and that failure is swallowed and reported
as a missing backend rather than an error.

The Jython job runs `test_st_fmt.py` only. `test_package.py` inspects the built
wheel and archive, which does not depend on the interpreter, and one of its
cases re-runs `sys.executable`, which is not a usable program under
`java -jar`.

