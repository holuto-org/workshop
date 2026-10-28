# UI testing with Slint

Use the real application and its callbacks, not a hand-built headless platform
or replacement callbacks. A model table is appropriate for a pure calculation;
it cannot prove that a visible button is wired to the worker.

## Backend and event loop

`i-slint-backend-testing` is an exact-version dev dependency matching Slint.
Initialize it **before** creating any component/backend. Use
`init_integration_test_with_mock_time` for application tests that receive
worker events through `slint::invoke_from_event_loop`, as the application
fixtures do. Run these scenarios with nextest's per-test process isolation;
do not assume `cargo test`'s shared-process runner is equivalent.

Use `init_no_event_loop` only for isolated component/unit cases that truly need
no event loop. Slint's mock-time backend may advance UI timers/animations; it
must not replace the fiscal printer's clock or the worker's production I/O
timing.
[Upstream backend documentation](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/)
explains these modes and the lack of rendered pixels.

## Drive what the operator can reach

1. Start the application with fresh state, loopback Holuto fake and simulator.
   Keep production callbacks/event bridge intact.
2. Find controls by accessible label with
   `ElementHandle::find_by_accessible_label`, require one unambiguous match,
   check enabled/value state and invoke the accessible default action.
3. Set inputs through their accessible values. Use element queries/roles to
   scope repeated labels; add meaningful accessibility metadata rather than
   guessing coordinates or scanning pixels.
4. Await an observable event/state acknowledgement, then assert exact visible
   text/control state and the fake/device/filesystem effect.

Handles do not keep elements alive. Re-query after close/reopen, tab/row
replacement or dialog changes; check validity before acting
([ElementHandle](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/struct.ElementHandle.html)).
No sleep after clicking and no layout-coordinate search.

## Scenarios that matter

- Save validates and persists; start is explicit unless autostart opted in;
  settings cannot silently reconfigure a running worker; telemetry opt-out
  round-trips.
- Pairing uses the real callback and background request; code/token never remain
  displayed after success; refused/expired pairing preserves saved settings and
  safe errors.
- Autostart toggles the per-user registration/preference, never bypasses fiscal
  preflight or an unresolved block.
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
- A block or stop arriving with a dialog open prevents submission. Ordinary
  unknown receipts have no local bypass; the special
  completed-but-failed/expired conflict exposes only its explicit
  acknowledgement flow with job/order/receipt evidence.
- Worker events show and hide the header's problem indicators and the red
  **Stan operacji** footer. An all-clear event removes the footer; successful
  operations and progress remain silent. Block explanations and orphaned-receipt
  acknowledgement evidence stay available. Events update the **Stan** rows and
  blocked state through the real Slint loop. While a simulator
  command is held, an event-loop marker still executes.
- Close with a tray hides/recreates the window without stopping work; no tray
  keeps it visible. Reopening shows the same model state. Linux watcher
  availability uses separate private-bus tests.
- Stop/quit during a held print completes it once, receives `Stopped`, joins,
  then exits. Do not test shutdown by killing the worker mid-operation.
- On macOS, `macos_quit` is a deliberate exception: a harness-free test target
  runs real AppKit/winit on the main thread. Its child must emit the held-quit
  marker; after exit, the parent checks stopped status, accepted Holuto result
  and journal settlement. An early native exit is a failure.

Backend tests prove semantics and accessibility, not fonts, pixels, compositor
integration or packaged startup. Those need
[platform/package checks](platforms.md); avoid screenshot snapshots whose
font/layout noise hides fiscal behavior. Binding printer output is the intended
`insta` snapshot surface.
