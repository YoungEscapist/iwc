# IWC tests

Run inside `nix-shell` from the project root:

```sh
cargo test --offline
cargo test --offline infinite_canvas_headless -- --ignored --nocapture
cargo test --offline touch_and_gesture_headless -- --ignored --nocapture
```

Run the two headless tests in separate processes: wlroots/global compositor state
is process-local. They create private runtime directories and never connect to
the user's active Wayland display. The canvas test uses foot, wtype and Xeyes
to exercise real XDG/Xwayland clients and keyboard bindings, not just geometry
helpers. It also checks two outputs, per-workspace mode/camera isolation, niri
scrolling, master tiling, floating geometry restoration and fullscreen toggles.
An unchanged, partially clipped projection must not keep damaging the outputs.
The headless tests load the bundled Lua config, not the user's config. Unit tests
cover real Lua evaluation, binding overrides, invalid config and projection clipping.
Master/stack is the default; niri is tested only after explicitly enabling it.
Regressions cover stale scheduled XDG sizes, hover after rapid mode toggles,
instant scene/camera updates, pointer tracking/cancellation at 50% and 200% zoom
for all four resize corners, and mouse resize tracking at minimum size.
New clients are checked against the panned camera at 50%, 100% and 200% zoom.
Pan tests check pointer focus protection, swipe cancellation, finger-axis stop
and releasing Alt without axis_stop; a screencopy includes the grabbing cursor.
Test implementations live in `src/infinity.rs`, `src/desktop.rs`, `src/lua_config.rs`
and `src/canvas_scene.rs`.
