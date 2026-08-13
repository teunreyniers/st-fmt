# Native artifacts

`build.py` stages one directory here per platform:

```
_native/linux-x86_64/libst_fmt_c.so   # loaded by the ctypes backend
_native/linux-x86_64/st-fmt           # spawned by the subprocess backend
_native/windows-x86_64/st_fmt_c.dll
_native/windows-x86_64/st-fmt.exe
```

The tag is `<system>-<machine>` with `system` in `linux`, `macos`, `windows` and
`machine` in `x86_64`, `aarch64`, `i686`, as computed by `st_fmt._platform`.

Nothing here is checked in — the contents are build output. A package staged for
more than one platform simply holds more than one directory, and `_platform`
picks at import time; see `build.py --keep` for assembling such a package from
builds made on several machines.
