{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    cargo
    rustc
    clippy
    rustfmt
    pkg-config
    wayland-scanner
    rustPlatform.bindgenHook
    foot
    grim
    wtype
    python3
    xeyes
  ];

  buildInputs = with pkgs; [
    wlroots_0_20
    wayland
    wayland-protocols
    libxkbcommon
    libinput
    pixman
    libdrm
    libxcb
    libxcb-wm
    lua5_4
    xwayland
  ];
}
