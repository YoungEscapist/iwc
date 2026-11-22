//! Owned `wl_listener`s.
//!
//! dwl embeds `struct wl_listener` in its objects and recovers the object with
//! `wl_container_of`; every listener then has to be unlinked by hand in the
//! destroy path.  Here a [`Listener`] is a small heap block holding the
//! `wl_listener`, the owner pointer and a plain Rust handler.  Dropping it
//! unlinks it, so dropping a `Box<Client>` disconnects all of its listeners.
//!
//! Handlers may free their own listener (and its owner) while they run: the
//! trampoline copies out what it needs before calling and touches nothing
//! afterwards, and libwayland's `wl_signal_emit_mutable` tolerates removal.

use crate::ffi::*;
use crate::util::wl_signal_add;
use std::ffi::c_void;
use std::ptr::null_mut;

type RawFn = unsafe fn(*mut c_void, *mut c_void);

#[repr(C)]
struct Raw {
    wl: wl_listener, // must stay first: the trampoline casts back to Raw
    owner: *mut c_void,
    f: RawFn,
}

unsafe extern "C" fn trampoline(l: *mut wl_listener, data: *mut c_void) {
    let raw = l as *mut Raw;
    let (owner, f) = ((*raw).owner, (*raw).f);
    f(owner, data);
}

pub struct Listener(*mut Raw);

impl Listener {
    pub const fn none() -> Self {
        Listener(null_mut())
    }

    /// Connect `f(owner, data)` to `signal`.
    pub unsafe fn new<T>(
        signal: *mut wl_signal,
        owner: *mut T,
        f: unsafe fn(*mut T, *mut c_void),
    ) -> Self {
        // SAFETY: `*mut T` and `*mut c_void` are ABI-compatible.
        let f = std::mem::transmute::<unsafe fn(*mut T, *mut c_void), RawFn>(f);
        let raw = Box::into_raw(Box::new(Raw {
            wl: wl_listener {
                link: wl_list::default(),
                notify: Some(trampoline),
            },
            owner: owner.cast(),
            f,
        }));
        wl_signal_add(signal, &mut (*raw).wl);
        Listener(raw)
    }

    pub fn is_connected(&self) -> bool {
        !self.0.is_null()
    }

    /// Disconnect (idempotent).
    pub fn remove(&mut self) {
        if !self.0.is_null() {
            unsafe {
                wl_list_remove(&mut (*self.0).wl.link);
                drop(Box::from_raw(self.0));
            }
            self.0 = null_mut();
        }
    }
}

impl Default for Listener {
    fn default() -> Self {
        Self::none()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.remove();
    }
}

#[repr(C)]
struct StaticRaw {
    wl: wl_listener,
    f: unsafe fn(*mut c_void) -> bool,
}

unsafe extern "C" fn static_trampoline(l: *mut wl_listener, data: *mut c_void) {
    let raw = l as *mut StaticRaw;
    if ((*raw).f)(data) {
        wl_list_remove(&mut (*raw).wl.link);
        drop(Box::from_raw(raw));
    }
}

/// dwl's `LISTEN_STATIC`: a listener nobody owns.  It disconnects and frees
/// itself once the handler returns `true`.
pub unsafe fn listen_static(signal: *mut wl_signal, f: unsafe fn(*mut c_void) -> bool) {
    let raw = Box::into_raw(Box::new(StaticRaw {
        wl: wl_listener {
            link: wl_list::default(),
            notify: Some(static_trampoline),
        },
        f,
    }));
    wl_signal_add(signal, &mut (*raw).wl);
}
