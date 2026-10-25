//! The C library functions Prism calls that are not defined in the libc headers
//! of `wasm-libc/`, for wasm32-unknown-unknown, which has no libc.

use std::alloc::{alloc, alloc_zeroed, dealloc, realloc as grow, Layout};
use std::ffi::{c_char, c_void};

// Each block starts with its size, which `free` and `realloc` need and C does not pass.
const HEADER: usize = 16;

unsafe fn layout(size: usize) -> Layout {
    Layout::from_size_align_unchecked(size + HEADER, HEADER)
}

unsafe fn finish(base: *mut u8, size: usize) -> *mut c_void {
    if base.is_null() {
        return std::ptr::null_mut();
    }
    (base as *mut usize).write(size);
    base.add(HEADER) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    finish(alloc(layout(size)), size)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
    let Some(size) = count.checked_mul(size) else { return std::ptr::null_mut() };
    finish(alloc_zeroed(layout(size)), size)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        return malloc(size);
    }
    let base = (ptr as *mut u8).sub(HEADER);
    let old = (base as *const usize).read();
    finish(grow(base, layout(old), size + HEADER), size)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    if !ptr.is_null() {
        let base = (ptr as *mut u8).sub(HEADER);
        dealloc(base, layout((base as *const usize).read()));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn abort() -> ! {
    std::process::abort()
}

/// Prism hands it a float literal with its underscores removed, NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtod(s: *const c_char, end: *mut *mut c_char) -> f64 {
    let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
    // The longest prefix that is a number: digits, a point, an exponent.
    let mut n = 0;
    let digits = |n: &mut usize| while bytes.get(*n).is_some_and(u8::is_ascii_digit) { *n += 1 };
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        n += 1;
    }
    digits(&mut n);
    if bytes.get(n) == Some(&b'.') {
        n += 1;
        digits(&mut n);
    }
    if matches!(bytes.get(n), Some(b'e' | b'E')) {
        let mut e = n + 1;
        if matches!(bytes.get(e), Some(b'+' | b'-')) {
            e += 1;
        }
        let start = e;
        digits(&mut e);
        if e > start {
            n = e;
        }
    }
    let value = std::str::from_utf8(&bytes[..n]).ok().and_then(|t| t.parse().ok());
    if !end.is_null() {
        *end = s.add(if value.is_some() { n } else { 0 }) as *mut c_char;
    }
    value.unwrap_or(0.0)
}
