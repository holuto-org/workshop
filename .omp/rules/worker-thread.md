---
description: "Preserve operator edits and safe shutdown across asynchronous desktop callbacks"
condition: '.*'
scope: "tool:edit(crates/desktop/src/**), tool:write(crates/desktop/src/**), tool:edit(crates/desktop/ui/**), tool:write(crates/desktop/ui/**), tool:edit(crates/agent/src/worker.rs), tool:write(crates/agent/src/worker.rs), tool:edit(crates/agent/src/worker/**), tool:write(crates/agent/src/worker/**)"
interruptMode: never
---

# Worker thread

The thread-ownership and cooperative-stop requirements live in
[the hard requirements](../RULES.md).

- A callback can overwrite input even without touching an input field:
  `with_app` syncs the whole model afterward. Capture still-editable window
  state **before** applying the callback's model changes, including worker,
  preferences, tray and termination callbacks. A late completion must not
  restore an older live preference.
- Keep slow persistence and OS autostart registration off the GUI thread.
  When adding background work, account for its completion in Start and quit:
  Start must use durably saved settings; a failed or unsubmitted write must
  not leave either action waiting for a completion that will never arrive.
- Keep window close, stop-processing and quit distinct. Closing must not
  strand the operator without a usable tray or stop processing implicitly.
  Trace every quit entry point, not just the window button, and retain cleanup
  for an event loop that ends before it can deliver `Event::Stopped`
  (`Application::finish_after_event_loop`). Waiting there is not a GUI callback.
- Native termination has platform-specific semantics; do not replace it with
  a generic immediate exit or a refusal to quit. Read `application/macos.rs`
  before changing its hook: holding AppKit termination requires an explicit
  reply, not merely quitting Slint's event loop. Do not claim that cooperative
  shutdown covers forced termination or an unhandled OS session-end path.
