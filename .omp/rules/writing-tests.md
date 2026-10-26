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
  tool:edit(crates/*/src/simulator*), tool:write(crates/*/src/simulator*),
  tool:edit(crates/desktop/src/bin_tests/**),
  tool:write(crates/desktop/src/bin_tests/**)"
interruptMode: never
---

# Writing tests

Pick the guarantee from [the checklist](../../docs/testing/checklist.md) and the
boundary from [the testing guide](../../docs/testing/README.md); this rule says
how to build the test.

## Placement

- Module unit tests: `foo/tests.rs`, declared from `foo.rs`. Scenarios spanning
  modules of one crate: `src/tests.rs` and `src/tests/`. No top-level `tests/`
  directories. The harness-free `src/bin_tests/macos_quit.rs` runs real AppKit
  on the main thread as an explicit Cargo test target. Throwaway probes live
  outside the repository ([shared checkout](../../AGENTS.md#shared-checkout)).
- One module's fixture helpers stay with it; cross-module helpers live in the
  crate's `src/testing.rs`. Cross-crate tests reuse the brand's simulator and
  feature-gated `agent::testing`; never copy an HTTP fake into desktop tests.
- A regression drives the boundary where its consumer sees the bug (driver,
  session, worker, `Api`, UI), never a private helper or constant. Run it
  against the unfixed code and see it fail for the stated reason; prove that
  in a copy or with a scoped `cargo mutants -f <file>`, never by editing the
  shared checkout.

## Harnesses

- Printers: the matching `posnet`, `novitus` or `elzab` simulator through the
  real driver/`Link`. Unix serial tests use allocated ptys; Windows only the
  configured com0com pair (`com_pair`). Never real hardware.
- Holuto: `agent::testing::Holuto`, a stateful loopback fake reached through the
  production `Api`/`ureq` client, plus a fresh `state_dir()`. It models binding,
  attempts and results and scripts faults. Do not mock our own code or add an
  in-process API fake.
- UI: the real application and callbacks on Slint's exact-version testing
  backend (`init_integration_test_with_mock_time` when worker events flow).
  Locate controls by accessible label; no replacement callbacks, coordinates or
  sleeps after clicks. See [UI guide](../../docs/testing/ui.md).
- Telemetry: a capturing Sentry transport/hub with synthetic canaries; inspect
  errors, breadcrumbs, logs and transactions. Never a live DSN.
- Data: reuse `job_json`/fixture helpers and create state through public
  behavior. Raw file edits only for corruption and crash fixtures.
  Helpers build data, never hidden assertions; keep setup, one action and
  literal expected values visible. Assert exact variants/messages and stored
  state, plus what did **not** change after a refusal.

## Per kind

- **Fiscal faults:** execute-then-lose/corrupt/disconnect; inspect simulator
  effects, wire frames and the persisted journal. A failed journal write must
  prevent the send. Resolution never reissues.
- **Properties:** `proptest` against an independent integer/reference oracle;
  mix extremes and boundaries; never copy production arithmetic. Commit
  `proptest-regressions/` seeds from real failures and never delete one: a
  passing replay is still the record of a past failure.
- **Models:** `proptest-state-machine` reference transitions independent of
  production helpers; invariants after every step. Keep real restart/HTTP
  scenarios too; sequential models do not prove races.
- **Snapshots:** `insta` only for binding receipt/report output with fixed
  clock/text/IDs. Review every diff against the protocol; never blanket-accept or
  redact fiscal fields. Use the crate-scoped review procedure in
  [workflows](../../docs/workflows.md#finishing-a-change).
- **Concurrency:** order contenders with channels and simulator `hold`/`reached`;
  prove one effect and the final state; release holds and join threads on
  failure. Stress worker/race tests before integration
  ([workflows](../../docs/workflows.md#finishing-a-change)).
- **Time:** fix the simulator printer clock; freeze the worker poll clock
  (`ClockScope`) when exact event sequences are asserted; inject `today` into
  pure date functions. No sleeps or wall-clock assertions. Bounded receives are
  failure diagnostics only. Simulator links divide driver deadlines by
  `Link::time_scale` (1000) with a 100 ms floor; to assert a production
  deadline, wrap the link with a scale of 1 (see the `elzab` tests'
  `Deadlines`). A simulator `Timeout` seen only under heavy parallel load:
  rerun the test alone and under stress before reporting; never answer it with
  sleeps or retries.
- **Platform:** `#[cfg(...)]` on what the test proves; no silent skips.
  `com_pair` requires `HOLUTO_TEST_COM_PAIR` and fails loudly without it. Linux
  `session_bus` tests self-spawn `dbus-run-session`. For `cfg` code this host
  cannot run, keep the test minimal, add no unsafe/FFI scaffolding, and report
  it as not run.
- **Processes:** environment overrides and locks re-run
  `std::env::current_exe()` with a child marker and child-local environment;
  never global `set_var`. Assert the child's exact outcome and stored state,
  not runner text such as `1 passed`: a filter matching no test also passes.
- **Files:** a temp directory per test; current-format literal fixtures for
  settings/journal/history; inject portable failures (not chmod under root);
  assert the previous durable record survives.

Never claim a check ran unless its result was observed.
