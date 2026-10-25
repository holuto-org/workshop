#!/usr/bin/env bash
# Download and verify this script before running it; never pipe a URL into Bash.
set -euo pipefail

repo=holuto-org/workshop
release=
dry_run=0
work=
staging=
mounted=0
backup=
target=
launcher=

fail() { printf 'Error: %s\n' "$*" >&2; exit 1; }
usage() { printf 'Usage: bash install.sh [--release vX.Y.Z] [--dry-run]\n'; }
valid_version() { [[ $1 =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]]; }
valid_release() { [[ $1 == v* ]] && valid_version "${1#v}"; }
# Numeric comparison avoids sort -V (unavailable in the system Bash/macOS tools).
version_gt() {
  local a_major a_minor a_patch b_major b_minor b_patch
  IFS=. read -r a_major a_minor a_patch <<< "$1"
  IFS=. read -r b_major b_minor b_patch <<< "$2"
  ((a_major > b_major || (a_major == b_major && a_minor > b_minor) ||
    (a_major == b_major && a_minor == b_minor && a_patch > b_patch)))
}
require() { command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"; }
check_not_running() {
  local status=0
  pgrep -x workshop >/dev/null 2>&1 || status=$?
  case $status in
    0) fail 'Holuto Workshop is running. Quit it normally (including the tray), then run this installer again.' ;;
    1) ;;
    *) fail 'Could not determine whether Holuto Workshop is running; nothing will be replaced.' ;;
  esac
}
check_launcher() {
  if [[ -L $launcher ]]; then
    [[ $(readlink "$launcher") == "$target/Contents/MacOS/workshop" ]] ||
      fail "The launcher $launcher points to an unrelated target; refusing to replace it."
  elif [[ -e $launcher ]]; then
    fail "The launcher $launcher already exists and is not this installer's symbolic link."
  fi
}
cleanup() {
  local status=$?
  trap - EXIT
  if ((mounted)); then
    if ! hdiutil detach "$work/mount" >/dev/null; then
      printf 'Could not detach %s; leaving its temporary directory intact. Detach it manually.\n' "$work/mount" >&2
      work=
      status=1
    fi
  fi
  if [[ -n $backup && -e $backup ]]; then
    if [[ ! -e $target && ! -L $target ]]; then
      if ! mv -- "$backup" "$target"; then
        printf 'Restore the previous app manually from %s; it has been preserved.\n' "$backup" >&2
        staging=
        status=1
      fi
    else
      printf 'Previous app preserved at %s; installation did not finish safely.\n' "$backup" >&2
      staging=
      status=1
    fi
  fi
  [[ -z $staging ]] || rm -rf -- "$staging"
  [[ -z $work ]] || rm -rf -- "$work"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

while (($#)); do
  case $1 in
    --release)
      [[ -z $release ]] || fail '--release may only be supplied once.'
      (($# >= 2)) || fail '--release requires vX.Y.Z.'
      release=$2
      valid_release "$release" || fail 'Release must be a stable tag vX.Y.Z (without leading zeroes).'
      shift 2
      ;;
    --dry-run) dry_run=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; fail "Unknown argument: $1" ;;
  esac
done

os=$(uname -s)
arch=$(uname -m)
case $os in
  Linux)
    case $arch in
      x86_64) asset=workshop-linux-x64.deb; deb_arch=amd64 ;;
      aarch64) asset=workshop-linux-arm64.deb; deb_arch=arm64 ;;
      *) fail "Unsupported Linux architecture: $arch" ;;
    esac
    [[ -r /etc/os-release ]] || fail 'Unsupported Linux distribution: /etc/os-release is unavailable.'
    # This is the native, administrator-owned OS identity, never a caller override.
    ID='' VERSION_ID=''
    # shellcheck source=/dev/null
    . /etc/os-release
    case $ID in
      ubuntu)
        [[ $VERSION_ID =~ ^([0-9]{1,4})\.([0-9]{1,2})$ ]] || fail 'Unsupported Linux distribution version.'
        ((10#${BASH_REMATCH[1]} > 24 || (10#${BASH_REMATCH[1]} == 24 && 10#${BASH_REMATCH[2]} >= 4))) ||
          fail 'Unsupported Linux distribution: Ubuntu 24.04 or newer is required.'
        ;;
      debian)
        [[ $VERSION_ID =~ ^([0-9]{1,4})(\.[0-9]{1,2})?$ ]] || fail 'Unsupported Linux distribution version.'
        ((10#${BASH_REMATCH[1]} >= 13)) || fail 'Unsupported Linux distribution: Debian 13 or newer is required.'
        ;;
      *) fail "Unsupported Linux distribution: $ID. Only Ubuntu 24.04+ and Debian 13+ are supported." ;;
    esac
    target='/usr/bin/workshop (managed by apt)'
    ;;
  Darwin)
    case $arch in
      x86_64|arm64) ;;
      *) fail "Unsupported macOS architecture: $arch" ;;
    esac
    # uname reports x86_64 under Rosetta; the hardware capability selects native ARM.
    native_arm=$(sysctl -n hw.optional.arm64 2>/dev/null) || native_arm=0
    [[ $native_arm == 0 || $native_arm == 1 ]] || fail 'Cannot determine native macOS architecture.'
    if [[ $arch == arm64 || $native_arm == 1 ]]; then
      asset=workshop-mac-arm64.dmg
    else
      asset=workshop-mac-x64.dmg
    fi
    mac_version=$(sw_vers -productVersion)
    [[ $mac_version =~ ^([0-9]{1,4})\.[0-9]{1,4}(\.[0-9]{1,4})?$ ]] || fail 'Cannot validate macOS version.'
    ((10#${BASH_REMATCH[1]} >= 11)) || fail 'macOS 11 or newer is required.'
    [[ ${HOME:-} == /* && $HOME != / ]] || fail 'An absolute, non-root HOME directory is required.'
    target="$HOME/Applications/Holuto Workshop.app"
    launcher="$HOME/.local/bin/workshop"
    ;;
  *) fail "Unsupported operating system: $os" ;;
esac

if [[ -z $release ]]; then
  require curl
  printf 'Resolving the latest release once with an HTTPS HEAD request (no artifact download).\n'
  latest=$(curl --disable --fail --silent --show-error --location --head --proto '=https' --proto-redir '=https' \
    --tlsv1.2 --connect-timeout 30 --max-time 60 --output /dev/null --write-out '%{url_effective}' \
    "https://github.com/$repo/releases/latest") ||
    fail 'Could not resolve the latest GitHub release.'
  prefix="https://github.com/$repo/releases/tag/"
  [[ $latest == "$prefix"* ]] || fail "Unexpected latest release redirect: $latest"
  release=${latest#"$prefix"}
  valid_release "$release" || fail 'The latest redirect did not resolve to a stable vX.Y.Z tag.'
fi
version=${release#v}
base="https://github.com/$repo/releases/download/$release"
printf 'Release: %s\nArtifact: %s/%s\nInstall target: %s\n' "$release" "$base" "$asset" "$target"
if [[ $os == Linux ]]; then
  printf 'Runs as your normal user; sudo is used only for: sudo apt install /ABSOLUTE_TEMP_DIR/%s (no -y).\n' "$asset"
else
  printf 'Commands: xattr -w com.apple.quarantine ARTIFACT; hdiutil attach -readonly -nobrowse -mountpoint TEMP_MOUNT ARTIFACT; ditto APP STAGING_APP; replace the stopped app.\n'
  printf 'Launcher: ln -s "%s/Contents/MacOS/workshop" "%s" (only if absent; existing own link is preserved).\n' "$target" "$launcher"
  printf 'Add %s/.local/bin to PATH yourself if needed; this installer never edits shell profiles.\n' "$HOME"
  printf 'No sudo. The image is quarantined like a browser download, so Gatekeeper checks the app on first launch; if it blocks opening, review the app in System Settings > Privacy & Security and approve it manually.\n'
fi
if command -v gh >/dev/null 2>&1; then
  printf 'Verification: SHA-256 plus gh attestation verify ARTIFACT --repo %s --signer-workflow %s/.github/workflows/release.yml --source-ref refs/heads/master\n' "$repo" "$repo"
else
  printf 'WARNING: gh is absent; checksum-only verification is weaker and does not authenticate the release workflow.\n' >&2
fi
if ((dry_run)); then
  printf 'Dry run: no artifact downloads, temporary files, or installation writes.\n'
  exit 0
fi
((EUID != 0)) || fail 'Do not run this installer as root or with sudo. It uses sudo only for apt on Linux.'
require curl
require pgrep
check_not_running
if [[ $os == Linux ]]; then
  require sudo
else
  [[ ! -L $target ]] || fail 'The target app is a symbolic link; refusing to replace it.'
  [[ ! -e $target || -d $target ]] || fail 'The target app is not a directory.'
  check_launcher
fi
umask 077
# Use /tmp so _apt can traverse the absolute package path even if TMPDIR is private.
work=$(mktemp -d /tmp/workshop-install.XXXXXXXX)
artifact="$work/$asset"
manifest="$work/SHA256SUMS.txt"
for file in "$asset" SHA256SUMS.txt; do
  # Abort a stalled connection (under 1 KiB/s for a minute) instead of hanging; no total cap for slow links.
  curl --disable --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 \
    --connect-timeout 30 --speed-limit 1024 --speed-time 60 \
    --output "$work/$file" "$base/$file" || fail "Download failed: $file"
done
count=0
expected=
while IFS= read -r line || [[ -n $line ]]; do
  read -r digest filename extra <<< "$line"
  filename=${filename#\*}
  if [[ $filename == "$asset" ]]; then
    count=$((count + 1))
    [[ $digest =~ ^[[:xdigit:]]{64}$ && -z $extra ]] || fail 'Malformed SHA256SUMS.txt entry for the selected artifact.'
    expected=$digest
  fi
done < "$manifest"
((count == 1)) || fail 'SHA256SUMS.txt must contain exactly one entry for the selected artifact.'
if [[ $os == Linux ]]; then
  actual=$(sha256sum "$artifact")
else
  actual=$(shasum -a 256 "$artifact")
fi
actual=${actual%% *}
expected=$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')
[[ $actual == "$expected" ]] || fail 'SHA-256 mismatch; nothing was installed.'
if command -v gh >/dev/null 2>&1; then
  # The signer workflow path alone also matches a modified release.yml pushed to any other branch.
  gh attestation verify "$artifact" --repo "$repo" --signer-workflow "$repo/.github/workflows/release.yml" --source-ref refs/heads/master ||
    fail 'GitHub attestation verification failed (gh must be 2.68.0 or newer and signed in to github.com); nothing was installed.'
fi

if [[ $os == Linux ]]; then
  [[ $(dpkg-deb --field "$artifact" Package) == workshop ]] || fail 'Unexpected Debian package name.'
  [[ $(dpkg-deb --field "$artifact" Architecture) == "$deb_arch" ]] || fail 'Unexpected Debian package architecture.'
  [[ $(dpkg-deb --field "$artifact" Version) == "$version" ]] || fail 'Debian package version does not match the release.'
  installed=$(dpkg-query -W -f='${Version}' workshop 2>/dev/null) || installed=
  if [[ -n $installed ]] && dpkg --compare-versions "$installed" gt "$version"; then
    fail "Installed version $installed is newer than $version; downgrades are not supported."
  fi
  chmod 755 "$work"
  chmod 644 "$artifact"
  check_not_running
  sudo apt install "$artifact"
else
  mountpoint="$work/mount"
  mkdir "$mountpoint"
  # curl sets no quarantine; mounting a quarantined image propagates it to the app that ditto copies.
  xattr -w com.apple.quarantine "0081;$(printf '%x' "$(date +%s)");curl;" "$artifact"
  # Set before attaching so an interrupted attach is still detached; a failed attach mounted nothing.
  mounted=1
  hdiutil attach -readonly -nobrowse -mountpoint "$mountpoint" "$artifact" >/dev/null ||
    { mounted=0; fail 'Could not mount the verified disk image; nothing was installed.'; }
  source_app="$mountpoint/Holuto Workshop.app"
  [[ -d $source_app && ! -L $source_app ]] || fail 'The disk image does not contain Holuto Workshop.app.'
  bundle_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$source_app/Contents/Info.plist")
  [[ $bundle_version == "$version" ]] || fail 'App version does not match the release.'
  [[ -x $source_app/Contents/MacOS/workshop ]] || fail 'The app executable is missing.'
  if [[ -d $target ]]; then
    installed=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$target/Contents/Info.plist") ||
      fail 'Cannot read the existing app version; refusing to replace it.'
    valid_version "$installed" || fail 'Cannot compare the existing app version; refusing to replace it.'
    if version_gt "$installed" "$version"; then
      fail "Installed version $installed is newer than $version; downgrades are not supported."
    fi
  fi
  mkdir -p "$HOME/Applications"
  staging=$(mktemp -d "$HOME/Applications/.workshop-install.XXXXXXXX")
  ditto "$source_app" "$staging/Holuto Workshop.app"
  mkdir -p "$HOME/.local/bin"
  check_not_running
  if [[ -e $target ]]; then
    backup="$staging/previous.app"
    mv -- "$target" "$backup"
  fi
  mv -- "$staging/Holuto Workshop.app" "$target"
  if [[ ! -L $launcher ]]; then
    ln -s "$target/Contents/MacOS/workshop" "$launcher"
  fi
  # The old tree is removed only after the complete staged tree has been renamed.
  if [[ -n $backup ]]; then
    rm -rf -- "$backup"
  fi
fi
printf 'Installed %s. The app was not launched; printer connections and app settings were not accessed.\n' "$release"
