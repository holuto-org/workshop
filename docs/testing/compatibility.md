# Compatibility testing

## Stored-file contracts

Keep sanitized, literal golden fixtures, not fixtures serialized by the
reader/writer under test. A round trip proves only today's writer agrees with
today's reader. Until the first Workshop release, format changes use a clean
cutover: update readers, writers and fixtures together, without migrations or
legacy fallbacks ([external surfaces](../../AGENTS.md#external-surfaces)).

Use the existing evidence homes when changing a stored format:

- **`operation.json`**:
  [journal tests and literal fixtures](../../crates/agent/src/journal/tests.rs)
  cover receipt/report variants, required nullable keys, device stages,
  results and settlements. Check both exact written bytes and restored values,
  including blocked/settled meaning after reopening the state directory.
- **`settings.json`**:
  [settings tests](../../crates/agent/src/settings/tests.rs) pin the format,
  diagnostics opt-out at startup and rejected saves preserving pairing.
  The [reader](../../crates/agent/src/settings.rs) requires nullable `vendor`
  and explicit `theme` and `language` keys. `language` is `system`, `en`, `pl`,
  `es` or `ru`; fresh settings use `system`.
- **`status.json`**:
  [snapshot tests](../../crates/agent/src/status/tests.rs) pin the display
  shape, disconnected identity/block flags and credential-free contents.
  A snapshot is not recovery authority; test its consumers' application-lock
  checks, not just serialization.

Stored readers reject unknown fields; wire response readers intentionally
ignore unused fields. Test these separately. Missing files, malformed files
and I/O failures must not collapse into the same “unconfigured” outcome.
An unreadable journal blocks; never delete, reset or silently rewrite it as
recovery. Exercise recovery through the
[agent tests](../../crates/agent/src/agent/tests.rs), not only the journal
parser: binding, attempt, device evidence and remote lifecycle state decide
settlement. Recovery must not rerun claim-time receipt validation or reissue
printer commands.

Run the [store tests](../../crates/agent/src/store/tests.rs) natively on each
supported OS for replacement, open readers, process locks, state paths and
Unix permissions. Check read-only loads create no state, failed replacement
preserves prior evidence and stale Windows temporary names neither block a
write nor lose crash evidence. Linux success does not establish Windows behavior.
Windows replacement can fail on sharing, access or filesystem restrictions;
the open-reader test covers its particular handle and filesystem, not every
Windows sharing mode. Synced temporary files and atomic replacement are not a
tested guarantee against power loss, storage-controller failure or every
filesystem. See [native platform procedures](platforms.md).

## `/agent` and the backend

Follow the [sync procedure](../../AGENTS.md#api-contract). The vendored OpenAPI
defines the expected wire contract; it is neither evidence of the deployed
version nor a complete lifecycle specification. Keep these checks distinct:

1. **Schema and wire types**:
   [contract tests](../../crates/agent/src/api/contract/tests.rs) check that
   every response field the client reads is required by the schema and that
   decoded enums match its values. They also exercise required nullable keys.
   These checks do not compare every field's Rust type with its schema.
   The fake validates normal JSON bodies against the vendored schema,
   including formats and patterns, in test and agent `testing` builds.
   Headers, authorization, transitions and idempotency need behavioral tests;
   body-schema validation cannot prove them.
2. **Fiscal boundary**: Runtime wire decoding is not full schema validation.
   Fiscal amount/name/NIP values stay strings until domain validation so a
   structurally decodable but invalid claimed job can be journaled and reported
   failed, without a fiscal command. Test that path independently of malformed
   JSON, missing required keys or unknown enums, which prevent wire decoding.
   Use `Fault::Body` for deliberately malformed responses that schema validation
   would otherwise stop before the client sees them.
3. **Lifecycle**: Update the [loopback fake](../../crates/agent/src/testing.rs)
   alongside contract changes, then exercise the production `ureq` client and
   assert both requests and resulting fake state in
   [API tests](../../crates/agent/src/api/tests.rs) and
   [agent recovery tests](../../crates/agent/src/agent/tests.rs).
   Cover pairing, session vendor/unique-number binding, heartbeat headers,
   capability/VAT telemetry, take attempts, job reads, terminal states, exact
   result echo, stale-attempt refusal and commit-then-drop/restart without
   reprinting. Staff resolution applies only to unknown jobs; completed
   evidence conflicting with failed/expired needs local acknowledgement, not a
   server write. The [fiscal contract](../fiscal-contract.md) owns these
   lifecycle decisions.
   Keep the fake's drop-time panic propagation: a schema violation on its server
   thread must fail the owning test, not appear only as a client HTTP error.

Capability telemetry must match the connected driver. Check unsupported
advances, zero-total sales and excess quantity precision at both boundaries:
backend refusal belongs to backend tests; local refusal before fiscal dispatch
belongs here. The fake accepts synthetic queued jobs and does not prove the
backend's job-creation eligibility rules. Company-footer availability is a
local printer-header check, not a server eligibility guarantee.

The fake proves only its modeled behavior. Review backend changes together,
obtain sanitized contract examples and backend test evidence, and deploy the
backend before a client that requires the new contract. No repository check
compares the live API with the vendored schema. Never probe production with a
real token or queue fiscal work as a compatibility check.

## Dependencies and features

Keep `Cargo.lock` and workspace dependency changes deliberate. The root
[`Cargo.toml`](../../Cargo.toml) owns versions and why dependencies exist;
do not duplicate its pins here. Slint's testing backend is an internal
preliminary API without patch-level compatibility guarantees
([upstream warning](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/)).
Upgrade it with Slint and rerun the [UI scenarios](ui.md), including accessible
labels; do not patch around a mismatched backend.

For a dependency/toolchain upgrade, use the [CI checks](../ci.md) and
[native package procedures](platforms.md#package-checks): compile/lint default
production features as well as all debug/test features, run native tests and
rebuild/verify all five OS/architecture packages. Include advisory/license and
unused-dependency checks. An all-feature run alone does not prove the real
serial branch builds.

Keep debug-assertion rejection checks for desktop `simulator`, agent `testing`
and the separate `workshop_acceptance` cfg. The drivers' own `simulator`
features have no release guard. Ordinary `--all-features` tests must retain
production fiscal-mode and unqualified-capability gates; acceptance is not a
feature and uses its
[isolated suite and release-rejection check](hardware-acceptance.md).
Neither native packages nor simulator acceptance qualify physical printers.
