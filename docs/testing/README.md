# Testing Holuto Workshop

Tests protect irreversible fiscal records, not a test count. Start here when
planning or reviewing a change; use the [feature checklist](checklist.md) to
select its required evidence.

- **[Fiscal safety](fiscal-safety.md)**: Exact output, delivery guarantees,
  journal, recovery and fault injection
- **[Concurrency](concurrency.md)**: Worker ownership, GUI responsiveness,
  duplicate actions and shutdown
- **[Security](security.md)**: Token, TLS, proxies, private files and telemetry
- **[Compatibility](compatibility.md)**: Stored formats, `/agent` and dependency
  upgrades
- **[Platforms](platforms.md)**: Serial pairs, process tests, tray and packages
  on each OS/architecture
- **[UI](ui.md)**: Real Slint application through the testing backend
- **[Hardware acceptance](hardware-acceptance.md)**: Human-only model/firmware
  qualification, separate debug cfg and production release gates

## Why tests exist

A test earns its place by catching a plausible user-visible failure: assert
behavior at the consumer boundary (printer frames and executed effects, HTTP
requests/results, durable files, visible UI and worker events), fake only
outside systems, and never hide ordering with sleeps or flakes with retries.
Coverage, assertion counts and tests per function are not proof. The
[writing-tests rule](../../.omp/rules/writing-tests.md) owns harnesses,
placement and fixtures.

## Rank what must never break

1. **Double print or reprint.** A timeout, restart, second click or API replay
   must never execute a fiscal command twice.
2. **Lost fiscal evidence or an uncertainty that fails to block.** Persist
   intent before dispatch; retain completed/ambiguous evidence across restarts
   until the appropriate explicit human resolution.
3. **Wrong amount, VAT or text.** Exact grosze, half-up rounding at the
   documented step, per-line VAT slots, unchanged Polish text, payments and
   deductions.
4. **Unverified printer state.** Verify identity and readiness in the session
   that executes, including fresh VAT mapping; never switch devices while
   blocked.
5. **Secret or personal-data disclosure.** Token, customer/NIP/receipt data and
   host information must not enter structured logs, errors or telemetry. Native
   dumps have a separate disclosed privacy risk; see
   [telemetry](../telemetry.md).
6. **Frozen UI or abandoned worker.** The event loop stays responsive and
   cooperative quit finishes the operation before joining.
7. **Corrupt local state on one OS.** Journal durability, private settings,
   process exclusion and stored formats must work on each supported platform.
8. **Availability.** Bound network bodies; recover from handled outages
   without stale status, busy loops or false fiscal certainty.

Prioritize stronger fault, model and concurrency tests for the higher ranks.
This is risk-based engineering, not a claim of safety certification.
[NASA's fault-tolerance guidance](https://www.nasa.gov/wp-content/uploads/2015/04/tb-23-06-091923.pdf)
motivates off-nominal input/fault testing and warns that reboot alone does not
repair erroneous state. Our hard constraint is stricter than
hardware-in-the-loop guidance: never exercise a real fiscal printer.

## What a change must prove

Every invariant in the changed module's docs and every applicable
[checklist](checklist.md) row stays proven. A fixed bug needs a regression that
fails without the fix. Document a new decision before encoding it as an expected
result.

For a fiscal effect, prove success, certain refusal, uncertainty after
execution, restart, and resolution without replay. For an API call, prove
headers/body and each failure through the production client. For a refusal,
prove unchanged journal, device effects and server state as appropriate. For a
UI change, prove the real callback wiring as well as its displayed state. Format
changes update the literal fixtures (after the first release they also read the
previous ones); platform changes need the platform that implements them.

## Choosing the test

Use the cheapest boundary that proves the whole guarantee, not a private helper
below it.

- **Driver preflight, command order, waits and device failures**: Driver scenario
  through the simulator; assert frames, actual simulated effects and journal
  callbacks
- **Claim, print, result, recovery, reports and worker**: Agent scenario through
  loopback Holuto fake + simulator + real state directory
- **Controls, confirmations, event delivery, close/quit**: Real application
  through Slint testing backend, accessible labels and real worker where involved
- **Holuto queue to receipt, result and visible state**: End-to-end scenario
  combining the three boundaries; one per essential legal path, not every
  parser case
- **Parser, formatter, date or value boundary**: Named unit table with exact
  accepted/rejected values
- **Large numeric/text space**: `proptest` against an independent
  integer/reference oracle, round trip or documented relation
- **Journal/recovery lifecycle**: `proptest-state-machine` reference model,
  invariants after **every** transition, plus real boundary scenarios
- **Binding receipt/report output**: Reviewed `insta` snapshot of fixed printer
  output, checked against the protocol; never a substitute for semantic assertions
- **Serial transport, locks, Known Folder, session bus**: Platform-gated test on
  the actual OS, using pty/com0com/fake bus, not real hardware
- **Release artifact**: Package metadata/dependencies/resources check and
  isolated headless launch of the packaged application

[Proptest's state-machine guide](https://proptest-rs.github.io/proptest/proptest/state-machine.html)
describes sequence shrinking and per-transition invariants. It currently
generates sequential models; it does not replace controlled thread races.
[Slint's testing backend](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/)
proves accessible behavior without rendering pixels, not real compositor/tray
integration.

## Measuring the suite

- **Mutation testing:** a changed condition/value must make a test fail. Resolve
  survivors with a missing boundary assertion or delete behaviorless code; see
  [finishing a change](../workflows.md#finishing-a-change). A mutant that fails
  to build is not proof. Diff-only mutation misses test-only regressions; widen
  the run when deleting tests
  ([cargo-mutants](https://mutants.rs/in-diff.html)).
- **Coverage:** local `cargo-llvm-cov` reports identify unexecuted paths; a
  covered line is not a proven one. No percentage target or coverage workflow.
- **Stress:** repeat controlled worker/race tests with nextest; repetitions
  reveal flakes but never justify sleeps/retries. Run generated models with more
  cases when changing recovery.
- **Resource bounds:** assert exact body limits and bounded work or output,
  not wall-clock speed on a shared runner.

## Tests that prove nothing

Delete constants/default/derive restatements; private-helper tests already
proven at a boundary; mocks of our own code; status/count-only assertions when
exact output is known; broad `contains` error checks; duplicate examples with no
new boundary; fixtures that bypass the transition being tested; and snapshots
accepted without reading them. A no-retry test must show executed effects and
retained evidence, not merely that one helper was invoked. A GUI test that wires
replacement callbacks proves those replacements, not the app.
