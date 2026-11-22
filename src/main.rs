//! iwc (dwl with Rust): dwl (dwm for Wayland) rewritten in Rust.
//!
//! Built directly on wlroots 0.20 through bindgen-generated bindings (no
//! Smithay): the compositor logic is a faithful port of dwl 0.9, while the
//! heavy lifting (DRM/KMS, rendering, scene graph, protocols) stays in
//! wlroots, exactly as in the C original.

#![allow(clippy::missing_safety_doc, clippy::too_many_arguments)]

mod canvas_scene;
mod client;
mod config;
mod dwl;
mod desktop;
mod ffi;
mod ime;
mod infinity;
mod listener;
mod lua_config;
mod state;
mod util;
mod workspace;
#[cfg(feature = "xwayland")]
mod xwayland;

pub mod prelude {
    pub use crate::client::*;
    pub use crate::config::*;
    pub use crate::dwl::*;
    pub use crate::ffi::*;
    pub use crate::ime::*;
    pub use crate::listener::*;
    pub use crate::state::*;
    pub use crate::util::*;
    #[cfg(feature = "xwayland")]
    pub use crate::xwayland::*;
    pub use crate::{container_of, g, wl_list_items};
    pub use std::ffi::{c_char, c_void, CStr, CString};
    pub use std::ptr::{null, null_mut};
}

use prelude::*;

/// Raised to WLR_DEBUG by `-d`.
pub static mut LOG_LEVEL: wlr_log_importance = config::LOG_LEVEL;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage(argv0: &str) -> ! {
    die(&format!("Usage: {argv0} [-v] [-d] [-s startup command] | --control command"));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let argv0 = args.first().map(String::as_str).unwrap_or("iwc");
    if args.get(1).is_some_and(|arg| arg == "--control") {
        let command = args[2..].join(" ");
        if desktop::parse_command(&command).is_none()
            || std::env::var_os("XDG_RUNTIME_DIR").is_none()
            || std::env::var_os("WAYLAND_DISPLAY").is_none() {
            usage(argv0);
        }
        std::os::unix::net::UnixDatagram::unbound()
            .and_then(|socket| socket.send_to(command.as_bytes(), desktop::runtime_path("sock")))
            .unwrap_or_else(|error| die(&format!("iwc control: {error}")));
        return;
    }
    let mut startup_cmd: Option<String> = None;

    // getopt(argc, argv, "s:hdv"), including bundled flags like -dv and -sCMD
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            i += 1;
            break;
        }
        if !arg.starts_with('-') || arg.len() < 2 {
            break;
        }
        let flags: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < flags.len() {
            match flags[j] {
                's' => {
                    let rest: String = flags[j + 1..].iter().collect();
                    if !rest.is_empty() {
                        startup_cmd = Some(rest);
                    } else {
                        i += 1;
                        match args.get(i) {
                            Some(cmd) => startup_cmd = Some(cmd.clone()),
                            None => usage(argv0),
                        }
                    }
                    j = flags.len();
                    continue;
                }
                'd' => unsafe { LOG_LEVEL = WLR_DEBUG },
                'v' => die(&format!("iwc {VERSION}")),
                _ => usage(argv0),
            }
            j += 1;
        }
        i += 1;
    }
    if i < args.len() {
        usage(argv0);
    }

    /* Wayland requires XDG_RUNTIME_DIR for creating its communications socket */
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        die("XDG_RUNTIME_DIR must be set");
    }
    unsafe {
        setup();
        run(startup_cmd.as_deref());
        cleanup();
    }
}
