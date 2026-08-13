# -*- coding: utf-8 -*-
"""The exceptions st-fmt raises, and the parser for the messages it returns.

Both backends surface a refusal as the same string -- the C ABI returns
``line:column: what: snippet`` and the CLI prints that with a prefix -- so the
message is picked apart in one place here rather than once per backend.
"""

from __future__ import absolute_import

import re

__all__ = ["StFmtError", "FormatError", "BackendError", "format_error"]


class StFmtError(Exception):
    """Base class for every error this package raises."""


class FormatError(StFmtError):
    """The source was refused: it does not parse, so nothing was formatted.

    ``line`` and ``column`` are 1-based, or ``None`` when the message did not
    carry a position (a formatter panic, for instance).
    """

    def __init__(self, message, line=None, column=None):
        StFmtError.__init__(self, message)
        self.message = message
        self.line = line
        self.column = column


class BackendError(StFmtError):
    """No usable formatter could be loaded for this platform and interpreter."""


# `1:5: syntax error: END_IF` -- the position the formatter stopped at, then a
# description. Anything that does not match is kept whole and reported as-is.
_POSITION = re.compile(r"^(\d+):(\d+):\s*(.*)$", re.DOTALL)

# The CLI writes `st-fmt: <stdin>:1:5: ...`; the C ABI omits both prefixes.
_PREFIXES = ("st-fmt: ", "<stdin>:")


def format_error(message):
    """Builds a :class:`FormatError` from a message produced by either backend."""
    text = message.strip()
    changed = True
    while changed:
        changed = False
        for prefix in _PREFIXES:
            if text.startswith(prefix):
                text = text[len(prefix):]
                changed = True
    match = _POSITION.match(text)
    if match is None:
        return FormatError(text)
    return FormatError(match.group(3), int(match.group(1)), int(match.group(2)))
