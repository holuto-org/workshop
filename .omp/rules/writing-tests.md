---
description:
  "Test placement, simulator/HTTP/UI/telemetry harnesses, fixtures and per-kind
  conventions"
condition: ".*"
scope:
  "tool:read(crates/**/tests.rs), tool:edit(crates/**/tests.rs),
  tool:write(crates/**/tests.rs), tool:read(crates/**/tests/**),
  tool:edit(crates/**/tests/**), tool:write(crates/**/tests/**),
  tool:read(crates/*/src/testing.rs), tool:edit(crates/*/src/testing.rs),
  tool:write(crates/*/src/testing.rs), tool:read(crates/*/src/testing/**),
  tool:edit(crates/*/src/testing/**), tool:write(crates/*/src/testing/**),
  tool:read(crates/posnet/src/simulator*),
  tool:edit(crates/posnet/src/simulator*),
  tool:write(crates/posnet/src/simulator*)"
interruptMode: never
---

# Writing tests

Choose guarantees and test shape from
[the checklist](../../docs/testing/checklist.md). This rule is about
constructing those tests, not test-count or coverage targets.

## Placement

- Module unit tests: `foo/tests.rs`, declared from `foo.rs`. Scenarios spanning
  modules of one crate: `src/tests.rs` and `src/tests/` submodules. No top-level
  `tests/` integration-test directories.
- One module's fixture helpers stay with it; cross-module helpers belong in the
  owning crate's `src/testing.rs`. Cross-crate boundaries reuse
  `posnet::simulator` and feature-gated `agent::testing`; never copy an HTTP
  fake into desktop tests.
- Add each regression at the boundary where its consumer sees the bug; a fixed
  bug's test fails without the fix.

## Harness and fixtures

- Printer scenarios use `posnet::simulator::Simulator`, never real
  hardware/serial devices. Unix serial tests use allocated ptys; Windows only
  the configured com0com virtual pair.
- Agent scenarios use `agent::testing::Holuto` loopback HTTP fake through
  production `Api`/`ureq`, simulator and fresh state directory. The fake models
  binding/attempts/results and scripts faults; do not restore the removed
  `Holuto` trait/in-process `FakeApi` or mock our own code.
- Reuse `job_json` and explicit fixture updates for data; create normal state
  through public behavior. Raw JSON/file edits are appropriate for old-format,
  corruption and crash-recovery fixtures, not to bypass the operation being
  tested.
- UI scenarios start the real application/callbacks with Slint's exact-version
  testing backend. Locate controls by accessible label; use its event-loop mode
  for worker events. No custom replacement backend, callback mock, coordinate
  scanning or sleeps after clicks. See [UI guide](../../docs/testing/ui.md).
- Telemetry tests bind a capturing Sentry transport/hub and synthetic canaries,
  never a live DSN. Inspect errors, breadcrumbs, logs and transactions, not only
  the top-level event. Native crashes run only in isolated children with
  synthetic data.
- Helpers build data, not hidden assertions. Keep setup, one action and literal
  expected results visible. Assert exact error variants/messages and stored
  state, not status/count/substrings alone.

## Per-kind conventions

- **Fiscal faults:** execute-then-lost/corrupt/disconnected reply; inspect
  simulated effects, wire frames and persisted journal. Hold/reached proves
  ordering; failed journal writes must prevent the relevant send. Resolution
  never reissues anything.
- **Properties:** `proptest`, independent integer/reference oracle or documented
  relation. Mix range extremes and boundaries; do not copy production
  arithmetic. Commit `proptest-regressions/` seeds produced by real failures
  alongside their fixes.
- **Models:** `proptest-state-machine` reference transitions independent of
  production helpers; check safety/evidence invariants after every step. Keep
  real restart/HTTP/device scenarios; sequential models do not prove thread
  races.
- **Snapshots:** `insta` for binding receipt/report output with fixed
  clock/text/IDs. Review every diff against protocol and semantics with
  `cargo insta review`; do not blanket-accept, redact fiscal fields or snapshot
  arbitrary JSON instead of precise assertions.
- **Concurrency:** channels and simulator hold/reached order contenders. Prove
  one effect and final state. Join fixture threads; release holds even after
  failure. Stress affected worker/race tests with nextest before integration.
- **Time:** fix simulator printer clock; never fake it in production driver
  paths. Inject `today` into pure date functions. No wall-clock duration asserts
  or sleeps to order events. Bounded receives are failure diagnostics; simulator
  timing acceleration preserves production timeouts.
- **Platform:** `#[cfg(...)]` on what the test proves. Windows virtual-serial
  module is `com_pair`, requires `HOLUTO_TEST_COM_PAIR` and fails loudly if
  absent; default nextest excludes it, CI includes it. Linux `session_bus`
  requires a private `DBUS_SESSION_BUS_ADDRESS` and serial test group. No silent
  platform skip.
- **Processes:** environment overrides/locks re-run `std::env::current_exe()`
  with child marker and command-local environment; never unsafe global
  `set_var`.
- **Files:** separate temp directory per test; kept old literal fixtures for
  settings/journal/history. Inject portable failures, not chmod-only assumptions
  under root. Assert the previous durable record survives.

Use [workflows](../../docs/workflows.md) for mutants, snapshot review, stress
and coverage; never claim a check ran unless its result was observed.
