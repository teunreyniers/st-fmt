# -*- coding: utf-8 -*-
"""Locating the native artifacts that ship inside the package.

Builds land in ``_native/<system>-<machine>/``. The tag has to be worked out
under CPython 2.7, CPython 3.x and Jython, which disagree about how to name the
machine they are on, so the detection is done by hand rather than through
``sysconfig``, which does not exist in a useful form on Jython.
"""

from __future__ import absolute_import

import os
import struct
import sys

__all__ = ["is_jython", "system", "candidate_tags", "find_library", "find_executable"]

_HERE = os.path.dirname(os.path.abspath(__file__))
NATIVE_DIR = os.path.join(_HERE, "_native")

# One shared library and one executable per tag. The library is what the ctypes
# backend loads; the executable is what Jython drives over a pipe.
_LIBRARY_NAMES = {
    "linux": "libst_fmt_c.so",
    "macos": "libst_fmt_c.dylib",
    "windows": "st_fmt_c.dll",
}
_EXECUTABLE_NAMES = {
    "linux": "st-fmt",
    "macos": "st-fmt",
    "windows": "st-fmt.exe",
}


def is_jython():
    """True on Jython, which is what Ignition embeds."""
    return sys.platform.startswith("java")


def _raw_system_and_machine():
    if is_jython():
        # `platform.machine()` on Jython reports the JVM's idea of the host in a
        # form that varies by vendor. The system properties are dependable, and
        # `os.arch` reflects the JVM's own bitness, which is what matters.
        from java.lang import System as _System  # noqa: F401  (Jython only)

        return (_System.getProperty("os.name") or "", _System.getProperty("os.arch") or "")
    import platform

    return (platform.system(), platform.machine())


def system():
    """Returns ``linux``, ``macos``, ``windows`` or the raw name, lowercased."""
    name = _raw_system_and_machine()[0].lower()
    if "windows" in name:
        return "windows"
    if "linux" in name:
        return "linux"
    if "darwin" in name or "mac os" in name or "macos" in name:
        return "macos"
    return name.replace(" ", "_")


def _machine():
    raw = _raw_system_and_machine()[1].lower()
    if raw in ("x86_64", "amd64", "x64", "em64t"):
        machine = "x86_64"
    elif raw in ("aarch64", "arm64"):
        machine = "aarch64"
    elif raw in ("i386", "i486", "i586", "i686", "x86", "i86pc"):
        machine = "i686"
    else:
        machine = raw.replace(" ", "_")

    # Windows reports the machine the *OS* runs on even to a 32-bit interpreter,
    # and a 32-bit process cannot load a 64-bit DLL. The pointer width is the
    # honest answer. Jython is excluded: `os.arch` already describes the JVM.
    if machine == "x86_64" and not is_jython() and struct.calcsize("P") == 4:
        machine = "i686"
    return machine


def candidate_tags():
    """Platform tags to look under, best match first.

    More than one is offered because a 32-bit build runs happily on a 64-bit
    host, and because a caller may only have built one of the two. A backend
    walks the list and takes the first artifact it can actually use, so a tag
    that turns out to be wrong -- a 64-bit library under a 32-bit interpreter --
    costs a failed load rather than an outright error.
    """
    host = system()
    tags = []

    def offer(tag):
        if tag not in tags:
            tags.append(tag)

    machine = _machine()
    offer("%s-%s" % (host, machine))
    if machine == "x86_64":
        offer("%s-i686" % host)
    elif machine == "i686":
        offer("%s-x86_64" % host)
    elif machine == "aarch64" and host == "macos":
        offer("macos-x86_64")  # Rosetta 2 runs it
    return tags


def _find(names_by_system):
    name = names_by_system.get(system())
    if name is None:
        # A bare return: this is a generator, and Python 2.7 rejects `return`
        # with a value inside one outright, at import time.
        return
    for tag in candidate_tags():
        path = os.path.join(NATIVE_DIR, tag, name)
        if os.path.isfile(path):
            yield path


def find_library():
    """Every bundled shared library that might load here, best match first."""
    return list(_find(_LIBRARY_NAMES))


def find_executable():
    """Every bundled CLI binary that might run here, best match first."""
    return list(_find(_EXECUTABLE_NAMES))


def installed_tags():
    """Tags that actually have artifacts staged, for error messages."""
    if not os.path.isdir(NATIVE_DIR):
        return []
    return sorted(
        entry
        for entry in os.listdir(NATIVE_DIR)
        if os.path.isdir(os.path.join(NATIVE_DIR, entry))
    )
