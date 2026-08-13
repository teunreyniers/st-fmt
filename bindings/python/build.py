#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Builds the st-fmt Python package: a wheel, and an archive for Jython.

Run it with any Python 3; it uses nothing but the standard library and cargo.

    python3 bindings/python/build.py

That produces, in ``bindings/python/dist/``:

    st_fmt-<version>-py2.py3-none-<platform>.whl   pip install, CPython 2.7+
    st_fmt-<version>-jython-<tag>.zip              unzip onto the Jython path

The wheel is written by hand rather than through setuptools. There is no
extension module to compile -- the package is pure Python around a shared
library it loads at runtime -- so the only thing a build backend would
contribute is a fight over the ``py2.py3-none-<platform>`` tag, and the
``wheel`` project is not installable on a modern interpreter's system Python
here anyway. A wheel is a zip with three metadata files; they are written below.

To ship one package that covers several platforms, build on each machine and
copy the resulting ``st_fmt/_native/<tag>/`` directories together, then run
``--skip-build --keep`` to package the lot.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import os
import re
import shutil
import subprocess
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
PACKAGE = os.path.join(HERE, "st_fmt")
NATIVE = os.path.join(PACKAGE, "_native")

# What cargo produces, and what it is called once staged. The names are kept
# identical -- st_fmt._platform looks for exactly these.
LIBRARY_NAMES = {
    "linux": "libst_fmt_c.so",
    "macos": "libst_fmt_c.dylib",
    "windows": "st_fmt_c.dll",
}
EXECUTABLE_NAMES = {
    "linux": "st-fmt",
    "macos": "st-fmt",
    "windows": "st-fmt.exe",
}

# Enough of the Rust target triples to infer a tag when cross-compiling.
TRIPLE_TAGS = {
    "x86_64-unknown-linux-gnu": "linux-x86_64",
    "x86_64-unknown-linux-musl": "linux-x86_64",
    "aarch64-unknown-linux-gnu": "linux-aarch64",
    "i686-unknown-linux-gnu": "linux-i686",
    "x86_64-pc-windows-msvc": "windows-x86_64",
    "x86_64-pc-windows-gnu": "windows-x86_64",
    "i686-pc-windows-msvc": "windows-i686",
    "i686-pc-windows-gnu": "windows-i686",
    "aarch64-apple-darwin": "macos-aarch64",
    "x86_64-apple-darwin": "macos-x86_64",
}

# Building on the host links against the host's glibc, and a binary will not run
# on anything older -- a Jython host on RHEL 8 rejects an Ubuntu 24.04
# build outright. These two build inside a container instead:
#
#   portable  an old-glibc Debian. Produces both artifacts; the floor lands
#             around glibc 2.30, which covers Debian 10+, Ubuntu 20.04+, RHEL 9.
#   static    Alpine. Produces a static-PIE executable with no libc dependency
#             at all, so it runs on any Linux however old. Executable only:
#             musl links the C runtime statically and cannot emit a cdylib,
#             which is why this cannot serve the ctypes backend.
#
# Use both to cover everything -- `portable` first, then `static` over the top
# to replace the executable. See the README.
CONTAINER_ALIASES = {
    "portable": ("docker.io/library/rust:1-bullseye", ""),
    "static": ("docker.io/library/rust:1-alpine", "apk add --no-cache musl-dev gcc"),
}

# pip refuses a wheel whose platform tag it does not recognise, and the tag has
# to describe the machine rather than the interpreter. `linux_x86_64` is not a
# tag PyPI accepts, but it installs locally, which is all this build is for.
WHEEL_PLATFORMS = {
    "linux-x86_64": "linux_x86_64",
    "linux-aarch64": "linux_aarch64",
    "linux-i686": "linux_i686",
    "macos-x86_64": "macosx_10_12_x86_64",
    "macos-aarch64": "macosx_11_0_arm64",
    "windows-x86_64": "win_amd64",
    "windows-i686": "win32",
}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--profile", choices=("release", "debug"), default="release",
        help="cargo profile to build (default: release)",
    )
    parser.add_argument(
        "--target", metavar="TRIPLE",
        help="cross-compile for a Rust target triple instead of the host",
    )
    parser.add_argument(
        "--tag", metavar="TAG",
        help="platform tag to stage under, e.g. windows-x86_64 "
             "(default: inferred from --target, or the host)",
    )
    parser.add_argument(
        "--container", metavar="IMAGE",
        help="build inside a container so the artifacts do not inherit this "
             "machine's glibc: 'portable', 'static', or an image name",
    )
    parser.add_argument(
        "--container-setup", metavar="CMD", default=None,
        help="shell command to run in the container before cargo, for an image "
             "that lacks a C toolchain (the aliases set their own)",
    )
    parser.add_argument(
        "--only", choices=("both", "library", "executable"), default="both",
        help="stage only one artifact, leaving any other in place "
             "(default: both)",
    )
    parser.add_argument(
        "--keep", action="store_true",
        help="keep native builds already staged for other platforms, so that "
             "one package can carry several",
    )
    parser.add_argument(
        "--skip-build", action="store_true",
        help="do not run cargo; package whatever is already staged",
    )
    parser.add_argument(
        "--out", metavar="DIR", default=os.path.join(HERE, "dist"),
        help="where to write the wheel and the archive (default: bindings/python/dist)",
    )
    args = parser.parse_args(argv)

    version = cargo_version()
    tag = args.tag or (TRIPLE_TAGS.get(args.target) if args.target else host_tag())
    if tag is None:
        parser.error("cannot infer a tag for --target %s; pass --tag" % args.target)

    image, setup = resolve_container(args, parser)
    if not args.skip_build:
        stage(tag, args.profile, args.target, image, setup, args.only)
    write_version(version)

    if not args.keep:
        drop_other_tags(tag)

    staged = sorted(
        name for name in os.listdir(NATIVE)
        if os.path.isdir(os.path.join(NATIVE, name))
    )
    if not staged:
        parser.error("nothing staged in %s; run without --skip-build" % NATIVE)

    ensure_dir(args.out)
    wheel = build_wheel(version, staged, args.out)
    archive = build_jython_archive(version, staged, args.out)

    print("")
    print("st-fmt %s, platforms: %s" % (version, ", ".join(staged)))
    print("  wheel:  %s" % os.path.relpath(wheel, REPO))
    print("  jython: %s" % os.path.relpath(archive, REPO))
    return 0


# --- gathering -------------------------------------------------------------


def cargo_version():
    """Reads the version out of the root Cargo.toml's [package] section."""
    with open(os.path.join(REPO, "Cargo.toml"), "r", encoding="utf-8") as handle:
        text = handle.read()
    package = re.search(r"^\[package\]\s*$(.*?)(?=^\[|\Z)", text, re.M | re.S)
    if package is None:
        raise SystemExit("no [package] section in Cargo.toml")
    match = re.search(r'^version\s*=\s*"([^"]+)"', package.group(1), re.M)
    if match is None:
        raise SystemExit("no version in Cargo.toml's [package] section")
    return match.group(1)


def host_tag():
    """Asks the package itself what platform it thinks it is on.

    Importing st_fmt here keeps one implementation of the tag rules; the build
    and the runtime lookup cannot drift apart.
    """
    sys.path.insert(0, HERE)
    from st_fmt import _platform

    return _platform.candidate_tags()[0]


def resolve_container(args, parser):
    """Turns --container into an (image, setup command) pair."""
    if args.container is None:
        if args.container_setup:
            parser.error("--container-setup means nothing without --container")
        return None, None
    image, setup = CONTAINER_ALIASES.get(args.container, (args.container, ""))
    if args.container_setup is not None:
        setup = args.container_setup
    if args.container == "static" and args.only != "executable":
        parser.error(
            "the 'static' container builds a musl target, which cannot produce "
            "a cdylib for the ctypes backend; pass --only executable"
        )
    return image, setup


def stage(tag, profile, target, image, setup, only):
    """Builds the artifacts with cargo and copies them under the tag."""
    system = tag.split("-", 1)[0]
    if system not in LIBRARY_NAMES:
        raise SystemExit("unknown system in tag %r" % tag)

    wanted = []
    if only in ("both", "library"):
        wanted.append((["-p", "st-fmt-ffi"], LIBRARY_NAMES[system]))
    if only in ("both", "executable"):
        wanted.append((["-p", "st-fmt", "--bin", "st-fmt"], EXECUTABLE_NAMES[system]))

    # The debug profile is cargo's default, so it is selected by saying nothing.
    common = ["cargo", "build"] + (["--release"] if profile == "release" else [])
    if target:
        common += ["--target", target]

    # A container build gets its own target directory. Sharing one with the host
    # would make every switch a full rebuild, and worse, leave artifacts from
    # two different libcs sitting under the same name.
    target_root = "target" if image is None else "target/container-%s" % slug(image)
    if image is None:
        for arguments, _ in wanted:
            run(common + arguments)
    else:
        run_in_container(image, setup, [common + a for a, _ in wanted], target_root)

    out_dir = os.path.join(REPO, target_root.replace("/", os.sep), target or "", profile)
    destination = os.path.join(NATIVE, tag)
    ensure_dir(destination)
    for _, name in wanted:
        source = os.path.join(out_dir, name)
        if not os.path.isfile(source):
            raise SystemExit("cargo did not produce %s" % source)
        shutil.copy2(source, os.path.join(destination, name))
        print("staged %s" % os.path.relpath(os.path.join(destination, name), REPO))


def slug(image):
    """A filesystem-safe fragment of an image name, for the target directory."""
    return re.sub(r"[^A-Za-z0-9._-]+", "-", image).strip("-")


def run_in_container(image, setup, commands, target_root):
    runtime = shutil.which("podman") or shutil.which("docker")
    if runtime is None:
        raise SystemExit("--container needs podman or docker on PATH")
    steps = ([setup] if setup else []) + [" ".join(command) for command in commands]
    run([
        runtime, "run", "--rm",
        "--volume", "%s:/src" % REPO,
        "--workdir", "/src",
        # Cargo's home goes under the target directory too, so the container
        # never writes to the invoking user's ~/.cargo.
        "--env", "CARGO_TARGET_DIR=/src/%s" % target_root,
        "--env", "CARGO_HOME=/src/%s/.cargo" % target_root,
        image, "sh", "-c", " && ".join(steps),
    ])


def run(command):
    print("$ %s" % " ".join(command))
    result = subprocess.call(command, cwd=REPO)
    if result != 0:
        raise SystemExit("command failed with %d" % result)


def write_version(version):
    """Pins the package version to the formatter's, in _version.py."""
    path = os.path.join(PACKAGE, "_version.py")
    with open(path, "r", encoding="utf-8") as handle:
        text = handle.read()
    updated = re.sub(r'^__version__ = ".*"$', '__version__ = "%s"' % version, text, flags=re.M)
    if updated != text:
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(updated)


def drop_other_tags(tag):
    """Removes staged builds for platforms other than the one just built.

    Without this a package quietly accumulates every platform ever built in the
    tree, and you ship a 20 MB wheel holding four stale copies of the formatter.
    ``--keep`` is the way to say you meant it.
    """
    if not os.path.isdir(NATIVE):
        return
    for name in os.listdir(NATIVE):
        path = os.path.join(NATIVE, name)
        if os.path.isdir(path) and name != tag:
            shutil.rmtree(path)
            print("dropped stale %s (pass --keep to retain)" % name)


def ensure_dir(path):
    if not os.path.isdir(path):
        os.makedirs(path)


def package_files(tags):
    """Yields (archive path, path on disk) for everything the package needs."""
    for root, directories, names in os.walk(PACKAGE):
        directories[:] = [d for d in directories if d != "__pycache__"]
        relative_root = os.path.relpath(root, HERE)
        # Only the requested platforms travel, whatever else is lying around.
        parts = relative_root.split(os.sep)
        if len(parts) >= 3 and parts[1] == "_native" and parts[2] not in tags:
            directories[:] = []
            continue
        for name in sorted(names):
            if name.endswith(".pyc"):
                continue
            disk = os.path.join(root, name)
            yield os.path.relpath(disk, HERE).replace(os.sep, "/"), disk


def is_native_artifact(archive_path):
    """True for the shared library and the executable, which need mode 755."""
    return "/_native/" in archive_path and not archive_path.endswith(".md")


# --- writing the archives --------------------------------------------------

# A fixed timestamp, so two builds of the same input produce identical archives.
EPOCH = (1980, 1, 1, 0, 0, 0)


def add(archive, name, data, executable=False):
    info = zipfile.ZipInfo(name, date_time=EPOCH)
    info.compress_type = zipfile.ZIP_DEFLATED
    # The high half of external_attr is the Unix mode. pip and unzip both honour
    # it, which is what keeps `st-fmt` runnable after installation.
    info.external_attr = (0o755 if executable else 0o644) << 16
    archive.writestr(info, data)


def record_line(name, data):
    digest = hashlib.sha256(data).digest()
    encoded = base64.urlsafe_b64encode(digest).rstrip(b"=").decode("ascii")
    return "%s,sha256=%s,%d" % (name, encoded, len(data))


def build_wheel(version, tags, out_dir):
    platform_tags = []
    for tag in tags:
        wheel_platform = WHEEL_PLATFORMS.get(tag)
        if wheel_platform is None:
            raise SystemExit("no wheel platform tag known for %r" % tag)
        if wheel_platform not in platform_tags:
            platform_tags.append(wheel_platform)

    # A wheel filename carries one platform. Several staged platforms only make
    # sense in the Jython archive, which is unzipped by hand; for the wheel,
    # `any` is the honest tag -- it says "this file is not specific to one
    # platform", which is true once it carries them all.
    platform = platform_tags[0] if len(platform_tags) == 1 else "any"
    distribution = "st_fmt-%s" % version
    filename = "%s-py2.py3-none-%s.whl" % (distribution, platform)
    path = os.path.join(out_dir, filename)
    dist_info = "%s.dist-info" % distribution

    records = []
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, disk in package_files(tags):
            with open(disk, "rb") as handle:
                data = handle.read()
            add(archive, name, data, executable=is_native_artifact(name))
            records.append(record_line(name, data))

        metadata = wheel_metadata(version).encode("utf-8")
        wheel_file = wheel_wheelfile(platform).encode("utf-8")
        license_text = read_license().encode("utf-8")
        for name, data in (
            ("%s/METADATA" % dist_info, metadata),
            ("%s/WHEEL" % dist_info, wheel_file),
            ("%s/licenses/LICENSE" % dist_info, license_text),
        ):
            add(archive, name, data)
            records.append(record_line(name, data))

        # RECORD lists itself with no hash -- it cannot hash its own contents.
        records.append("%s/RECORD,," % dist_info)
        add(archive, "%s/RECORD" % dist_info, ("\n".join(records) + "\n").encode("utf-8"))
    return path


def wheel_metadata(version):
    return "\n".join([
        "Metadata-Version: 2.1",
        "Name: st-fmt",
        "Version: %s" % version,
        "Summary: An opinionated formatter for IEC 61131-3 Structured Text",
        "License: MIT",
        "Classifier: Programming Language :: Python :: 2.7",
        "Classifier: Programming Language :: Python :: 3",
        "Classifier: Programming Language :: Other",
        "Classifier: Topic :: Software Development :: Quality Assurance",
        "Requires-Python: >=2.7, !=3.0.*, !=3.1.*, !=3.2.*, !=3.3.*",
        "Description-Content-Type: text/markdown",
        "",
        read_readme(),
    ])


def wheel_wheelfile(platform):
    return "\n".join([
        "Wheel-Version: 1.0",
        "Generator: st-fmt build.py",
        # False: the package carries native artifacts and belongs in platlib.
        "Root-Is-Purelib: false",
        "Tag: py2-none-%s" % platform,
        "Tag: py3-none-%s" % platform,
        "",
    ])


def build_jython_archive(version, tags, out_dir):
    """Writes the drop-in archive for a Jython host's third-party Python path.

    A Jython host generally has no pip: a library is a directory on the Python
    path. So this is just the package tree, unzipped where the host imports
    from; the README covers placement.
    """
    suffix = tags[0] if len(tags) == 1 else "multi"
    path = os.path.join(out_dir, "st_fmt-%s-jython-%s.zip" % (version, suffix))
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, disk in package_files(tags):
            with open(disk, "rb") as handle:
                data = handle.read()
            add(archive, name, data, executable=is_native_artifact(name))
        add(archive, "LICENSE", read_license().encode("utf-8"))
    return path


def read_readme():
    path = os.path.join(HERE, "README.md")
    if not os.path.isfile(path):
        return "See https://github.com/teunreyniers/st-fmt\n"
    with open(path, "r", encoding="utf-8") as handle:
        return handle.read()


def read_license():
    with open(os.path.join(REPO, "LICENSE"), "r", encoding="utf-8") as handle:
        return handle.read()


if __name__ == "__main__":
    sys.exit(main())
