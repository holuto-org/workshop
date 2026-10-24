# Holuto Fiscal

One application on the workshop PC, next to the Posnet Temo Online fiscal printer.
It prints **only Holuto sales and advances**: staff queue a receipt in the web app or
select an eligible document in Holuto Fiscal. There is no manual receipt editor or separate agent.

One uv workspace, two packages:

| Package | Import | Purpose |
| --- | --- | --- |
| `packages/posnet` | `posnet` | Posnet USB/serial driver: framing, CRC, CP1250, printer state, receipts, reports, durable intent journal and socket simulator. |
| `packages/desktop` | `fiscal_desktop` | **Holuto Fiscal** — Polish PySide6 app, background receipt loop, reports and history. Ships as `.exe`, `.dmg` and `.deb`. |

## Install

Download the build for your machine from the
[latest release](https://github.com/holuto-org/workshop/releases/latest):

| Platform | File |
| --- | --- |
| Windows x64 | [`holuto-fiscal-win-x64.exe`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-win-x64.exe) — single file, no installation |
| macOS Apple Silicon | [`holuto-fiscal-mac-arm64.dmg`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-mac-arm64.dmg) |
| macOS Intel | [`holuto-fiscal-mac-x64.dmg`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-mac-x64.dmg) |
| Ubuntu / Debian x64 | [`holuto-fiscal-linux-x64.deb`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-linux-x64.deb) |
| Ubuntu / Debian ARM64 | [`holuto-fiscal-linux-arm64.deb`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-linux-arm64.deb) |

File names are stable; the version is in the release tag, `SHA256SUMS.txt` and app.
Builds are unsigned: Windows SmartScreen and macOS Gatekeeper may ask before the
first start (macOS: right-click → Open). The `.deb` installs `holuto-fiscal` and a
launcher entry. Linux CI builds require Ubuntu 24.04 or newer; locally built bundles
require a compatible host libc.

## Sign in and connect

1. In the Holuto web app, add a fiscal printer under **Ustawienia → Drukarki fiskalne**
   and copy its device token, which is shown only once.
2. Open Holuto Fiscal. In **Ustawienia**, enter **Adres API** and **Token**, then select
   the printer port and speed. The token identifies this printer, not a staff session.
3. Click **Zaloguj i uruchom**. The app connects the physical printer to its Holuto
   registration and processes the branch's queued receipts in the background.
   **Zapisz** saves without starting. On the next launch, settings are restored but
   processing starts only after you click **Zaloguj i uruchom** again.

Use the final API URL, including any path prefix. HTTPS is required except for loopback
development URLs (`localhost`, `127.0.0.1`, `::1`); redirects are rejected. After token
rotation, replace the saved token. A 401 stops processing; there is no token refresh.

The API URL, token, port and speed are stored in
`$XDG_STATE_HOME/holuto-fiscal/fiscal.json` (default
`~/.local/state/holuto-fiscal/fiscal.json`). On Unix this file has mode **0600**.
On Windows keep the state directory in a private user profile with restricted ACLs.
Do not share this file or include it in logs. Legacy standalone state is migrated
from `workshop-agent` together with its journal and result outbox; keep those files
together, never discard them to unblock the printer.

### Printer

Printer side: **PC services → USB → POSNET → Windows 1250**, 9600 baud. On Linux the
port is usually `/dev/ttyACM0` (prefer `/dev/serial/by-id/...`), on Windows `COM3`-style,
and on macOS `/dev/cu.usbmodem…`. The user needs read/write access (`dialout` group on
Linux). Never run through sudo. Only serial connections are supported.

Saved printer settings also apply to `--probe`. `--serial` and `--baudrate` override
only their respective saved values, both in the app and in probe mode.

A read-only probe, when an operator explicitly wants to inspect the device:

```bash
uv run fiscal-desktop --serial /dev/ttyACM0 --probe
```

## Desktop app

- **Do wydruku** — eligible advance and sale documents for this printer's branch,
  with their kind, amount and event date. Queue the selected document with an optional
  buyer NIP; Holuto derives its payment forms and advance deductions. Final sales
  need not be fully paid: the unpaid balance prints as credit.
  Receipt lines and totals come from Holuto and cannot be edited locally.
  Listing preserves documents above the printer's amount limit; execution still
  enforces that limit. The list shows one page of up to 100 documents, oldest first,
  refreshed on each poll; **Następna strona**/**Poprzednia strona** move between pages.
- **Raporty** — daily, monthly and periodic fiscal reports, with confirmation before
  printing. Reports and queued receipts use the **same printer lock**, so they cannot
  interleave commands.
- **Historia** — local operation history, including printer-assigned receipt numbers.
  It is a convenience log, not a replacement for the printer's fiscal memory.
- **Ustawienia** — API URL/token and printer port/speed, plus processing controls.

The background loop polls every **3 seconds**. Closing the window hides it in the
system tray without stopping processing; open it again from the tray. **Zatrzymaj**
stops processing; **Zakończ** exits. Both wait for the current operation to finish.
If a system tray is unavailable,
closing keeps the window visible instead of leaving an inaccessible background app.
Run one Holuto Fiscal instance per printer; do not run an old standalone agent beside it.

Prices are gross, quantities and monetary values use decimal strings at the API
boundary, and the printer calculates the fiscal VAT block. Holuto supplies unit
prices with at most two decimal places and each line's gross total. The desktop
prints quantities and prices unchanged; a higher-precision price or a line total
that differs from quantity × price rounded half up to grosze fails the job before
printing. A receipt may have **mixed VAT rates**, each line using its assigned
printer VAT slot. Supported rates are `23`, `8`, `5`, `0` and `zw`; there is no
highest-rate or whole-receipt fallback. Zero-price lines are omitted. Missing
VAT-slot mappings reject queueing rather than silently changing the tax rate.
Buyer NIP, advance deductions and payment amounts are carried through the job to
the device. Receipt text is sanitized at the driver boundary (transliteration,
control-character removal and device length limits).

## Fiscal safety

- Before each operation verify printer identity, fiscal mode, header, mechanism,
  command queue, transaction state and VAT table.
- Journal intent **before the first command reaches the printer**. Never retry a
  device command and never reprint a receipt.
- An uncertain result, including a process death during dispatch, is **unknown** and
  blocks receipts and reports across restarts. Inspect the paper and printer state,
  then resolve the job in the **web app**. Resolution unblocks; it never reissues a receipt.
  A completed print without a readable receipt number also remains **unknown**
  until staff supply the number in Holuto; completed journal evidence is retained.
- The receipt journal (`operation.json`) and result outbox (`operation.agent.json`)
  live in the same state directory. A migrated standalone agent retains its existing
  `fiscal-operation.json`/`fiscal-operation.agent.json` pair. Persist the job,
  attempt and execution-start timestamp before dispatch, and the result before sending
  it. Re-sending a saved API result is not reprinting: it keeps the same attempt and
  timestamp. Restart recovery reconciles durable evidence without issuing printer commands again.
- A terminal server resolution is authoritative. Corrupt, orphaned or unresolved
  journals fail closed; deleting them is not a recovery procedure.
- Local history is only a display cache: cache read/write errors are reported
  separately and do not prevent acknowledgement or settlement of a server-accepted result.

## Fiscal API contract

The app uses `/agent` with `Authorization: Bearer <device token>`.
`POST /agent/session` and `POST /agent/heartbeat` send fresh printer telemetry:

```json
{
  "serial": "device-serial",
  "model": "Temo Online",
  "firmware": "2.01",
  "appVersion": "0.1.2",
  "vatSlots": [{"slot": 0, "vatRate": "23"}, {"slot": 1, "vatRate": "8"}],
  "maxItemNameLength": 80,
  "maxLinesPerReceipt": 500,
  "allowedCharacters": " !\"#$%&'()*+,-./0123…"
}
```

`vatSlots` reports the actual device table, not a set of rates: slots are integers
0–15 and `vatRate` is one of `"23"`, `"8"`, `"5"`, `"0"`, `"zw"`. Every session and
heartbeat replaces the backend's slot mapping. Telemetry uses `appVersion` and
`vatSlots`, not the retired `agentVersion`/`vatRates` fields. `maxItemNameLength`,
`maxLinesPerReceipt` and `allowedCharacters` (every printable character, in NFC) are
required; the backend queues no receipt until it has them. The protocol cannot report
them, so the driver states the documented Temo Online values once, in `posnet.models`:
an 80-character item name (`trline`), 500 lines per receipt (`trinit`) and the
CP1250 characters from 0x20 to 0xFF the driver prints unchanged (all but DEL, the
no-break space and the soft hyphen). Session/heartbeat bind
and verify the physical serial; a serial already paired to another live printer is
refused (409). Other requests include the unchanged
`X-Device-Serial` header; a device swap is rejected before a job is taken or printed.

| Endpoint | Purpose |
| --- | --- |
| `GET /agent/documents?limit=100[&cursor=…]` | One page of eligible documents: `{items:[{kind, orderId, orderNumber, allocationId, amount, customerName, occurredAt}], nextCursor}`. Pass `nextCursor` back unchanged (URL-encoded) for the next page; null means the last page. Kind is `advance` or `sale`; advance documents identify their allocation and payment time, sales use null allocation and posting time. |
| `POST /agent/jobs` | Queue `{kind, orderId, allocationId, buyerNip}`; allocation is required for an advance and null for a sale; buyer NIP may be null. Holuto derives the payments. |
| `POST /agent/jobs/take` | Claim a job; no body, 204 when the queue is empty. |
| `GET /agent/jobs/{id}` | Reconcile current job state without printing. |
| `POST /agent/jobs/{id}/result` | Send `{attempt, state, receiptNumber, issuedAt, result}`; state is `done`, `failed` or `unknown`. `done` requires a receipt number and issue timestamp; `failed`/`unknown` leave both null. |

Job lines contain `name`, `quantity`, `unitPrice`, `total`, `vatRate`, `vatSlot`. Job metadata
includes `orderId`, `orderNumber` (integer), `printerId`, `kind`, nullable `allocationId`,
`etag`, `buyerNip`, `companyName`, `total`, `advances`, `payments`, the immutable attempt
and lifecycle timestamps. Advance jobs identify their allocation and have one line
per VAT rate, each with quantity 1.
`total` is the gross sum of line totals. `advances` contains `{vatSlot, vatRate, amount}`
deductions (always empty for advance jobs), with at most one deduction per VAT slot;
duplicate slots fail validation before fiscal commands. `payments` contains `{form, amount}`.
Forms are `cash`, `card`, `transfer`, `mobile` and `credit`. Decimals are strings.
Deploy the app with the matching backend; older protocol shapes are unsupported.

The driver sends `trinit`, optional `trnipset`, `trline` for each line, `tradvance`
for each deduction (name `Zaliczka`, VAT slot, amount, gross sales at that slot), then
`trpayment` for each nonzero payment. Types are cash 0, card 2, transfer 8, mobile 6
(named `Płatność mobilna`) and credit 5. It closes with `trend`, whose `to` is the
gross total minus advances and `fp` is the sum of payments. Fully prepaid sales
close at zero without payment commands. Before dispatch, every slot/rate must match
fresh telemetry, deductions cannot exceed sales at their slot, and payments must
exactly cover the fiscal value. Amounts must be nonnegative whole grosze within the
printer's range. Journal/outbox recovery and the no-replay rule are unchanged.

## Development and releases

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh   # once, if uv is missing
uv sync --locked
uv run fiscal-desktop
QT_QPA_PLATFORM=offscreen uv run pytest -q
uv run ruff check packages && uv run ruff format --check packages
```

Python 3.14 or newer. `bash packages/desktop/build.sh` produces the host OS/CPU bundle
in `packages/desktop/release/`. No separate agent is built or installed; its loop is
part of `fiscal_desktop`. HTTP uses the Python standard library, with no additional
HTTP dependency.

CI builds the desktop once per OS/CPU target (Windows x64, macOS ARM64/x64, Linux
ARM64/x64), only when a push to `master` increases the root `pyproject.toml` version
above every published release. A manual **Release** run also requires a version
increase relative to the selected commit's parent and an unpublished version. Manual
runs produce Actions artifacts without publishing a release. Retry failed releases
by rerunning the original workflow run.

Printer behavior must be exercised through the socket simulator and automated tests,
not real fiscal commands. Buyer NIP and payment forms follow
[Posnet POT-I-DEV-37 v5978, pp. 295, 297 and 314](https://raw.githubusercontent.com/kashiash/PosnetLibrary/master/DOCS/pot-i-dev-37-5978-specyfikacja-protokolu-posnet-online-mr2021-2%20(1).pdf)
(Online MR2021; Temo Online). No vendor SDK or specification is redistributed.
