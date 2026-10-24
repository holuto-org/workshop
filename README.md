# Holuto Fiscal

One application on the workshop PC, next to the Posnet Temo Online fiscal printer.
It prints **only Holuto sales and advances**: staff queue a receipt in the web app or
select an eligible document in Holuto Fiscal. There is no manual receipt editor or separate agent.
It lives in the system tray and keeps processing while its window is closed.

One Cargo workspace, three crates:

| Crate | Purpose |
| --- | --- |
| `crates/posnet` | Posnet serial driver: framing, CRC, CP1250, printer state, receipts, reports, and the socket simulator tests use. |
| `crates/agent` | Holuto `/agent` API, the durable operation journal, receipt execution and recovery, and the single printer worker thread. |
| `crates/desktop` | **Holuto Fiscal** (`holuto-fiscal`) — Polish Slint app with tray, reports and history. Ships as `.exe`, `.dmg` and `.deb`. |

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
launcher entry. Linux CI builds require Ubuntu 24.04 or newer; locally built packages
require a compatible host libc. The window renders in software: no GPU or OpenGL driver is
needed (remote desktops and virtual machines work).

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

The UI toolkit, [Slint](https://slint.dev), is used under the Slint Royalty-free Desktop,
Mobile, and Web Applications License 2.0; the badge above is its required attribution.

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
development URLs (`localhost`, `127.0.0.0/8`, `::1`); redirects are rejected. TLS uses the
operating system's certificate store. After token rotation, replace the saved token. A 401
stops processing; there is no token refresh.

The app keeps its state in one directory, readable only by the user (mode **0700**, files
**0600** on Unix):

| OS | Directory |
| --- | --- |
| Linux | `$XDG_STATE_HOME/holuto-fiscal` (default `~/.local/state/holuto-fiscal`) |
| macOS | `~/Library/Application Support/holuto-fiscal` |
| Windows | `%LOCALAPPDATA%\holuto-fiscal` |

`settings.json` holds the API URL, token, port and speed; on Windows keep the profile private
with restricted ACLs. Do not share it or include it in logs. `operation.json` is the fiscal
journal and `history.jsonl` the local receipt history. `instance.lock` allows one running
Holuto Fiscal per state directory; a second start is refused. `--data-dir` selects another
directory.

### Printer

Printer side: **PC services → USB → POSNET → Windows 1250**, 9600 baud. On Linux the
port is usually `/dev/ttyACM0` (prefer `/dev/serial/by-id/...`), on Windows `COM3`-style,
and on macOS `/dev/cu.usbmodem…`. The user needs read/write access (`dialout` group on
Linux). Never run through sudo. Only serial connections are supported.

When the saved port does not answer, the app scans the other serial ports with a single
read-only identification command and adopts (and saves) the first that is a Temo Online. It
never scans while an operation is unresolved: the device must not change then.

Saved printer settings also apply to `--probe`. `--serial` and `--baudrate` override
only their respective saved values, both in the app and in probe mode.

A read-only probe, when an operator explicitly wants to inspect the device (close the app
first; it holds the state directory):

```bash
holuto-fiscal --serial /dev/ttyACM0 --probe
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
  Clicking a column header sorts the current page; the selection stays on the same document.
- **Raporty** — daily, monthly and periodic fiscal reports, with confirmation before
  printing. The periodic range is typed as `dd.mm.rrrr` and is checked (valid, not in the
  future, start not after end) only when printing is requested. Reports and queued receipts
  run on the **same worker thread**, so they cannot interleave commands.
- **Historia** — receipts printed on this computer, including printer-assigned receipt numbers
  (or the number staff entered in Holuto when the printer's could not be read). It is a
  convenience log, not a replacement for the printer's fiscal memory. Column headers sort it.
- **Ustawienia** — API URL/token and printer port/speed, plus processing controls.

The background loop polls every **3 seconds** (15 seconds after an error or while blocked).
Closing the window hides it in the system tray and releases the window's memory without
stopping processing; open it again from the tray. **Zatrzymaj** stops processing;
**Zakończ** exits. Both wait for the current operation to finish. If a system tray is
unavailable, closing keeps the window visible instead of leaving an inaccessible background
app.

Prices are gross, quantities and monetary values use decimal strings at the API
boundary, and the printer calculates the fiscal VAT block. Holuto supplies unit
prices with at most two decimal places and each line's gross total. The app
prints quantities and prices unchanged; a higher-precision price or a line total
that differs from quantity × price rounded half up to grosze fails the job before
printing. A receipt may have **mixed VAT rates**, each line using its assigned
printer VAT slot. Supported rates are `23`, `8`, `5`, `0` and `zw`; there is no
highest-rate or whole-receipt fallback. Zero-price lines are omitted. Missing
VAT-slot mappings reject queueing rather than silently changing the tax rate.
Buyer NIP, advance deductions and payment amounts are carried through the job to
the device. Receipt text prints exactly as Holuto sends it: a line name or company name
the printer cannot print in full fails the job before any fiscal command, never
transliterated or truncated.

## Fiscal safety

- Before each operation verify printer identity, fiscal mode, header, mechanism,
  command queue, transaction state and VAT table, in the same session that prints.
- Journal intent **before the first command reaches the printer**: a taken job is claimed in
  `operation.json` before preflight, and every printer command is journaled before it is sent.
  Never retry a device command and never reprint a receipt.
- An uncertain result, including a process death during dispatch, is **unknown** and
  blocks receipts and reports across restarts. Inspect the paper and printer state,
  then resolve the job in the **web app**. Resolution unblocks; it never reissues a receipt.
  A completed print without a readable receipt number also remains **unknown**
  until staff supply the number in Holuto; completed journal evidence is retained.
- An interrupted **report** blocks the same way; after checking the printout, an operator
  confirms it with **Wyjaśnij raport**, which requires the same printer connected and ready
  and never prints anything.
- The journal persists the job, attempt and execution-start timestamp before dispatch, and the
  result before sending it. Re-sending a saved API result is not reprinting: it keeps the same
  attempt and timestamp. Restart recovery reconciles durable evidence with the job's server
  state without issuing printer commands again.
- A terminal server resolution is authoritative. A corrupt or unreadable journal fails
  closed; deleting it is not a recovery procedure.
- Local history is only a display cache: its read or write errors are reported
  separately and do not prevent settling a server-accepted result.

## Fiscal API contract

The app uses `/agent` with `Authorization: Bearer <device token>`, `App-Version: agent/<version>`
and `Accept-Language: pl` (error details from Holuto are shown in Polish).
`POST /agent/session` and `POST /agent/heartbeat` send fresh printer telemetry:

```json
{
  "serial": "device-serial",
  "model": "POSNET TEMO ONLINE",
  "firmware": "32.01",
  "appVersion": "0.2.0",
  "vatSlots": [{"slot": 0, "vatRate": "23"}, {"slot": 1, "vatRate": "8"}],
  "maxItemNameLength": 80,
  "maxLinesPerReceipt": 500,
  "maxCompanyNameLength": 40,
  "allowedCharacters": " !\"#$%&'()*+,-./0123…"
}
```

`vatSlots` reports the actual device table, not a set of rates: slots are integers
0–15 and `vatRate` is one of `"23"`, `"8"`, `"5"`, `"0"`, `"zw"`. Every session and
heartbeat replaces the backend's slot mapping. `maxItemNameLength`, `maxLinesPerReceipt`,
`maxCompanyNameLength` and `allowedCharacters` (every printable character, in NFC) are required;
the backend queues no receipt until it has them, and rejects a line name or company name that
exceeds them. The protocol cannot report them, so the driver states the documented Temo Online
values once, in `posnet::text`: an 80-character item name (`trline`), 500 lines per receipt
(`trinit`), a 40-character company footer line (`trftrln`) and the CP1250 characters from 0x20
to 0xFF the driver prints (all but DEL, the no-break space and the soft hyphen).
Session/heartbeat bind and verify the physical serial; a serial already paired to another live
printer is refused (409). Other requests include the unchanged `X-Device-Serial` header; a device
swap is rejected before a job is taken or printed.

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
Forms are `cash`, `card`, `transfer`, `mobile` and `credit`. Decimals are plain strings such as
`12.50`: digits with an optional `.` fraction, no sign, exponent or digit grouping.
Deploy the app with the matching backend; older protocol shapes are unsupported.

The driver sends `trinit`, optional `trnipset`, `trline` for each line, `tradvance`
for each deduction (name `Zaliczka`, VAT slot, amount, gross sales at that slot), then
`trpayment` for each nonzero payment. Types are cash 0, card 2, transfer 8, mobile 6
(named `Płatność mobilna`) and credit 5. It closes with `trend`, whose `to` is the
gross total minus advances and `fp` is the sum of payments. Fully prepaid sales
close at zero without payment commands. Before dispatch, every slot/rate must match
fresh telemetry, deductions cannot exceed sales at their slot, and payments must
exactly cover the fiscal value. Amounts must be nonnegative whole grosze within the
printer's range. Journal recovery and the no-replay rule are unchanged.

## Development and releases

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # once, if rustup is missing
rustup toolchain install nightly-2026-10-06 --profile minimal --component rustfmt
cargo run -p holuto-fiscal
cargo nextest run --workspace --all-features      # or: cargo test --workspace --all-features
cargo +nightly-2026-10-06 fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny --locked check && cargo shear --deny-warnings
```

The toolchain is pinned in `rust-toolchain.toml`; formatting uses the pinned nightly because
`.rustfmt.toml` enables unstable options. Linux builds need `pkg-config` and the fontconfig
headers (`libfontconfig1-dev` / `fontconfig-devel`). Clippy runs pedantic, nursery and a strict
set of restriction lints (`Cargo.toml`, `clippy.toml`); `deny.toml` holds the dependency and
license policy.

For a local run without a printer, the `simulator` feature adds an in-process Temo peer:
`cargo run -p holuto-fiscal --features simulator -- --simulator --data-dir /tmp/holuto`.
Release builds never enable it.

`bash packaging/build.sh` produces the host OS/CPU artifact in `release/`. CI builds the
desktop once per OS/CPU target (Windows x64, macOS ARM64/x64, Linux ARM64/x64), only when a
push to `master` increases the root `Cargo.toml` `[workspace.package] version` above every
published release. A manual **Release** run produces Actions artifacts without publishing a
release. Retry failed releases by rerunning the original workflow run.

Printer behavior must be exercised through the socket simulator and automated tests,
not real fiscal commands. Buyer NIP and payment forms follow
[Posnet POT-I-DEV-37 v5978, pp. 295, 297 and 314](https://raw.githubusercontent.com/kashiash/PosnetLibrary/master/DOCS/pot-i-dev-37-5978-specyfikacja-protokolu-posnet-online-mr2021-2%20(1).pdf)
(Online MR2021; Temo Online). No vendor SDK or specification is redistributed.
