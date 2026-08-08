# -*- coding: utf-8 -*-
"""The package version.

Rewritten by ``bindings/python/build.py`` from the version in Cargo.toml, so
that a built package always reports the formatter it actually contains. It is
committed in step with Cargo.toml, so an ordinary build leaves no diff here; a
change to this line means the crate version moved and was picked up.
"""

__version__ = "0.1.0"
