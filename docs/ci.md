# CI

- **Checks (`checks.yml`)**
  - **Trigger**: Pull requests into `master`, merge groups and reusable release
    invocation.
  - **Runner and work**: Installer checks/dry runs on Linux, Windows and macOS;
    static checks on Ubuntu; default/all-feature Clippy on those three OSes;
    native nextest on the five targets below; Package for PR artifacts.
- **Package (`package.yml`)**
  - **Trigger**: Reused by Checks and Release.
  - **Runner and work**: Build and verify Windows x64 `.exe`, Linux x64/ARM64
    `.deb`, macOS ARM64/x64 `.dmg`; isolated installed-package CLI checks and
    main-window launch without a configured printer; retain matching debug
    artifacts.
- **Release (`release.yml`)**
  - **Trigger**: Push to `master` with an increased workspace version; manual
    artifact build.
  - **Runner and work**: Checks + reusable Package; version/published-release
    guard; required Sentry configuration and debug upload; publish stable
    filenames/checksums and finalize the Sentry release only for qualifying
    version increases.
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
  session bus under `dbus-run-session`, `.deb` checks.
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
production code and all-feature debug/test code must compile/lint. Production
release builds reject simulator/testing features; packaging never enables them.
See [local platform procedures](testing/platforms.md).

Windows package smoke requires a clean disposable user profile and refuses if
app state already exists. Windows Known Folders ignore a `LOCALAPPDATA`
environment override; never smoke-launch against someone's saved state.

## Release telemetry artifacts

Set repository variables `SENTRY_DSN`, `SENTRY_ORG=holuto`,
`SENTRY_PROJECT=workshop`, `SENTRY_URL=https://de.sentry.io`, and
organization-token secret `SENTRY_AUTH_TOKEN`. Provision the `holuto/workshop`
project in the EU; local workflow files do not prove provisioning or repository
variable values. A missing build-time DSN disables client telemetry.

Release identity is `workshop@<workspace version>` and release builds use
environment `production`. Keep debug information in the unstripped build,
produce the distributable package separately, and upload matching debug
files/source context with pinned `@sentry/cli` 3.8.2
(`debug-files upload --include-sources`). Release creation/commit
association/finalization and symbol upload must use that same identity. Never
put the auth token in a binary or package; the public DSN is not that token.
Fork PRs must remain buildable without release secrets.

Every Release-workflow package, including manual artifact-only runs, uses
Package's production mode: Sentry configuration/credentials are required and
symbols are uploaded. PR packages use non-production mode without telemetry
credentials. Release creation, commit association and finalization happen only
in the publish job for an actual version-bump push, not a manual artifact-only
run.

A successful upload is not proof of symbolication: inspect a synthetic, approved
release event against that exact binary as described in
[workflows](workflows.md#sentry-release-check). [Telemetry](telemetry.md)
records runtime data and privacy boundaries.
[Windows signing](windows-code-signing.md) is a separate provisioning procedure,
not a claim that current artifacts are signed.
