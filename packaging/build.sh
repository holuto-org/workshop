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
if [[ $os == linux ]]; then
  glibc_version=$(getconf GNU_LIBC_VERSION)
  glibc_version=${glibc_version#glibc }
  # Keep DWARF in one ELF before extracting a Sentry-compatible debug companion.
  export CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=off
fi

metadata=$(cargo metadata --locked --no-deps --format-version 1)
version=$(jq -er '.packages[] | select(.name == "workshop") | .version' <<< "$metadata")
target_dir=$(jq -er '.target_directory | gsub("\\\\"; "/")' <<< "$metadata")
stem="workshop-${os}-${arch}"
build_dir="packaging/build/${os}-${arch}"
symbols=packaging/build/symbols
# Each invocation produces exactly one package; old packages never enter a release.
rm -rf -- "$build_dir" "$symbols" release
mkdir -p "$build_dir" "$symbols" release

# Never package simulator/testing features.
cargo build --release --locked -p workshop
binary="$target_dir/release/workshop"

case $os in
  win)
    cp -- "$binary.exe" "release/$stem.exe"
    # The x64 EXE carries the unwind information Sentry needs to walk minidumps.
    cp -- "$target_dir/release/workshop.pdb" "$binary.exe" "$symbols/"
    ;;
  mac)
    app="$build_dir/dmg/Holuto Workshop.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp -R -- "$binary.dSYM" "$symbols/"
    install -m 755 "$binary" "$app/Contents/MacOS/workshop"
    strip "$app/Contents/MacOS/workshop"
    iconutil -c icns packaging/macos/icon.iconset -o "$app/Contents/Resources/icon.icns"
    info=$(< packaging/macos/Info.plist.in)
    printf '%s\n' "${info//@VERSION@/"$version"}" > "$app/Contents/Info.plist"
    # Stripping changes the Mach-O; ARM64 requires a fresh ad-hoc signature.
    codesign --force --sign - "$app"
    # The dSYM holds no unwind information; the shipped Mach-O does.
    cp -- "$app/Contents/MacOS/workshop" "$symbols/"
    # Keep normal activation/Dock behavior: this tray app also has a regular window.
    ln -s /Applications "$build_dir/dmg/Applications"
    hdiutil create -volname "Holuto Workshop" -srcfolder "$build_dir/dmg" -ov -format UDZO "release/$stem.dmg"
    ;;
  linux)
    package="$build_dir/package"
    # Icon theme spec: hicolor needs a 48x48 PNG; SVG support is optional, so 256x256 covers large sizes.
    install -d "$package/DEBIAN" "$package/usr/bin" "$package/usr/share/applications" \
      "$package/usr/share/icons/hicolor/scalable/apps" "$package/usr/share/icons/hicolor/48x48/apps" \
      "$package/usr/share/icons/hicolor/256x256/apps"
    objcopy --only-keep-debug "$binary" "$symbols/workshop.debug"
    install -m 755 "$binary" "$package/usr/bin/workshop"
    strip --strip-all "$package/usr/bin/workshop"
    objcopy --add-gnu-debuglink="$symbols/workshop.debug" "$package/usr/bin/workshop"
    # --only-keep-debug turns .eh_frame into NOBITS: upload the shipped ELF for unwinding.
    cp -- "$package/usr/bin/workshop" "$symbols/"
    install -m 644 packaging/linux/workshop.desktop "$package/usr/share/applications/workshop.desktop"
    install -m 644 crates/desktop/assets/icon.svg "$package/usr/share/icons/hicolor/scalable/apps/workshop.svg"
    install -m 644 packaging/linux/icon-48x48.png "$package/usr/share/icons/hicolor/48x48/apps/workshop.png"
    install -m 644 crates/desktop/assets/icon.png "$package/usr/share/icons/hicolor/256x256/apps/workshop.png"

    # Include dlopened display libraries, TLS roots and Warsaw timezone data too.
    depends="libc6 (>= $glibc_version), libgcc-s1, libfontconfig1, fonts-dejavu-core, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, ca-certificates, tzdata"
    control=$(< packaging/linux/control.in)
    control=${control//@VERSION@/"$version"}
    control=${control//@ARCH@/"$deb_arch"}
    printf '%s\n' "${control//@DEPENDS@/"$depends"}" > "$package/DEBIAN/control"
    chmod 755 "$package" "$package/DEBIAN"
    dpkg-deb --root-owner-group -Zxz --build "$package" "release/$stem.deb"
    ;;
esac
printf 'Built release/%s\n' "$stem.*"
