---
description:
  "Desktop never touches the printer or Holuto itself; it talks to the Worker"
condition:
  - 'Printer::new|Api::new|\.probe\(|thread::Builder|\.join\(\)|block_on|recv\(\)'
scope: "tool:edit(crates/desktop/src/**), tool:write(crates/desktop/src/**)"
interruptMode: never
---

# Worker thread

- All printer I/O and Holuto requests run on the single `agent::worker::Worker`
  thread. The GUI submits `Command`s and renders `Event`s; events arrive on the
  worker thread and reach Slint only through `slint::invoke_from_event_loop`.
- Never call a driver, `Api` or a blocking wait on the GUI thread. Pairing and
  settings/status persistence run in background work (`Worker::pair`,
  `preferences::Writer`) and report completion through the event loop.
- Every event-loop callback that later syncs the model into the window first
  captures the window (`app.capture()`), so operator input is never overwritten.
- Closing the window only hides it when a tray host shows this app's icon
  (otherwise it stays visible) and never stops the worker. **Zatrzymaj** stops
  processing; **Zakończ**, tray quit, macOS Dock/⌘Q quit, logout, restart and
  shutdown, and Unix SIGTERM/SIGHUP/SIGINT route through `Worker::stop` and
  join only after the final `Event::Stopped`. macOS termination is held
  (`TerminateLater`), never
  refused: a refusal would cancel the logout. On macOS no worker (including
  pairing) starts unless `macos::install()` succeeds. A failed status write
  never keeps quit waiting.
- CLI commands never open a printer; mutations take the same instance lock and
  the validated atomic settings store. Autostart is explicit per-user opt-in and
  never bypasses preflight or an unresolved journal.
