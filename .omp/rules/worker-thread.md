---
description:
  "The desktop never touches the printer or Holuto itself; it talks to the
  Worker"
condition:
  - 'Printer::new|Api::new|\.probe\(|thread::Builder|\.join\(\)|block_on|recv\(\)'
scope: "tool:edit(crates/desktop/src/**), tool:write(crates/desktop/src/**)"
---

# Worker thread

All printer I/O and Holuto requests run on the single `agent::worker::Worker`
thread. The GUI submits `Command`s and renders `Event`s, which arrive on the
worker thread and must reach Slint through `slint::invoke_from_event_loop`.
Never call the printer, the API or a blocking wait on the GUI thread;
`Worker::join` only after `Event::Stopped`, when it returns at once. Quitting
stops the worker and waits for `Event::Stopped`, so the current fiscal operation
always finishes. The desktop never uses `Printer` directly.

Pairing and settings persistence must not block Slint either: submit background
work and deliver completion through the event loop. CLI configuration never
opens a printer; all mutations acquire the same instance lock and use the
validated atomic settings store. Autostart is explicit per-user opt-in and never
bypasses preflight or unresolved journals.
