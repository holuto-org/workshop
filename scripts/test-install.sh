#!/usr/bin/env bash
# Focused installer contract tests. All installer side effects are PATH fixtures.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/workshop-install-tests.XXXXXXXX")
trap 'rm -rf -- "$work"' EXIT
mkdir -p "$work/bin" "$work/home"
export FIXTURE_LOG="$work/calls" FIXTURE_OS=Darwin FIXTURE_ARCH=x86_64
export FIXTURE_MAC=11.0 FIXTURE_ARM=0 FIXTURE_GLIBC='glibc 2.39'
export FIXTURE_DOWNLOAD=forbidden FIXTURE_MANIFEST=bad FIXTURE_GH=0
export FIXTURE_RUNNING=none FIXTURE_MOUNT=forbidden
FIXTURE_REAL_UNAME=$(command -v uname)
export FIXTURE_REAL_HASH
FIXTURE_REAL_HASH=$(command -v sha256sum || command -v shasum)
cat > "$work/bin/fixture" <<'FIXTURE'
#!/usr/bin/env bash
set -eu
name=${0##*/}
case $name in
  uname)
    case $1 in -s) printf '%s\n' "$FIXTURE_OS" ;; -m) printf '%s\n' "$FIXTURE_ARCH" ;; *) exit 91 ;; esac ;;
  sw_vers) [[ $1 == -productVersion ]]; printf '%s\n' "$FIXTURE_MAC" ;;
  sysctl) [[ $* == '-n hw.optional.arm64' ]]; printf '%s\n' "$FIXTURE_ARM" ;;
  getconf) [[ $1 == GNU_LIBC_VERSION ]]; printf '%s\n' "$FIXTURE_GLIBC" ;;
  pgrep)
    printf 'pgrep %s\n' "$*" >> "$FIXTURE_LOG"
    [[ $1 == -x && $# == 2 ]]
    [[ $2 == "$FIXTURE_RUNNING" ]] && exit 0
    exit 1 ;;
  shasum)
    if [[ ${FIXTURE_REAL_HASH##*/} == sha256sum ]]; then
      "$FIXTURE_REAL_HASH" "${@: -1}"
    else
      "$FIXTURE_REAL_HASH" "$@"
    fi ;;
  curl)
    printf 'curl %s\n' "$*" >> "$FIXTURE_LOG"
    [[ $FIXTURE_DOWNLOAD != forbidden ]] || exit 92
    if [[ $* == *--head* ]]; then
      printf 'https://github.com/holuto-org/workshop/releases/tag/v1.2.3'
      exit
    fi
    out='' url=''
    while (($#)); do
      case $1 in --output) out=$2; shift 2 ;; *) url=$1; shift ;; esac
    done
    case $url in
      */SHA256SUMS.txt)
        case $FIXTURE_MANIFEST in
          bad) printf 'not-a-digest  workshop-mac-x64.dmg\n' > "$out" ;;
          missing) printf '%064d  unrelated.dmg\n' 0 > "$out" ;;
          duplicate) printf '%064d  workshop-mac-x64.dmg\n%064d  workshop-mac-x64.dmg\n' 0 0 > "$out" ;;
          mismatch) printf '%064d  workshop-mac-x64.dmg\n' 0 > "$out" ;;
          valid) printf '%s  workshop-mac-x64.dmg\n' "$FIXTURE_DIGEST" > "$out" ;;
          *) exit 93 ;;
        esac ;;
      */workshop-mac-x64.dmg) printf 'fixture artifact\n' > "$out" ;;
      *) exit 94 ;;
    esac ;;
  gh) printf 'gh %s\n' "$*" >> "$FIXTURE_LOG"; exit "$FIXTURE_GH" ;;
  xattr|hdiutil)
    if [[ $FIXTURE_MOUNT == forbidden ]]; then
      printf 'FORBIDDEN %s %s\n' "$name" "$*" >> "$FIXTURE_LOG"; exit 95
    fi
    printf '%s %s\n' "$name" "$*" >> "$FIXTURE_LOG"
    # Mounting fails (e.g. blocked by device policy), so detaching has nothing to detach.
    [[ $name == xattr ]] || exit 1 ;;
  apt|sudo|ditto|open)
    printf 'FORBIDDEN %s %s\n' "$name" "$*" >> "$FIXTURE_LOG"; exit 95 ;;
  *) exit 96 ;;
esac
FIXTURE
chmod +x "$work/bin/fixture"
for command in uname sw_vers sysctl getconf pgrep shasum curl gh apt sudo hdiutil ditto open xattr; do
  ln -s fixture "$work/bin/$command"
done
export PATH="$work/bin:$PATH" HOME="$work/home"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
run() {
  : > "$FIXTURE_LOG"
  status=0
  bash "$root/scripts/install.sh" "$@" > "$work/output" 2>&1 || status=$?
}
contains() { grep -F -- "$1" "$work/output" >/dev/null || fail "missing output: $1"; }
no_side_effects() { [[ ! -s $FIXTURE_LOG ]] || fail "dry-run/validation invoked a side-effect command"; }

# Removing argument validation or accepting ambiguous tags must fail these cases.
for argument in '--unknown' '--release' 'v1.2.3' '--release=v1.2.3'; do
  run "$argument"
  [[ $status != 0 ]] || fail "accepted invalid argument: $argument"
  no_side_effects
done
for release in latest 1.2.3 v1.2 v1.2.3-rc.1 v01.2.3 'v1.2.3/other' 'v1.2.3?x' ''; do
  run --release "$release" --dry-run
  [[ $status != 0 ]] || fail "accepted invalid release: $release"
  no_side_effects
done

# Wrong architecture, baseline, or Rosetta selection must be visible without writes.
for row in 'x86_64 0 11.0 workshop-mac-x64.dmg' 'arm64 1 14.0 workshop-mac-arm64.dmg' 'x86_64 1 12.3 workshop-mac-arm64.dmg'; do
  read -r FIXTURE_ARCH FIXTURE_ARM FIXTURE_MAC expected <<< "$row"
  run --release v1.2.3 --dry-run
  [[ $status == 0 ]] || fail "supported macOS dry-run failed"
  contains "https://github.com/holuto-org/workshop/releases/download/v1.2.3/$expected"
  contains "$HOME/Applications/Holuto Workshop.app"
  contains 'hdiutil attach -readonly -nobrowse'
  contains "$HOME/.local/bin/workshop"
  contains 'PATH'
  no_side_effects
  [[ ! -e $HOME/Applications ]] || fail 'dry-run created Applications'
  [[ ! -e $HOME/.local ]] || fail 'dry-run created launcher directory'
done
FIXTURE_ARCH=ppc64
run --release v1.2.3 --dry-run
[[ $status != 0 ]] || fail 'accepted unsupported macOS architecture'
no_side_effects
FIXTURE_ARCH=x86_64 FIXTURE_ARM=0 FIXTURE_MAC=10.15.7
run --release v1.2.3 --dry-run
[[ $status != 0 ]] || fail 'accepted macOS below baseline'
no_side_effects
FIXTURE_MAC=11.0 FIXTURE_OS=FreeBSD
run --release v1.2.3 --dry-run
[[ $status != 0 ]] || fail 'accepted unsupported OS'
no_side_effects

# Native /etc/os-release is deliberately not replaceable by an installer override.
if [[ $("$FIXTURE_REAL_UNAME" -s) == Linux ]]; then
  FIXTURE_OS=Linux FIXTURE_ARCH=x86_64
  run --release v1.2.3 --dry-run
  if [[ $status == 0 ]]; then
    contains 'workshop-linux-x64.deb'
    contains 'sudo apt install'
    FIXTURE_ARCH=aarch64
    run --release v1.2.3 --dry-run
    [[ $status == 0 ]] || fail 'supported Linux ARM dry-run failed'
    contains 'workshop-linux-arm64.deb'
    FIXTURE_GLIBC='glibc 2.38'
    run --release v1.2.3 --dry-run
    [[ $status != 0 ]] || fail 'accepted glibc below baseline'
    FIXTURE_GLIBC='musl 1.2.5'
    run --release v1.2.3 --dry-run
    [[ $status != 0 ]] || fail 'accepted non-glibc Linux'
  else
    contains 'Unsupported Linux distribution'
    printf 'Native Linux distribution rejected; supported Linux paths need Ubuntu/Debian CI.\n'
  fi
  no_side_effects
fi

# Default resolution is a single HEAD, never an artifact download during dry-run.
FIXTURE_OS=Darwin FIXTURE_ARCH=x86_64
FIXTURE_DOWNLOAD=allowed
run --dry-run
[[ $status == 0 ]] || fail 'latest resolution dry-run failed'
contains '/releases/download/v1.2.3/workshop-mac-x64.dmg'
[[ $(wc -l < "$FIXTURE_LOG") == 1 ]] || fail 'latest resolution made extra calls'
grep -F -- '--head' "$FIXTURE_LOG" >/dev/null || fail 'latest used a body download'

# Invalid/duplicate/mismatched checksums and failed provenance never reach a mount.
if ((EUID != 0)); then
  FIXTURE_RUNNING=workshop
  run --release v1.2.3
  [[ $status != 0 ]] || fail "accepted running process: $FIXTURE_RUNNING"
  contains 'is running'
  ! grep -F 'curl ' "$FIXTURE_LOG" >/dev/null || fail 'downloaded while an app was running'
  FIXTURE_RUNNING=none
  mkdir -p "$HOME/.local/bin"
  printf 'unrelated command\n' > "$HOME/.local/bin/workshop"
  run --release v1.2.3
  [[ $status != 0 ]] || fail 'overwrote unrelated launcher file'
  contains 'launcher'
  [[ $(cat "$HOME/.local/bin/workshop") == 'unrelated command' ]] || fail 'changed unrelated launcher'
  ! grep -F 'curl ' "$FIXTURE_LOG" >/dev/null || fail 'downloaded despite launcher conflict'
  rm "$HOME/.local/bin/workshop"
  ln -s /unrelated/workshop "$HOME/.local/bin/workshop"
  run --release v1.2.3
  [[ $status != 0 ]] || fail 'accepted unrelated launcher symlink'
  contains 'launcher'
  [[ $(readlink "$HOME/.local/bin/workshop") == /unrelated/workshop ]] || fail 'changed unrelated symlink'
  rm "$HOME/.local/bin/workshop"
  ln -s "$HOME/Applications/Holuto Workshop.app/Contents/MacOS/workshop" "$HOME/.local/bin/workshop"
  for FIXTURE_MANIFEST in bad missing duplicate mismatch valid; do
    if [[ $FIXTURE_MANIFEST == valid ]]; then
      if command -v sha256sum >/dev/null 2>&1; then
        FIXTURE_DIGEST=$(printf 'fixture artifact\n' | sha256sum)
      else
        FIXTURE_DIGEST=$(printf 'fixture artifact\n' | shasum -a 256)
      fi
      FIXTURE_DIGEST=${FIXTURE_DIGEST%% *}
      export FIXTURE_DIGEST
      FIXTURE_GH=1
    fi
    run --release v1.2.3
    [[ $status != 0 ]] || fail "accepted failed verification: $FIXTURE_MANIFEST"
    case $FIXTURE_MANIFEST in
      bad) contains 'Malformed SHA256SUMS.txt entry' ;;
      missing|duplicate) contains 'exactly one entry' ;;
      mismatch) contains 'SHA-256 mismatch' ;;
      valid) contains 'GitHub attestation verification failed' ;;
    esac
    ! grep -F 'FORBIDDEN' "$FIXTURE_LOG" >/dev/null || fail 'verification failure reached installation'
    if [[ $FIXTURE_MANIFEST == valid ]]; then
      grep -F -- '--repo holuto-org/workshop --signer-workflow holuto-org/workshop/.github/workflows/release.yml' "$FIXTURE_LOG" >/dev/null || fail 'missing workflow-bound attestation verification'
    fi
  done
  # A verified image is quarantined before mounting; a failed mount is reported
  # as such, never as a detach failure, and leaves no downloaded artifact behind.
  FIXTURE_GH=0 FIXTURE_MOUNT=fail
  run --release v1.2.3
  [[ $status != 0 ]] || fail 'accepted a failed mount'
  contains 'Could not mount the verified disk image'
  ! grep -F 'Could not detach' "$work/output" >/dev/null || fail 'reported a detach for an image that was never mounted'
  ! grep -F 'hdiutil detach' "$FIXTURE_LOG" >/dev/null || fail 'detached an image that was never mounted'
  grep -E '^xattr -w com\.apple\.quarantine 0081;[0-9a-f]+;curl; /tmp/workshop-install\.[^ ]+/workshop-mac-x64\.dmg$' "$FIXTURE_LOG" >/dev/null ||
    fail 'downloaded image was not quarantined'
  [[ $(grep -E '^(xattr|hdiutil) ' "$FIXTURE_LOG" | cut -d' ' -f1) == $'xattr\nhdiutil' ]] || fail 'image was mounted before it was quarantined'
  attached=$(grep '^hdiutil attach ' "$FIXTURE_LOG")
  [[ ! -e ${attached##* } ]] || fail 'left the downloaded image behind after a failed mount'
  FIXTURE_MOUNT=forbidden
else
  printf 'Root runner: full-mode verification fixtures need an unprivileged CI user.\n'
fi
printf 'Installer contract tests passed.\n'
