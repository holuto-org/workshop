# Holuto Workshop

Rust/Slint tray application and `workshop` CLI for workshop PCs connected to a
supported Posnet, Novitus or Elzab online fiscal printer. Prints only Holuto
sales/advances and fiscal reports; no manual receipt editor and no separate
agent service. Closing to the tray keeps processing; starting is explicit
unless the user opts into autostart.

## Where guidance lives

Each piece of guidance has one home: the first item that fits. When a change
makes guidance untrue, update its home in the same change.

- **A machine can decide it**: Checks: workspace lints, tests, dependency
  policy, nextest configuration and CI. Do not duplicate checkable rules as
  prose.
- **Must hold for every agent on every request, including subagents**:
  [`.omp/RULES.md`](.omp/RULES.md); keep it tiny.
- **Every task needs it: product, boundaries, conventions and commands**:
  This file; before editing/verifying collect every `AGENTS.md` from root to
  changed files. Rules add along the chain.
- **Area judgment triggered by a file/code pattern**:
  [`.omp/rules/`](.omp/rules/) index below; read the applicable rule before
  planning a change there.
- **Crate/module ownership, invariants, decisions and exceptions**:
  `//!` / `///` on that code; read them before changing it.
- **Why one statement is unusual**: A local `//` comment explaining why, not
  what.
- **Review priorities**: [`.omp/WATCHDOG.md`](.omp/WATCHDOG.md).
- **Procedures and concern-specific evidence**:
  [Testing strategy/checklist](docs/testing/README.md), [CI](docs/ci.md),
  [workflows](docs/workflows.md), [telemetry](docs/telemetry.md),
  [Windows signing](docs/windows-code-signing.md).
- **Facts plainly visible in code, helper locations**: Nowhere; search the
  code instead.

- **[worker-thread](.omp/rules/worker-thread.md)**: Read when editing desktop
  printer/API access, event bridge, blocking calls, stop/join/quit.
- **[posnet-wire-encoding](.omp/rules/posnet-wire-encoding.md)**: Read when
  editing Posnet frame text/bytes, Windows-1250 encoding/decoding.
- **[novitus-wire-encoding](.omp/rules/novitus-wire-encoding.md)**: Read when
  editing Novitus native frame text/bytes and Windows-1250 fields.
- **[elzab-wire-encoding](.omp/rules/elzab-wire-encoding.md)**: Read when
  editing ElzabSTX frames, configured code pages or fiscal completion.
- **[writing-tests](.omp/rules/writing-tests.md)**: Read when editing test
  modules, shared testing harnesses or simulator.
- **[telemetry](.omp/rules/telemetry.md)**: Read when editing logging/tracing,
  error capture, Sentry setup, transport or scrubbing.

## Crates and boundaries

One Cargo workspace; dependency versions and lints live only in the root
`Cargo.toml`; crates use `workspace = true`.

- **`crates/fiscal` / `fiscal`**: Shared fiscal values, capability-driven receipt
  validation, `Driver` trait and `Device`/`Capabilities`, report/journal
  contracts, Windows-1250 codec and serial links.
- **`crates/posnet` / `posnet`**: CP1250 frames, sessions, preflight,
  receipt/report commands; feature `simulator` provides the printer peer used
  by tests.
- **`crates/novitus` / `novitus`**: Native ESC-P driver, strict model/firmware
  profiles, same-session preflight and feature-gated offline simulator.
  Unqualified capabilities are disabled in production.
- **`crates/elzab` / `elzab`**: Native ElzabSTX ONLINE candidate profiles,
  same-session read-only preflight and feature-gated offline simulator.
  Zero-total receipts and company footers stay disabled; accepted daily reports
  remain uncertain until human resolution.
- **`crates/agent` / `agent`**: Holuto `/agent` client, durable
  journal/recovery, settings/history and single `Worker`; feature `testing`
  exposes all three printer simulators and the loopback Holuto fake through
  the production client.
- **`crates/desktop` / `workshop`**: Polish Slint UI/tray, CLI, pairing,
  per-user autostart and Sentry lifecycle/privacy; feature `simulator` uses
  separate local state.

Fiscal invariants and worker ownership are hard requirements in
[`.omp/RULES.md`](.omp/RULES.md). Agent `testing` and desktop `simulator` are
test/development features rejected without debug assertions; posnet's simulator
feature alone has no release guard. Never validate behavior with a real printer.
The debug-only `cfg(workshop_acceptance)` is not a Cargo feature; its commands
and human-only qualification procedure live in
[hardware acceptance](docs/testing/hardware-acceptance.md).

## Code conventions

- A justified lint exception is `#[expect(lint, reason = "…")]` at the call
  site, never `allow`.
- Modules are `foo.rs` + `foo/`, never `mod.rs`; unit tests live in
  `foo/tests.rs`. `macos_quit` is the harness-free native AppKit exception.
- No `pub use`: import items from the module that defines them.
- Errors callers match on are `thiserror` enums; their `Display` is the Polish,
  operator-facing text.
- Prefer `rtk cargo …`; no `rtk` installed → plain `cargo …`.

## API contract

The backend is the Rust project `../backend`, never the legacy `../api`. Its
generated `openapi/agent.json` is the `/agent` contract authority. After
changing backend `/agent`, run `cargo run -p holuto -- openapi` there, then
`bash scripts/openapi-sync.sh` here (`BACKEND_DIR` overrides the default
`../backend`) and commit the `openapi/` diff. Rust wire types are generated into
`OUT_DIR` at build time, not committed. Deploy the backend contract first;
vendoring/generation does not compare the live API with the schema. See
[compatibility](docs/testing/compatibility.md).

## Tests

Plan each change from [the feature checklist](docs/testing/checklist.md) and the
changed module's invariants. The
[writing-tests rule](.omp/rules/writing-tests.md) covers
placement/harness/fixtures; [testing strategy](docs/testing/README.md) explains
what earns a test its place. Use nextest, simulator + HTTP fake, real
application callbacks and capturing telemetry transport. Never hide races with
sleeps/retries. A bug fix must leave a test that fails without it.

## Verify

Subagents skip checks below unless their assignment explicitly asks for one. The
integrator runs checks once after slices land, from the repository root:

```sh
# Unstable rustfmt options; builds stay on the pinned stable toolchain
cargo +nightly-2026-10-06 fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
```

During scoped development, select the changed crate and its dependents:

```sh
cargo nextest run --workspace --all-features -E 'rdeps(fiscal)'
cargo nextest run --workspace --all-features -E 'rdeps(posnet)'
cargo nextest run --workspace --all-features -E 'rdeps(novitus)'
cargo nextest run --workspace --all-features -E 'rdeps(elzab)'
cargo nextest run --workspace --all-features -E 'rdeps(agent)'
cargo nextest run --workspace --all-features -E 'package(workshop)'
```

These filters select tests, not hardware prerequisites. Default nextest excludes
Windows `com_pair`; CI includes it with a configured signed virtual pair. Linux
`session_bus` tests need `dbus-run-session` and run serially. See
[platform commands](docs/testing/platforms.md); all five native targets and
packages matter, and default production features must compile alongside
all-feature tests.

Before finishing dependency changes: `cargo deny --locked check` and
`cargo shear --deny-warnings`. For fiscal snapshots,
`cargo insta test --test-runner nextest --review -p posnet --all-features`, read
each diff, then rerun ordinary nextest (the insta collector can force snapshots
to pass). Read [workflows](docs/workflows.md) before a PR: it holds diff
mutation policy, worker stress runs, local coverage, Sentry/debug artifacts and
parallel ownership.
