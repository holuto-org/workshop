# Workshop Hard Requirements

- Never run the app or scripts against a real printer, including through a
  serial port. Exercise printer code only through the `posnet`, `novitus` and
  `elzab` simulators and tests: fiscal receipts and reports are irreversible
  legal records.
- Preserve README "Fiscal safety" in driver, agent and desktop changes: verify
  printer state before each operation; journal fiscal intent before sending any
  fiscal command; never retry a device command or reprint. Uncertain outcomes
  block receipts and reports across restarts until a person resolves them.
  Resolution never reissues anything.
- Run printer I/O and Holuto requests only on the single
  `agent::worker::Worker` thread. The GUI only submits commands and receives
  events. During GUI shutdown, call `Worker::stop`, receive `Event::Stopped`,
  then `join`; never block the live GUI on an active worker. If the event loop
  has ended, stop and join outside it (`Application::finish_after_event_loop`).
  Never abandon an operation: interruption can leave its fiscal outcome unknown.
- Configure `workshop` through its CLI or GUI and pair with a one-time Holuto
  code. Use `Worker::pair` without opening a printer: pairing does not permit
  printing. Never ask for, read or log a live device token. Use synthetic
  credentials in tests; keep real credentials internal.
