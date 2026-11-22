//! Port of util.h plus the few libwayland `static inline` helpers bindgen
//! cannot see.

use crate::ffi::*;
use std::borrow::Cow;
use std::ffi::{c_char, CStr};

/// `wl_signal_add` is `static inline` in wayland-server-core.h.
pub unsafe fn wl_signal_add(signal: *mut wl_signal, listener: *mut wl_listener) {
    wl_list_insert((*signal).listener_list.prev, &mut (*listener).link);
}

/// `wl_container_of`.
#[macro_export]
macro_rules! container_of {
    ($ptr:expr, $T:ty, $field:ident) => {
        ($ptr as *mut u8).sub(::core::mem::offset_of!($T, $field)) as *mut $T
    };
}

/// Snapshot of the elements of a C intrusive `wl_list`.  Taking a copy makes
/// iteration safe against the list being changed by whatever we call while
/// walking it (the moral equivalent of `wl_list_for_each_safe`).
pub unsafe fn wl_list_collect<T>(head: *mut wl_list, offset: usize) -> Vec<*mut T> {
    let mut out = Vec::new();
    let mut pos = (*head).next;
    while pos != head {
        out.push((pos as *mut u8).sub(offset) as *mut T);
        pos = (*pos).next;
    }
    out
}

#[macro_export]
macro_rules! wl_list_items {
    ($head:expr, $T:ty, $field:ident) => {
        $crate::util::wl_list_collect::<$T>($head, ::core::mem::offset_of!($T, $field))
    };
}

pub unsafe fn wl_list_is_empty(list: *const wl_list) -> bool {
    wl_list_empty(list) != 0
}

/// Borrow a C string; NULL becomes "".
pub unsafe fn cstr<'a>(p: *const c_char) -> Cow<'a, str> {
    if p.is_null() {
        Cow::Borrowed("")
    } else {
        CStr::from_ptr(p).to_string_lossy()
    }
}

pub fn die(msg: &str) -> ! {
    if msg.ends_with(':') {
        eprintln!("{msg} {}", std::io::Error::last_os_error());
    } else {
        eprintln!("{msg}");
    }
    std::process::exit(1);
}

pub fn fd_set_nonblock(fd: i32) -> bool {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            eprintln!("fcntl(F_GETFL): {}", std::io::Error::last_os_error());
            return false;
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            eprintln!("fcntl(F_SETFL): {}", std::io::Error::last_os_error());
            return false;
        }
    }
    true
}

pub unsafe fn keymap_get_one_sym_by_level(
    keymap: *mut xkb_keymap,
    key: xkb_keycode_t,
    layout: xkb_layout_index_t,
    level: xkb_level_index_t,
) -> xkb_keysym_t {
    let mut syms: *const xkb_keysym_t = std::ptr::null();
    let count = xkb_keymap_key_get_syms_by_level(keymap, key, layout, level, &mut syms);
    if count > 0 {
        *syms
    } else {
        XKB_KEY_NoSymbol
    }
}

pub fn monotonic_now() -> libc::timespec {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    now
}

pub fn now_msec() -> u32 {
    let now = monotonic_now();
    (now.tv_sec as u64 * 1000 + now.tv_nsec as u64 / 1_000_000) as u32
}

/// Write everything to stdout without ever blocking or panicking: stdout is
/// non-blocking and may be a pipe nobody reads (see run()).
pub fn write_stdout(s: &str) {
    let mut buf = s.as_bytes();
    while !buf.is_empty() {
        let n = unsafe { libc::write(1, buf.as_ptr().cast(), buf.len()) };
        if n <= 0 {
            return;
        }
        buf = &buf[n as usize..];
    }
}

pub const fn wlr_box(x: i32, y: i32, width: i32, height: i32) -> wlr_box {
    wlr_box { x, y, width, height }
}
