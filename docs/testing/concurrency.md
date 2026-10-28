# Concurrency and cooperative shutdown

Test the production [Worker](../../crates/agent/src/worker.rs), desktop
[event bridge](../../crates/desktop/src/application.rs) and
[preferences writer](../../crates/desktop/src/application/preferences.rs), not
serialized substitutes. Worker module docs own fiscal single-flight and stop
invariants; this guide describes how to obtain evidence for them.

## Control overlap, do not hope for it

1. Start the real worker with a loopback Holuto fake, simulator and fresh state
   directory. Keep the production client, driver and, for GUI scenarios,
   callbacks/event bridge intact; use the [UI fixture](ui.md#backend-and-event-loop).
2. Choose the boundary to hold:
   - Posnet's `hold(command)` signals `reached` after recording the incoming
     frame, before executing it. Inspect `operation.json` while held to check
     the matching durable dispatch intent; do not describe this as a hold
     before the host sends bytes. Novitus and Elzab have no equivalent hold.
   - The loopback fake's `Fault::Hold` controls an HTTP request, for example a
     connection or settlement request. Its server thread is not evidence of
     which thread issued the request.
   - A channel in the preferences completion sink can hold acknowledgement
     after persistence. Assert the stored value before releasing delivery.
3. Wait for arrival, then submit the contender: report, repeated confirmation,
   preference edit, stop or quit.
4. While held, run an event-loop marker and a permitted non-fiscal UI action.
   Assert their result before releasing; an eventual result after release
   does not prove responsiveness during the operation.
5. Release the boundary; assert exact frame/effect order, HTTP result,
   journal, stored settings/status and final events as relevant.

Bounded receives diagnose deadlocks; they are not elapsed-time expectations.
Make failure paths release gates and stop/join fixture threads before
propagating an error. Freeze the worker poll clock when asserting exact event
sequences; Slint mock time does not freeze worker I/O or the printer clock.
See the [test construction rule](../../.omp/rules/writing-tests.md#per-kind).

## Required guarantees

Use the smallest real path that can disprove the guarantee:

- **Thread ownership:** observe client/fiscal operation spans and event sink
  thread IDs, with a positive control on the GUI/test thread. The worker test
  `http_receipts_reports_and_events_share_the_single_worker_thread` covers
  HTTP, a receipt and a report. Serialized wire effects alone do not prove
  thread ownership; a span proves only the instrumented operation it covers.
  Pairing also needs its real `Worker::pair` path and no printer traffic.
- **Fiscal overlap:** hold a receipt while submitting a report, then repeat
  with stop queued. Check one completed receipt, report ordering or refusal,
  and journal/HTTP settlement. Stop must not start a queued fiscal operation.
  Use the [fiscal fault matrix](fiscal-safety.md) for uncertain outcomes,
  restart blocking and resolution without replay.
- **Confirmation races:** repeat acceptance of one dialog; deliver a
  disconnected, blocked, not-ready or stopping update before acceptance.
  Assert the submitted command count and unchanged device effects, not just
  a disabled button. Cancel submits nothing.
- **Panic:** inject it at fiscal dispatch, then check the problem, durable
  block and final `Stopped`; no fabricated certain outcome. Check restart
  against retained evidence.
- **Process exclusion:** hold the state-directory lock in a child; assert
  the contender's exact failure and unchanged files.

Existing fiscal race scenarios and span capture live in
[worker tests](../../crates/agent/src/worker/tests.rs).

## Event bridge and write completions

Exercise asynchronous delivery, not just `Model` methods:

- Type without committing a field, then deliver a worker event or preferences
  acknowledgement through the event loop. Check the typed value survives the
  ensuing full-model sync. Repeat for tray/native quit, which has no preceding
  window action callback. A test that captures input itself can hide a missing
  capture in the production callback.
- Close/reopen while events are queued; assert updates reach the current
  window, not a retained old component. Background callbacks carry owned
  payloads, not GUI handles. Re-query controls after recreation. Exercise a
  nested callback too: `with_app` queues reentrant work instead of borrowing
  the application twice.
- Hold a save/reload acknowledgement, change a live preference, then release
  it. Check both the visible choice and eventual durable value; an older
  completion must not roll the choice back. Do not assume every preference
  remains editable during pairing, saving or quitting.
- Queue settings and status writes and assert FIFO completion after each
  persistence. Hold a completion while submitting the next request to check
  submission stays nonblocking. Autostart registration uses this writer too;
  isolate OS registration as described in [platform tests](platforms.md).
- Press Start while a settings save is pending; check the worker uses the
  acknowledged saved configuration. Stop/quit before that acknowledgement
  must cancel the pending start. Inject persistence failure and a closed
  writer separately: a failed or unsubmitted save cannot start the worker or
  leave Start/quit waiting for a completion that will never arrive.

Examples are in [application tests](../../crates/desktop/src/application/tests.rs),
[Start tests](../../crates/desktop/src/application/tests/start.rs) and
[writer tests](../../crates/desktop/src/application/preferences/tests.rs).

## Shutdown and platform evidence

Keep these distinct in the test:

- **Close:** wait for the asynchronous tray probe. Hide/recreate only with a
  usable tray; otherwise keep the window reachable. Closing does not stop
  processing.
- **Stop:** let the current operation finish or retain its uncertain evidence,
  receive final `Event::Stopped`, then join without quitting the application.
  Do not equate stopping with successful printing or successful Holuto delivery.
- **Quit while the event loop is running:** exercise window Quit, tray Quit
  and the relevant native/signal entry points. Wait for `Stopped` and join,
  then wait for all submitted settings/status completions, including the
  stopped publication. A write error is a completion, not a successful save;
  quit does not promise a new status file when persistence fails.
- **Event loop already ended:** `finish_after_event_loop` stops and joins
  without waiting for GUI delivery of `Stopped`. Blocking here does not freeze
  a live GUI. This fallback protects the in-flight fiscal operation; it does
  not drain GUI write callbacks or guarantee a final status/preferences flush.

The [held-print lifecycle tests](../../crates/desktop/src/application/tests/end_to_end.rs)
cover normal quit, final status acknowledgement and, outside macOS, simulated
event-loop loss. Quitting the testing event loop is not a real compositor-loss
or OS logout test.

- **Linux/Unix signals:** the child-process
  [signal test](../../crates/desktop/src/application/tests/signals.rs) delivers
  TERM, HUP and INT to an idle app. It proves routing to quit, not a held-print
  OS shutdown or a shutdown deadline. Signal registration failure is reported;
  a session manager need not deliver one of these signals.
- **Linux tray:** the private-bus
  [watcher test](../../crates/desktop/src/tray/session_bus.rs) covers absent,
  unregistered, registered, hostless, vanished and unresponsive hosts. It
  proves D-Bus registration checks, not icon visibility on every desktop.
- **macOS:** the ordinary direct-hook unit scenario is not native AppKit
  evidence. The main-thread, harness-free
  [`macos_quit` target](../../crates/desktop/src/bin_tests/macos_quit.rs) invokes
  real AppKit termination while a print is held; its parent checks the marker,
  stopped status, accepted Holuto result and settled journal. AppKit's
  [held termination](https://developer.apple.com/documentation/appkit/nsapplication/terminatereply/terminatelater)
  needs an explicit reply, not merely a Slint event-loop quit. This is not an
  actual logout/restart/shutdown test.
- **Windows:** Slint lifecycle tests cover application close/stop/quit, not
  Windows session-end handling. There is no desktop session-end hook to test.
  On Windows and macOS the tray availability function assumes a host; Linux's
  registration evidence does not apply.

Forced termination, power loss and OS-enforced shutdown deadlines are outside
cooperative-stop guarantees. None of these offline tests qualifies a physical
printer. Native runner/package requirements and exclusive `session_bus` /
`com_pair` groups live in [platform procedures](platforms.md).

Before merging worker, timing or shared-state changes, run selected tests
repeatedly with
[nextest stress testing](https://nexte.st/docs/features/stress-tests/), as shown
in [workflows](../workflows.md#finishing-a-change). Repetition supplements
controlled overlap; it cannot prove all schedules or fix a flake. Generated
sequential state models complement races, not replace them.
