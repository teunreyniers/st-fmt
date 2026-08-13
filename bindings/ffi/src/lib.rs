//! A C ABI over [`st_fmt::format_source`].
//!
//! This exists for runtimes that cannot link a Rust or CPython extension
//! module: Python 2.7, which no modern binding generator targets, and Jython,
//! which has no CPython ABI at all. A plain `cdylib` reaches both through
//! `ctypes`, JNA, or anything else that can call C.
//!
//! The contract is deliberately small — one call, one free, no handles:
//!
//! ```c
//! char *out = NULL;
//! int rc = st_fmt_format(src, src_len, &out);
//! /* out now holds either the formatted source or the error message */
//! st_fmt_free(out);
//! ```
//!
//! `out` must be freed with [`st_fmt_free`] whatever `rc` was, because an error
//! return still allocates the message. Freeing a null pointer is a no-op, so
//! the caller can free unconditionally.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

/// The source was formatted; `*out` holds the result.
pub const ST_FMT_OK: c_int = 0;
/// The source did not parse; `*out` holds the rendered [`st_fmt::FormatError`],
/// in the same `line:column: what: snippet` form the CLI prints.
pub const ST_FMT_ERR_PARSE: c_int = 1;
/// The bytes handed in are not valid UTF-8; `*out` holds a message.
pub const ST_FMT_ERR_ENCODING: c_int = 2;
/// The formatter panicked. `*out` holds a message and the library is still
/// usable — the unwind was caught at the boundary rather than crossing it,
/// which would abort the host process.
pub const ST_FMT_ERR_PANIC: c_int = 3;
/// A required pointer argument was null. Nothing was written or allocated.
pub const ST_FMT_ERR_NULL: c_int = 4;

/// Formats `len` bytes of UTF-8 Structured Text read from `src`.
///
/// Writes a freshly allocated, NUL-terminated C string to `*out` and returns
/// one of the `ST_FMT_*` codes above. `src` is only read for the duration of
/// the call and need not be NUL-terminated.
///
/// # Safety
///
/// `src` must point to `len` readable bytes and `out` to a writable pointer.
/// `len` may be 0, in which case `src` is not dereferenced at all.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn st_fmt_format(src: *const u8, len: usize, out: *mut *mut c_char) -> c_int {
    if out.is_null() || (src.is_null() && len != 0) {
        return ST_FMT_ERR_NULL;
    }
    unsafe { *out = std::ptr::null_mut() };

    // A panic must not unwind into the caller: across an `extern "C"` boundary
    // that aborts, and taking down a Python interpreter over one malformed file
    // would be a poor trade. Catching it turns a formatter bug into an
    // exception the caller can report against the offending source.
    let result = catch_unwind(AssertUnwindSafe(|| {
        let bytes = if len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(src, len) }
        };
        let Ok(source) = std::str::from_utf8(bytes) else {
            return (ST_FMT_ERR_ENCODING, "input is not valid UTF-8".to_string());
        };
        match st_fmt::format_source(source) {
            Ok(formatted) => (ST_FMT_OK, formatted),
            Err(e) => (ST_FMT_ERR_PARSE, e.to_string()),
        }
    }));

    let (code, text) = match result {
        Ok(pair) => pair,
        Err(_) => (ST_FMT_ERR_PANIC, "the formatter panicked".to_string()),
    };

    // Formatted Structured Text cannot contain a NUL — the source it came from
    // would not have parsed — so this only trips on a hostile error message,
    // and a truncated message beats losing the code entirely.
    let owned = CString::new(text).unwrap_or_else(|e| {
        let mut bytes = e.into_vec();
        bytes.truncate(bytes.iter().position(|&b| b == 0).unwrap_or(0));
        CString::new(bytes).expect("truncated at the first NUL")
    });
    unsafe { *out = owned.into_raw() };
    code
}

/// Frees a string produced by [`st_fmt_format`]. A null pointer is ignored.
///
/// # Safety
///
/// `ptr` must have come from [`st_fmt_format`] and must not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn st_fmt_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // Reclaiming the CString runs Rust's deallocator, which is the only one
    // that may free this pointer — the caller's `free()` would be wrong.
    drop(unsafe { CString::from_raw(ptr) });
}

/// Returns the st-fmt version as a static NUL-terminated string.
///
/// The pointer is valid for the life of the process and must not be freed.
#[unsafe(no_mangle)]
pub extern "C" fn st_fmt_version() -> *const c_char {
    static VERSION: OnceLock<CString> = OnceLock::new();
    VERSION
        .get_or_init(|| CString::new(st_fmt::VERSION).expect("a version string has no NUL"))
        .as_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Calls `st_fmt_format` and returns the code alongside the string it
    /// produced, freeing it the way a caller is required to.
    fn format(source: &[u8]) -> (c_int, String) {
        let mut out: *mut c_char = std::ptr::null_mut();
        let code = unsafe { st_fmt_format(source.as_ptr(), source.len(), &mut out) };
        assert!(
            !out.is_null(),
            "every code but ERR_NULL allocates a message"
        );
        let text = unsafe { std::ffi::CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        unsafe { st_fmt_free(out) };
        (code, text)
    }

    #[test]
    fn formats_valid_source() {
        let (code, text) = format(b"if a then x:=1; end_if;");
        assert_eq!(code, ST_FMT_OK);
        assert!(text.contains("IF"), "got {text:?}");
    }

    #[test]
    fn reports_a_parse_error_as_text() {
        let (code, text) = format(b"IF a THEN");
        assert_eq!(code, ST_FMT_ERR_PARSE);
        assert!(text.starts_with('1'), "expected line:column, got {text:?}");
    }

    #[test]
    fn rejects_non_utf8() {
        let (code, _) = format(&[0xff, 0xfe]);
        assert_eq!(code, ST_FMT_ERR_ENCODING);
    }

    #[test]
    fn accepts_empty_input_without_reading_the_pointer() {
        let mut out: *mut c_char = std::ptr::null_mut();
        let code = unsafe { st_fmt_format(std::ptr::null(), 0, &mut out) };
        assert_eq!(code, ST_FMT_OK);
        unsafe { st_fmt_free(out) };
    }

    #[test]
    fn refuses_a_null_out_pointer() {
        let code = unsafe { st_fmt_format(b"".as_ptr(), 0, std::ptr::null_mut()) };
        assert_eq!(code, ST_FMT_ERR_NULL);
    }

    #[test]
    fn freeing_null_is_a_no_op() {
        unsafe { st_fmt_free(std::ptr::null_mut()) };
    }

    #[test]
    fn reports_a_version() {
        let version = unsafe { std::ffi::CStr::from_ptr(st_fmt_version()) };
        assert_eq!(version.to_str().unwrap(), st_fmt::VERSION);
    }
}
