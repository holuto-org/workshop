# Workflows

## Finishing a change

Choose evidence from the changed behavior and the
[testing checklist](testing/checklist.md), then use the applicable
[Verify commands](../AGENTS.md#verify) on the integrated change. Documentation
alone needs documentation checks, not a Rust rebuild or package launch.
Report commands, scope and results, and name required evidence not obtained.
Never run the app against a real printer as verification.

Additional evidence by changed area:

- **Binding receipt/report output**: Review affected snapshots against the
  protocol using the procedure below, then rerun ordinary nextest.
- **Worker, race, time or shared state**: Repeat the affected controlled
  concurrency tests with nextest stress mode below.
- **Recovery/numeric properties**: Run the affected model/property with more
  generated cases; retain real regression seeds as described below.
- **Dependencies**: `cargo deny --locked check` and
  `cargo shear --deny-warnings`, plus affected callers' tests. Native/runtime
  or packaging dependencies also need evidence on the affected targets; a
  development-only dependency does not automatically require every package.
- **Backend `/agent` contract**: Follow the
  [sync procedure](../AGENTS.md#api-contract) and the
  [compatibility scenarios](testing/compatibility.md) through the production
  client.
- **Platform/persistence/package**: Obtain native evidence for affected OS
  behavior. Use [platform procedures](testing/platforms.md) for native test
  prerequisites and package build/verification commands, including clean-state
  launch requirements; a cross-build alone is not runtime proof.
- **Rust module docs**: Check rustdoc links/warnings for affected crates; for a
  workspace-wide documentation change:

  ```sh
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features \
    --document-private-items
  ```

- **Markdown**: With the tools installed, run
  `markdownlint-cli2 <changed files>` and
  `lychee --offline --include-fragments --no-progress <changed files>`.
  Offline success proves neither remote links nor command behavior; inspect
  changed public sources online when available, and report unavailable checks.
- **Driver profiles, preflight or acceptance gates**: Ordinary all-feature
  tests prove production gates; then run the simulator-only acceptance suite
  and release rejection from
  [hardware acceptance](testing/hardware-acceptance.md) in their own target
  directory. Never use acceptance flags for packaging or substitute automated
  checks for supervised hardware qualification.

For stress, select affected tests, not every agent/desktop test by default.
For example, replace the test-name substring in this command:

```sh
cargo nextest run -p agent -p workshop --all-features --stress-count 10 \
  -E 'test(<affected test-name substring>)'
```

Confirm the selection actually runs the intended tests. Ten repetitions are a
starting diagnostic batch, not proof that a race cannot occur. Stress is
[nextest's repetition mode](https://nexte.st/docs/features/stress-tests/), not
parallel copies of the same test or permission to tolerate flakes. A failure
needs investigation, not a passing rerun substituted for it. If setup failed,
correct it and report both attempts; a stale `TMPDIR` is not a race failure.
Watcher and COM-pair tests have native prerequisites; see
[platform procedures](testing/platforms.md).

For properties, `PROPTEST_CASES` raises the default generated-case count unless
the test overrides it. Check the affected test's configuration before claiming
an expanded run. Scope the run to that test and allow for the durable model's
larger runtime and nextest timeout. More cases complement boundary examples and
independent oracles, not replace them. Keep
[persisted failure seeds](https://proptest-rs.github.io/proptest/proptest/failure-persistence.html)
from real failures with the regression fix. If a strategy changes, preserve a
concrete regression too: the same seed need not generate the original input.

For snapshots, run ordinary `cargo nextest run -p <driver> --all-features`
(`posnet`, `novitus` or `elzab`) to expose mismatches. Outside CI, file snapshots
write `.snap.new`; inline snapshots, such as Elzab's, write `.pending-snap`.
CI detection disables pending output by default. Review only snapshots owned
by the change, using absolute `.snap` paths for file snapshots:

```sh
cargo insta review --manifest-path crates/<driver>/Cargo.toml \
  --snapshot /absolute/path/to/owned.snap
```

For inline snapshots, use the absolute Rust source path followed by `:line`
instead of a `.snap` path. `cargo insta pending-snapshots` lists pending entries.
Repeat `--snapshot` for more entries. Crate scope alone does not protect another
session's pending snapshots. Never blanket-accept fiscal output or reject/delete
another session's pending files. The
[`cargo insta test`](https://insta.rs/docs/cli/) command forces snapshots to pass
by default and may remove old pending output; it is not final verification.
After review, rerun ordinary nextest without snapshot-update/force-pass
overrides. Commit accepted `.snap` files or inline Rust changes, not pending
files. Fixed inputs should remove nondeterminism; do not redact amounts, VAT,
text or command order.

Coverage is an optional local diagnostic, not a finish gate. Install Rust's
`llvm-tools-preview` component for the project toolchain and `cargo-llvm-cov`:

```sh
cargo llvm-cov nextest --workspace --all-features --html
```

The HTML report defaults to `target/llvm-cov/html`. Scope to affected crates
when useful, and isolate coverage builds from concurrent checks. Use the report
to find unexecuted error branches, not to claim correctness or meet a percentage.
[CI](ci.md) owns automated checks and tool versions, some pinned only to a major;
the [platform guide](testing/platforms.md) owns native prerequisites.

## Sentry release check

[CI](ci.md#release-telemetry-artifacts) defines release identity, configuration
and matching debug/source uploads. A new workspace version publishes
automatically after the configured jobs succeed; an already published version
is checked but not republished. This procedure is not an extra approval gate.
When setting up or changing release telemetry, verify the project/DSN, opt-out
and the server-side privacy settings described in [telemetry](telemetry.md).
Inspect debug upload results for each target and confirm that symbols and the
released package came from the same binaries.

With explicit approval, a synthetic event from an isolated release-test
environment may confirm readable function/file/line frames, release association
and, when relevant, backend trace linkage. Do not use live customer data,
credentials or a configured printer, and never crash the production app during
a fiscal operation. Local capturing transports do not send such a probe.
Structured-envelope and shutdown tests do not prove native crash capture; the
Linux crashing-child regression checks dump placement and directory privacy.
Neither proves remote delivery or server-side symbolication.
[Sentry's source-context guide](https://docs.sentry.io/platforms/rust/source-context/)
explains symbol/source artifacts; [telemetry](telemetry.md) states what is sent.

## Parallel agent work

This section covers one integrator and its slice agents. Independent sessions
sharing the checkout follow [shared checkout](../AGENTS.md#shared-checkout).

One integrator owns shared contracts, dependency versions/lockfile and final
reconciliation. Give each slice explicit file ownership, invariants and
acceptance evidence. Agree changes to shared simulator/fake/worker APIs before
callers change; only add dependencies actually needed by the change. Ask the
owner for cross-slice edits rather than writing around them. Shared-tree agents
preserve other agents' and the user's edits; separate worktrees exchange
deliberate patches, not destructive resets.

Slice agents skip builds/tests/lints/formatters unless their assignment
explicitly requests a check. When it does, coordinate a stable read set,
including dependencies, or run in an isolated worktree. Use a distinct
`CARGO_TARGET_DIR=target/slice-<name>` and check only assigned crates/files.
A separate target directory isolates build artifacts, not source changes or
other outputs. Snapshot review and
[package checks](testing/platforms.md#package-checks) write outside it; run them
only with exclusive ownership of their outputs or in an isolated copy.

The integrator runs applicable integrated verification after slices land;
scoped diagnostic runs do not replace the configured CI suites or native
evidence for affected platforms. No agent may touch a real printer. Hand-off
lists changed files, regressions, external contract changes, cross-document
requests, checks actually run and blockers; never call unexecuted checks passing.
