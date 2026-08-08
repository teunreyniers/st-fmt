# -*- coding: utf-8 -*-
"""Tests for the st-fmt Python bindings.

Written to the Python 2.7 unittest API so the same file runs everywhere the
package claims to work:

    python3 -m unittest discover -s bindings/python/tests
    python2.7 -m unittest discover -s bindings/python/tests
    jython -m unittest discover -s bindings/python/tests

Every test runs against each backend the interpreter can actually load, so the
subprocess path -- which only Jython takes in production -- is still covered by
a CPython run.
"""

from __future__ import absolute_import

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import st_fmt
from st_fmt import _ctypes_backend, _subprocess_backend

UNFORMATTED = "if a then x:=1; end_if;"
FORMATTED = "IF a THEN\n    x := 1;\nEND_IF\n"


def available_backends():
    """The backends this interpreter can load, as (name, instance) pairs."""
    found = []
    for module in (_ctypes_backend, _subprocess_backend):
        backend = module.load()
        if backend is not None:
            found.append((backend.name, backend))
    return found


BACKENDS = available_backends()


class BackendTestCase(unittest.TestCase):
    """Runs each check once per loadable backend."""

    def setUp(self):
        if not BACKENDS:
            self.skipTest("no native build staged; run bindings/python/build.py")

    def each(self):
        for name, backend in BACKENDS:
            st_fmt.select_backend()
            st_fmt._backend = backend
            yield name, backend

    def tearDown(self):
        st_fmt.select_backend()


class TestFormatting(BackendTestCase):
    def test_formats_source(self):
        for name, _ in self.each():
            self.assertEqual(st_fmt.format_source(UNFORMATTED), FORMATTED, name)

    def test_formatting_is_idempotent(self):
        for name, _ in self.each():
            once = st_fmt.format_source(UNFORMATTED)
            self.assertEqual(st_fmt.format_source(once), once, name)

    def test_accepts_utf8_bytes_and_returns_text(self):
        for name, _ in self.each():
            result = st_fmt.format_source(UNFORMATTED.encode("utf-8"))
            self.assertEqual(result, FORMATTED, name)
            self.assertIsInstance(result, st_fmt._TEXT_TYPES, name)

    def test_round_trips_non_ascii_comments(self):
        # A comment is the one place arbitrary text survives into the output, so
        # it is where a botched encode/decode would show up.
        source = u"// café — grüße\nx := 1;\n"
        for name, _ in self.each():
            self.assertIn(u"café", st_fmt.format_source(source), name)

    def test_empty_source_stays_empty(self):
        for name, _ in self.each():
            self.assertEqual(st_fmt.format_source(u""), u"", name)

    def test_rejects_a_non_string(self):
        for name, _ in self.each():
            self.assertRaises(TypeError, st_fmt.format_source, 42)


class TestRefusals(BackendTestCase):
    def test_raises_format_error_on_bad_source(self):
        for name, _ in self.each():
            self.assertRaises(st_fmt.FormatError, st_fmt.format_source, u"IF a THEN")

    def test_error_carries_a_position(self):
        for name, _ in self.each():
            try:
                st_fmt.format_source(u"IF a THEN")
            except st_fmt.FormatError as error:
                self.assertEqual(error.line, 1, name)
                self.assertTrue(error.column >= 1, name)
                self.assertTrue(error.message, name)
            else:
                self.fail("%s did not refuse invalid source" % name)

    def test_both_backends_agree_on_the_message(self):
        if len(BACKENDS) < 2:
            self.skipTest("only one backend available")
        messages = []
        for name, _ in self.each():
            try:
                st_fmt.format_source(u"IF a THEN")
            except st_fmt.FormatError as error:
                messages.append((error.line, error.column, error.message))
        self.assertEqual(messages[0], messages[1])


class TestIsFormatted(BackendTestCase):
    def test_true_for_formatted_source(self):
        for name, _ in self.each():
            self.assertTrue(st_fmt.is_formatted(FORMATTED), name)

    def test_false_for_unformatted_source(self):
        for name, _ in self.each():
            self.assertFalse(st_fmt.is_formatted(UNFORMATTED), name)

    def test_propagates_a_refusal(self):
        # "does not parse" must not be reported as "not formatted".
        for name, _ in self.each():
            self.assertRaises(st_fmt.FormatError, st_fmt.is_formatted, u"IF a THEN")


class TestVersions(BackendTestCase):
    def test_native_version_matches_the_package(self):
        for name, _ in self.each():
            self.assertEqual(st_fmt.native_version(), st_fmt.__version__, name)

    def test_backend_name(self):
        for name, _ in self.each():
            self.assertEqual(st_fmt.backend_name(), name)


class TestSelection(unittest.TestCase):
    def tearDown(self):
        st_fmt.select_backend()

    def test_rejects_an_unknown_kind(self):
        self.assertRaises(ValueError, st_fmt.select_backend, "jna")

    def test_explicit_path_is_used(self):
        backend = _subprocess_backend.load()
        if backend is None:
            self.skipTest("no executable staged")
        self.assertEqual(st_fmt.select_backend("subprocess", backend.path), "subprocess")
        self.assertEqual(st_fmt.format_source(UNFORMATTED), FORMATTED)


class TestErrorParsing(unittest.TestCase):
    """The message parser is shared by both backends, so it is tested alone."""

    def test_parses_the_c_abi_form(self):
        error = st_fmt._errors.format_error("3:11: syntax error: END_IF")
        self.assertEqual((error.line, error.column), (3, 11))
        self.assertEqual(error.message, "syntax error: END_IF")

    def test_parses_the_cli_form(self):
        error = st_fmt._errors.format_error("st-fmt: <stdin>:3:11: syntax error: END_IF")
        self.assertEqual((error.line, error.column), (3, 11))
        self.assertEqual(error.message, "syntax error: END_IF")

    def test_keeps_a_message_with_no_position(self):
        error = st_fmt._errors.format_error("the parser did not return a tree")
        self.assertEqual(error.line, None)
        self.assertEqual(error.message, "the parser did not return a tree")


class TestPlatform(unittest.TestCase):
    def test_offers_at_least_one_tag(self):
        from st_fmt import _platform

        tags = _platform.candidate_tags()
        self.assertTrue(tags)
        self.assertTrue(all("-" in tag for tag in tags))

    def test_tags_are_distinct(self):
        # Backends walk the list in order; a repeat means a wasted load attempt
        # and a duplicated failure in the diagnostics.
        from st_fmt import _platform

        tags = _platform.candidate_tags()
        self.assertEqual(len(tags), len(set(tags)))


if __name__ == "__main__":
    unittest.main()
