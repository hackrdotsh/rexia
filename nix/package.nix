{
  lib,
  rustPlatform,
  fetchzip,
  runCommand,
  zig_0_17,
  pkg-config,
  makeWrapper,
  desktop-file-utils,
  fontconfig,
  freetype,
  libxcb,
  libxkbcommon,
  wayland,
  vulkan-loader,
  libGL,
}:
let
  # ghostty-vt-sys builds libghostty-vt from this source archive with Zig. The
  # crate's GHOSTTY.lock pins the same URL; fetching it here keeps builds offline.
  ghosttySource = fetchzip {
    url = "https://tip.files.ghostty.org/83fb95624b0439ee660c2679b43b38c8b63769cd/libghostty-vt-source.tar.gz";
    hash = "sha256-uRtcTrlsOhUl1lBOwn+I2lc+dP9wDs0G8hxqr4T/5Vk=";
  };
  ghosttyDeps = zig_0_17.fetchDeps {
    pname = "libghostty-vt";
    version = "83fb956";
    src = ghosttySource;
    fetchAll = true;
    hash = "sha256-82Ife6Av0wfZLmovdEdJFgsG5MI4swCeOB3dbvgBDSU=";
  };
  # Zig 0.17 fetches archives, while --system expects extracted package trees.
  ghosttySystemDeps = runCommand "libghostty-vt-system-deps" { } ''
    mkdir -p $out
    for archive in ${ghosttyDeps}/*.tar.gz; do
      tar -xzf "$archive" -C $out
    done
  '';
  runtimeLibraries = [
    fontconfig
    freetype
    libxcb
    libxkbcommon
    wayland
    vulkan-loader
    libGL
  ];
in
rustPlatform.buildRustPackage {
  pname = "rexia";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../resources
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    desktop-file-utils
    zig_0_17
  ];
  buildInputs = runtimeLibraries;

  GHOSTTY_SOURCE_DIR = ghosttySource;
  GHOSTTY_ZIG_SYSTEM_DIR = ghosttySystemDeps;
  dontUseZigBuild = true;
  dontUseZigCheck = true;
  dontUseZigInstall = true;

  preConfigure = ''
    export ZIG_GLOBAL_CACHE_DIR="$TMPDIR/zig-cache"
  '';

  postInstall = ''
    desktop-file-install --dir=$out/share/applications \
      --set-key=Exec --set-value=$out/bin/rexia \
      --set-key=TryExec --set-value=$out/bin/rexia \
      resources/dev.hackr.rexia.desktop
    desktop-file-validate $out/share/applications/dev.hackr.rexia.desktop
    install -Dm644 resources/dev.hackr.rexia.svg \
      $out/share/icons/hicolor/scalable/apps/dev.hackr.rexia.svg
    install -Dm644 resources/noctalia/rexia.toml $out/share/rexia/noctalia/rexia.toml
    wrapProgram $out/bin/rexia \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibraries}
  '';

  passthru = {
    inherit ghosttySource ghosttySystemDeps runtimeLibraries;
  };

  meta = {
    description = "Native Linux client for Rex, built on GPUI and themed by Noctalia";
    mainProgram = "rexia";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
}
