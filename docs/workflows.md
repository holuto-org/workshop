# Workflows

## Finishing a change

Run once on the integrated change, from the repository root. Never run the app
against a real printer as verification.

```sh
cargo +nightly-2026-10-06 fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
# Include staged, unstaged and committed product changes against the PR base:
git diff origin/master > /tmp/workshop-changes.diff
cargo mutants --all-features --in-diff /tmp/workshop-changes.diff \
  -p <changed-package>
```

Use `fiscal`, `posnet`, `novitus`, `elzab`, `agent` or `workshop` as the changed
package; repeat mutation runs for each changed product package.
`.cargo/mutants.toml` selects nextest and
workspace tests so dependent-crate scenarios can kill driver/agent mutants; its
`mutants` profile excludes unavailable COM-pair tests. It excludes
simulator/testing fixtures, not fiscal production behavior. If `/tmp` is
RAM-backed, set `TMPDIR` to a disk-backed scratch directory.

No observable mutant of changed product code may survive. Add the missing
boundary assertion, delete code with no observable effect, or document a
genuinely equivalent/unreachable mutation in the configuration with its reason.
Do not exclude a hard-to-test safety path to make the run green. Inspect
timed-out, unviable and baseline failures separately.
[cargo-mutants' diff documentation](https://mutants.rs/in-diff.html) warns that
changing/removing only tests selects no mutants: widen to the affected
package/full safety path when reducing tests or changing shared harnesses.

Additional finish evidence by changed area:

- **Binding receipt/report output**:
  `cargo insta test --test-runner nextest --review -p posnet --all-features`,
  read each diff against the protocol, then rerun ordinary nextest.
  Select the relevant driver (`posnet`, `novitus` or `elzab`).
- **Worker, race, time or shared state**: The scoped nextest stress command
  below; ten repetitions with no flakes.
- **Recovery/numeric properties**: Increase `PROPTEST_CASES` for the affected
  model/property; commit real regression seeds with the fix.
- **Dependencies**: `cargo deny --locked check` and
  `cargo shear --deny-warnings`; all native/package targets.
- **Backend `/agent` contract**: Regenerate backend OpenAPI, run
  `bash scripts/openapi-sync.sh`, review/commit `openapi/` diff; compatibility
  scenarios through production client.
- **Platform/persistence/package**: Native tests and
  `bash packaging/build.sh` then `bash packaging/verify.sh` on affected
  OS/architecture.
- **Rust module docs**:

  ```sh
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features \
    --document-private-items
  ```

- **Markdown links**: `lychee --offline AGENTS.md README.md docs .omp llms.txt`;
  online/fragment checks when available.
- **Driver profiles/acceptance**: Run ordinary all-feature tests first to prove
  production gates, then the separate cfg-gated suite and release rejection
  from [hardware acceptance](testing/hardware-acceptance.md). CI runs that
  simulator-only acceptance suite in its own job. Never use acceptance flags
  for packaging or substitute automated checks for supervised hardware evidence.

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

For snapshots, `cargo insta review` reviews pending changes; never
blanket-accept unseen fiscal output. The
[`cargo insta test` command](https://insta.rs/docs/cli/) can force snapshots to
pass to collect changes, so it is **not** the final verification run. Commit
accepted snapshots, not pending `.snap.new` files. Fixed inputs should remove
nondeterminism; do not redact amounts, VAT, text or command order.

Coverage is a local diagnostic only:

```sh
rustup component add llvm-tools
cargo llvm-cov nextest --workspace --all-features --html
```

The HTML report is under `target/llvm-cov/html`. Use it to find unexecuted error
branches, not to claim correctness or meet a percentage. Install project tools
with
`cargo binstall cargo-nextest cargo-mutants cargo-llvm-cov cargo-insta lychee`
if cargo-binstall is available, otherwise use their documented installation
procedure. No weekly coverage/mutation workflow is required.

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

One integrator owns the shared contract, dependency versions/lockfile and final
reconciliation. Assign every file to one slice; agree simulator/fake/worker
event APIs before callers change. Each agent reads a shared brief with
ownership, invariants and acceptance evidence; it asks the owner for cross-slice
edits, never writes around them. Shared-tree agents preserve other agents' and
the user's edits; separate worktrees exchange deliberate patches, not
destructive resets.

Slice agents do not run workspace builds/tests/formatters while siblings are
mid-edit. If an assignment explicitly requests a slice check, use a distinct
`CARGO_TARGET_DIR=target/slice-<name>` and only its assigned crate/files. The
integrator runs integrated verification once after changes land, including
default production features, platform-specific checks and packages. No agent may
touch a real printer. Hand-off lists files, regressions, public API changes,
stale documentation, checks actually run and blockers; do not call unexecuted
checks passing.
