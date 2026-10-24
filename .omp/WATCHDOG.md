# Project review priorities

The project hard requirements below are blocker-level when violated.

@RULES.md

Review the highest fiscal risks first using
[the test checklist](../docs/testing/checklist.md): duplicate effects,
lost/overwritten evidence, uncertainty that fails to block, exact output, then
responsiveness and availability.

Require boundary evidence for fixed bugs, not private-helper/count-only tests.
Challenge fakes that skip the production HTTP client, UI tests replacing
application callbacks, unreviewed fiscal snapshots and platform claims proved
only on Linux.

For instrumentation, inspect every envelope surface and the
[telemetry policy](../docs/telemetry.md); structured scrubbing is not a promise
that native dump memory contains no secrets.
