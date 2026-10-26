# Concurrency and cooperative shutdown

One `agent::worker::Worker` owns **all** printer I/O and Holuto requests. The
GUI submits commands and renders events via `slint::invoke_from_event_loop`. A
serialized mock session does not prove the production worker obeys this rule.

## Control overlap, do not hope for it

1. Start the real worker with a loopback Holuto fake, simulator and fresh state
   directory.
2. Hold a meaningful Posnet simulator command (`hold`, then wait for
   `reached`; the other simulators have no hold) and inspect `operation.json`
   while held to prove intent precedes dispatch.
3. Submit the contender (report, repeated confirmation, stop or quit).
4. Wait for an observable acknowledgement/event-loop marker, not a sleep or
   `is_finished` timing guess.
5. Release the command; assert exact frame/effect order, HTTP result,
   journal/history and final events.

A bounded receive timeout is a deadlock diagnostic, not an elapsed-time
expectation. A failure must still release the hold and stop/join fixture
threads.

## Required guarantees

- Receipt/report commands never interleave; both I/O boundaries stay on the
  single worker thread, not the GUI or detached helper threads.
- Duplicate confirmation consumes one pending action. Recheck
  connected/ready/blocked/stopping state at confirmation time; cancellation
  submits nothing.
- While a print is held, an event-loop marker and permitted non-fiscal UI action
  still run. This proves responsiveness without asserting milliseconds.
- `Worker::stop` is cooperative: current operation finishes and persists/reports
  its outcome before final `Event::Stopped`. No later events or fiscal
  operations follow that final event.
- Join only after `Stopped`. Quit waits for that event and joins; close-to-tray
  does not stop the worker. Test application quit paths, not only a model flag.
- A worker panic surfaces an actionable error and does not manufacture a certain
  fiscal outcome. Restart still reconciles durable evidence.
- A second process cannot use the same state directory. Hold the instance lock
  in a child and assert the contender's exact failure and unchanged files.

Use `std::env::current_exe()` and a child-only environment marker for
process-level tests; set environment on the child command, never unsafe global
`set_var`. Each test gets its own directory/server/simulator. Linux bus-global
tray scenarios belong to the serial `session_bus` nextest group; COM pair tests
share an exclusive pair and must not race for it.

Before merging worker, timing or shared-state changes, run selected tests
repeatedly with
[nextest stress testing](https://nexte.st/docs/features/stress-tests/), as shown
in [workflows](../workflows.md#finishing-a-change). A flake is not fixed by
rerunning it. Generated sequential state models complement these controlled
races, not replace them.
