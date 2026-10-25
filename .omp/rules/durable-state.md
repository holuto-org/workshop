---
description:
  "Journal, settings, status, history and instance lock: durable, private,
  fail-closed stored contracts"
condition: ".*"
scope:
  "tool:read(crates/agent/src/journal*), tool:edit(crates/agent/src/journal*),
  tool:write(crates/agent/src/journal*), tool:read(crates/agent/src/store*),
  tool:edit(crates/agent/src/store*), tool:write(crates/agent/src/store*),
  tool:read(crates/agent/src/settings*), tool:edit(crates/agent/src/settings*),
  tool:write(crates/agent/src/settings*), tool:edit(crates/agent/src/status*),
  tool:write(crates/agent/src/status*), tool:edit(crates/agent/src/history*),
  tool:write(crates/agent/src/history*)"
interruptMode: never
---

# Durable state

- `operation.json` is fiscal evidence. Persist intent before preflight and each
  stage before its command (`fiscal::journal::Journal::dispatching`); an
  unreadable, corrupt or mismatched journal blocks and is never deleted or reset
  as recovery. Resolution requires the same vendor and serial.
- Every write goes through `store::write_durably` (temp file, fsync, atomic
  replace; write-through on Windows). Unix modes: state directory 0700, files
  0600. Windows state lives in the Local AppData Known Folder, never an
  environment override.
- One process per state directory (`instance.lock`); CLI mutations refuse while
  the app holds it. Readers that skip the lock (`Settings::load_at`,
  `Snapshot::load_at`) must not create state.
- Stored formats are contracts. Keep current-format literal fixtures beside the
  module; a format change needs an explicit migration (see `journal/legacy.rs`)
  and a fixture for the old shape. `deny_unknown_fields` means downgrades fail
  closed.
- The device token lives only in `settings.json`: `Token` redacts `Debug` and
  `Display`, serde errors never echo values, and a different API URL forgets it
  (`Settings::set_api_url`). `status.json` and history never hold credentials.
- History is a display cache: its failures produce a notice and never prevent
  settling a server-accepted result.

Details: [compatibility](../../docs/testing/compatibility.md),
[security](../../docs/testing/security.md).
