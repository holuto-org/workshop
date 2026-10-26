# Platforms and packages

A test only proves the implementation it runs. Cross-compilation is useful but
cannot prove serial timeouts, file replacement, OS trust roots, locks or GUI
startup on another OS.

- **Linux x64**: Unix pty serial framing/timeouts, private modes and atomic
  replace, process locks, XDG paths, fake session-bus tray behavior, `.deb`
  metadata/dependencies and headless launch
- **Linux ARM64**: The same Linux guarantees on ARM64, native dependencies and
  `.deb` launch; not merely an x64 cross-build
- **Windows x64**: Signed com0com serial pair, COM path/timeouts, Known Folder
  and write-through replacement, cross-process locks,
  GUI-subsystem/version/icon resources, portable executable launch
- **macOS ARM64**: Unix pty, Application Support path and file/lock behavior,
  native UI dependencies, `.app` metadata, mountable `.dmg` and launch
- **macOS x64**: The same on Intel, with its own binary, bundle and launch

The five-runner matrix is documented in [CI](../ci.md). Runner labels are
deployment configuration, not OS-compatibility promises;
[GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
describes the available architectures.

## Code for another OS

A Linux host cannot build the Windows or macOS targets: `ring`'s build script
needs that target's C toolchain, and stub compilers or extra rustup targets do
not produce trustworthy results. Change `cfg(windows)` and
`cfg(target_os = "macos")` code only when its correctness is clear from
reading, keep tests for it minimal (no new unsafe/FFI scaffolding), and report
it as not compiled or run. The [CI](../ci.md) native matrix is the check; never
claim a platform behavior that only Linux exercised.

## Ordinary local run

```sh
cargo nextest run --workspace --all-features
```

Unix pty tests allocate their own pseudoterminals; no real serial path,
connected printer or socat daemon is required. Keep the tests `#[cfg(unix)]` (or
the more specific platform they prove). They must exercise production serial
transport connected only to the simulator peer. macOS port filtering and
platform path tests remain gated to the relevant OS.

The default nextest profile excludes `com_pair` tests.
[Nextest configuration](../../.config/nextest.toml) assigns `session_bus` and
`com_pair` tests to separate single-threaded groups. The watcher scenario
re-executes itself under `dbus-run-session`; the caller need not supply a
session-bus address. CI also wraps the complete Linux run:

```sh
dbus-run-session -- cargo nextest run --profile ci --workspace --all-features
# Only the fake tray watcher scenarios:
dbus-run-session -- cargo nextest run --profile ci -p workshop --all-features \
  -E 'test(session_bus)'
```

Install `dbus-run-session` from the OS D-Bus package. The child requires its
private `DBUS_SESSION_BUS_ADDRESS` and creates its own watcher, never using the
logged-in desktop's watcher. The Slint testing backend needs no X11/Wayland
display; package startup uses the display setup in the packaging verifier.

## Windows com0com locally

Use a dedicated Windows test machine with the signed com0com null-modem driver;
do not disable driver-signature enforcement. Create a virtual pair with
com0com's setup utility, choose unused COM names, and disconnect all physical
fiscal printers. Give **only that virtual pair** to the tests (PowerShell):

```powershell
$env:HOLUTO_TEST_COM_PAIR = 'COM10,COM11'
cargo nextest run --profile ci -p posnet --all-features -E 'test(com_pair)'
```

`com_pair` reads both names, opens both ends and fails loudly if the variable is
missing or invalid; no silent skip and no fallback to enumerated hardware. The
`ci` profile includes the tests; a default-profile filter alone still excludes
them. The configured `com_pair` group serializes access to the shared pair.
CI provisions the signed pair before the Windows suite.

## Autostart artefacts

Autostart registers the current executable and saves `startOnLaunch`:

- Linux: `$XDG_CONFIG_HOME/autostart/workshop.desktop`, falling back to
  `~/.config/autostart/workshop.desktop` for an absent/relative base.
- macOS: `~/Library/LaunchAgents/com.holuto.workshop.plist`, label
  `com.holuto.workshop`, with `RunAtLoad`.
- Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
  `REG_SZ` value `Holuto Workshop` containing the quoted executable path.

Disabling removes this registration and clears the preference. See the
[CLI guide](../install-with-ai.md#4-configure-through-the-cli-not-files) for use.

## Process environment and filesystem

For XDG/HOME/timezone/proxy overrides, Windows Known Folder behavior and process
locks, re-run the test binary as a child with an isolated state directory and
synthetic credentials, as the
[writing-tests rule](../../.omp/rules/writing-tests.md) describes.

## Package checks

```sh
bash packaging/build.sh
bash packaging/verify.sh
```

On Linux the build also needs `binutils` and `dpkg-deb`, and the verifier
needs Xvfb, `xauth`, D-Bus, `xdotool`, `x11-utils`, `desktop-file-utils` and
desktop fonts; the Package workflow's install step is the authoritative list.
Release builds use fat LTO: give containers enough memory instead of weakening
the release profile.

Run on the host target, without configured printer credentials; never launch a
package against the user's saved state. The verifier uses isolated home/state
and proves `workshop --help`, `workshop check --json` and the **Holuto Workshop
main window** work: a process that merely survives a timeout could be stuck in
an error dialog. Check `.deb` dependencies/desktop entry
and installed executable (only CI, `CI=true`, installs the `.deb` system-wide;
elsewhere the extracted tree is launched); `.dmg` mounting and `.app`
Info.plist/executable; Windows PE subsystem and version/icon resources. Keep
symbol/debug artifacts for Sentry separate from stripped distribution packages.

Headless launch is not proof of physical printer access, real tray integration
on every desktop shell, Wayland layout, SmartScreen/Gatekeeper reputation or
signing.
All printer behavior in checks remains simulator/pty/com0com only.

## Multi-vendor qualification

The ordinary all-feature suite proves production gates for Posnet, Novitus and
Elzab. Serial enumeration fixtures distinguish OS metadata from verified
identity; USB hints never qualify a model. Native simulator tests do not prove
USB descriptors, interface-to-PC1 mapping or physical print fidelity.
The [hardware checklist](hardware-acceptance.md) owns the separate debug
acceptance command and human-only evidence collection on each OS/model.
