# Fiscal correctness and reliability

The device offers no idempotency key for receipt/report commands. Its reply
follows execution: a timeout does not prove nothing happened.
[AWS's retry guidance](https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/)
explains that distinction for side effects. Here the decision is **at-most-once
dispatch**, not blind retry and not a claim of exactly-once printing.

## The two delivery guarantees

- **Fiscal device**: Persist intent before the first command and each stage
  before sending. Never retry a fiscal command. If execution cannot be
  determined, retain evidence and block across restarts for a person.
- **Holuto result**: Persist the result before reporting; resend only that
  identical result for the same attempt and issue time, subject to server
  reconciliation/idempotency. This is not a second print.

Use the real state directory and production HTTP client. The Holuto fake must
commit a result and then lose its answer as well as fail before commit. After
restart, assert the fake's state/request log, exact journal and history, device
effects, and no replayed fiscal frames.

## Fault matrix

At the first, middle, final and footer commands of each driver, exercise:
matched explicit refusal; mismatched reply correlation; execute then lose reply;
execute then corrupt reply; execute then close connection. Posnet/Novitus replies
name commands; Elzab correlates IDs, not command names. Inspect simulator effects
separately from what the driver received. A pre-execution error cannot prove
post-execution uncertainty.

Each brand's `simulator::Simulator` fixes printer time (`set_clock`), varies
documented readiness (`set_status`) and injects execute-first faults through its
`Fault` enum; Posnet's `hold`/`reached` order a test while asynchronous
`!sprn`/`!sdev` remain answerable. Simulator links accelerate timing: tests must
not change production deadlines or take a real ten-minute operator wait. Always
release holds on failure paths.

Prove identity, fiscal mode, header, mechanism, queue, transaction/footer state
and current VAT before every operation in its execution session. Refusal must
leave zero fiscal effects. Discovery only identifies candidates; it is not
authorization to skip preflight or switch printers while blocked.

## Recovery classification

- **Claimed, `Device::NotStarted`, including after crash; server still taken**:
  Report failed; no fiscal command was sent, so do not block after acceptance.
- **`NotStarted` but server already unknown (for example taken-attempt
  expiry)**: Keep journal and notice for staff to resolve failed in Holuto; the
  backend does not accept an automatic failed result over unknown. Never print.
- **`Dispatching` or otherwise uncertain**: Report/retain unknown as appropriate;
  block receipts and reports after restart. No automatic failed result or replay.
- **Completed with readable receipt number**: Persist and reconcile the same
  done result and printer issue time.
- **Completed with unreadable number**: Keep completed evidence, remain
  unresolved; staff reconcile the number in Holuto. Never reprint.
- **Completed but Holuto already failed/expired**: Retain journal and block.
  Notice names job/order and receipt number or `nieczytelny`. Only explicit
  **local operator acknowledgement** on the same ready vendor/unique number
  appends history and settles; it neither prints nor reports to Holuto. Backend
  staff resolution cannot resolve these terminal states.
- **Mismatched attempt/vendor/unique number, corrupt or unreadable journal**:
  Fail closed, preserving evidence; never delete the file as recovery.
- **Interrupted report**: Block until explicit report resolution with the same
  ready printer; resolution prints nothing. An overdue daily report or read-only
  mode alone does not count as not ready for either local resolution, or the
  report could never run again.

The worker command for the local exception is
`Command::AcknowledgeOrphanedReceipt`; `Event::Device.orphaned_receipt` exposes
that state to the UI. It is not a general-purpose acknowledgement of an ordinary
unknown receipt.

Backend expiry distinguishes queued jobs (expired) from taken attempts
(unknown). Exercise both rather than making a fake expire every state
identically. Result notes are trimmed and NFC-canonicalized to the backend's
form before journaling and sending; replay preserves that canonical note.

A post-send refusal is **certain** only with a vendor statement that it rejects
the operation before any print or fiscal-memory effect. An error-code name or
table alone is insufficient. Posnet `Refused`, Novitus `Refuse` and Elzab
`Device`/`Transport` simulator faults skip execution: they test classification,
never prove that hardware has no effects. Default to **uncertain**; admit no
additional code without that vendor evidence.

The complete certain-refusal list (only at the initial dispatch stage, with a
valid reply matched to the sent command) is:

- **Posnet:** `trinit` refusal; `ERR` frame rejection naming `dailyrep`,
  `monthlyrep` or `periodicrepbydates`; code 382 naming `dailyrep` (second
  consecutive zero daily report, refused before printing).
- **Novitus:** `$h` refusal; codes 36 and 37 naming `#r` (zero daily report
  already recorded, or keyboard cancellation before printing); code 4 naming
  `#o` (invalid range or no fiscal records in it).
- **Elzab:** transport rejection of the initial single-frame `LOR` only.

All other execution errors, lost replies and mismatched replies remain uncertain,
including report date-error replies and later receipt refusals through fiscal
close. No refusal permits resending.
A successful Posnet `trend` completes the fiscal record (`strns.fe`,
POT-I-DEV-37 p. 377), so a failing `trftrln`/`trftrend` footer still returns the
printed receipt; the unfinished footer keeps preflight not ready. An accepted
Elzab daily `RRP` remains uncertain even if counters advance. Sources are in the
[fiscal contract](../fiscal-contract.md#specification-sources).

Table-test each state/command pair, then use `proptest-state-machine` to
generate claim/dispatch/complete/report/restart/reconcile/acknowledge sequences.
The reference model records at-most-once effects and retained evidence; it must
not call production transition helpers. Check invariants after every transition
([model-testing guide](https://proptest-rs.github.io/proptest/proptest/state-machine.html)).
Keep real filesystem/HTTP scenarios for each legal path: a pure model alone
cannot prove fsync or serialization.

## Exact fiscal output

Use independent published frame/CRC vectors and fixed-clock output snapshots.
Review every receipt kind: advance, sale, mixed VAT, deductions, split payment,
credit, fully prepaid and optional footer. Assert exact command order and field
values in addition to the snapshot. Each brand's
[published specification](../fiscal-contract.md#specification-sources) is the
output oracle; simulator and driver sharing a helper is not an independent one.

Properties compare quantity × unit price half-up rounding to independent integer
arithmetic, validate range/overflow, per-slot sales/deductions and exact payment
coverage. Generate boundaries around half-grosz and maximum values. Reordering
independent lines must preserve totals, not necessarily wire order. Never invent
a rounding relation that the contract does not promise.

Preserve complete names/NIP/payment fields or reject before dispatch; never
normalize away unsupported text. Freeze printer time, including Warsaw DST
repeated/skipped times; never replace printer time with the host clock. Inject
`today` only into pure UI/date functions.

History is a display cache: its read/write failure produces a notice without
preventing settlement of a server-accepted result. Assert both the accepted
result and retained fiscal evidence when injecting that failure.
