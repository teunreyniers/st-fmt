# st-fmt for Python

Python bindings for [st-fmt](https://github.com/teunreyniers/st-fmt), an
opinionated formatter for IEC 61131-3 Structured Text. One package covers
**CPython 2.7, CPython 3.x, and Jython 2.7 — including Ignition**.

```python
import st_fmt

st_fmt.format_source("if a then x:=1; end_if;")
# u'IF a THEN\n    x := 1;\nEND_IF\n'

st_fmt.is_formatted(source)   # True if st-fmt would leave it alone
```

## Why it works on Python 2.7

There is no binding generator that targets both 2.7 and 3.x — PyO3 and maturin
are Python 3 only — so the package does not use one. The formatter is exposed
as a plain C ABI (`bindings/ffi`) and called through `ctypes`, which has been in
the standard library since 2.5. Nothing links against `libpython`, so a single
build of the native library serves every interpreter version on a platform.

Jython, which is what Ignition embeds, has no working `ctypes` at all. There the
package falls back to spawning the bundled `st-fmt` executable and piping the
source through it. Both paths are behind the same API and return the same
results and the same error messages; `st_fmt.backend_name()` says which is live.

| Interpreter | Backend | Cost per call |
| --- | --- | --- |
| CPython 2.7 / 3.x | `ctypes`, in-process | microseconds |
| Jython 2.7 / Ignition | `subprocess` | a process launch, a few ms |

## Building a package

The build needs cargo and any Python 3. It has no third-party dependencies —
not even `setuptools` or `wheel`.

```sh
python3 bindings/python/build.py
```

That writes into `bindings/python/dist/`:

```
st_fmt-0.1.0-py2.py3-none-linux_x86_64.whl   pip install, CPython 2.7+
st_fmt-0.1.0-ignition-linux-x86_64.zip       unzip into Ignition's pylib
```

Useful flags:

| Flag | Effect |
| --- | --- |
| `--container <image>` | build in a container, not against this machine's libc |
| `--only library\|executable` | stage one artifact, leaving the other in place |
| `--target <triple>` | cross-compile, e.g. `x86_64-pc-windows-gnu` |
| `--tag <tag>` | stage under a platform tag other than the host's |
| `--keep` | keep builds already staged for other platforms |
| `--skip-build` | package what is staged without running cargo |
| `--profile debug` | build unoptimised |

### Linux: mind the glibc floor

A host build links against the host's glibc and **will not run on anything
older**. Building on Ubuntu 24.04 produces artifacts needing glibc 2.39; an
Ignition gateway on RHEL 8 has 2.28 and refuses them:

```
st-fmt: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.34' not found
```

`--container` builds somewhere older instead. It needs podman or docker, and
takes two aliases:

| Alias | Image | Produces | Runs on |
| --- | --- | --- | --- |
| `portable` | `rust:1-bullseye` | library + executable | glibc 2.30 and newer |
| `static` | `rust:1-alpine` | executable only | any Linux, no libc needed |

`static` builds a musl target, which links the C runtime statically. That is why
it cannot produce the shared library — musl targets cannot emit a cdylib at all
— and also why the executable it does produce is a static-PIE with no dynamic
dependencies whatsoever.

So run both, `static` second, to replace the executable in place:

```sh
python3 bindings/python/build.py --container portable
python3 bindings/python/build.py --container static --only executable
```

The package now holds a shared library for modern CPython and an executable that
runs anywhere. That is the right split, because the two backends have different
audiences: the ctypes path serves CPython on a developer machine, while the
subprocess path is what an old Ignition gateway takes — and that is exactly the
one that must survive an ancient distro.

To check what you built:

```sh
objdump -T bindings/python/st_fmt/_native/*/st-fmt | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1
```

No output at all means a static binary, which is the goal.

### One package, several platforms

Each build stages into `st_fmt/_native/<system>-<machine>/`, and the package
picks a directory at import time. To cover more than one platform, build on each
machine, copy the `_native/<tag>/` directories together, and package the lot:

```sh
python3 bindings/python/build.py --skip-build --keep
```

The wheel is then tagged `none-any`, since it is no longer specific to one
platform.

## Installing

### CPython

```sh
pip install dist/st_fmt-0.1.0-py2.py3-none-linux_x86_64.whl
```

The same file installs under 2.7 and 3.x — the wheel is tagged `py2.py3-none-…`
and carries no compiled Python extension.

**Windows bitness matters.** A 32-bit interpreter cannot load a 64-bit DLL, and
Windows reports the *machine's* architecture even to a 32-bit process, so the
package uses the pointer width instead. Surviving 2.7 installs on engineering
workstations are often 32-bit: build `--target i686-pc-windows-msvc` for those.

### Ignition

Ignition has no pip. Unzip the `-ignition-` archive into the gateway's
third-party Python path and restart:

```
<Ignition>/user-lib/pylib/st_fmt/
```

Extract it — do not drop the `.zip` on the path, because the binary inside
cannot be executed from within an archive. On Linux the executable bit must
survive; the package chmods the binary itself on first use, which fails only if
`pylib` is read-only. Full notes are in `INSTALL.txt` inside the archive.

From a script console:

```python
import st_fmt
print st_fmt.backend_name()          # 'subprocess'
print st_fmt.format_source(source)
```

If `pylib` cannot hold an executable, put the binary elsewhere and say so once
at startup:

```python
st_fmt.select_backend("subprocess", "/opt/st-fmt/st-fmt")
```

or set `ST_FMT_BINARY` in the gateway's environment.

## API

| Name | Description |
| --- | --- |
| `format_source(source)` | Formats text or UTF-8 bytes; returns text. |
| `is_formatted(source)` | True if the source is already formatted. |
| `backend_name()` | `'ctypes'` or `'subprocess'`. |
| `native_version()` | The version of the bundled formatter. |
| `select_backend(kind, path)` | Force a backend, optionally from a given path. |
| `FormatError` | Raised on source that does not parse; has `.line`, `.column`, `.message`. |
| `BackendError` | No usable native build for this platform. |

st-fmt refuses source it cannot fully parse rather than half-formatting it, so
`FormatError` means *nothing was changed*. `is_formatted` raises it too — "does
not parse" is a different answer from "is not formatted", and collapsing the two
would let a broken file pass a check.

Environment overrides: `ST_FMT_BACKEND` (`ctypes` or `subprocess`),
`ST_FMT_LIBRARY`, `ST_FMT_BINARY`.

## Tests

```sh
python3 -m unittest discover -s bindings/python/tests -v
```

Each test runs against every backend the interpreter can load, so a CPython run
still exercises the subprocess path that Jython depends on. Run
`bindings/python/build.py` first — without staged binaries the tests skip.
