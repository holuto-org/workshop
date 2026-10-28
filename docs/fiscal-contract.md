# Fiscal contract

Reference for the Holuto `/agent` API, the device profile each driver
advertises, discovery, and the printer command sequences. Operator-facing
behavior lives in the [README](../README.md); the invariants are in
[README "Fiscal safety"](../README.md#fiscal-safety).

## Evidence and scope

- **Implemented support** means a path is enabled in an ordinary production build
  for a matching profile and fresh preflight. It is not hardware certification.
- **Vendor specification** describes the named protocol revision. **Host policy**
  may be stricter: capability gates, conservative limits, date guards, deadlines
  and uncertain-outcome handling below are Workshop's decisions, not universal
  printer properties.
- **Simulator evidence** exercises command bytes, validation and recovery through
  the real driver. It does not measure paper output, firmware behavior or timing.
- **Hardware qualification** needs the separate
  [supervised acceptance record](testing/hardware-acceptance.md). This reference
  does not establish a measured model/firmware/transport/OS qualification matrix.

## `/agent` API

The contract is the backend's generated OpenAPI document, vendored as
[`openapi/agent.json`](../openapi/agent.json); `crates/agent` hand-writes its
wire types and its contract tests check them against it. The test fake validates
normal requests/responses against it; scripted malformed-response faults
deliberately bypass validation.
See [contract synchronization](testing/compatibility.md#agent-and-the-backend)
for the update procedure. Nothing checks the live API against this copy: deploy
backend changes first.

Authenticated requests use `/agent`, `Authorization: Bearer <device token>`,
`App-Version: agent/<version>` and `Accept-Language: pl`, plus SDK-provided
`sentry-trace` propagation. `POST /agent/pair` exchanges `{code}` for
`{token, printerId}` without bearer or device-identity headers.
`POST /agent/session` sends `Telemetry` at startup and when the profile changes;
its exact fields are defined in the linked schema.

`vendor` is `"posnet"`, `"novitus"` or `"elzab"` and binds alongside the physical
unique number. `vatRates` contains supported active entries from the device
table: letters A–G and rates `"23"`, `"8"`, `"5"`, `"0"` or `"zw"`. Each session
replaces the backend mapping.
A–E mean 23%, 8%, 5%, 0% and exempt respectively. Other programmed rates stay
visible locally, but telemetry omits rates outside Holuto's supported set.
Illegal A–E assignments remain not ready and are not uploaded.
The `capabilities` object carries `maxItemNameLength`, `maxLinesPerReceipt`,
`maxCompanyNameLength`, `allowedCharacters` (printable characters, NFC),
`maxQuantityDecimals` (0–8), `advanceDeductions` and `zeroTotalReceipts`.
The schema documents 409 `fiscal_printer_limits_unknown` when queueing without
telemetry. It does not encode every backend business-validation rule.
Workshop does not call the queueing route: it independently checks the fresh
device profile before fiscal commands and reports unprintable claimed jobs as
failed. Neither a queued job nor an accepted session authorizes bypassing local
validation.

Sessions bind the fiscal memory unique number (`^[A-Z]{3}[0-9]{10}$`) and vendor;
a mismatch fails with 403 `fiscal_unique_number_mismatch` or
`fiscal_vendor_mismatch`. Bodyless `POST /agent/heartbeat` returns 204.
Authenticated routes other than `/agent/session` carry
`X-Device-Unique-Number`; the session carries `uniqueNumber` in its body.
Device swaps are rejected before taking or printing jobs.

Workshop calls only pair, session, heartbeat and the job routes below. The
vendored schema also lists backend document-search, queueing and job-search
operations; Workshop does not call them.

- **`POST /agent/jobs/take`**: Claim a job; no body, 204 for an empty queue.
- **`GET /agent/jobs/{id}`**: Reconcile job state without printing.
- **`POST /agent/jobs/{id}/result`**:
  `{attempt, state, receiptNumber, issuedAt, result}`. State is `done`,
  `failed` or `unknown`; `done` requires receipt number and issue time,
  otherwise both are null.

The client requires this backend lifecycle policy, modeled by the HTTP fake,
not guaranteed by OpenAPI response shapes or a check of the deployed service:
a lost `jobs/take` response is never recovered by re-serving the taken job.
The client executes only a job whose response it received; there is no local
claim journal for a lost response. While that printer has a taken or unknown
job, subsequent takes return 204. Queued jobs expire to expired; taken attempts
expire to unknown and need reconciliation or staff resolution, never redispatch.
`instance.lock` protects one state directory, not the paired token across
directories. Re-serving could therefore dispatch one attempt from two instances.
The same attempt's identical saved result may be resent, not its printer
commands. An unknown attempt may accept completed evidence as done, but cannot
be cleared automatically as failed. Terminal server states, conflicting printed
evidence and local acknowledgement follow
[recovery classification](testing/fiscal-safety.md#recovery-classification).

Job lines: `name`, `quantity`, `unitPrice`, `total`, `vatRate`, `vatLetter`.
Metadata: `orderId`, integer `orderNumber`, `printerId`, `kind`, nullable
`allocationId`, `etag`, `buyerNip`, `companyName`, `total`, `advances`,
`payments`, immutable attempt and lifecycle timestamps. Advances identify their
allocation and have one line per VAT rate, quantity 1. Outcome fields are
always present and nullable: `receiptNumber`, `issuedAt` (the printer's issue
time of the receipt, set exactly when the job has one: `done` or `voided`) and
`result`. Every job response (take, get, result reply) carries them.

`total` sums gross line totals. `advances` contains `{vatLetter, vatRate, amount}`
deductions, at most one per letter, each positive; duplicates and zero deductions
fail before fiscal commands. Advance jobs have no deductions. `payments`
contains `{form, amount}`; forms are `cash`, `card`, `transfer`, `mobile` and
`credit`. Decimals are unsigned plain strings such as `12.50`, with an optional
`.` fraction; no exponent or digit grouping. Use a matching backend; older
protocol shapes are unsupported.

### Receipt validation

These are shared host checks in `fiscal::receipt` and `agent::agent`, followed by
each driver's same-session preflight and wire validation:

- Quantity and unit price must be positive; each line's quantity × unit price is
  rounded half up to grosze and must remain positive. Supplied line totals and the
  gross job total must match exactly. Although the API unit-price scalar permits
  eight fractional places, Workshop accepts money only with at most two,
  including trailing zeros; it never rounds a supplied price to make it fit.
- Deductions cannot exceed gross sales in their VAT slot; slot/rate pairs must
  match current telemetry. Payments must exactly cover gross sales minus
  deductions, including a zero balance. A zero-value payment sends no payment
  command. An advance receipt is distinct from a sale deducting earlier advances.
- Text is sent unchanged or rejected before dispatch: no truncation,
  transliteration or replacement. Lengths count Unicode scalar values, not UTF-8
  bytes. Each field must fit the fresh repertoire and local limits.
  Header matching alone folds case and whitespace; it does not rewrite output.
  Paper fidelity and printer-side name comparison still need hardware evidence.

## Device capabilities

These limits come from each driver's `fiscal::device::Capabilities`. The wire
group contains seven fields (five limits/repertoire fields and two receipt
flags); buyer-NIP length and `company_footer` stay local.
`posnet::capabilities` defines the documented Temo Online limits: eight quantity
decimals, 80-character item names (`trline`), 500 lines (`trinit`),
40-character company footer lines (`trftrln`) and printable CP1250 from
0x20–0xFF, excluding DEL, no-break space, soft hyphen and table-drawing glyphs
(`¤ ¨ ˛ ´ ¸`, POT-I-DEV-37 p. 14).

| Enabled in production build | Posnet | Novitus | Elzab |
| --- | --- | --- | --- |
| Advance deductions | yes | no | yes |
| Zero-total receipts | yes | no | no |
| Company footer | yes | no | no |

Novitus uses a host ceiling of 255 lines, eight quantity decimals,
40/60-character names and conservative model-specific 40/42/44-character
company-name widths. Exact firmware spellings live in
[`novitus::profile`](../crates/novitus/src/profile.rs); unknown versions have no
fallback. Elzab uses a host ceiling of 255 lines, 64-character names,
42-character company names and five quantity decimals. The 255-line ceilings
are not measured hardware buffer sizes. Neither brand inherits Posnet's limits.
A missing footer capability is a local check: the company name must already
occur in the decoded header or the job fails before dispatch. Buyer-NIP limits
and extra wire restrictions also remain local: for example Novitus accepts at
most 16 ASCII alphanumeric NIP characters and ten quantity digits in total.
Debug acceptance flags never belong in production telemetry.

## Discovery and readiness

Discovery first checks the configured port's remembered vendor, otherwise its
USB hint. Unhinted ports receive only read-only identity probes, in order:
Posnet `getrealid`, ElzabSTX `XTR`, Novitus `30#v`. Novitus also requires PC1;
non-native dialects remain unready. Failed cached identities can be
rediscovered only when no operation blocks. The detected port and vendor are
saved together. An unresolved journal pins the vendor and forbids scanning
other ports, even when preferences are stale.

An unidentified capability profile (for example unknown Novitus firmware or an
Elzab code page other than CP1250) stays visible as not ready. The worker keeps
running and rechecks it every 15 seconds, without sending invalid session
telemetry or claiming jobs. Once the device has describable capabilities,
connection and normal readiness checks resume. Novitus discovery accepts both
power-on error mode 0 and preconfigured mode 3 without changing printer settings.

## Report dates and printer clock

Workshop's date guards use the **printer's date**, not the host date or a guessed
UTC instant. Its Posnet driver permits a monthly report for the printer's current
month: `monthlyrep` is specified as a periodic report covering the selected month
(POT-I-DEV-37 p. 216). The driver rejects a monthly/periodic range starting after
the printer date or ending before fiscalisation; it does not universally require
`to ≤ today`. Novitus and Elzab drivers require a completed month (its last date
strictly before the printer date); periodic reports require
`from ≤ to ≤ printer date`. These guards do not prove acceptance on every
candidate firmware or turn a post-send error into a certain refusal.

Receipt issue times must identify a Warsaw instant. Posnet prefers
offset-bearing `rtcget.tm`; legacy civil `da` chooses the earlier repeated
instant and refuses a receipt in the skipped spring hour.
Novitus (`#c`) and Elzab (`XCR`) send civil time without an offset:
only receipts require it to be an unambiguous Warsaw instant. Probe, identify,
verify and reports validate civil time and use its date, so the repeated or
skipped hour does not block them. Both native two-digit clock parsers currently
accept 2000–2049; their report encoders permit 2000–2099. The narrower clock
range is host policy, not a claimed vendor protocol limit. Receipt issue times
are sampled before dispatch and recorded to minute precision, not measured at
physical paper completion.

## Specification sources

- **Posnet:** [POT-I-DEV-37 v5978][posnet-spec], Online MR2021, dated
  2 November 2023 (vendor-authored PDF hosted in a public third-party mirror).
  Bare `p.`/`pp.` references in Posnet code and this document mean the
  specification's printed page numbers (for example `295/450`), not
  extracted-text lines. Receipt/NIP/payment definitions are on pp. 295,
  297 and 314; `tradvance` explicitly permits an advance-covered zero close on
  p. 323.
- **Novitus:** [Protokół komunikacyjny NOVITUS v1.15 (ver Online)][novitus-spec];
  [v1.13][novitus-113] is also publicly available. Cite the version and command
  or section when comparing revisions; do not silently transfer semantics.
- **Elzab:** *Protokół ElzabSTX dla drukarek fiskalnych*, edition 0.36,
  19 December 2025, and *Drukarki online – obsługa protokołu*, distributed as
  `om_iprg.zip` under **Drukarki fiskalne ONLINE** on the
  [vendor download page][elzab-downloads]. The same archive includes
  *Basics of ElzabSTX communications protocol* in `ENGLISH/ElzabSTX`.
  Cite document/revision and command, using the ONLINE-specific definition
  where present: shared legacy names do not establish ONLINE semantics.
  Do not redistribute vendor documents or extracted text.

For local searching: `pdftotext -layout /tmp/vendor.pdf /tmp/vendor.txt`;
keep downloads and extracted text in `/tmp`, never commit them.
Label claims supported only by specification examples **unconfirmed on
hardware** and track them in
[open hardware questions](testing/hardware-acceptance.md#open-hardware-questions).
Examples are parsing fixtures, not observations of a qualified printer.

[posnet-spec]: <https://raw.githubusercontent.com/kashiash/PosnetLibrary/master/DOCS/pot-i-dev-37-5978-specyfikacja-protokolu-posnet-online-mr2021-2%20(1).pdf>
[novitus-spec]: https://novitus.pl/file/get/695
[novitus-113]: https://point.novitus.pl/files/protokol-komunikacyjny-NOVITUS-1.13-ver-Online.pdf
[elzab-downloads]: https://www.elzab.com.pl/pl/strefa-dla-klienta/do-pobrania/programy

## Drivers

### Posnet

Printer command order: `trinit`, optional `trnipset`, `trline` per line,
`tradvance` per deduction (name `Zaliczka`, VAT slot, amount, gross sales at
that slot), `trpayment` per nonzero payment, then `trend`. Payment types: cash
0, card 2, transfer 8, mobile 6 (name `Płatność mobilna`), credit 5. `trend.to`
is gross total minus advances; `trend.fp` sums payments. Fully prepaid sales
close at zero without payment commands. The [shared checks](#receipt-validation)
apply before dispatch. Every item name needs a letter, digit or one of
`, . / \ %`; the printer treats other names as empty (p. 430). Every enabled PC
interface using the POSNET protocol must report Windows-1250 (`pccfgget` `cp0`,
pp. 153–155) because the session's interface cannot be identified and all text
is sent in Windows-1250. The parser canonicalizes a valid spaced unique number
such as `ABC 1234567890` to `ABC1234567890`; the device's `getrealid` spelling
is **unconfirmed on hardware** (specification examples, pp. 378, 391–392).
After `trend` succeeds the fiscal record is complete; an optional `trftrln`
company line and `trftrend` finish the footer. Their failure does not undo the
receipt or permit a reprint; an unfinished footer blocks the next preflight.
A read-only (`scomm.td`) printer refuses sales and daily reports but still
prints monthly and periodic reports, which the specification permits in that
mode. Report dates follow the
[brand-specific guards](#report-dates-and-printer-clock); refusal certainty
follows [recovery classification](testing/fiscal-safety.md#recovery-classification).

### Novitus

`novitus::printer` implements native ESC-P receipts and daily/monthly/periodic
reports with strict model/firmware profiles for HD II Online, Deon Online,
Bono Online and Point. It verifies PC1, native dialect, CP1250 input/output,
fiscal mode, idle status, header, VAT and Warsaw clock in the printing session.
Production receipt order is `$h`, optional buyer `100;1$z`, `$l` lines,
nonzero `$b` payments, `$y`, `12$z` and `28;0$z`. The acceptance-only `@z`
deduction path is not advertised as production advance support.
Unknown firmware is not ready; there is no compatibility fallback. Read-only
`52$i` attribute 1 bit `0x0200` enables all assigned, non-control Windows-1250
characters, including Polish names; without that verified bit the repertoire
remains printable ASCII. Advances, zero-total receipts and changing company
footers are disabled pending qualification; a company absent from the header
is rejected before begin. DLE2 byte order remains unverified: readiness requires
zero flags, and execution wait reasons use only documented DLE bits. Native
identity strings and firmware-specific behavior still require qualification;
simulator tests are not hardware certification.

### Elzab

`elzab::printer` implements native ElzabSTX receipts and reports for the Zeta,
Mera and Cube ONLINE candidate profiles (exact XTR model names, not a qualified
firmware matrix). It requires CP1250 (`XHR`), online `ySR` readiness, `LPC`
receipt permission, header, VAT and Warsaw clock in the printing session.
Receipt order is `LOR`, optional `LNP`, `LIT` lines, per-slot advance `LDS`,
nonzero `LBP` payments and `LCL`. The ONLINE `LDS` extension is the basis for
enabled deductions; it does not establish zero-total support. Zero-total
receipts and company footers stay disabled.

Daily `RRP` promises acceptance, not completion (ONLINE §2.9.5.2); Workshop
therefore leaves even a positive reply uncertain and blocking. Counter changes
are not proof that this operation completed. Monthly/periodic `RPP` requests
use synchronous `S0`. Outcome handling follows
[recovery classification](testing/fiscal-safety.md#recovery-classification);
unmeasured behavior belongs in
[open hardware questions](testing/hardware-acceptance.md#open-hardware-questions).

### Silence and operator waits

- **Posnet:** item/payment reply limit 10 s, close 30 s, daily report 120 s,
  monthly/periodic report 300 s. After a timeout, asynchronous `!sprn`/`!sdev`
  can observe operator waits; the cap is ten minutes from the first observed
  wait. Intervention resets the step deadline, never resends the command.
- **Novitus:** item/payment limit 10 s, close 30 s, daily report 120 s,
  monthly/periodic 300 s. After timeout, DLE2/DLE observe documented wait bits;
  ENQ is not sent while a fiscal command is pending. The ten-minute cap starts
  at the first timeout; absent a wait, a second step deadline ends the wait.
  DLE2 byte order is not guessed.
- **Elzab:** item/payment limit 10 s, close/daily acceptance 30 s,
  monthly/periodic 600 s. No concurrent status polling is assumed: silence
  is not evidence of an operator wait. Paper-out auto-cancellation does not
  establish a certain outcome.

These are host deadlines, not measured hardware guarantees. Unexplained silence
or unreadable wait status leaves dispatch uncertain; no driver retries a command.

## Voided receipts

A `voided` job is a terminal, already-printed receipt cancelled in the
obvious-mistake register. Recovery acknowledges
its local journal without printing or reporting a replacement.
