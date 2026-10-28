# Fiscal correctness and reliability

The supported fiscal commands have no idempotent replay contract. A missing
reply does not prove nothing happened; even a successful reply can mean only
acceptance (Elzab daily `RRP`), not completion.
[AWS's retry guidance](https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/)
explains the side-effect distinction. Workshop chooses **at-most-once
dispatch**, not exactly-once printing.

## The two delivery guarantees

- **Fiscal device**: Persist each fiscal stage before sending its command.
  Never retry or reprint. If execution cannot be determined, retain evidence
  and block across restarts for a person.
- **Holuto result**: Persist the result before reporting. Recovery reconciles
  server state first and, when permitted, resends the identical saved result
  for the same attempt and printer issue time. This is not a second print or
  an unconditional guarantee that Holuto will accept it.

Use a fresh temporary `StateDir` with the production durable writer and HTTP
client, never a workstation's saved state. Script both failure before result
commit and `agent::testing::Fault::DropAfterCommit`. After reconstructing the
agent or worker against that directory, assert server state/request bodies,
exact journal, device effects and no replayed fiscal frames. A lost committed
acknowledgement may settle by reading terminal server state without sending the
result again.

Receipt claim intent (`NotStarted`) is saved before receipt preflight. Reports
enter the journal only at their first fiscal dispatch, after preflight. In both
paths, `Journal::dispatching` must finish durably before the corresponding send;
failure prevents that send. Inject failures at claim, first/later dispatch,
completion, result-outbox and settlement writes. The previous durable evidence,
not a command name or an advanced counter, decides recovery after interruption.

## Fault matrix

For each supported receipt/report path, select the initial command, intermediate
commands, fiscal close and any supported footer. Exercise these distinct shapes:

- Matched explicit refusal **without execution**: checks classification only.
- Wrong-command reply (Posnet/Novitus) or wrong response ID (Elzab): checks
  correlation, never proof of refusal.
- Execute once, then lose, corrupt or disconnect before delivering the reply:
  checks uncertainty after an effect.

Inspect simulator effects separately from driver errors, and assert the exact
sent prefix and journal stage. `Fault::Reply` differs by simulator: Posnet
replaces normal command handling; Novitus/Elzab execute before substituting the
response. Posnet `Error`/`Device`/`Refused`, Novitus `Refuse` and Elzab
`Device`/`Transport` skip execution. None proves the corresponding hardware
error has no effects.

Fix the printer clock and readiness with each brand's `simulator::Simulator`.
Posnet's `hold`/`reached` lets a worker test read the journal before a held
command executes while asynchronous `!sprn`/`!sdev` remain answerable.
Release holds and join threads on failure paths. Use accelerated simulator
deadlines rather than a real ten-minute wait; production-deadline assertions
need an unscaled link as described in the
[test rule](../../.omp/rules/writing-tests.md#per-kind).

Vary identity, fiscal mode, header, mechanism, queue, transaction/footer state
and current VAT in the execution session. A **preflight** refusal must leave
zero fiscal effects; a later refusal may follow earlier effects. Discovery is
not permission to skip preflight or switch printers while blocked.

## Recovery classification

For receipt recovery, vary both durable device evidence **and saved result**,
server state, printer binding and attempt:

- **`NotStarted`; server still taken**: report failed; no fiscal command was
  sent. Acceptance settles the journal.
- **`NotStarted`; server unknown (for example taken-attempt expiry)**: retain
  the journal and staff notice. Holuto does not accept automatic failed over
  unknown. Never print.
- **`Dispatching`, with a saved certain-failure result**: while taken, replay
  that failed result, not commands. If the server is unknown, retain evidence
  for staff. `Dispatching` alone does not override a saved refusal result.
- **`Dispatching`, without certain-failure or completed evidence**: report or
  retain unknown and block receipts/reports across restart. Never infer failure
  or completion from a stage name, readiness or counter.
- **Completed with readable receipt number**: persist and reconcile the same
  done result and printer issue time.
- **Completed with unreadable number**: keep completed evidence and report
  unknown; staff supply the number in Holuto. Never reprint.
- **Completed but Holuto failed/expired**: retain the journal and block.
  The notice identifies job/order and receipt number or an unreadable-number
  marker. Only explicit **local operator acknowledgement**, on the same ready
  vendor/unique number and after rechecking server binding/attempt/state, settles.
  It neither prints nor reports to Holuto; staff cannot resolve an already
  failed/expired job as if it were unknown.
- **Ordinary unknown receipt resolved by staff**: reconcile Holuto's terminal
  decision without printing. Local orphan acknowledgement must refuse this
  case. A terminal `voided` receipt also settles without replacement printing.
- **Mismatched binding/attempt/vendor/unique number, corrupt or unreadable
  journal**: fail closed and preserve evidence; never delete/reset the file.

An interrupted report requires explicit `Command::ResolveReport` on the same
ready printer; it records acknowledgement, prints nothing and sends no receipt
result. An overdue daily report or read-only mode alone does not prevent either
local resolution, or the report needed to recover could never run again.

The receipt exception is `Command::AcknowledgeOrphanedReceipt`, exposed through
`Event::Device.orphaned_receipt`. It is not a general acknowledgement of an
ordinary unknown receipt. Test successful resolution and every refused
resolution for unchanged evidence and zero new fiscal frames.

Backend expiry distinguishes queued jobs (expired) from taken attempts
(unknown). Exercise both rather than making a fake expire every state
identically. Result notes are trimmed and NFC-canonicalized to the backend's
form before journaling and sending; replay preserves that canonical note.

Admitting a new post-send **certain** refusal requires vendor evidence that the
operation is rejected before printing or fiscal-memory effects. An error-code
name/table or an execute-skipping simulator fault alone is insufficient. Default
new cases to **uncertain**; do not broaden certainty to make recovery easier.

The **currently implemented** initial-stage classifier accepts only valid replies
correlated to the sent command:

- **Posnet:** `trinit` refusal; `ERR` frame rejection naming `dailyrep`,
  `monthlyrep` or `periodicrepbydates`; code 382 naming `dailyrep`.
  POT-I-DEV-37 pp. 13–15 distinguishes frame rejection from execution errors;
  `dailyrep` p. 211 prohibits consecutive zero reports, and p. 441 names 382.
- **Novitus:** `$h` refusal; codes 36 and 37 naming `#r`; code 4 naming `#o`.
  v1.15 §3.4.1 pp. 91–93 says `$h` sets transaction bits without printing the
  header. §3.7.1 pp. 137–138 describes an already-recorded zero report (36)
  and cancellation before printing (37). §3.7.2 pp. 138–139 describes code 4
  as invalid dates/range or no fiscal records **in an error table**; that entry
  alone is not an independent no-effects guarantee. Preserve that distinction
  when reviewing the existing exception; a passing `Refuse(4)` test supplies no
  additional vendor evidence. Track the missing evidence under
  [open hardware questions](hardware-acceptance.md#open-hardware-questions).
- **Elzab:** transport rejection of the initial single-frame `LOR` only.
  *Basics of ElzabSTX communications protocol*, release 0.16,
  §§1.4.3, 1.4.7 and 2 describe assembly before execution, response IDs and
  transport statuses. A command-level error is not a transport rejection.

This list records host policy, not measured hardware qualification or proof
that every listed error has no effects. Sources and acquisition links live in
the [fiscal contract](../fiscal-contract.md#specification-sources); hardware
observations belong in [hardware acceptance](hardware-acceptance.md).

All other execution errors, lost replies and mismatched replies remain uncertain,
including other report date-error replies and later receipt refusals through
fiscal close. No refusal permits resending. A successful Posnet `trend` completes
the fiscal record (`strns.fe`, POT-I-DEV-37 p. 377), so a failing
`trftrln`/`trftrend` footer still returns the printed receipt; unfinished footer
state keeps preflight not ready. An accepted Elzab daily `RRP` remains uncertain
even if counters advance; acceptance is not completion.

### Existing evidence entry points

These are starting points to inspect, **not a claim that every matrix cell or
hardware outcome is covered**:

- [Agent scenarios](../../crates/agent/src/agent/tests.rs): result replay before
  commit/after lost acknowledgement, write failures, restart, expired attempts,
  unreadable numbers and refused/local/staff resolution.
- [Worker scenarios](../../crates/agent/src/worker/tests.rs): journal observed
  before each held Posnet fiscal command, cooperative stop, lost replies across
  restart and explicit report resolution.
- [Driver faults: Posnet](../../crates/posnet/src/tests.rs),
  [Novitus](../../crates/novitus/src/printer/tests.rs) and
  [Elzab](../../crates/elzab/src/tests.rs): exact sent prefixes, effects,
  journal failures and brand-specific refusal/completion classification.
- [Reconciliation model](../../crates/agent/src/agent/tests/reconciliation.rs):
  generates settlement, network failure, staff resolution and orphan
  acknowledgement from seeded receipt evidence/server states. It reconstructs
  the agent per action, but does **not** generate the claim/dispatch/print
  lifecycle, report operations or concurrent worker actions.

For changed transitions, use table scenarios plus an independent reference model
with invariants after each action
([model-testing guide](https://proptest-rs.github.io/proptest/proptest/state-machine.html)).
Keep filesystem/HTTP scenarios: a pure model does not exercise serialization or
the durable writer, and simulated write failures do not prove power-loss behavior
on every filesystem.

## Exact fiscal output

Use independent published frame/CRC vectors and fixed-clock output snapshots.
Review sale, advance, mixed VAT, deductions, split payment, credit, fully prepaid
and optional footer where the brand/profile supports them; unsupported paths
must instead refuse before dispatch. Assert exact command order and field values
in addition to snapshots. Each brand's
[published specification](../fiscal-contract.md#specification-sources) is the
protocol oracle; a driver and simulator sharing a helper are not independent
evidence, and examples do not qualify hardware.

Properties compare quantity × unit price half-up rounding to independent integer
arithmetic, validate range/overflow, per-slot sales/deductions and exact payment
coverage. Generate boundaries around half-grosz and maximum values. Reordering
independent lines must preserve totals, not necessarily wire order. Never invent
a rounding relation that the contract does not promise.

Preserve complete names/NIP/payment fields or reject before dispatch; never
normalize away unsupported text. Freeze printer time, including Warsaw DST
repeated/skipped times; never replace printer time with the host clock. Inject
`today` only into pure UI/date functions.

The **Stan** tab is display state, not fiscal evidence. Unknown outcomes remain
blocked until a person resolves them; display state must never turn
uncertainty into permission to print.
