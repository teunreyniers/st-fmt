# -*- coding: utf-8 -*-
"""The portable backend: the ``st-fmt`` CLI driven over a pipe.

Jython -- which is what Ignition runs -- has no working ctypes, so the only way
to reach a native formatter from an Ignition script is to spawn it. ``st-fmt -``
reads stdin and writes the formatted source to stdout, which is exactly the
shape needed here.

It costs a process launch per call, a few milliseconds, so the ctypes backend is
preferred wherever it can be loaded.
"""

from __future__ import absolute_import

import os
import subprocess

from ._errors import BackendError, StFmtError, format_error
from . import _platform

# The CLI reserves 2 for a refusal -- a file it will not rewrite -- and 1 for
# `--check` reporting a file that would change, which this backend never asks
# for. Anything else means the process itself went wrong.
_EXIT_OK = 0
_EXIT_REFUSED = 2


class SubprocessBackend(object):
    """Formats by piping the source through the ``st-fmt`` executable."""

    name = "subprocess"

    def __init__(self, executable_path):
        self.path = executable_path
        _ensure_executable(executable_path)

    def _run(self, arguments, stdin_bytes=None):
        try:
            process = subprocess.Popen(
                [self.path] + arguments,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                startupinfo=_startupinfo(),
            )
        except OSError as exc:
            raise BackendError("could not run %s: %s" % (self.path, exc))
        out, err = process.communicate(stdin_bytes)
        return process.returncode, out, err

    def format(self, source_bytes):
        """Formats UTF-8 bytes and returns UTF-8 bytes."""
        code, out, err = self._run(["-"], source_bytes)
        if code == _EXIT_OK:
            return out
        message = err.decode("utf-8", "replace").strip()
        if code == _EXIT_REFUSED:
            raise format_error(message)
        raise StFmtError(
            "st-fmt exited with %d: %s" % (code, message or "no diagnostic")
        )

    def version(self):
        code, out, err = self._run(["-V"])
        if code != _EXIT_OK:
            raise StFmtError("st-fmt -V exited with %d: %s" % (code, err.decode("utf-8", "replace")))
        # `st-fmt 0.1.0`
        return out.decode("utf-8").strip().split()[-1]


def _startupinfo():
    """Keeps Windows from flashing a console window for every call.

    Returns ``None`` everywhere else, and on any interpreter that does not offer
    ``STARTUPINFO`` -- Jython among them -- where the flag is simply unavailable.
    """
    if _platform.system() != "windows":
        return None
    try:
        info = subprocess.STARTUPINFO()
        info.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        return info
    except (AttributeError, TypeError):
        return None


def _ensure_executable(path):
    """Restores the executable bit, which unzipping the package strips.

    The Ignition install path is "extract this into user-lib/pylib", and Python's
    zipfile does not carry permissions across, so a freshly deployed binary is
    typically mode 644 and will not run.
    """
    if _platform.system() == "windows" or os.access(path, os.X_OK):
        return
    try:
        os.chmod(path, 0o755)
    except OSError as exc:
        raise BackendError(
            "%s is not executable and could not be chmod'ed (%s); "
            "run: chmod +x %s" % (path, exc, path)
        )


def load():
    """Loads the first bundled executable for this platform, or ``None``."""
    for path in _platform.find_executable():
        try:
            return SubprocessBackend(path)
        except BackendError:
            continue
    return None


def require():
    """Like :func:`load`, but raises rather than returning ``None``."""
    backend = load()
    if backend is None:
        raise BackendError("no bundled st-fmt executable for this platform")
    return backend
