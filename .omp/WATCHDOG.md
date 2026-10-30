# Project review priorities

The project hard requirements below are blocker-level when violated.

@RULES.md

Before reporting a product bug, trace a concrete input through current callers
to a wrong result. Cite the path, line and broken guarantee. Label missing
evidence separately from demonstrated defects. An unreachable state in a
defensive fixture alone is not a product bug.

For an exhaustive audit, compare claimed coverage with the maintained-file
inventory, including test bodies, fixtures and harnesses. Test names and passing
suites do not prove review coverage.

Review in this order, using [the test checklist](../docs/testing/checklist.md):

1. Duplicate fiscal effects: any retry, replay or second submission of a printer
   command, including after timeout, restart or a repeated click.
2. Lost or overwritten evidence: journal writes after dispatch, deleted or reset
   journals or stale completions overwriting newer state.
3. Uncertainty that fails to block, or a "certain" refusal broader than
   [recovery classification](../docs/testing/fiscal-safety.md#recovery-classification)
   and its vendor-evidence admission criterion.
4. Exact output: amounts, VAT slots, payments, Polish text and command order.
5. Secrets and privacy: the device token or pairing code in logs, errors,
   telemetry, `Debug`, or sent to a host it was not paired with.
6. Responsiveness and shutdown: blocking calls on the GUI thread, callbacks that
   sync the window without capturing input, quit paths that skip
   `Event::Stopped`.

Require boundary evidence for fixed bugs (a test that fails without the fix), not
private-helper or count-only tests. Challenge fakes that skip the production HTTP
client, UI tests replacing application callbacks, unreviewed fiscal snapshots,
tests asserting the buggy behavior, wall-clock timing in exact-sequence tests,
platform claims proved only on Linux, and cleanups that delete fail-closed
branches, redaction, installer integrity checks or regression tests.

For instrumentation, inspect every envelope surface against the
[telemetry policy](../docs/telemetry.md); structured scrubbing is not a promise
that native dump memory contains no secrets.

This repository is public: flag committed references to gitignored paths other
than build output, local-agent plans, private backend source files or lines, and
documentation that restates a fact owned elsewhere.
