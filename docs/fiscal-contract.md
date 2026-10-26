# Fiscal contract

Reference for the Holuto `/agent` API, the device profile each driver
advertises, discovery, and the printer command sequences. Operator-facing
behavior lives in the [README](../README.md); the invariants are in
[README "Fiscal safety"](../README.md#fiscal-safety).

## `/agent` API

The contract is the backend's generated OpenAPI document, vendored as
[`openapi/agent.json`](../openapi/agent.json); `crates/agent` generates its wire
types from it at build time. The test fake validates normal requests/responses
against it; scripted malformed-response faults deliberately bypass validation.
See [contract synchronization](testing/compatibility.md#agent-and-the-backend)
for the update procedure. Nothing checks the live API against this copy: deploy
backend changes first.

Authenticated requests use `/agent`, `Authorization: Bearer <device token>`,
`App-Version: agent/<version>` and `Accept-Language: pl`, plus SDK-provided
`sentry-trace` propagation. `POST /agent/pair` exchanges `{code}` for
`{token, printerId}` without bearer or device-identity headers.
`POST /agent/session` sends telemetry at startup and when the profile changes:

```json
{
  "vendor": "posnet",
  "model": "POSNET TEMO ONLINE",
  "firmware": "32.01",
  "appVersion": "0.2.0",
  "capabilities": {
    "advanceDeductions": true,
    "zeroTotalReceipts": true,
    "maxItemNameLength": 80,
    "maxLinesPerReceipt": 500,
    "maxCompanyNameLength": 40,
    "maxQuantityDecimals": 8,
    "allowedCharacters": " !\"#$%&'()*+,-./0123…"
  },
  "uniqueNumber": "ABC1234567890",
  "vatRates": [
    {
      "letter": "A",
      "vatRate": "23"
    },
    {
      "letter": "B",
      "vatRate": "8"
    }
    },
    {
      "letter": "C",
      "vatRate": "5"
    },
    {
      "letter": "D",
      "vatRate": "0"
    },
    {
      "letter": "E",
      "vatRate": "zw"
  ]
}
```

`vendor` is `"posnet"`, `"novitus"` or `"elzab"` and binds alongside the physical
unique number. `vatRates` is the actual device table, with letters A–G and rates
`"23"`, `"8"`, `"5"`, `"0"` or `"zw"`. Each session replaces the backend mapping.
A–E mean 23%, 8%, 5%, 0% and exempt respectively; F/G retain other programmed
rates. Illegal A–E assignments remain visible locally as not ready and are not
uploaded. `maxItemNameLength`,
`maxLinesPerReceipt`, `maxCompanyNameLength`, `allowedCharacters` (all printable
characters, NFC), `maxQuantityDecimals` (0–8) and `capabilities` are required
before queueing. The backend rejects names outside the limits, excessive
quantity precision (ignoring trailing zeros), advance deductions without
`advanceDeductions`, and fully prepaid sales without `zeroTotalReceipts`
(409 `fiscal_printer_quantity_precision`, `fiscal_printer_advances_unsupported`,
`fiscal_printer_zero_total_unsupported`). The agent independently checks the
fresh device profile before any fiscal command and reports unprintable jobs as
failed.

Sessions bind the fiscal memory unique number (`^[A-Z]{3}[0-9]{10}$`) and vendor;
a mismatch fails with 403 `fiscal_unique_number_mismatch` or
`fiscal_vendor_mismatch`. Bodyless `POST /agent/heartbeat` returns 204; it and
other authenticated requests carry the same `X-Device-Unique-Number` header;
device swaps are rejected before taking or printing jobs.

- **`GET /agent/documents?limit=100[&cursor=…]`**:
  `{items:[{kind, orderId, orderNumber, allocationId, amount, customerName,`
  `occurredAt, dueAt, latestAt, isOverdue}], nextCursor}`. Return `nextCursor`
  unchanged and URL-encoded;
  null ends pagination. Kind is `advance` or `sale`. Advances identify their
  allocation and payment time; sales use null allocation and posting time.
- **`POST /agent/jobs`**: Queue `{kind, orderId, allocationId, buyerNip}`.
  Advances require an allocation; sales use null. Buyer NIP is nullable.
  Holuto derives payments.
- **`POST /agent/jobs/take`**: Claim a job; no body, 204 for an empty queue.
- **`GET /agent/jobs/{id}`**: Reconcile job state without printing.
- **`POST /agent/jobs/{id}/result`**:
  `{attempt, state, receiptNumber, issuedAt, result}`. State is `done`,
  `failed` or `unknown`; `done` requires receipt number and issue time,
  otherwise both are null.

A lost `jobs/take` response is never recovered by re-serving the taken job.
The client executes only a job whose response it received; there is no local
claim journal for a lost response. While that printer has a taken or unknown
job, subsequent takes return 204. A taken attempt expires to unknown and needs
staff resolution, not another dispatch. The HTTP fake models this policy;
OpenAPI specifies the response shapes, not these lifecycle semantics.
`instance.lock` protects one state directory, not the paired token across
directories. Re-serving could therefore dispatch one attempt from two instances.

Job lines: `name`, `quantity`, `unitPrice`, `total`, `vatRate`, `vatLetter`.
Metadata: `orderId`, integer `orderNumber`, `printerId`, `kind`, nullable
`allocationId`, `etag`, `buyerNip`, `companyName`, `total`, `advances`,
`payments`, immutable attempt and lifecycle timestamps. Advances identify their
allocation and have one line per VAT rate, quantity 1.

`total` sums gross line totals. `advances` contains `{vatLetter, vatRate, amount}`
deductions, at most one per letter, each positive; duplicates and zero deductions
fail before fiscal commands. Advance jobs have no deductions. `payments`
contains `{form, amount}`; forms are `cash`, `card`, `transfer`, `mobile` and
`credit`. Decimals are unsigned plain strings such as `12.50`, with an optional
`.` fraction; no exponent or digit grouping. Use a matching backend; older
protocol shapes are unsupported.

## Device capabilities

These limits come from each driver's `fiscal::device::Capabilities`; the wire
group contains all seven limits and receipt flags; `company_footer` stays local.
`posnet::capabilities` defines the documented Temo Online limits: eight quantity
decimals, 80-character item names (`trline`), 500 lines (`trinit`),
40-character company footer lines (`trftrln`) and printable CP1250 from
0x20–0xFF, excluding DEL, no-break space, soft hyphen and table-drawing glyphs
(`¤ ¨ ˛ ´ ¸`, POT-I-DEV-37 p. 14).

| Production capability | Posnet | Novitus | Elzab |
| --- | --- | --- | --- |
| Advance deductions | yes | no | yes |
| Zero-total receipts | yes | no | no |
| Company footer | yes | no | no |

Novitus uses 255 lines, 40/60-character names and model-specific 40/42/44
company-name widths; Elzab uses a conservative 255-line ceiling, 64-character
names, 42-character company names and five quantity decimals. Neither brand
inherits Posnet's limits. A missing footer capability is a local check:
the company name must already occur in the decoded header or the job fails
before dispatch. Debug acceptance flags never belong in production telemetry.

## Discovery and readiness

Discovery first checks the configured port's remembered vendor, otherwise its
USB hint. Unhinted ports receive only read-only identity probes, in order:
Posnet `getrealid`, ElzabSTX `XTR`, Novitus `30#v`. Novitus also requires PC1;
non-native dialects remain unready. Failed cached identities can be
rediscovered only when no operation blocks. The detected port and vendor are
saved together. An unresolved journal pins the vendor and forbids scanning
other ports, even when preferences are stale.

An unidentified capability profile (for example unknown firmware or an
unsupported code page) stays visible as not ready. The worker keeps running
and rechecks it every 15 seconds, without sending invalid session
telemetry or claiming jobs. Once the device has describable capabilities,
connection and normal readiness checks resume. Novitus discovery accepts both
power-on error mode 0 and preconfigured mode 3 without changing printer settings.

## Report dates and printer clock

Date guards use the **printer's date**, not the host date or a guessed UTC
instant. Posnet permits a monthly report for the printer's current month:
`monthlyrep` is a periodic report covering the selected month (POT-I-DEV-37
p. 216). It rejects a monthly/periodic range starting after the printer date or
ending before fiscalisation; it does not universally require `to ≤ today`.
Novitus and Elzab require a completed month (its last date strictly before the
printer date); periodic reports require `from ≤ to ≤ printer date`.

Receipt issue times must identify a Warsaw instant. Posnet prefers
offset-bearing `rtcget.tm`; legacy civil `da` chooses the earlier repeated
instant. Novitus (`#c`) and Elzab (`XCR`) send civil time without an offset:
only receipts require it to be an unambiguous Warsaw instant. Probe, identify,
verify and reports validate civil time and use its date, so the repeated or
skipped hour does not block them. Both native two-digit clock parsers currently
accept 2000–2049.

## Specification sources

- **Posnet:** [POT-I-DEV-37 v5978][posnet-spec], Online MR2021, dated
  2 November 2023. Bare `p.`/`pp.` references in Posnet code and this document
  mean the specification's printed page numbers (for example `295/450`),
  not extracted-text lines. Receipt/NIP/payment definitions are on pp. 295,
  297 and 314.
- **Novitus:** [Protokół komunikacyjny NOVITUS v1.15 (ver Online)][novitus-spec];
  [v1.13][novitus-113] is also publicly available. Cite the version and command
  or section when comparing revisions; do not silently transfer semantics.
- **Elzab:** *Protokół ElzabSTX dla drukarek fiskalnych* and its ONLINE
  supplement, distributed as `om_iprg.zip` under **Drukarki fiskalne ONLINE**
  on the [vendor download page][elzab-downloads]. Cite the document name and
  command; do not redistribute vendor documents or extracted text.

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
close at zero without payment commands. Before dispatch, slot/rate pairs must
match fresh telemetry, deductions cannot exceed slot sales, and payments must
exactly cover fiscal value. Amounts must be nonnegative whole grosze within the
printer's range. Every item name needs a letter, digit or one of `, . / \ %`;
the printer treats other names as empty (p. 430). Every enabled PC interface
using the POSNET protocol must report Windows-1250 (`pccfgget` `cp0`,
pp. 153–155) because the session's interface cannot be identified and all text
is sent in Windows-1250. The parser canonicalizes a valid spaced unique number
such as `ABC 1234567890` to `ABC1234567890`; the device's `getrealid` spelling
is **unconfirmed on hardware** (specification examples, pp. 378, 391–392).
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
Unknown firmware is not ready; there is no compatibility fallback. Profiles
advertise 40/60-character item names and 255 sale lines. Read-only `52$i`
attribute 1 bit `0x0200` enables all assigned, non-control Windows-1250
characters, including Polish names; without that verified bit the repertoire
remains printable ASCII. Advances, zero-total receipts and changing company
footers are disabled pending qualification; a company absent from the header
is rejected before begin. DLE2 byte order remains unverified: readiness requires
zero flags, and execution wait reasons use only documented DLE bits. Native
identity strings and firmware-specific behavior still require qualification;
simulator tests are not hardware certification.

### Elzab

`elzab::printer` implements native ElzabSTX receipts and reports for the Zeta,
Mera and Cube ONLINE candidate profiles (exact XTR model names). It requires the
CP1250 code page (`XHR`), online `ySR` readiness, `LPC` receipt permission,
header, VAT and Warsaw clock in the printing session. Advances use per-slot
`LDS`; zero-total receipts and company footers stay disabled. Refusal and daily
report outcomes follow
[recovery classification](testing/fiscal-safety.md#recovery-classification).

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

## Advisory deadlines and voided receipts

Every listed document is due now (`dueAt`: sale completion or the advance's
recorded payment); `latestAt` is the statutory hard limit, later only for card,
transfer and BLIK advances. The table shows "Niezwłocznie · najpóźniej …" until
that limit and "PO TERMINIE · …" after it (`isOverdue`), sorted by the limit.
Deadlines never queue or print anything; staff still choose every document.
A `voided` job is a terminal, already-printed receipt cancelled in the
obvious-mistake register. Recovery acknowledges
its local journal without printing or reporting a replacement.
