# Testing Holuto Workshop

Tests protect irreversible fiscal records, not a test count. Start here to rank
the risk and choose a boundary; use the [feature checklist](checklist.md) and the
changed module's invariants to select the guarantees affected by a change.
The concern guides below describe evidence to seek, not a claim that the suite
already covers every case.

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

A test earns its place by catching a plausible failure of a supported guarantee.
Consumer boundaries include printer frames and executed effects, HTTP
requests/results, durable files, visible UI and worker events. Small value and
parser tests can prove their own contracts directly; not every guarantee needs
a worker or an end-to-end scenario. Coverage, assertion counts and tests per
function are not proof.

This guide owns risk and boundary selection. The
[writing-tests rule](../../.omp/rules/writing-tests.md) owns test construction,
placement and shared fixtures; [workflows](../workflows.md#finishing-a-change)
owns commands, snapshot review and regression-seed handling.

## Rank what must never break

1. **Double print or reprint.** A timeout, restart, second click or API replay
   must never redispatch the same fiscal operation. At-most-once dispatch is
   not a guarantee that a receipt printed.
2. **Lost fiscal evidence or uncertainty that fails to block.** Persist intent
   before dispatch; retain completed/ambiguous evidence across restarts.
   Recovery may settle a receipt from confirmed server state; unresolved
   uncertainty needs the appropriate human resolution, never replay.
3. **Wrong amount, VAT or text.** Exact grosze, half-up rounding at the
   documented step, per-line VAT slots, unchanged Polish text, payments and
   deductions.
4. **Unverified printer state.** Verify identity and readiness in the session
   that executes, including fresh VAT mapping; never switch devices while
   blocked.
5. **Secret or personal-data disclosure.** Keep tokens, customer/NIP/receipt
   data and host identity out of structured diagnostics; preserve the
   [telemetry allowlist and native-dump privacy distinction](../telemetry.md).
6. **Frozen UI or abandoned worker.** The event loop stays responsive and
   cooperative quit finishes the operation before joining.
7. **Corrupt local state on one OS.** Journal durability, private settings,
   process exclusion and stored formats must work on each supported platform.
8. **Availability.** Bound network bodies; recover from handled outages
   without stale status, busy loops or false fiscal certainty.

Prioritize fault, model and concurrency evidence where these risks are present,
not merely because a test technique is available. This is risk-based
engineering, not a claim of safety certification. Automated tests use offline
simulators and local fakes, never real fiscal printers or live Holuto.
Simulator traces establish modeled behavior, not measured printer behavior;
[physical qualification](hardware-acceptance.md) is a separate human-only task.

## What a change must prove

Preserve evidence for existing safety invariants. Identify which behavior the
change can affect, including callers and dependents, then retain or extend the
applicable [checklist](checklist.md) evidence at the cheapest sufficient boundary.
Do not add a new test per row or repeat an unchanged matrix just to satisfy a
test shape. A fixed bug needs a regression that fails without the fix; a new
expected behavior needs a documented decision, not an accidental implementation
detail promoted into a requirement.

When changing dispatch or recovery, keep the affected success, certain-refusal,
post-execution uncertainty, restart and resolution-without-replay paths proven;
use the [fiscal fault matrix](fiscal-safety.md#fault-matrix). A refusal's evidence
includes which device, journal and server effects did **not** occur. API
serialization and error handling need the production-client boundary; UI callback
or event-delivery changes need the real wiring, not just a displayed-state test.
When moving an input, assert its exact value at the downstream boundary too.
[Compatibility](compatibility.md) owns stored-format evidence and
[platforms](platforms.md) owns native evidence and isolation.

## Choosing the test

Use the cheapest boundary that proves the whole guarantee. These are choices,
not a requirement to use every technique for every change:

- **Driver preflight, command order, waits and device failures**: Driver scenario
  through the simulator; assert frames, actual simulated effects and journal
  callbacks
- **Claim, print, result, recovery, reports and worker**: Agent scenario through
  loopback `agent::testing::Holuto` + simulator + temporary on-disk state directory
- **Controls, confirmations, event delivery, close/quit**: Real application
  through Slint testing backend, accessible labels and real worker where involved
- **Holuto queue to receipt, result and visible state**: End-to-end scenario
  combining the three boundaries when their integration is the risk; keep parser
  cases at their smaller boundary
- **Parser, formatter, date or value boundary**: Named unit table with exact
  accepted/rejected values
- **Large numeric/text space**: `proptest` with an independent reference oracle
  or documented relation; a round trip alone cannot prove correct encoding
- **Lifecycle sequence interactions**: `proptest-state-machine` reference model
  plus real boundary scenarios; a model does not prove filesystem durability
- **Receipt/report wire output**: Reviewed `insta` snapshot of fixed printer
  frames, checked against the protocol; keep separate assertions for device effects
- **Serial transport, locks, Known Folder, session bus**: Platform-gated test on
  the actual OS, using pty/com0com/private bus rather than hardware or a live bus
- **Release artifact**: Package metadata/dependencies/resources and isolated
  packaged-application checks from the [platform guide](platforms.md#package-checks)

[Proptest's state-machine guide](https://proptest-rs.github.io/proptest/proptest/state-machine.html)
describes sequence shrinking and per-transition invariants. It currently
generates sequential models; it does not replace controlled thread races.
[Slint's testing backend](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/)
proves accessible behavior without rendering pixels, not real compositor/tray
integration.

## Measuring the suite

- **Coverage:** local `cargo-llvm-cov` reports help find unexecuted paths; a
  covered line is not a proven guarantee.
- **Stress:** repeat controlled worker/race tests to expose flakes; increase
  generated cases when sequence interactions are the risk. Neither repetition
  nor a passing rerun explains a failure.
- **Resource bounds:** assert the documented body, work or output limit rather
  than wall-clock speed on a shared runner. Deadline tests follow the
  [writing-tests rule](../../.omp/rules/writing-tests.md).

Run and review procedures live in
[workflows](../workflows.md#finishing-a-change); CI's actual checks live in
[CI configuration](../ci.md).

## Tests that prove nothing

Judge a test by what regression it can detect, not its size or whether it calls
a private helper. Defaults, enum values and literal bytes deserve tests when
they are stored-format, wire or user-visible contracts; merely restating an
initializer does not.

Before deleting an apparent duplicate, identify the guarantee and failure mode
it covers and where that evidence will remain. Replace weak assertions when
they miss the promised behavior: a success/count assertion alone cannot prove
exact output, one helper invocation cannot prove no replay after restart, and
a snapshot cannot prove the device effect. Fixtures that bypass the transition
under test, mocks replacing the promised production path and UI replacement
callbacks do not prove that path.
