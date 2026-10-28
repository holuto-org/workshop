# CI

- **Checks (`checks.yml`)**
  - **Trigger**: Pull requests into `master`, merge groups and every Release
    run.
  - **Runner and work**: Installer checks/dry runs on Linux, Windows and macOS;
    static checks on Ubuntu; default/all-feature Clippy on those three OSes;
    native nextest on the five test runners below (`ubuntu-24.04` only
    packages); separate simulator-only debug acceptance and release-rejection
    checks. Pull requests do not build packages.
- **Release (`release.yml`)**
  - **Trigger**: Every push to `master`.
  - **Runner and work**: Checks; then the Package job builds and verifies
    Windows x64 `.exe`, Linux x64/ARM64 `.deb`, macOS ARM64/x64 `.dmg`
    (isolated installed-package CLI checks and main-window launch without a
    configured printer). If no GitHub release exists for the workspace version,
    it publishes stable filenames/checksums. A push that
    keeps a released version only builds and checks: bump the version to
    release. Retry a failed release by rerunning its original run.
- **Cleanup merged PR branches (`cleanup-merged-pr-branches.yml`)**
  - **Trigger**: Merged pull request.
  - **Runner and work**: `ubuntu-latest`; remove the merged head branch subject
    to workflow safeguards.

The workflow YAML is authoritative for triggers, permissions, pinned tools and
job dependencies. There is **no weekly workflow**. Mutation testing is a
finishing-change policy; coverage is a local report, not a merge percentage
target.

## Native matrix

- **`ubuntu-latest` — Linux x64**: Unix ptys, private modes/locks, fake
  session bus under `dbus-run-session`.
- **`ubuntu-24.04` — Linux x64 package**: `.deb` build and checks; the build
  host's glibc is the package's `libc6` floor.
- **`ubuntu-24.04-arm` — Linux ARM64**: Native tests and ARM64 package launch.
- **`windows-latest` — Windows x64**: Signed com0com pair via
  `HOLUTO_TEST_COM_PAIR`; PE resources/subsystem and executable launch.
- **`macos-latest` — macOS ARM64**: Unix ptys; bundle/plist/dmg and launch.
- **`macos-26-intel` — macOS x64**: Native Intel tests and package launch.

[GitHub runner documentation](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
explains runner architectures and image labels. All serial peers in CI are
simulator, pty or virtual com0com; no job may use a real fiscal printer.

The `ci` nextest profile includes platform tests; the default profile excludes
`com_pair`. Session-bus tests run in their serial group. Both default-feature
production code and all-feature debug/test code must compile/lint. Release builds
reject desktop `simulator` and agent `testing`; packaging never enables them.
See [local platform procedures](testing/platforms.md).
The acceptance job follows
[hardware acceptance](testing/hardware-acceptance.md): a dedicated
`RUSTFLAGS='--cfg workshop_acceptance'` build, whole-workspace suite and Clippy,
and a release compile-error check. The normal `--all-features` suite never
sets this cfg and must prove production gates. No CI job qualifies hardware.

Windows package smoke requires a clean disposable user profile and refuses if
app state already exists. Windows Known Folders ignore a `LOCALAPPDATA`
environment override; never smoke-launch against someone's saved state.

## Release telemetry artifacts

Set repository variables `HOLUTO_API_URL` (the production Holuto API URL that
release builds carry as their default), `SENTRY_DSN`, `SENTRY_ORG=holuto`,
`SENTRY_PROJECT=workshop` and organization-token secret `SENTRY_AUTH_TOKEN`
(it carries the Sentry URL). Release builds report as
`workshop@<workspace version>` in environment `production`; Sentry creates the
release from the first event. The Package job uploads the matching debug files
and source context from the unstripped build; the token never enters a binary
or package. [Telemetry](telemetry.md) records runtime data and privacy
boundaries; [workflows](workflows.md#sentry-release-check) describes checking
symbolication.
