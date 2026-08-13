# -*- coding: utf-8 -*-
"""st-fmt — an opinionated formatter for IEC 61131-3 Structured Text.

    >>> import st_fmt
    >>> st_fmt.format_source("if a then x:=1; end_if;")
    u'IF a THEN\\n    x := 1;\\nEND_IF\\n'

The formatter itself is the Rust library; this package carries a build of it and
picks whichever way of calling it works on the interpreter you are running:

* **ctypes**, on CPython 2.7 and 3.x, calling the bundled shared library
  in-process. This is the fast path.
* **subprocess**, on Jython, which has no working ctypes. The bundled
  ``st-fmt`` executable is driven over a pipe.

Nothing here links against libpython, so a single build serves every version.

Source that does not parse is refused with a :class:`FormatError` rather than
half-formatted: st-fmt never rewrites a file it does not fully understand.
"""

from __future__ import absolute_import

import os
import sys

from ._errors import BackendError, FormatError, StFmtError
from ._version import __version__
from . import _ctypes_backend, _platform, _subprocess_backend

__all__ = [
    "format_source",
    "is_formatted",
    "backend_name",
    "native_version",
    "select_backend",
    "FormatError",
    "BackendError",
    "StFmtError",
    "__version__",
]

if sys.version_info[0] >= 3:
    _TEXT_TYPES = (str,)
    _BYTE_TYPES = (bytes, bytearray)
else:  # pragma: no cover - exercised only on Python 2
    _TEXT_TYPES = (unicode,)  # noqa: F821
    _BYTE_TYPES = (str, bytearray)

_backend = None


def _to_utf8(source):
    """Normalises the argument to UTF-8 bytes for the backend."""
    if isinstance(source, _TEXT_TYPES):
        return source.encode("utf-8")
    if isinstance(source, _BYTE_TYPES):
        return bytes(source)
    raise TypeError("source must be text or UTF-8 bytes, not %s" % type(source).__name__)


def _to_text(source):
    """Normalises the argument to text, for comparing against a result."""
    if isinstance(source, _TEXT_TYPES):
        return source
    if isinstance(source, _BYTE_TYPES):
        return bytes(source).decode("utf-8")
    raise TypeError("source must be text or UTF-8 bytes, not %s" % type(source).__name__)


def _choose():
    """Picks a backend, honouring the environment overrides.

    ``ST_FMT_LIBRARY`` and ``ST_FMT_BINARY`` name an artifact outside the
    package. Both matter on a Jython host, where the third-party library
    directory may be read-only or on a share that cannot hold an executable
    file, so the binary has to live elsewhere on the machine.

    ``ST_FMT_BACKEND`` pins the mechanism to ``ctypes`` or ``subprocess``,
    which is mostly useful for testing the Jython path from CPython.
    """
    library = os.environ.get("ST_FMT_LIBRARY")
    if library:
        return _ctypes_backend.CtypesBackend(library)
    binary = os.environ.get("ST_FMT_BINARY")
    if binary:
        return _subprocess_backend.SubprocessBackend(binary)

    kind = os.environ.get("ST_FMT_BACKEND", "").strip().lower()
    if kind == "ctypes":
        return _ctypes_backend.require()
    if kind == "subprocess":
        return _subprocess_backend.require()
    if kind:
        raise BackendError("ST_FMT_BACKEND must be 'ctypes' or 'subprocess', not %r" % (kind,))

    # In-process first, then the pipe. Jython declines the first outright.
    chosen = _ctypes_backend.load() or _subprocess_backend.load()
    if chosen is None:
        raise BackendError(_no_backend_message())
    return chosen


def _no_backend_message():
    tags = _platform.installed_tags()
    lines = [
        "st-fmt has no native build for this platform.",
        "  interpreter: %s" % ("Jython" if _platform.is_jython() else "CPython"),
        "  looked for:  %s" % ", ".join(_platform.candidate_tags()),
        "  installed:   %s" % (", ".join(tags) if tags else "nothing"),
    ]
    problems = _ctypes_backend.load.last_problems
    for problem in problems:
        lines.append("  failed:      %s" % problem)
    lines.append("Rebuild with bindings/python/build.py, or point ST_FMT_BINARY at an st-fmt executable.")
    return "\n".join(lines)


def select_backend(kind=None, path=None):
    """Forces the backend, replacing whatever was chosen automatically.

    Pass ``kind='subprocess', path='/opt/st-fmt/st-fmt'`` to use a binary that
    is not inside the package. Passing nothing discards the current choice so
    the next call re-runs the automatic selection. Returns the backend's name.
    """
    global _backend
    if kind is None and path is None:
        _backend = None
        return None
    if kind == "ctypes":
        _backend = _ctypes_backend.CtypesBackend(path) if path else _ctypes_backend.require()
    elif kind == "subprocess":
        _backend = _subprocess_backend.SubprocessBackend(path) if path else _subprocess_backend.require()
    else:
        raise ValueError("kind must be 'ctypes' or 'subprocess', not %r" % (kind,))
    return _backend.name


def _active():
    global _backend
    if _backend is None:
        _backend = _choose()
    return _backend


def format_source(source):
    """Formats Structured Text and returns it as text.

    ``source`` may be text or UTF-8 bytes; the result is always text. Raises
    :class:`FormatError` if the source does not parse, in which case nothing
    was formatted at all.
    """
    return _active().format(_to_utf8(source)).decode("utf-8")


def is_formatted(source):
    """True if ``source`` is already exactly what st-fmt would write.

    Raises :class:`FormatError` for source that does not parse — "not valid" is
    a different answer from "not formatted", and squashing them into ``False``
    would let a broken file pass a check.
    """
    return format_source(source) == _to_text(source)


def backend_name():
    """The mechanism in use: ``'ctypes'`` or ``'subprocess'``."""
    return _active().name


def native_version():
    """The version reported by the bundled formatter itself."""
    return _active().version()
