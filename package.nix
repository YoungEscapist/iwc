{ lib, rustPlatform, pkg-config, wayland-scanner, wlroots_0_20, wayland
, wayland-protocols, libxkbcommon, libinput, pixman, libdrm, libxcb, libxcb-wm
, writeText, lua5_4 }:

rustPlatform.buildRustPackage {
  pname = "iwc";
  version = "0.3.3";
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type: !(builtins.elem (baseNameOf path) [ "target" ".git" "result" ]);
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config wayland-scanner rustPlatform.bindgenHook ];
  buildInputs = [
    wlroots_0_20 wayland wayland-protocols libxkbcommon libinput pixman libdrm
    libxcb libxcb-wm lua5_4
  ];
  doCheck = true;

  postInstall = ''
    install -Dm755 iwcctl $out/bin/iwcctl
    patchShebangs $out/bin/iwcctl
    install -Dm644 config.lua $out/share/iwc/config.lua
    install -Dm644 ${writeText "iwc.desktop" ''
      [Desktop Entry]
      Name=IWC
      Comment=Infinity Wayland Compositor
      Exec=iwc
      Type=Application
      DesktopNames=IWC
    ''} $out/share/wayland-sessions/iwc.desktop
  '';

  passthru.providedSessions = [ "iwc" ];
  meta = {
    description = "Rust Wayland compositor with Lua-configured tiling and an infinite floating canvas";
    license = lib.licenses.gpl3Only;
    mainProgram = "iwc";
    platforms = lib.platforms.linux;
  };
}
