---
description: "Meaningful regressions, shared test harnesses and isolated fixtures"
condition: ".*"
scope:
  "tool:edit(crates/**/*.rs), tool:write(crates/**/*.rs),
  tool:edit(crates/**/*.slint), tool:write(crates/**/*.slint),
  tool:edit(crates/**/Cargo.toml), tool:write(crates/**/Cargo.toml),
  tool:edit(Cargo.toml), tool:write(Cargo.toml),
  tool:edit(.config/nextest.toml), tool:write(.config/nextest.toml),
  tool:edit(.github/workflows/**), tool:write(.github/workflows/**),
  tool:edit(scripts/test-*), tool:write(scripts/test-*),
  tool:edit(packaging/test-*), tool:write(packaging/test-*)"
interruptMode: never
---

# Writing tests

Choose the guarantee from [the checklist](../../docs/testing/checklist.md) and
the cheapest boundary that proves it from
[the testing guide](../../docs/testing/README.md).
Do not turn implementation details into requirements just to make them testable.

## Placement

- Follow the existing layout: module tests in `foo/tests.rs`, crate-wide
  scenarios in `src/tests.rs` / `src/tests/`, rather than a new top-level
  `tests/` tree. Native main-thread tests may need a harness-free Cargo target,
  as `crates/desktop/src/bin_tests/macos_quit.rs` does.
- Keep local fixtures with their tests; share cross-module fixtures through the
  crate's `testing` module. Reuse the existing brand simulators and
  `agent::testing` across crates instead of maintaining competing fakes.
- A regression must fail for the reported behavior, not merely track a private
  helper's implementation. Small parser/value unit tests are appropriate when
  they prove the guarantee; worker/UI wiring needs the real consumer path.
  Demonstrate failure without the fix in an isolated copy, never by undoing
  changes in the [shared checkout](../../AGENTS.md#shared-checkout).

## Harnesses

- Device behavior goes through the matching simulator and production
  driver/`Link`; HTTP behavior through `agent::testing::Holuto` and the
  production client. Do not replace the code whose behavior the test promises.
  Low-level transport tests may fake the external serial API.
- UI wiring tests keep production callbacks and event delivery; use the
  existing Slint application fixture and semantic controls. Await the action's
  acknowledgement before asserting effects; queuing a callback is not completion.
  Backend setup and runner requirements belong in the [UI guide](../../docs/testing/ui.md).
- Use fresh temporary state, synthetic credentials and local capturing
  telemetry transports, never saved user settings, live services or printers.
  See the [privacy tests](../../docs/testing/security.md).
- Build ordinary state through its real transitions. Literal stored-format,
  corruption and crash fixtures are exceptions: generating them with the same
  serializer under test can hide a format regression. Helpers may assert shared
  invariants, but keep the scenario's action and expected result visible.

## Per kind

- For fiscal faults, distinguish a refusal before execution from an answer lost
  after execution. Inspect device effects and retained evidence, not just the
  returned error. Use the [fiscal fault matrix](../../docs/testing/fiscal-safety.md).
- Choose an independent oracle for the guarantee: published output vectors,
  reference arithmetic, a state model or a documented relation. A round trip
  alone cannot prove two mutually consistent implementations are correct.
  Snapshot review and regression-seed procedures live in
  [workflows](../../docs/workflows.md#finishing-a-change).
- Control races with acknowledgements/holds, not sleeps. Keep failure cleanup
  able to release holds and stop/join fixture threads. Follow the
  [concurrency guide](../../docs/testing/concurrency.md) for overlap scenarios.
- For deadline values, record configured budgets, not elapsed wall time.
  For serial commands, wrap the simulator's `Link`/`Port`, run the real command
  and assert its reply and the timeout passed to `write_all` at simulator and
  production scales. See `crates/elzab/src/session/tests.rs` for the recording
  wrapper and scale cases.
- Environment overrides and process locks use a child with child-local
  environment, not process-global mutation. Assert an explicit child outcome
  and durable state; exit success alone can mean the test filter matched
  nothing.
- Gate tests by the platform behavior they prove and obtain native evidence for
  platform-specific changes. Do not change OS gates to run a native test on the
  wrong host, weaken it or silently skip it; report the native evidence missing.
  Serial pairs, private buses and package isolation are documented in
  [platform procedures](../../docs/testing/platforms.md).
