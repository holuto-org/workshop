# Workflows

## Finishing a change

Run [the Verify commands](../AGENTS.md#verify) on the integrated change. Never
run the app against a real printer as verification.

Additional finish evidence by changed area:

- **Binding receipt/report output**:
  `cargo insta test --test-runner nextest --review -p <driver> --all-features`
  (`posnet`, `novitus` or `elzab`), read each diff against the protocol, then
  rerun ordinary nextest.
- **Worker, race, time or shared state**: The scoped nextest stress command
  below; ten repetitions with no flakes.
- **Recovery/numeric properties**: Increase `PROPTEST_CASES` for the affected
  model/property; commit real regression seeds with the fix.
- **Dependencies**: `cargo deny --locked check` and
  `cargo shear --deny-warnings`; all native/package targets.
- **Backend `/agent` contract**: Follow the
  [sync procedure](../AGENTS.md#api-contract); prove compatibility scenarios
  through the production client.
- **Platform/persistence/package**: Native tests and
  `bash packaging/build.sh` then `bash packaging/verify.sh` on affected
  OS/architecture.
- **Rust module docs**:

  ```sh
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features \
    --document-private-items
  ```

- **Markdown**: `npx -y markdownlint-cli2 <changed files>` and
  `lychee --offline --include-fragments --no-progress <changed files>`;
  online checks when available.
- **Driver profiles, preflight or acceptance gates**: Ordinary all-feature
  tests prove production gates; then run the simulator-only acceptance suite
  and release rejection from
  [hardware acceptance](testing/hardware-acceptance.md) in their own target
  directory. CI runs that suite in its own job. Never use acceptance flags for
  packaging or substitute automated checks for supervised hardware evidence.

Worker stress command:

```sh
cargo nextest run --all-features --stress-count 10 \
  -E 'package(agent) | package(workshop)'
```

Stress is
[nextest's repetition mode](https://nexte.st/docs/features/stress-tests/), not
permission to tolerate flakes. Linux watcher tests launch their own private bus
and require `dbus-run-session`; Windows COM tests require the `ci` profile and
a configured virtual pair. See [platform procedures](testing/platforms.md).

For snapshots, `cargo insta review --manifest-path crates/<driver>/Cargo.toml`
reviews pending changes for that crate only; `--workspace` or blanket accept
also takes other sessions' pending output, so never use them for fiscal snapshots.
The [`cargo insta test`](https://insta.rs/docs/cli/) command can force
snapshots to pass to collect changes, so it is **not** the final verification run.
Commit accepted snapshots, not pending `.snap.new` files. Fixed inputs should
remove nondeterminism; do not redact amounts, VAT, text or command order.

Coverage is a local diagnostic only:

```sh
rustup component add llvm-tools
cargo llvm-cov nextest --workspace --all-features --html
```

The HTML report is under `target/llvm-cov/html`. Use it to find unexecuted error
branches, not to claim correctness or meet a percentage. Install project tools
with `cargo binstall cargo-nextest cargo-llvm-cov cargo-insta lychee` if
cargo-binstall is available, otherwise use their documented installation
procedure. No weekly coverage workflow is required.

## Sentry release check

[CI](ci.md) produces `workshop@<version>` and matching debug files/source
bundles. Before a release, verify the project/DSN, opt-out and privacy settings;
keep dump attachment retention disabled unless a deliberate privacy decision
changes it. Verify debug upload logs for **each** target and that the released
package comes from those same binaries. Release mode retains build debug
information before distribution stripping.

With explicit approval, a synthetic event from an isolated release-test
environment may confirm readable function/file/line frames and release
association; never crash the production app during a fiscal operation and never
send customer data as a probe. Local automated tests capture structured Sentry
envelopes and exercise transport shutdown; they do not prove native crash capture.
Confirm trace linkage to the backend without leaking a URL or
token.
[Sentry's source-context guide](https://docs.sentry.io/platforms/rust/source-context/)
explains symbol/source artifacts; [telemetry](telemetry.md) states what is sent.

## Parallel agent work

This section covers one integrator and its slice agents. Independent sessions
sharing the checkout follow [shared checkout](../AGENTS.md#shared-checkout).

One integrator owns the shared contract, dependency versions/lockfile and final
reconciliation. Assign every file to one slice; agree simulator/fake/worker
event APIs and add new dependencies before callers change. Each agent reads a
shared brief with ownership, invariants and acceptance evidence; it asks the
owner for cross-slice edits, never writes around them. Shared-tree agents
preserve other agents' and the user's edits; separate worktrees exchange
deliberate patches, not destructive resets.

Slice agents do not run workspace builds/tests/formatters while siblings are
mid-edit. A separate target directory does not isolate source files: if an
assignment explicitly requests a slice check, run it only when no sibling edits
the same crate (otherwise in a separate worktree), with a distinct
`CARGO_TARGET_DIR=target/slice-<name>`, on the assigned crate/files only. The
integrator runs integrated verification once after changes land, including
default production features, platform-specific checks and packages. No agent may
touch a real printer. Hand-off lists files, regressions, public API changes,
stale documentation, checks actually run and blockers; do not call unexecuted
checks passing.
