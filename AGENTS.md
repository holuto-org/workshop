# Holuto Workshop

Rust/Slint tray application and `workshop` CLI for workshop PCs connected to a
supported Posnet, Novitus or Elzab online fiscal printer. It prints only Holuto
sales/advances and fiscal reports: no manual receipt editor and no separate
agent service. Closing to the tray keeps processing; starting is explicit
unless the user opts into autostart. Hard requirements are in
[`.omp/RULES.md`](.omp/RULES.md).

## Where guidance lives

Each fact has one home: the first item that fits. When a change makes guidance
untrue, update its home in the same change. This repository is public: never
commit references to gitignored paths (see `.gitignore`), local-agent plans or
private backend source files.

- **A machine can decide it**: lints, tests, `deny.toml`, `.config/nextest.toml`
  and CI. Do not restate checkable rules as prose.
- **Every agent, every request**: [`.omp/RULES.md`](.omp/RULES.md); keep it
  tiny.
- **Every task: product, boundaries, conventions, commands**: this file.
- **Judgment for one area**: a rule in [`.omp/rules/`](.omp/rules/) (index
  below).
- **Crate/module ownership, invariants, decisions**: `//!` / `///` on that
  code; read them before changing it. A local `//` comment says why, not what.
- **Review priorities**: [`.omp/WATCHDOG.md`](.omp/WATCHDOG.md).
- **Procedures and evidence**: [testing](docs/testing/README.md),
  [workflows](docs/workflows.md), [CI](docs/ci.md),
  [telemetry](docs/telemetry.md), [fiscal contract](docs/fiscal-contract.md).
- **End-user and LLM install material**: [README](README.md),
  [install guide](docs/install-with-ai.md) and [`llms.txt`](llms.txt).
- **Facts plainly visible in code**: nowhere; search the code.

## Rules

omp injects a rule when you read or edit matching files; other agents read the
file before changing that area.

- **[worker-thread](.omp/rules/worker-thread.md)**: desktop printer/API access,
  event bridge, blocking calls, stop/join/quit.
- **[durable-state](.omp/rules/durable-state.md)**: journal, store, settings,
  status, history and their stored formats.
- **[holuto-api](.omp/rules/holuto-api.md)**: `/agent` client, OpenAPI
  contract, loopback Holuto fake.
- **[posnet-wire-encoding](.omp/rules/posnet-wire-encoding.md)**,
  **[novitus-wire-encoding](.omp/rules/novitus-wire-encoding.md)**,
  **[elzab-wire-encoding](.omp/rules/elzab-wire-encoding.md)**: native frames,
  Windows-1250 text and fiscal completion per brand.
- **[writing-tests](.omp/rules/writing-tests.md)**: test modules, shared
  harnesses and simulators.
- **[telemetry](.omp/rules/telemetry.md)**: logging/tracing, error capture,
  Sentry setup, transport and scrubbing.
- **[documentation](.omp/rules/documentation.md)**: any Markdown or `llms.txt`.

## Crates and boundaries

One Cargo workspace; dependency versions and lints live only in the root
`Cargo.toml`; crates use `workspace = true`.

- **`crates/fiscal`**: shared fiscal values, capability-driven receipt
  validation, `Driver` trait, `Device`/`Capabilities`, journal/report contracts,
  Windows-1250 codec and serial links.
- **`crates/posnet`**: Temo Online frames, sessions, preflight and
  receipt/report commands.
- **`crates/novitus`**: native ESC-P driver with strict model/firmware
  profiles; unqualified capabilities are disabled in production.
- **`crates/elzab`**: native ElzabSTX ONLINE candidate profiles; zero-total
  receipts and company footers stay disabled; accepted daily reports remain
  uncertain until a person resolves them.
- **`crates/agent`**: Holuto `/agent` client, durable journal and recovery,
  settings/status/history and the single `Worker`.
- **`crates/desktop`** (binary `workshop`): Polish Slint UI/tray, CLI, pairing,
  per-user autostart and Sentry lifecycle/privacy.

Each driver's `simulator` feature provides its offline peer. Agent `testing`
exposes the simulators and the loopback Holuto fake; desktop `simulator` runs
all three peers with separate local state. Agent `testing` and desktop
`simulator` fail to compile without debug assertions; the drivers' `simulator`
features have no release guard of their own. The debug-only
`cfg(workshop_acceptance)` is not a Cargo feature; its human-only procedure is
in [hardware acceptance](docs/testing/hardware-acceptance.md).

## Code conventions

- A justified lint exception is `#[expect(lint, reason = "…")]` at the call
  site, never `allow`.
- Modules are `foo.rs` + `foo/`, never `mod.rs`; unit tests live in
  `foo/tests.rs`.
- No `pub use`: import items from the module that defines them.
- Errors callers match on are `thiserror` enums; their `Display` is the Polish,
  operator-facing text.

## API contract

`openapi/agent.json` is the `/agent` contract, vendored from the backend
repository (`../backend` by default). After a backend `/agent` change, run
`cargo run -p holuto -- openapi` there, then `bash scripts/openapi-sync.sh` here
(`BACKEND_DIR` overrides the path) and commit the `openapi/` diff. Wire types are
generated into `OUT_DIR` at build time. Nothing compares the live API with the
schema: deploy the backend first. See [fiscal contract](docs/fiscal-contract.md)
and [compatibility](docs/testing/compatibility.md).

## Tests

Plan each change from [the feature checklist](docs/testing/checklist.md) and the
changed module's invariants; [the testing guide](docs/testing/README.md) explains
what earns a test its place. A bug fix leaves a test that fails without it.

## Verify

Subagents skip these unless their assignment asks for a check; the integrator
runs them once after slices land, from the repository root:

```sh
# Unstable rustfmt options; builds stay on the pinned stable toolchain
cargo +nightly-2026-10-06 fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --all-features
```

Scoped runs select a changed crate and its dependents:

```sh
cargo nextest run --workspace --all-features -E 'rdeps(fiscal)'   # or posnet, novitus, elzab, agent
cargo nextest run --workspace --all-features -E 'package(workshop)'
```

Smoke-run the app against simulators, never a printer:

```sh
cargo run -p workshop --features simulator          # Posnet peer
WORKSHOP_SIMULATOR=novitus cargo run -p workshop --features simulator   # or elzab
```

Linux builds need `pkg-config` and fontconfig headers (`libfontconfig1-dev` /
`fontconfig-devel`). Default nextest excludes Windows `com_pair`; Linux
`session_bus` tests spawn their own `dbus-run-session` and run serially. See
[platforms](docs/testing/platforms.md): all five native targets and packages
matter.

Dependency changes also need `cargo deny --locked check` and
`cargo shear --deny-warnings`. [Workflows](docs/workflows.md) holds the finishing
evidence: mutation runs, snapshot review, worker stress, coverage, Sentry
release checks and parallel-agent ownership.
