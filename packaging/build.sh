#!/usr/bin/env bash
# Windows requires Git Bash.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=mac ;;
  MINGW* | MSYS* | CYGWIN*) os=win ;;
  *) echo "Unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | AMD64) arch=x64; deb_arch=amd64 ;;
  arm64 | aarch64) arch=arm64; deb_arch=arm64 ;;
  *) echo "Unsupported CPU: $(uname -m)" >&2; exit 1 ;;
esac
if [[ $os == win && $arch != x64 ]]; then
  echo "Only Windows x64 is distributed." >&2
  exit 1
fi

metadata=$(cargo metadata --locked --no-deps --format-version 1)
version=$(jq -er '.packages[] | select(.name == "holuto-fiscal") | .version' <<< "$metadata")
target_dir=$(jq -er '.target_directory | gsub("\\\\"; "/")' <<< "$metadata")
stem="holuto-fiscal-${os}-${arch}"
build_dir="packaging/build/${os}-${arch}"
rm -rf -- "$build_dir"
mkdir -p "$build_dir" release

# Never package the simulator.
cargo build --release --locked -p holuto-fiscal
binary="$target_dir/release/holuto-fiscal"

case $os in
  win)
    cp -- "$binary.exe" "release/$stem.exe"
    ;;
  mac)
    app="$build_dir/Holuto Fiscal.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    install -m 755 "$binary" "$app/Contents/MacOS/holuto-fiscal"
    # ImageMagick cannot write ICNS; use the committed PNG iconset.
    iconutil -c icns packaging/macos/icon.iconset -o "$app/Contents/Resources/icon.icns"
    VERSION="$version" python3 - "$app/Contents/Info.plist" <<'PY'
import os
import plistlib
import sys

with open("packaging/macos/Info.plist", "rb") as source:
    info = plistlib.load(source)
info["CFBundleShortVersionString"] = os.environ["VERSION"]
info["CFBundleVersion"] = os.environ["VERSION"]
with open(sys.argv[1], "wb") as destination:
    plistlib.dump(info, destination)
PY
    hdiutil create -volname "Holuto Fiscal" -srcfolder "$app" -ov -format UDZO "release/$stem.dmg"
    ;;
  linux)
    package="$build_dir/package"
    install -d "$package/DEBIAN" "$package/usr/bin" "$package/usr/share/applications" \
      "$package/usr/share/icons/hicolor/scalable/apps" "$package/usr/share/icons/hicolor/256x256/apps"
    install -m 755 "$binary" "$package/usr/bin/holuto-fiscal"
    install -m 644 packaging/linux/holuto-fiscal.desktop "$package/usr/share/applications/holuto-fiscal.desktop"
    install -m 644 crates/desktop/assets/icon.svg "$package/usr/share/icons/hicolor/scalable/apps/holuto-fiscal.svg"
    install -m 644 crates/desktop/assets/icon.png "$package/usr/share/icons/hicolor/256x256/apps/holuto-fiscal.png"

    # Most display libraries are dlopened; ldd/dpkg-shlibdeps miss them.
    # libc/libgcc: Rust runtime, using the host's glibc baseline.
    # fontconfig: fontique; fonts-dejavu-core: Polish glyphs.
    # X11/X11-xcb/Xcursor/Xi: winit; xcb: softbuffer; xkbcommon[-x11]: keyboard maps.
    # wayland-client: winit/softbuffer transport; cursor parsing is pure Rust.
    # ca-certificates: HTTPS roots; pure-Rust zbus/tray needs no libdbus/GTK/GL.
    # tzdata: jiff reads /usr/share/zoneinfo for the Europe/Warsaw printer clock.
    glibc_version=$(getconf GNU_LIBC_VERSION)
    glibc_version=${glibc_version#glibc }
    depends="libc6 (>= $glibc_version), libgcc-s1, libfontconfig1, fonts-dejavu-core, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, ca-certificates, tzdata"
    VERSION="$version" ARCH="$deb_arch" DEPENDS="$depends" python3 - "$package/DEBIAN/control" <<'PY'
import os
import pathlib
import sys

control = pathlib.Path("packaging/linux/control.in").read_text()
for key in ("VERSION", "ARCH", "DEPENDS"):
    control = control.replace(f"@{key}@", os.environ[key])
pathlib.Path(sys.argv[1]).write_text(control)
PY
    chmod 755 "$package" "$package/DEBIAN"
    if ! command -v dpkg-deb >/dev/null; then
      echo "dpkg-deb is required to package the staged tree at $package" >&2
      exit 1
    fi
    dpkg-deb --root-owner-group -Zxz --build "$package" "release/$stem.deb"
    ;;
esac
printf 'Built release/%s\n' "$stem.*"
