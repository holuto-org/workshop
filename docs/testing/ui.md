# UI testing with Slint

When claiming UI wiring, use the real application, callbacks and worker event
bridge. Model tests are cheaper for calculation and state-transition cases;
they cannot prove that an operator's action reaches the worker.

## Backend and event loop

The workspace currently resolves Slint and its exact-version
`i-slint-backend-testing` dev dependency to 1.18.1. Follow the
[version-matched backend documentation](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/),
not examples for another release of this unstable API.

Initialize the backend **before** creating a component. Use
`init_integration_test_with_mock_time` when testing worker or preferences-writer
delivery through `slint::invoke_from_event_loop`, and actually run the Slint
event loop until the scenario acknowledges completion. This initializer is
[once per process](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/fn.init_integration_test_with_mock_time.html):
use nextest's per-test process isolation, not a shared-process `cargo test`
run, even with `--test-threads=1`.

Use `init_no_event_loop` for synchronous component/model-to-window cases that
do not need cross-thread delivery. It is not evidence that asynchronous
callbacks completed. Existing fixtures live in
[`application/tests.rs`](../../crates/desktop/src/application/tests.rs);
worker scenarios live in
[`application/tests/end_to_end.rs`](../../crates/desktop/src/application/tests/end_to_end.rs).

Both modes mock Slint time. Advance UI timers/animations explicitly with
`mock_elapsed_time`; this does not advance the simulator printer clock, the
worker's clock or I/O deadlines. Keep those controls separate; see
[concurrency evidence](concurrency.md). Use event acknowledgements/holds, not a
delay after clicking; bounded waits diagnose a missing acknowledgement.

## Drive what the operator can reach

1. Create fresh temporary state. For HTTP/printer effects, use loopback Holuto
   and the matching simulator; keep the production callbacks/event bridge.
2. Locate controls with `ElementHandle::find_by_accessible_label`, scoped by
   role/container when labels repeat. Require an unambiguous target and check
   enabled, checked or value state before acting.
3. Invoke the accessible action or set an input's accessible value. A property
   assignment is not a keyboard edit, focus change or menu selection: exercise
   those events when their callbacks are the behavior under test. Existing
   combo-box tests expand the control and click the semantically located item;
   the confirmation test sends Return to check safe initial focus.
4. Await the relevant acknowledgement, then assert exact displayed text/control
   state and the fake/device/filesystem effect, including absent side effects
   after refusal. A queued event alone is not completion.

[ElementHandle](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/struct.ElementHandle.html)
does not keep an element alive. Re-query after window/dialog or dynamic-row
replacement and check validity. A matching accessible label is not proof of
visibility, focus, enabled state or pointer reachability. Prefer meaningful
accessibility metadata to guessed coordinates. For pointer/focus behavior, use
the located element's input helpers or window events rather than replacing
callbacks. These still exercise the testing backend, not the native input stack.

## Scenarios that matter

Use these as scenario boundaries, not a claim that every case runs on every OS:

- Save validates and persists; Start waits for required durable settings and
  remains explicit unless autostart was opted into. Connection settings cannot
  silently reconfigure a running worker. A background completion must not
  overwrite edits made meanwhile; live telemetry opt-out persists.
- Set the fixture language explicitly when locating translated labels. Exercise
  real language selection before pairing and during processing: preserve input,
  update existing warnings/dialogs and the tray, persist without a printer, and
  reopen in the chosen language. Fiscal output and diagnostic evidence must not
  change. Check system negotiation and English fallback independently of the
  test runner's locale.
- Pairing uses the real callback and background request without opening a
  printer. Success clears the pairing code; synthetic token canaries must
  never appear in UI text/accessibility values. Refused/expired pairing
  preserves saved settings and gives a safe error.
- Autostart toggles registration and preference without committing unrelated
  connection edits or starting work immediately. Isolate OS registration from
  the operator's account as in [platform checks](platforms.md).
- **Stan** is read-only: verify each row (**Obsługa drukarki**, **Holuto**,
  **Drukarka**, **Stan drukarki**) follows worker events, including **Gotowa**,
  **Niegotowa: …** and **Zablokowana: wymaga rozstrzygnięcia**. A job enqueued
  in the loopback Holuto fake prints without any click.
- Report confirmation shows exact dates/summary. With injected `today` in pure
  validation, test leap days, equal endpoints, today, future/reversed dates and
  periodic 1999-12-31 versus 2000-01-01.
- An overdue daily report alone enables only the daily-report action. Exercise
  Elzab's real worker path: one RRP acceptance leaves an unresolved durable block,
  never a claimed receipt or an automatically retried report.
- A block or stop arriving with a dialog open prevents submission. Cancellation
  sends nothing, Return initially cancels, and controls behind the modal cannot
  replace its pending report. Ordinary unknown receipts have no local bypass;
  the special completed-but-failed/expired conflict exposes only its explicit
  acknowledgement flow with job/order/receipt evidence.
- Worker events show and hide the header's problem indicators and the
  **Stan operacji** footer. Assert text and presence semantically; check red
  styling with a renderer. An all-clear event removes the footer; successful
  operations and progress remain silent. Block explanations and orphaned-receipt
  evidence stay available. While a simulator command is held, an event-loop
  marker still executes.

## Native and rendered evidence

Keep close, Stop and Quit separate:

- Close keeps the window shown while its asynchronous tray probe runs. It hides
  and releases the component only when a tray component exists and the probe
  permits hiding; reopening reconstructs it from the captured model without
  stopping work. With no usable tray, the window stays available.
- Linux private-bus tests exercise the actual watcher probe, including an
  absent/hung host or an icon not registered for this process. They do not prove
  shell rendering or tray usability on every desktop. The Windows/macOS probe
  currently returns true, so backend close tests are not proof of native icon
  availability. See [platform procedures](platforms.md).
- Stop does not exit the application; Quit requests cooperative shutdown.
  Follow [shutdown evidence](concurrency.md#shutdown-and-platform-evidence)
  for held operations, early event-loop termination and the real main-thread
  `macos_quit` harness. Invoking the Rust termination callback on the testing
  backend cannot prove AppKit's nested-loop/reply behavior.

The testing backend renders **no pixels** and uses fixed text measurement. It
proves application semantics and declared accessibility metadata, not fonts,
clipping, contrast, native screen-reader integration, compositor behavior or
packaged startup. Use a real renderer/native session for visual or OS claims
and [platform/package checks](platforms.md) for packaged launch. Screenshots can
support a visual claim when fonts, renderer, theme and viewport are controlled;
they cannot replace assertions about worker dispatch or fiscal effects.
Receipt/report snapshot review belongs in
[workflows](../workflows.md#finishing-a-change).
