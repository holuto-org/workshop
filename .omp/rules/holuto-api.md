---
description:
  "Holuto /agent client, vendored OpenAPI contract and the loopback test fake"
condition: ".*"
scope:
  "tool:read(crates/agent/src/api.rs), tool:edit(crates/agent/src/api.rs),
  tool:write(crates/agent/src/api.rs), tool:read(crates/agent/src/api/**),
  tool:edit(crates/agent/src/api/**), tool:write(crates/agent/src/api/**),
  tool:read(crates/agent/src/testing.rs),
  tool:edit(crates/agent/src/testing.rs), tool:write(crates/agent/src/testing.rs),
  tool:read(crates/agent/src/testing/**),
  tool:edit(crates/agent/src/testing/**), tool:write(crates/agent/src/testing/**),
  tool:read(openapi/**), tool:edit(openapi/**), tool:write(openapi/**),
  tool:read(scripts/openapi-sync.sh), tool:edit(scripts/openapi-sync.sh),
  tool:write(scripts/openapi-sync.sh)"
interruptMode: never
---

# Holuto API

- Keep `openapi/agent.json` as the backend's exported contract, not a list of
  Workshop callers. Never hand-edit it or prune it when removing client code.
  Change the backend first, regenerate its export, then follow the
  [sync procedure](../../AGENTS.md#api-contract). The sync script only copies
  the export; confirm the backend branch and deploy it before a client that
  requires the change.
- Keep wire decoding separate from per-job fiscal validation (see
  `crates/agent/src/api/model.rs`). Leave amounts as strings so an invalid
  claimed job can be journaled and reported failed without fiscal commands.
  Read only consumed response fields, ignore unused fields, and require
  nullable consumed keys to be present. Do not add full runtime OpenAPI
  validation.
- Recovery must not repeat claim-time fiscal validation. In `settle()` and
  `acknowledge_orphaned_receipt()`, keep printer binding, attempt, remote state
  and durable journal evidence checks. Test a malformed claim whose FAILED
  result commits but its reply is dropped: restart with the same journal,
  settle the failure and assert no fiscal commands.
- Schema-valid fake bodies do not prove lifecycle behavior or deployed
  compatibility. Model the [documented lifecycle](../../docs/fiscal-contract.md#agent-api),
  exercise changed transitions through the production client, and assert
  requests and final fake state. A lost take reply does not authorize
  re-serving the taken job; taken or unknown jobs block later takes with 204.
  Do not infer safe retries from an HTTP error: claim and result delivery have
  [different guarantees](../../docs/testing/fiscal-safety.md#the-two-delivery-guarantees).
  Keep retry decisions in the agent/worker, not the HTTP client.
- Redacted `Debug` does not make error `Display` safe for diagnostics.
  `ApiError::Status` details and `ApiError::Response` parser text can contain
  peer-controlled data; keep them operator-only. Never log or capture tokens,
  pairing codes, raw bodies or full URLs; follow the
  [telemetry policy](../../docs/telemetry.md).
