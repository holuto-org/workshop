# Platforms and packages

A test only proves the implementation it runs. Cross-compilation is useful but
cannot prove serial timeouts, file replacement, OS trust roots, locks or GUI
startup on another OS. The native suites and package checks cover:

- **Linux x64**: Unix pty serial framing/timeouts, private modes and atomic
  replacement, process locks, XDG paths, fake session-bus tray behavior, `.deb`
  metadata/dependencies and X11 launch under Xvfb
- **Linux ARM64**: The same checks on ARM64, native dependencies and `.deb`
  launch; not merely an x64 cross-build
- **Windows x64**: Signed com0com serial pair, COM path/timeouts, Known Folder
  paths, replacement with an ordinary open file reader on the runner filesystem,
  cross-process locks, GUI-subsystem/version/icon resources, static CRT and
  portable executable launch
- **macOS ARM64**: Unix pty, Application Support path and file/lock behavior,
  native AppKit quit handling, `.app` metadata/ad-hoc signature, mountable `.dmg`
  and launch
- **macOS x64**: The same checks on Intel, with its own binary, bundle and launch

The five-runner matrix is documented in [CI](../ci.md). Runner labels are
deployment configuration, not OS-compatibility promises;
[GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
describes the available architectures.

## Code for another OS

Cross-compilation is not inherently impossible from Linux. It depends on the
target and the available compiler, linker, SDK and native dependency support.
[Rustup's cross-compilation guide](https://rust-lang.github.io/rustup/cross-compilation.html)
explains why adding the Rust target alone is insufficient. Even `cargo check`
can require native dependency build tools; a missing toolchain is a limitation
of that environment, not evidence that the changed code is wrong.

Keep the evidence separate: a successful target-specific `cargo check`
typechecks the selected code; a build also produces linked artifacts; neither
runs native tests. Record the target, features, command and any blocker, and
report native execution separately. Do not force built-in OS `cfg` flags on the
host or substitute stub compilers to manufacture a passing check.

Foreign-platform changes still need meaningful behavior tests. Reuse existing
native harnesses and platform bindings rather than restricting tests to trivial
assertions because this host cannot run them. The code's lint and safety
contracts govern FFI, not the host OS. The [CI](../ci.md) native matrix provides
the runtime check; never claim Windows or macOS behavior from Linux execution.

## Ordinary local run

```sh
cargo nextest run --workspace --all-features
```

Unix pty tests allocate their own pseudoterminals; no real serial path,
connected printer or socat daemon is required. The Posnet session tests connect
the production serial transport to a scripted fake peer, not a physical device.
On macOS they use baud 0 because the pty rejects `IOSSIOSPEED`: this checks native
opening, exclusivity, framing and timeouts, not physical baud-rate configuration.
See [the harness](../../crates/posnet/src/session/tests.rs).
Gate tests to the OS behavior they prove; macOS port filtering and platform path
tests remain specific to their OS.

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

Install `dbus-run-session` from the OS D-Bus package. The child receives its
private `DBUS_SESSION_BUS_ADDRESS` and creates its own watcher, never using the
logged-in desktop's watcher. Do not set the internal
`WORKSHOP_PRIVATE_TEST_BUS` marker yourself: that bypasses the scenario's
private-bus setup. The Slint testing backend needs no X11/Wayland display;
the native macOS quit harness and package startup are separate native UI checks.

## Windows com0com locally

Use a dedicated Windows test machine with the signed com0com null-modem driver;
do not disable driver-signature enforcement. Create a virtual pair with
com0com's setup utility, choose unused COM names, and disconnect all physical
fiscal printers. Give **only that virtual pair** to the tests (PowerShell):

```powershell
$env:HOLUTO_TEST_COM_PAIR = 'COM10,COM11'
cargo nextest run --profile ci -p posnet --all-features -E 'test(com_pair)'
```

`com_pair` reads both names and opens exactly those endpoints; a missing
variable, malformed pair or failed open fails the test, with no silent skip or
fallback to enumerated hardware. It does **not** verify that those names belong
to com0com: confirming the pair before setting the variable is essential.
The `ci` profile includes the tests; a default-profile filter alone still
excludes them. The configured `com_pair` group serializes access within that
nextest run, not across separate runs; do not run them concurrently on one pair.
CI provisions the signed pair before the Windows suite with
[com0com.ps1](../../.github/scripts/com0com.ps1). That script installs a driver
and imports its pinned certificate into machine trust stores; it is intended
for disposable CI machines, not an ordinary workstation setup command.

## Autostart artefacts

Autostart registers the current executable and saves `startOnLaunch`:

- Linux: `$XDG_CONFIG_HOME/autostart/workshop.desktop`, falling back to
  `~/.config/autostart/workshop.desktop` for an absent/relative base.
- macOS: `~/Library/LaunchAgents/com.holuto.workshop.plist`, label
  `com.holuto.workshop`, with `RunAtLoad`, naming the resolved bundle
  executable even when the CLI runs through the `~/.local/bin/workshop` link.
- Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
  `REG_SZ` value `Holuto Workshop` containing the quoted executable path.

Disabling removes this registration and clears the preference. See the
[CLI guide](../install-with-ai.md#4-configure-through-the-cli-not-files) for use.

These tests check registration content, removal and isolated filesystem/registry
behavior, not a complete desktop login. Windows tests use a temporary registry
key, not the operator's real Run key.

## Process environment and filesystem

For XDG/HOME/timezone/proxy overrides, Windows Known Folder behavior and process
locks, re-run the test binary as a child with an isolated state directory and
synthetic credentials, as the
[writing-tests rule](../../.omp/rules/writing-tests.md) describes.
See [compatibility](compatibility.md) for replacement and durability limits:
passing on a runner filesystem does not qualify every filesystem, handle-sharing
mode or power-loss scenario.

## Package checks

Use a native host with its normal host-target Cargo configuration (no foreign
`CARGO_BUILD_TARGET` override). Windows runs these Bash scripts from Git Bash;
the verifier also needs PowerShell 7 (`pwsh.exe`). Use a separate checkout for
concurrent packaging: `build.sh` deletes `release/`, the host-specific staging
directory and all `packaging/build/symbols` before building. A separate
`CARGO_TARGET_DIR` does not isolate those outputs. Verification expects exactly
one fresh package in `release/`.

```sh
bash packaging/build.sh
bash packaging/verify.sh
```

Both scripts need the build toolchain and `jq`; the platform tools are required
as well. On Linux the build needs `binutils` and `dpkg-deb`, and verification
needs Python 3, Xvfb, `xauth`, D-Bus, `xdotool`, `x11-utils`,
`desktop-file-utils` and desktop fonts. The
[Package job in the Release workflow](../../.github/workflows/release.yml)
owns the CI dependency installation. The native macOS tools include `iconutil`,
`hdiutil`, `strip`, `plutil`, `lipo`, `codesign` and `osascript`;
launch verification needs a logged-in graphical session with System Events
access, not just an SSH shell.

Run on the host target, without configured printer credentials; never launch a
package against the user's saved state. On Unix, `verify.sh` uses temporary
home/state directories; the standalone Linux smoke script creates its own fresh
state too. On Windows, the verifier requires a clean disposable user profile
and refuses existing Workshop state rather than overriding Known Folders.
Every GUI smoke launch saves diagnostics opt-out through the CLI before starting
the app, including when the executable embeds the production DSN.
The verifier checks `workshop --help`, expects fresh unpaired
`workshop check --json` to exit 3, and requires the **Holuto Workshop main
window**: a process that merely survives a timeout could be stuck in an error
dialog. Linux checks `.deb` dependencies/desktop entry and ELF compatibility;
only with `CI=true` and `apt-get` available does it install the `.deb`
system-wide, using sudo. Otherwise it launches the extracted tree.
macOS mounts the `.dmg`, copies the `.app` to a temporary location, checks
Info.plist, host architecture and ad-hoc signature, then launches that copy.
Windows checks PE subsystem, static CRT and version/icon resources before
launching a temporary executable copy. Keep symbol/debug artifacts for Sentry
separate from stripped distribution packages.

On Linux, `python3 packaging/test-smoke.py target/release/workshop` exercises
standalone smoke isolation and diagnostics opt-out through the real CLI, with a
launch probe instead of a GUI. It never starts a printer worker; run it after
building the executable. Substitute the actual release executable path when
using a custom `CARGO_TARGET_DIR`.

Headless launch is not proof of physical printer access, real tray integration
on every desktop shell, Wayland layout, SmartScreen/Gatekeeper reputation,
Windows Authenticode signing or macOS Developer ID signing/notarization.
All printer behavior in checks remains simulator/pty/com0com only.

## Multi-vendor qualification

The ordinary all-feature suite proves production gates for Posnet, Novitus and
Elzab. Serial enumeration fixtures distinguish OS metadata from verified
identity; USB hints never qualify a model. Native simulator tests do not prove
USB descriptors, interface-to-PC1 mapping or physical print fidelity.
The [hardware checklist](hardware-acceptance.md) owns the separate debug
acceptance command and human-only evidence collection on each OS/model.
