# Compatibility testing

## Current stored-file contracts

The repository pins the current on-disk formats; it does not establish deployment
history. Keep sanitized, literal golden fixtures, not fixtures serialized by the
reader/writer under test. Decide migrations explicitly before changing persisted
contracts; never infer that an unreadable journal can be discarded.

- **`operation.json`**: Receipt/report variants include vendor and serial;
  pre-vendor evidence is migrated durably once to Posnet under the instance
  lock, never discarded. `NotStarted`, `Dispatching`, completed with/without
  number, results and settlements retain their meaning across restart.
  Resolution requires the same vendor and serial and never reissues commands.
- **`settings.json`**: API URL/token/port/baud/diagnostics/autostart and nullable
  vendor retain meaning; old settings without vendor use detection.
  Unknown fields still fail closed, so downgrades are not generally supported.
  Diagnostics opt-out survives save/load/restart.
- **`history.jsonl`**: Current entries preserve ordering, duplicate-job handling
  and staff-entered numbers; torn lines do not swallow the next valid entry.

A round trip proves only today's writer agrees with today's reader; literal
fixtures make future format changes deliberate. Never treat an unreadable
journal as permission to print. File replacement/locking behavior must pass on
each OS; Linux rename does not prove Windows write-through.

## `/agent` and the backend

The production authority is `../backend`'s `/agent` handlers and OpenAPI, not
the fake. Compare session/heartbeat vendor + serial binding, current per-driver
limits/VAT/capabilities, document pagination, queue inputs, take attempts,
terminal states and result idempotency/refusals with those handlers. Update
`agent::testing` loopback fake in the same change, then prove requests through
production `ureq` and the observable stored server state.

The source is the backend's `openapi/agent.json`. After regenerating it with
`cargo run -p holuto -- openapi` in the
backend, run `bash scripts/openapi-sync.sh` here (`BACKEND_DIR` defaults to
`../backend`) and commit the vendored `openapi/` diff. Typify generates wire
types into `OUT_DIR/agent-wire.rs` during builds, not committed source. Deploy
backend changes first: generation follows the vendored schema, not the live
server.

Full OpenAPI schema validation belongs to the feature-gated testing fake, not
the runtime HTTP client. Fiscal amount/name/NIP scalars deliberately remain
strings until domain validation; a malformed individual fiscal job must fail at
the fiscal boundary rather than making an entire batch unreadable. Generated
types do not replace those domain checks.

Assert required nullable fields, decimal strings, unchanged opaque cursor
encoding, Polish problem details and exact result echo. Include commit-then-drop
and refusal for a stale attempt. Do not invent staff resolution for
failed/expired jobs: only unknown jobs have that backend resolution path. A
completed print conflicting with failed/expired uses explicit local
acknowledgement and no server write.

A fake alone cannot prove compatibility with a deployed server. Review backend
contract changes together; use sanitized recorded contract examples and the
backend's own tests, never probe production with a real token or queue fiscal
work as a compatibility check.

Deploy the multi-vendor backend before Workshop: session/heartbeat require
`vendor`, `maxQuantityDecimals`, `capabilities.advanceDeductions` and
`capabilities.zeroTotalReceipts`. Older agents without these fields are not
supported. Test backend refusal of unsupported advances, zero-total sales and
excess quantity precision, plus independent local refusal before any fiscal
command. Company-footer availability remains a local header check.

## Dependencies and features

Keep Cargo.lock and workspace versions deliberate. `slint`, `slint-build` and
`i-slint-backend-testing` move in lockstep; the testing backend is pinned
exactly (`=1.18.1` for Slint 1.18.1). It is an internal preliminary API without
patch-level semver guarantees
([upstream warning](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/)).
Compile and run accessible-label scenarios on upgrades; do not patch around a
mismatched backend.

Exercise default production features as well as all debug/test features. Desktop
release builds reject `simulator`; agent builds without debug assertions reject
`testing`. Posnet's standalone simulator has no release guard. A green
all-features test run does not prove the real serial branch builds. Rebuild and
verify all five packages for dependency/toolchain upgrades; run advisory/license
and unused-dependency checks. See [CI](../ci.md).
`--all-features` must retain production fiscal-mode and unqualified-capability
gates. Acceptance uses a separate debug-only cfg, not a feature; run its
[isolated suite and release-rejection check](hardware-acceptance.md).
