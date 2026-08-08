# -*- coding: utf-8 -*-
"""Checks on the archives ``build.py`` writes.

These validate what pip and the Ignition operator will find inside: that the
RECORD is honest, that the native artifacts carry an executable mode, and that
the package still works when it does not. They skip when nothing has been built.
"""

from __future__ import absolute_import

import base64
import hashlib
import os
import shutil
import stat
import sys
import tempfile
import unittest
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
PYTHON_DIR = os.path.dirname(HERE)
DIST = os.path.join(PYTHON_DIR, "dist")

sys.path.insert(0, PYTHON_DIR)

import st_fmt
from st_fmt import _subprocess_backend

UNFORMATTED = "if a then x:=1; end_if;"


def newest(suffix, infix=""):
    """The most recently written archive in dist/, or None."""
    if not os.path.isdir(DIST):
        return None
    candidates = [
        os.path.join(DIST, name)
        for name in os.listdir(DIST)
        if name.endswith(suffix) and infix in name
    ]
    if not candidates:
        return None
    return max(candidates, key=os.path.getmtime)


class WheelTestCase(unittest.TestCase):
    def setUp(self):
        self.path = newest(".whl")
        if self.path is None:
            self.skipTest("no wheel in dist/; run bindings/python/build.py")
        self.archive = zipfile.ZipFile(self.path)

    def tearDown(self):
        self.archive.close()

    def dist_info(self):
        for name in self.archive.namelist():
            if name.endswith(".dist-info/RECORD"):
                return name[: -len("/RECORD")]
        self.fail("no .dist-info/RECORD in the wheel")

    def test_record_hashes_match_the_contents(self):
        # pip verifies these on install and refuses the wheel if they disagree.
        record = self.archive.read("%s/RECORD" % self.dist_info()).decode("utf-8")
        checked = 0
        for line in record.splitlines():
            name, digest, size = line.rsplit(",", 2)
            if not digest:
                continue  # RECORD cannot hash itself
            data = self.archive.read(name)
            expected = base64.urlsafe_b64encode(hashlib.sha256(data).digest())
            self.assertEqual(digest, "sha256=%s" % expected.rstrip(b"=").decode("ascii"), name)
            self.assertEqual(int(size), len(data), name)
            checked += 1
        self.assertTrue(checked > 0)

    def test_record_lists_every_file(self):
        record = self.archive.read("%s/RECORD" % self.dist_info()).decode("utf-8")
        listed = set(line.rsplit(",", 2)[0] for line in record.splitlines())
        self.assertEqual(set(self.archive.namelist()) - listed, set())

    def test_carries_a_library_and_an_executable(self):
        names = self.archive.namelist()
        self.assertTrue(any("/_native/" in n and "st_fmt_c" in n for n in names), names)
        self.assertTrue(any(n.rstrip(".exe").endswith("/st-fmt") for n in names), names)

    def test_native_artifacts_are_marked_executable(self):
        # Python's zipfile ignores modes on extraction, but pip and unzip honour
        # them -- and a subprocess backend that cannot exec its binary is dead.
        for info in self.archive.infolist():
            if "/_native/" not in info.filename or info.filename.endswith(".md"):
                continue
            mode = info.external_attr >> 16
            self.assertTrue(mode & stat.S_IXUSR, "%s is mode %o" % (info.filename, mode))

    def test_declares_both_python_majors(self):
        wheel = self.archive.read("%s/WHEEL" % self.dist_info()).decode("utf-8")
        self.assertIn("Tag: py2-none-", wheel)
        self.assertIn("Tag: py3-none-", wheel)
        self.assertIn("Root-Is-Purelib: false", wheel)

    def test_metadata_admits_python_2_7(self):
        metadata = self.archive.read("%s/METADATA" % self.dist_info()).decode("utf-8")
        self.assertIn("Requires-Python: >=2.7", metadata)
        self.assertIn("Name: st-fmt", metadata)

    def test_carries_no_bytecode(self):
        for name in self.archive.namelist():
            self.assertFalse(name.endswith(".pyc"), name)
            self.assertNotIn("__pycache__", name)


class IgnitionArchiveTestCase(unittest.TestCase):
    def setUp(self):
        self.path = newest(".zip", "-ignition-")
        if self.path is None:
            self.skipTest("no Ignition archive in dist/; run bindings/python/build.py")

    def test_holds_the_package_and_install_notes(self):
        with zipfile.ZipFile(self.path) as archive:
            names = archive.namelist()
        self.assertIn("INSTALL.txt", names)
        self.assertIn("st_fmt/__init__.py", names)
        self.assertTrue(any("/_native/" in name for name in names), names)

    def test_extracts_to_a_working_package(self):
        # The Ignition install is "unzip this onto the path", so the extracted
        # tree has to import and format as it stands.
        root = tempfile.mkdtemp()
        try:
            with zipfile.ZipFile(self.path) as archive:
                archive.extractall(root)
            script = (
                "import sys; sys.path.insert(0, %r)\n"
                "import st_fmt\n"
                "sys.stdout.write(st_fmt.format_source(%r))\n" % (root, UNFORMATTED)
            )
            import subprocess

            out = subprocess.check_output([sys.executable, "-c", script])
            self.assertIn(b"END_IF", out)
        finally:
            shutil.rmtree(root)


class ExecutableBitTestCase(unittest.TestCase):
    """The failure mode that bites an Ignition deployment.

    Python's zipfile drops permissions on extraction, so a package unpacked by a
    script -- or by a tool that ignores the mode -- leaves ``st-fmt`` at 644 and
    unrunnable. The backend restores it on first use; this proves it does.
    """

    def setUp(self):
        if os.name == "nt":
            self.skipTest("permission bits are not how Windows decides this")
        backend = _subprocess_backend.load()
        if backend is None:
            self.skipTest("no executable staged; run bindings/python/build.py")
        self.directory = tempfile.mkdtemp()
        self.copy = os.path.join(self.directory, os.path.basename(backend.path))
        shutil.copyfile(backend.path, self.copy)
        os.chmod(self.copy, 0o644)

    def tearDown(self):
        shutil.rmtree(self.directory)

    def test_restores_the_executable_bit(self):
        self.assertFalse(os.access(self.copy, os.X_OK))
        backend = _subprocess_backend.SubprocessBackend(self.copy)
        self.assertTrue(os.access(self.copy, os.X_OK))
        self.assertIn("END_IF", backend.format(UNFORMATTED.encode("utf-8")).decode("utf-8"))

    def test_reports_clearly_when_it_cannot(self):
        # A read-only pylib is the real cause and cannot be staged portably, so
        # this drives the same path through a missing file: what matters is that
        # the constructor raises BackendError naming the fix, rather than
        # letting a bare OSError out of an import-time backend probe.
        missing = os.path.join(self.directory, "not-here")
        try:
            _subprocess_backend.SubprocessBackend(missing)
        except st_fmt.BackendError as error:
            self.assertIn("chmod", str(error))
        else:
            self.fail("expected a BackendError")


if __name__ == "__main__":
    unittest.main()
