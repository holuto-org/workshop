---
description:
  "Holuto /agent client, vendored OpenAPI contract and the loopback test fake"
condition: ".*"
scope:
  "tool:read(crates/agent/src/api*), tool:edit(crates/agent/src/api*),
  tool:write(crates/agent/src/api*), tool:read(crates/agent/src/testing.rs),
  tool:edit(crates/agent/src/testing.rs), tool:write(crates/agent/src/testing.rs),
  tool:edit(crates/agent/build.rs), tool:write(crates/agent/build.rs),
  tool:edit(openapi/**), tool:write(openapi/**)"
interruptMode: never
---

# Holuto API

- `openapi/agent.json` is vendored from the backend by
  `scripts/openapi-sync.sh`; never hand-edit it. `build.rs` generates wire types
  into `OUT_DIR`. Fiscal scalars stay strings until domain validation, so one
  malformed job fails alone.
- The client allows HTTPS only (HTTP for genuine loopback), refuses redirects,
  uses OS trust roots and bounds response bodies. 401 always stops; there is no
  token refresh. Retry policy belongs to the agent/worker, never `api`.
- Never place the token, pairing code, response/problem bodies or full URLs in
  logs, telemetry or `Debug`; `ApiError::Response` text reaches only the
  operator.
- A contract change updates `agent::testing::Holuto` in the same change and is
  proven through the production `ureq` client. The fake validates normal traffic
  against the OpenAPI schema; scripted malformed faults deliberately bypass it.
- The fake documents behavior, not backend source: never cite files or lines of
  the private backend repository.

Contract reference: [fiscal contract](../../docs/fiscal-contract.md);
procedure: [compatibility](../../docs/testing/compatibility.md).
