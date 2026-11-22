//! Generates FFI bindings to wlroots 0.20 (and the libraries dwl talks to
//! directly: libwayland-server, xkbcommon, libinput) with bindgen, and the two
//! wlr-protocols enum headers wlroots expects the compositor to provide.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn scanner(out: &PathBuf, kind: &str, xml: &str, header: &str) {
    let scanner = pkg_config::get_variable("wayland-scanner", "wayland_scanner")
        .unwrap_or_else(|_| "wayland-scanner".into());
    let status = Command::new(&scanner)
        .arg(kind)
        .arg(xml)
        .arg(out.join(header))
        .status()
        .unwrap_or_else(|e| panic!("running {scanner}: {e}"));
    assert!(status.success(), "{scanner} {kind} {xml} failed");
    println!("cargo:rerun-if-changed={xml}");
}

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let xwayland = env::var_os("CARGO_FEATURE_XWAYLAND").is_some();

    // wlroots' public headers #include these by quoted name.
    scanner(
        &out,
        "server-header",
        "protocols/wlr-layer-shell-unstable-v1.xml",
        "wlr-layer-shell-unstable-v1-protocol.h",
    );
    scanner(
        &out,
        "server-header",
        "protocols/wlr-output-power-management-unstable-v1.xml",
        "wlr-output-power-management-unstable-v1-protocol.h",
    );

    let mut pkgs = vec![
        "wlroots-0.20",
        "wayland-server",
        "xkbcommon",
        "libinput",
        "pixman-1",
        "libdrm",
    ];
    if xwayland {
        pkgs.extend(["xcb", "xcb-icccm"]);
    }

    let mut clang_args = vec![format!("-I{}", out.display())];
    for name in &pkgs {
        let lib = pkg_config::Config::new()
            // Only wlroots, libwayland, xkbcommon and libinput are called
            // directly; the rest are needed for struct definitions alone.
            .cargo_metadata(matches!(
                *name,
                "wlroots-0.20" | "wayland-server" | "xkbcommon" | "libinput"
            ))
            .probe(name)
            .unwrap_or_else(|e| panic!("pkg-config {name}: {e}"));
        for inc in lib.include_paths {
            clang_args.push(format!("-I{}", inc.display()));
        }
    }
    if let Ok(dir) = pkg_config::get_variable("wayland-protocols", "pkgdatadir") {
        // .../share/wayland-protocols -> .../include (enum headers live there)
        let inc = PathBuf::from(dir).join("../../include");
        clang_args.push(format!("-I{}", inc.display()));
    }
    if xwayland {
        clang_args.push("-DXWAYLAND".into());
    }

    let allow = "(wlr|wl|xkb|libinput|xdg|zwlr|ext|wp|xcb)_.*";
    let allow_vars =
        "(WLR|WL|XKB|LIBINPUT|XDG|ZWLR|EXT|WP|XCB|BTN|KEY)_.*";

    bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&clang_args)
        .allowlist_function(allow)
        .allowlist_type(allow)
        .allowlist_var(allow_vars)
        .allowlist_var(allow)
        .default_enum_style(bindgen::EnumVariation::Consts)
        .prepend_enum_name(false)
        .derive_default(true)
        .layout_tests(false)
        .generate_comments(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("bindgen failed on wrapper.h")
        .write_to_file(out.join("bindings.rs"))
        .expect("writing bindings.rs");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=build.rs");
}
