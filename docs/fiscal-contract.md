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
`POST /agent/session` sends telemetry at startup and whenever the device profile changes:

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
  ]
}
```

`vendor` is `"posnet"`, `"novitus"` or `"elzab"` and binds alongside the physical
unique number. `vatRates` is the actual device table, with letters A–G and rates `"23"`,
`"8"`, `"5"`, `"0"` or `"zw"`. Each
session replaces the backend mapping. A–E mean 23%, 8%, 5%, 0% and exempt respectively;
F/G retain other programmed rates. Illegal A–E assignments remain visible locally as not
ready and are not uploaded. `maxItemNameLength`,
`maxLinesPerReceipt`, `maxCompanyNameLength`, `allowedCharacters` (all printable
characters, NFC), `maxQuantityDecimals` (0–8) and `capabilities` are required
before queueing. The backend rejects names outside the limits, excessive
quantity precision (ignoring trailing zeros), advance deductions without
`advanceDeductions`, and fully prepaid sales without `zeroTotalReceipts`
(409 `fiscal_printer_quantity_precision`, `fiscal_printer_advances_unsupported`,
`fiscal_printer_zero_total_unsupported`). The agent independently checks the
fresh device profile before any fiscal command and reports unprintable jobs as
failed.

Sessions bind the fiscal memory unique number (`^[A-Z]{3}[0-9]{10}$`) and vendor; a mismatch
fails with 403 `fiscal_unique_number_mismatch` or `fiscal_vendor_mismatch`.
Bodyless `POST /agent/heartbeat` returns 204; it and other authenticated requests carry
the same `X-Device-Unique-Number` header; device swaps are rejected
before taking or printing jobs.

- **`GET /agent/documents?limit=100[&cursor=…]`**:
  `{items:[{kind, orderId, orderNumber, allocationId, amount, customerName,`
  `occurredAt, dueAt, isOverdue}], nextCursor}`. Return `nextCursor` unchanged and URL-encoded;
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

Job lines: `name`, `quantity`, `unitPrice`, `total`, `vatRate`, `vatLetter`.
Metadata: `orderId`, integer `orderNumber`, `printerId`, `kind`, nullable
`allocationId`, `etag`, `buyerNip`, `companyName`, `total`, `advances`,
`payments`, immutable attempt and lifecycle timestamps. Advances identify their
allocation and have one line per VAT rate, quantity 1.

`total` sums gross line totals. `advances` contains `{vatLetter, vatRate, amount}`
deductions, at most one per slot, each positive; duplicates and zero deductions
fail before fiscal commands. Advance jobs have no deductions. `payments`
contains `{form, amount}`; forms are `cash`, `card`, `transfer`, `mobile` and
`credit`. Decimals are unsigned plain strings such as `12.50`, with an optional
`.` fraction; no exponent or digit grouping. Use a matching backend; older
protocol shapes are unsupported.

## Device capabilities

These limits come from each driver's `fiscal::device::Capabilities`; the wire
capabilities group contains all seven limits and receipt flags; `company_footer` stays local.
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
and rechecks it every 15 seconds, without sending invalid session/heartbeat
telemetry or claiming jobs. Once the device has describable capabilities,
connection and normal readiness checks resume. Novitus discovery accepts both
power-on error mode 0 and preconfigured mode 3 without changing printer settings.

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
printer's range.

Buyer NIP and payment forms follow
[Posnet POT-I-DEV-37 v5978, pp. 295, 297 and
314](<https://raw.githubusercontent.com/kashiash/PosnetLibrary/master/DOCS/pot-i-dev-37-5978-specyfikacja-protokolu-posnet-online-mr2021-2%20(1).pdf>)
(Online MR2021; Temo Online), the output oracle for Posnet tests. No vendor SDK
or specification is redistributed.

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
`LDS`; zero-total receipts and company footers stay disabled. Only a matched
transport rejection of the initial `LOR` is a certain refusal; every later
failure, and every accepted daily `RRP`, is unknown until a person resolves it.

## Advisory deadlines and voided receipts

The documents table displays `dueAt` and marks `isOverdue` explicitly. Deadlines never
queue or print anything; staff still choose every document. A `voided` job is a terminal,
already-printed receipt cancelled in the obvious-mistake register. Recovery acknowledges
its local journal without printing or reporting a replacement.
