---
description: "Preserve operator edits and safe shutdown across asynchronous desktop callbacks"
condition: '.*'
scope: "tool:edit(crates/desktop/src/**), tool:write(crates/desktop/src/**), tool:edit(crates/desktop/ui/**), tool:write(crates/desktop/ui/**), tool:edit(crates/agent/src/worker.rs), tool:write(crates/agent/src/worker.rs), tool:edit(crates/agent/src/worker/**), tool:write(crates/agent/src/worker/**)"
interruptMode: never
---

# Worker thread

The thread-ownership and cooperative-stop requirements live in
[the hard requirements](../RULES.md).

- In `crates/desktop/src/application.rs`, `with_app` syncs the whole model
  after each callback. Capture still-editable window state before applying
  model changes, even for a status-only callback. Include worker, preferences,
  tray and termination callbacks. Keep newer live telemetry, theme and
  language choices when adopting an older save result.
- Keep persistence and OS autostart registration off the GUI thread. Use the
  existing `application/preferences.rs` writer. Start only with durably saved
  settings. Cancel pending Start on a failed save or submission.
- Track only successfully submitted writes as pending. Quit waits for their
  completions, not for successful `status.json` persistence. Report write
  errors and allow quit after pending writes drain. A failed submission has
  no completion; an older completion must not revive that wait.
- Keep window close, stop-processing and quit distinct. Close must not stop
  processing. Keep the window visible when no usable tray is available.
  Trace the Quit action, tray Quit, Unix SIGTERM/SIGHUP/SIGINT, macOS native
  termination and event-loop loss.
- In a live GUI, request `Worker::stop` and join after `Event::Stopped`.
  Preserve `Application::finish_after_event_loop`: after the loop ends,
  stop and join there even when the GUI cannot receive `Stopped`. No live
  GUI remains to block; do not remove this cleanup.
- Read `crates/desktop/src/application/macos.rs` before changing native quit.
  Hold pending AppKit termination with `TerminateLater`, then explicitly
  reply when shutdown is safe. Quitting Slint's loop alone does not reply.
  Do not replace the hook with immediate exit or `TerminateCancel`, which
  cancels logout/shutdown. Do not claim coverage for forced termination or
  an unhandled OS session-end path.
