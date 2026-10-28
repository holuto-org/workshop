# Holuto Workshop

Rust/Slint tray application and `workshop` CLI for workshop PCs connected to a
supported Posnet, Novitus or Elzab online fiscal printer. It prints only Holuto
sales/advances queued in the Holuto web app, plus fiscal reports: no receipt
list, manual queuing or receipt editor and no separate agent service. Closing
to the tray keeps processing; starting is explicit
unless the user opts into autostart. Hard requirements are in
[`.omp/RULES.md`](.omp/RULES.md).

## Where guidance lives

Each fact has one home: the first item that fits. When a change makes guidance
untrue, update its home in the same change. This repository is public: never
commit references to local-agent plans, private backend source files or
gitignored paths (see `.gitignore`) other than build output.

- **A machine can decide it**: lints, tests, `deny.toml`, `.config/nextest.toml`
  and CI. Do not restate checkable rules as prose.
- **Every agent, every request**: [`.omp/RULES.md`](.omp/RULES.md); keep it
  tiny.
- **Every task: product, boundaries, conventions, commands**: this file.
- **Judgment for one area**: a rule in [`.omp/rules/`](.omp/rules/) (index
  below).
- **Crate/module ownership, invariants, decisions**: `//!` / `///` on that
  code; read them before changing it. A local `//` comment says why, not what.
- **Why a dependency is used**: the comment above it in the root `Cargo.toml`.
  Read it before proposing to drop or replace the crate.
- **Review priorities**: [`.omp/WATCHDOG.md`](.omp/WATCHDOG.md).
- **Procedures and evidence**: [testing](docs/testing/README.md),
  [workflows](docs/workflows.md), [CI](docs/ci.md),
  [telemetry](docs/telemetry.md), [fiscal contract](docs/fiscal-contract.md),
  [translations](docs/localization.md).
- **End-user and LLM install material**: [README](README.md),
  [install guide](docs/install-with-ai.md) and [`llms.txt`](llms.txt).
- **Facts plainly visible in code**: nowhere; search the code.

## Rules

omp injects a rule when you edit matching files (some rules also when you read
them); other agents read the rule before changing that area.

- **[worker-thread](.omp/rules/worker-thread.md)**: asynchronous desktop state,
  operator edits, persistence-dependent actions and native shutdown.
- **[holuto-api](.omp/rules/holuto-api.md)**: `/agent` client, OpenAPI
  contract, loopback Holuto fake.
- **[fiscal-evidence](.omp/rules/fiscal-evidence.md)**: vendor evidence versus
  host policy, hardware qualification and completion/refusal certainty.
- **[writing-tests](.omp/rules/writing-tests.md)**: test placement, harnesses,
  simulators and per-kind conventions.
- **[telemetry](.omp/rules/telemetry.md)**: logging/tracing, error capture,
  Sentry setup, transport and scrubbing.
- **[installers-packaging](.omp/rules/installers-packaging.md)**: installer trust,
  fiscal boundaries and native package compatibility.
- **[documentation](.omp/rules/documentation.md)**: evidence, end-user contracts
  and incoming references.
- **[localization](.omp/rules/localization.md)**: safety-critical translation
  meaning and presentation-versus-fiscal boundaries.

## Crates and boundaries

One Cargo workspace; dependency versions and lints live only in the root
`Cargo.toml`; crates use `workspace = true`.

- **`crates/i18n`**: embedded Fluent catalogs, deferred operator messages and
  system-language negotiation; UI rendering takes an explicit language.
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
  settings/status and the single `Worker`.
- **`crates/desktop`** (binary `workshop`): multilingual Slint UI/tray, CLI,
  pairing, per-user autostart and Sentry lifecycle/privacy.

Each driver's `simulator` feature provides its offline peer. Agent `testing`
enables those features and exposes the loopback Holuto fake; desktop `simulator`
runs all three peers and a loopback Holuto with sample queued jobs that print
once processing starts, with
state in a separate `workshop-simulator` directory; it never reaches a real
Holuto. Agent `testing` and desktop `simulator` fail to compile without debug
assertions; the drivers' `simulator` features have no release guard of their
own. The debug-only `cfg(workshop_acceptance)` is not a Cargo feature; see
[hardware acceptance](docs/testing/hardware-acceptance.md).

### External surfaces

The crates are workspace-internal (`publish = false`) and no other repository
depends on them: `pub` is not external API and needs no search outside this
repository. The simulators' and
`agent::testing`'s `pub` items are a shared harness: check `agent::testing`
and desktop tests before calling one dead. External surfaces are the `/agent`
API, the files in the state directory, the CLI and its JSON
([install guide](docs/install-with-ai.md)), the telemetry allowlist and the
release assets the install scripts fetch.

Before the first Workshop release, change stored formats, CLI and package
metadata by clean cutover: no migrations, aliases, legacy names or package
replacement metadata. After it they are contracts.

## Code conventions

- Errors callers match on are `thiserror` enums. Catalog-backed `Display` stays
  Polish for CLI and persisted diagnostics; the UI renders deferred
  `i18n::Message` values in its selected language. Follow the
  [translation procedure](docs/localization.md) for wording changes.
- Comments and docs cite only public sources, such as the vendor spec pages
  listed in the [fiscal contract](docs/fiscal-contract.md); never private
  repositories, their file/line numbers or unpublished documents.
- `posnet`, `novitus` and `elzab` share one shape (session, preflight, profile,
  simulator). A bug or simplification found in one driver: check the other
  two, and change all three or say why one differs.
- For inline Rust changes, run scoped clippy after the first coherent edit,
  not only at the end. Parallel slices defer checks to their integrator.

### Simplifying and deleting

These look removable but are deliberate:

- fail-closed error branches and pre-dispatch validation in driver and agent
  fiscal paths, even when they look unreachable; never trade one for a
  panicking API (a worker panic leaves an uncertain fiscal outcome);
- redacting `Debug`/`Display` impls and telemetry allowlist entries: privacy
  guards, changed only with a canary test;
- driver profile slots awaiting [hardware acceptance](docs/testing/hardware-acceptance.md);
- derives and items used only by tests, simulators or other crates' tests.

Before calling an item dead, check it with and without
`--cfg workshop_acceptance` and with `cargo check --workspace --all-targets
--all-features --keep-going`, so one failing crate cannot hide another's use.

## API contract

`openapi/agent.json` is the vendored `/agent` contract. After the backend
regenerates it, run `bash scripts/openapi-sync.sh` and commit the `openapi/`
diff. `BACKEND_DIR` selects the backend checkout (default `../backend`).
The script copies that checkout's exported schema without regenerating it:
confirm it is on the branch that will be deployed. A feature needing a new
`/agent` field starts in the backend and cannot be finished here alone.
After a sync, the `api::contract` tests check the hand-written wire types in
`agent::api::model` against it.
Nothing compares the live API with the schema: deploy the backend first. See
[fiscal contract](docs/fiscal-contract.md) and
[compatibility](docs/testing/compatibility.md).

## Tests

Plan each change from [the feature checklist](docs/testing/checklist.md) and the
changed module's invariants; [the testing guide](docs/testing/README.md) explains
what earns a test its place. A bug fix leaves a test that fails without it; see
it fail for the stated reason before fixing.

Brands deliberately differ: before reporting a behavior as a bug, check the
[fiscal contract](docs/fiscal-contract.md) and the module docs. A finding that
depends on unmeasured printer behavior is reported as suspected, not fixed (see
[open hardware questions](docs/testing/hardware-acceptance.md#open-hardware-questions)).

## Shared checkout

Other agent sessions and the maintainer's editor edit and commit this checkout
while you work. Changes and commits you did not make are someone else's work in
progress: never revert, stash, reset, stage, commit or reformat them, and do
not attribute them to the user.

- Format only files you changed:
  `rustfmt +nightly-2026-10-06 --config skip_children=true <files>`.
- Lint and test only your crates:
  `cargo clippy -p <crate> --all-targets [--all-features] --no-deps -- -D warnings`
  and `cargo nextest run -p <crate> --all-features`. `--workspace` filters
  still compile every test binary, so a break elsewhere stops them.
- A compile failure in code you did not touch: report it, do not fix or wait
  for it. Verify your change in a clean copy (`git worktree add --detach
  /tmp/workshop-<task>`, apply your diff, remove the worktree afterwards).
- Every copy, probe or `RUSTFLAGS` variant uses its own `CARGO_TARGET_DIR`
  (for example `target/acceptance`), never the shared `target/`.
- Experiments never edit tracked files: trial code edits go in a copy, probes in
  a throwaway crate outside the repository that depends on workspace crates by
  path. Probes under `crates/*/tests` or `examples` get swept into commits and
  break workspace clippy.

## Verify

Select checks from the changed behavior and
[finishing a change](docs/workflows.md#finishing-a-change). Documentation-only
changes need Markdown/link checks, not a Rust rebuild or package launch.
For Rust changes, cover affected crates and dependents with production and
all-feature builds/tests. Slice agents defer checks until integration.

The full workspace batch below is for a clean integrated tree or workspace-wide
changes. In a shared checkout, use the scoped crate and changed-file formatting
commands above instead:

```sh
# Unstable rustfmt options; builds stay on the pinned stable toolchain
cargo +nightly-2026-10-06 fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --all-features
```

Scoped runs select a changed crate and its dependents (in a shared checkout,
use the `-p` forms above):

```sh
# or rdeps(posnet), rdeps(novitus), rdeps(elzab), rdeps(agent)
cargo nextest run --workspace --all-features -E 'rdeps(fiscal)'
cargo nextest run --workspace --all-features -E 'package(workshop)'
```

A change is done when its area's evidence from
[finishing a change](docs/workflows.md#finishing-a-change) also exists.
Report every check you did not run and why.

For native dependencies, COM-pair/session-bus setup and changes this host cannot
compile or run, follow [platform procedures](docs/testing/platforms.md).
Do not treat Linux evidence as Windows or macOS coverage; report native checks
not run and rely on CI's native matrix.

User-level evidence an agent can produce comes from the end-to-end scenarios in
`crates/desktop/src/application/tests/end_to_end.rs` (real application, worker,
simulator and loopback Holuto). The GUI smoke run needs a display, so it is a
human check, against simulators and the built-in sample Holuto, never a
printer:

```sh
cargo run -p workshop --features simulator          # Posnet peer
WORKSHOP_SIMULATOR=novitus cargo run -p workshop --features simulator   # or elzab
```
