---
description:
  "Journal, settings, status and instance lock: durable, private,
  fail-closed stored contracts"
condition: ".*"
scope:
  "tool:read(crates/agent/src/journal*), tool:edit(crates/agent/src/journal*),
  tool:write(crates/agent/src/journal*), tool:read(crates/agent/src/store*),
  tool:edit(crates/agent/src/store*), tool:write(crates/agent/src/store*),
  tool:read(crates/agent/src/settings*), tool:edit(crates/agent/src/settings*),
  tool:write(crates/agent/src/settings*), tool:edit(crates/agent/src/status*),
  tool:write(crates/agent/src/status*),
  tool:read(crates/agent/src/journal/**),
  tool:edit(crates/agent/src/journal/**),
  tool:write(crates/agent/src/journal/**),
  tool:read(crates/agent/src/store/**), tool:edit(crates/agent/src/store/**),
  tool:write(crates/agent/src/store/**),
  tool:read(crates/agent/src/settings/**),
  tool:edit(crates/agent/src/settings/**),
  tool:write(crates/agent/src/settings/**),
  tool:edit(crates/agent/src/status/**), tool:write(crates/agent/src/status/**)"
interruptMode: never
---

# Durable state

- `operation.json` is fiscal evidence. Persist claimed receipt intent before
  preflight, and every fiscal stage before its command
  (`fiscal::journal::Journal::dispatching`). An unreadable, corrupt or mismatched
  journal blocks and is never deleted or reset as recovery. Resolution requires
  the same vendor and unique number.
  Settlement and recovery decide on binding, attempt, server state and journal
  evidence only; they never re-run claim-time job validation, which would
  strand an already printed receipt.
- Every write goes through `store::write_durably` (temp file, fsync, atomic
  replace; on Windows a POSIX-semantics rename that tolerates open readers,
  then a flush). Unix modes: state directory 0700, files 0600. Windows state
  lives in the Local AppData Known Folder, never an environment override.
- One process per state directory (`instance.lock`); CLI mutations refuse while
  the app or another command holds it. Only the app also holds
  `application.lock`, which `status`/`check` report. Readers that skip the lock
  (`Settings::load_at`, `Snapshot::load_at`) must not create state.
- Keep literal fixtures of each stored format beside its module. Settings,
  status and journal reject unknown fields. Format changes follow
  [AGENTS.md](../../AGENTS.md#external-surfaces).
- The device token lives only in `settings.json`: `Token` redacts `Debug` and
  has no `Display` implementation; settings parse errors never echo values.
  A different API URL forgets it (`Settings::set_api_url`).
  `status.json` never holds credentials.

Details: [compatibility](../../docs/testing/compatibility.md),
[security](../../docs/testing/security.md).
