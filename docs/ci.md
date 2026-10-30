# CI

The workflow YAML is authoritative for triggers, permissions, tools and job
dependencies:

- **[Checks](../.github/workflows/checks.yml)** runs for pull requests into
  `master`, merge groups and calls from Release. It checks installer contracts
  with fixtures and dry runs on Linux, Windows and macOS (Windows also uses
  PowerShell 5.1); it does not install a published release.
  Ubuntu static analysis runs formatting, default/all-feature Clippy,
  `cargo deny`, unused-dependency detection, ShellCheck, actionlint and OpenAPI
  JSON syntax validation. Windows and macOS also run default/all-feature Clippy.
  Native nextest runs on the five test runners below; a separate job exercises
  debug-only acceptance gates. Pull requests and merge groups do not build
  packages, so packaging failures first appear after a push to `master`.
- **[Release](../.github/workflows/release.yml)** runs on every push to `master`:
  Checks → all five native Package jobs → version lookup → Publish when new.
  Package verification and Sentry uploads must succeed on every target before
  publication. The tag is `v<workspace version>` from `Cargo.toml`. If its GitHub
  release already exists, the run still builds, verifies and uploads symbols,
  but does not publish or replace assets. Bump the version for a new release;
  retry a failed release by rerunning its original run. Release runs are
  serialized without cancelling an in-progress release.
- **[Cleanup merged PR branches](../.github/workflows/cleanup-merged-pr-branches.yml)**
  runs when a pull request closes. It deletes the head branch only for a merged
  PR from this repository, excluding the repository's default branch.

`production` is the packaged app's Sentry environment, not a second release
path. Required checks are repository settings, not workflow YAML. When changing
job names or matrix entries, update the required-check list too, or a PR can
wait for a check that no longer reports.

CI does not run the local Markdown/link checks, rustdoc build, coverage report,
stress repetitions or manual symbolication check described in
[finishing a change](workflows.md#finishing-a-change).
Coverage is not a merge percentage target. Dependency policy is enforced by
[`deny.toml`](../deny.toml) across all five release targets, including its
documented advisory exceptions; this is not a promise of zero vulnerabilities.

## Native matrix

- **`ubuntu-latest`: Linux x64**: Native tests, Unix ptys, private modes/locks
  and session-bus tests under `dbus-run-session`; static/acceptance checks.
- **`ubuntu-24.04`: Linux x64 package only**: `.deb` build, system installation
  and launch; the build host's glibc is the package's `libc6` floor.
- **`ubuntu-24.04-arm`: Linux ARM64**: Native tests and `.deb` installation/launch.
- **`windows-latest`: Windows x64**: Native tests with the signed com0com
  pair supplied through `HOLUTO_TEST_COM_PAIR`; PE resources/subsystem, static
  CRT imports and executable launch checks.
- **`macos-latest`: macOS ARM64**: Native tests, Unix ptys; bundle/plist/dmg,
  architecture, ad-hoc signature and launch checks.
- **`macos-26-intel`: macOS x64**: Native Intel tests and the same macOS
  package checks.

[GitHub runner documentation](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
explains runner architectures and image labels; `-latest` labels move over time.
All serial peers in CI are simulator, pty or virtual com0com; no job may use a
real fiscal printer.

The [`ci` nextest profile](../.config/nextest.toml) includes platform tests;
the default profile excludes `com_pair`. Session-bus and COM-pair tests each
have a serial group; test retries are disabled. Native and acceptance jobs
always attempt to upload JUnit reports with seven-day retention. An early
setup or build failure may leave no report.
Ordinary tests use all features; Clippy also checks default-feature production
code. Packaging never enables desktop `simulator` or agent `testing` and each
package job checks that a simulator release build fails with the expected guard.
See [local platform procedures](testing/platforms.md).

The acceptance job follows
[hardware acceptance](testing/hardware-acceptance.md): a dedicated
`RUSTFLAGS='--cfg workshop_acceptance'` debug build, whole-workspace suite and
Clippy, and a release compile-error check. The normal `--all-features` suite
never sets this cfg and must prove production gates. No CI job qualifies hardware.

[`packaging/verify.sh`](../packaging/verify.sh) checks fresh, unpaired CLI state
and main-window launch with diagnostics disabled, not printing or live Holuto
connectivity. Linux uses Xvfb/X11; this is not a Wayland launch check. macOS
launches a copied app bundle and Windows a copied standalone EXE, not the
end-user install scripts. Windows smoke requires a clean disposable profile and
refuses existing app state: Known Folders ignore a `LOCALAPPDATA` override.

## Diagnose failed checks

- **Windows serial setup hangs**: Compare
  [the com0com setup script](../.github/scripts/com0com.ps1) with the
  [com0com setup requirements](testing/platforms.md#windows-com0com-locally).
- **actionlint installation fails**: Keep the pinned, checksum-verified download
  in Checks. `install-action` has no actionlint manifest, and its
  `cargo-binstall` fallback cannot install this Go tool.
- **A durable model test times out**: Read the nextest profile before changing
  a suite-wide timeout or adding retries. The reconciliation model has a longer
  timeout because each generated case syncs journal writes to disk, which was
  slower on Windows and macOS runners.

Linux checks do not prove Windows or macOS fixes. Read the affected native job
results; cross-compilation alone does not exercise OS behavior.

## Release assets and trust

[`packaging/build.sh`](../packaging/build.sh) produces five stable filenames:
`workshop-win-x64.exe`, `workshop-linux-x64.deb`, `workshop-linux-arm64.deb`,
`workshop-mac-arm64.dmg` and `workshop-mac-x64.dmg`. The Windows asset is a
standalone EXE, not an installer. Publish adds the version-matched `install.sh`,
`install.ps1`, `install-with-ai.md` and `SHA256SUMS.txt`; it verifies the manifest
and attests every staged asset before creating the GitHub release at the
triggering commit. Package artifacts are retained for seven days; debug files
are not uploaded as GitHub artifacts or release assets.

Checks and packaging have read-only repository permissions; only Publish gets
release-write and provenance-attestation permissions. Checkout credentials are
not persisted. Branch cleanup separately has branch-delete permission. The
Windows virtual-driver installer/certificate and actionlint archive are
checksum-verified, but action major-version tags and the Sentry CLI major
version are not immutable dependency pins.

GitHub provenance is not OS publisher signing: this workflow does not
Authenticode-sign Windows binaries or Developer-ID-sign/notarize macOS apps.
macOS uses an ad-hoc signature. Installer checksum/provenance verification is
described in the [installation guide](install-with-ai.md); a passing package
smoke does not prove those public downloads or attestations can be retrieved.

## Release telemetry artifacts

Release builds embed `https://app.<APP_NAME lowercased>.<TLD_PRODUCTION>/api` as the
production Holuto API default. Configure Actions variables `APP_NAME`,
`TLD_PRODUCTION`, `SENTRY_DSN`, `SENTRY_ORG` and `SENTRY_PROJECT`, plus secret
`SENTRY_AUTH_TOKEN` for debug uploads. Package jobs require a nonempty API URL
and DSN; that check does not validate either service's availability or
configuration.

Release builds report as `workshop@<workspace version>` in Sentry environment
`production`. CI uploads debug files/source context; it does not explicitly
create a Sentry release record or associate commits. Runtime events supply the
release identity. Each Package job uploads matching ELF/DWARF, dSYM or PDB
companions and the shipped executable needed for unwinding, waiting for Sentry
processing before uploading its package artifact. The upload token is scoped
to that step and never enters the build or package.

[Telemetry](telemetry.md) owns runtime privacy and debug-file requirements;
[workflows](workflows.md#sentry-release-check) owns symbolication verification.
Successful upload does not itself prove native crash delivery or readable
stack frames.
