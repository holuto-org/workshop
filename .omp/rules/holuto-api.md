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

- Keep wire decoding separate from per-job fiscal validation (see
  `crates/agent/src/api/model.rs`). Rejecting a malformed amount while decoding
  a claim loses the job response instead of letting the agent report that job
  as failed. Do not add full runtime OpenAPI validation.
- Schema-valid fake bodies do not prove backend lifecycle behavior or deployed
  compatibility. For contract changes, follow the
  [sync procedure](../../AGENTS.md#api-contract), check the
  [documented lifecycle](../../docs/fiscal-contract.md#agent-api), and exercise
  affected fake transitions through the production client. Do not infer safe
  retries from an HTTP error: claim and result delivery have
  [different guarantees](../../docs/testing/fiscal-safety.md#the-two-delivery-guarantees);
  keep retry decisions in the agent/worker, not the HTTP client.
- Redacted `Debug` does not make error `Display` safe for diagnostics.
  `ApiError::Status` details and `ApiError::Response` parser text can contain
  peer-controlled data; keep them operator-only. Never log or capture tokens,
  pairing codes, raw bodies or full URLs; follow the
  [telemetry policy](../../docs/telemetry.md).
