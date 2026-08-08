# -*- coding: utf-8 -*-
"""The fast backend: the C ABI in ``libst_fmt_c``, called through ctypes.

This is the whole reason the bindings are ctypes rather than an extension
module. ctypes has been in the standard library since 2.5 and talks to a plain
C ABI, so one shared library serves CPython 2.7 and every CPython 3 without
being built against any of them. Jython has no working ctypes and falls to
:mod:`st_fmt._subprocess_backend` instead.
"""

from __future__ import absolute_import

from ._errors import BackendError, StFmtError, format_error
from . import _platform

# Mirrors the ST_FMT_* constants in bindings/ffi/src/lib.rs.
_OK = 0
_ERR_PARSE = 1
_ERR_ENCODING = 2
_ERR_PANIC = 3
_ERR_NULL = 4


class CtypesBackend(object):
    """Formats by calling into the shared library in-process."""

    name = "ctypes"

    def __init__(self, library_path):
        import ctypes

        self._ctypes = ctypes
        self.path = library_path
        library = ctypes.CDLL(library_path)

        self._format = library.st_fmt_format
        self._format.argtypes = [
            ctypes.c_char_p,
            ctypes.c_size_t,
            ctypes.POINTER(ctypes.c_void_p),
        ]
        self._format.restype = ctypes.c_int

        # Declared void* rather than char* so ctypes hands the pointer back
        # untouched; a c_char_p restype would copy the bytes and lose the
        # address we have to give to st_fmt_free.
        self._free = library.st_fmt_free
        self._free.argtypes = [ctypes.c_void_p]
        self._free.restype = None

        self._version = library.st_fmt_version
        self._version.argtypes = []
        self._version.restype = ctypes.c_char_p

    def format(self, source_bytes):
        """Formats UTF-8 bytes and returns UTF-8 bytes."""
        ctypes = self._ctypes
        out = ctypes.c_void_p()
        code = self._format(source_bytes, len(source_bytes), ctypes.byref(out))
        try:
            payload = ctypes.cast(out, ctypes.c_char_p).value if out.value else b""
        finally:
            # The library owns this memory and only its own deallocator may
            # release it, so this runs even if the cast above went wrong.
            self._free(out)

        if code == _OK:
            return payload
        raise _translate(code, payload)

    def version(self):
        return self._version().decode("utf-8")


def _translate(code, payload):
    message = payload.decode("utf-8", "replace")
    if code == _ERR_PARSE:
        return format_error(message)
    if code == _ERR_ENCODING:
        return ValueError(message or "input is not valid UTF-8")
    if code == _ERR_PANIC:
        return StFmtError("st-fmt failed internally: %s" % (message or "panic",))
    return StFmtError("st-fmt returned an unexpected code %d: %s" % (code, message))


def load():
    """Loads the first bundled library this interpreter can use.

    Returns ``None`` when ctypes is unavailable or nothing loads, so that the
    caller can fall back rather than fail. The reasons are collected onto the
    exception only if every backend is exhausted.
    """
    if _platform.is_jython():
        return None
    try:
        import ctypes  # noqa: F401
    except ImportError:
        return None

    problems = []
    for path in _platform.find_library():
        try:
            return CtypesBackend(path)
        except (OSError, AttributeError) as exc:
            # OSError: wrong architecture, or a missing system dependency.
            # AttributeError: a library that loaded but is not ours.
            problems.append("%s: %s" % (path, exc))
    if problems:
        load.last_problems = problems
    return None


load.last_problems = []


def require():
    """Like :func:`load`, but raises rather than returning ``None``."""
    backend = load()
    if backend is None:
        raise BackendError(
            "no usable st-fmt shared library; tried %s" % (load.last_problems or "nothing",)
        )
    return backend
