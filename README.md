# Holuto Fiscal

Tray app for the workshop PC connected to a Posnet Temo Online fiscal printer.
Prints **only Holuto sales and advances** queued in the web app or selected in
Holuto Fiscal. No manual receipt editor or separate agent; processing continues
with the window closed.

| Crate | Purpose |
| --- | --- |
| `crates/posnet` | Serial driver: framing, CRC, CP1250, printer state, receipts, reports and socket simulator. |
| `crates/agent` | Holuto `/agent` API, durable journal, receipt execution and recovery, single printer worker. |
| `crates/desktop` | Polish Slint app (`holuto-fiscal`) with tray, reports and history; `.exe`, `.dmg` and `.deb` packages. |

## Install

Download from the [latest release](https://github.com/holuto-org/workshop/releases/latest):

| Platform | File |
| --- | --- |
| Windows x64 | [`holuto-fiscal-win-x64.exe`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-win-x64.exe) — no installation |
| macOS Apple Silicon | [`holuto-fiscal-mac-arm64.dmg`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-mac-arm64.dmg) |
| macOS Intel | [`holuto-fiscal-mac-x64.dmg`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-mac-x64.dmg) |
| Ubuntu / Debian x64 | [`holuto-fiscal-linux-x64.deb`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-linux-x64.deb) |
| Ubuntu / Debian ARM64 | [`holuto-fiscal-linux-arm64.deb`](https://github.com/holuto-org/workshop/releases/latest/download/holuto-fiscal-linux-arm64.deb) |

Filenames are stable; the release tag, `SHA256SUMS.txt` and app identify the version.
Builds are unsigned. Windows SmartScreen and macOS Gatekeeper may prompt on first
launch (macOS: right-click → Open). The `.deb` installs `holuto-fiscal` and a launcher.
Linux CI packages require Ubuntu 24.04 or newer; local packages require a compatible
host libc. Software rendering needs no GPU or OpenGL driver, including on remote
desktops and VMs.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

[Slint](https://slint.dev) uses the Slint Royalty-free Desktop, Mobile, and Web
Applications License 2.0; the badge is required attribution.

## Sign in and connect

1. In Holuto, add a printer under **Ustawienia → Drukarki fiskalne**. Copy the
   device token; it is shown once and identifies the printer, not a staff session.
2. In Holuto Fiscal's **Ustawienia**, enter **Adres API** and **Token**, then select
   the printer port and speed.
3. Click **Zaloguj i uruchom** to connect the printer to its Holuto registration and
   process the branch's queued receipts. **Zapisz** saves without starting.
   Settings persist, but each launch requires **Zaloguj i uruchom** to start processing.

Use the final API URL, including any path prefix. HTTPS is required except on
loopback (`localhost`, `127.0.0.0/8`, `::1`); redirects are rejected. TLS uses the OS
certificate store. Replace the saved token after rotation. A 401 stops processing;
there is no token refresh.

State directory (Unix: **0700**, data files **0600**):

| OS | Directory |
| --- | --- |
| Linux | `$XDG_STATE_HOME/holuto-fiscal` (default `~/.local/state/holuto-fiscal`) |
| macOS | `~/Library/Application Support/holuto-fiscal` |
| Windows | `%LOCALAPPDATA%\holuto-fiscal` (Local AppData Known Folder) |

`settings.json` stores the API URL, token, port and speed. Keep it private, out of
logs, and protected by profile ACLs on Windows. `operation.json` is the fiscal
journal; `history.jsonl` is local receipt history. `instance.lock` prevents a second
instance from using the same state directory.

### Printer

Printer settings: **PC services → USB → POSNET → Windows 1250**, 9600 baud.
Typical ports: Linux `/dev/ttyACM0` (prefer `/dev/serial/by-id/...`), Windows `COM3`,
macOS `/dev/cu.usbmodem…`. Only serial connections are supported. The user needs
read/write access (`dialout` group on Linux); never run with sudo.

If the saved port does not answer, the app sends one read-only identification
command per other serial port, then uses and saves the first Temo Online found.
It never scans with an unresolved operation: the device must not change.

## Desktop app

- **Do wydruku** — eligible advances and sales for the printer's branch, showing
  kind, amount and event date. Queue a document with an optional buyer NIP; Holuto
  derives payments and advance deductions. Unpaid final-sale balances print as
  credit. Receipt lines and totals are not locally editable. Documents above the
  printer's amount limit remain listed but cannot execute. Each poll refreshes the
  current page of up to 100 documents, oldest first; **Następna strona** and
  **Poprzednia strona** navigate. Column headers sort the page without changing the
  selected document.
- **Raporty** — daily, monthly and periodic fiscal reports, confirmed before printing.
  Periodic dates use `dd.mm.rrrr`; printing validates the dates, rejects future dates
  and requires start ≤ end. Reports and receipts share one worker thread; commands
  cannot interleave.
- **Historia** — receipts printed on this computer, with printer-assigned numbers or
  numbers entered in Holuto when unreadable. Sort by column header. This local log
  does not replace the printer's fiscal memory.
- **Ustawienia** — API URL/token, printer port/speed and processing controls.

Polling runs every **3 seconds**, or **15 seconds** after errors or while blocked.
Closing the window hides it in the tray and releases its memory without stopping
processing; reopen from the tray. Without a tray, closing keeps the window visible.
**Zatrzymaj** stops processing; **Zakończ** exits. Both finish the current operation first.

Prices are gross; API quantities and amounts are decimal strings. Holuto supplies
unit prices with at most two decimal places and line gross totals. Quantities and
prices print unchanged. Higher-precision prices or totals differing from quantity ×
price rounded half up to grosze fail before printing. The printer calculates VAT.
Each line uses its assigned slot for `23`, `8`, `5`, `0` or `zw`; mixed rates are
supported, with no highest-rate or whole-receipt fallback. Zero-price lines are
omitted. Missing VAT-slot mappings reject queueing, never substitute a rate.
Buyer NIP, advance deductions and payments reach the device unchanged. Line and
company names must print in full as received; unsupported text fails before fiscal
commands, never transliterated or truncated.

## Fiscal safety

- Before each operation, verify printer identity, fiscal mode, header, mechanism,
  command queue, transaction state and VAT table in the session that prints.
- Journal intent **before the first command reaches the printer**: claim taken jobs
  in `operation.json` before preflight; journal every command before sending.
  Never retry a device command or reprint a receipt.
- Printer replies follow execution. After a silent command timeout, asynchronous
  `!sprn`/`!sdev` queries check for operator intervention (paper, cover, lever,
  temperature, power or a key). Wait up to 10 minutes and show the required action;
  resend nothing. No reported wait reason or no response leaves the outcome **unknown**.
- Uncertain outcomes, including process death during dispatch, are **unknown** and
  block receipts and reports across restarts. Inspect the printout and printer,
  then resolve the job in the **web app**. Resolution unblocks, never reissues.
  Completed prints with unreadable receipt numbers also remain **unknown** until
  staff enter the number in Holuto; retain completed journal evidence.
- A `trinit` error (new certificates, read-only mode, full memory) opens no
  transaction: fail the job with the device's reason, without blocking. Refusal of
  a later command or report remains **unknown**.
- Interrupted reports also block. After inspecting the printout, use **Wyjaśnij raport**
  with the same printer connected and ready. Resolution prints nothing.
- Persist the job, attempt and execution-start timestamp before dispatch; persist
  the result before sending it. A `done` result uses the printer clock (`rtcget`,
  Polish time, minute precision) read just before `trinit`, not the PC clock, which
  Holuto rejects if ahead. An unreadable clock fails before fiscal commands.
  Resending a saved API result preserves its attempt and issue time; it is not
  reprinting. Recovery reconciles durable evidence with server state without
  replaying printer commands.
- Terminal server resolutions are authoritative. Corrupt or unreadable journals
  fail closed; deleting the journal is not recovery.
- History is a display cache. Report its read/write errors separately; they must
  not prevent settling a server-accepted result.

## Fiscal API contract

Requests use `/agent`, `Authorization: Bearer <device token>`,
`App-Version: agent/<version>` and `Accept-Language: pl` for Polish error details.
`POST /agent/session` and `POST /agent/heartbeat` send fresh telemetry:

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

`vatSlots` is the actual device table: slots 0–15, rates `"23"`, `"8"`, `"5"`, `"0"`
or `"zw"`. Each session/heartbeat replaces the backend mapping.
`maxItemNameLength`, `maxLinesPerReceipt`, `maxCompanyNameLength` and
`allowedCharacters` (all printable characters, NFC) are required before queueing;
the backend rejects line or company names outside these limits. The protocol cannot
report them, so `posnet::text` defines the documented Temo Online limits: 80-character
item names (`trline`), 500 lines (`trinit`), 40-character company footer lines
(`trftrln`) and printable CP1250 from 0x20–0xFF, excluding DEL, no-break space and
soft hyphen.

Session/heartbeat bind and verify the physical serial; pairing with another live
printer fails with 409. Other requests carry the same `X-Device-Serial` header;
device swaps are rejected before taking or printing jobs.

| Endpoint | Contract |
| --- | --- |
| `GET /agent/documents?limit=100[&cursor=…]` | `{items:[{kind, orderId, orderNumber, allocationId, amount, customerName, occurredAt}], nextCursor}`. Return `nextCursor` unchanged and URL-encoded; null ends pagination. Kind is `advance` or `sale`. Advances identify their allocation and payment time; sales use null allocation and posting time. |
| `POST /agent/jobs` | Queue `{kind, orderId, allocationId, buyerNip}`. Advances require an allocation; sales use null. Buyer NIP is nullable. Holuto derives payments. |
| `POST /agent/jobs/take` | Claim a job; no body, 204 for an empty queue. |
| `GET /agent/jobs/{id}` | Reconcile job state without printing. |
| `POST /agent/jobs/{id}/result` | `{attempt, state, receiptNumber, issuedAt, result}`. State is `done`, `failed` or `unknown`; `done` requires receipt number and issue time, otherwise both are null. |

Job lines: `name`, `quantity`, `unitPrice`, `total`, `vatRate`, `vatSlot`.
Metadata: `orderId`, integer `orderNumber`, `printerId`, `kind`, nullable
`allocationId`, `etag`, `buyerNip`, `companyName`, `total`, `advances`, `payments`,
immutable attempt and lifecycle timestamps. Advances identify their allocation
and have one line per VAT rate, quantity 1.

`total` sums gross line totals. `advances` contains `{vatSlot, vatRate, amount}`
deductions, at most one per slot; duplicates fail before fiscal commands.
Advance jobs have no deductions. `payments` contains `{form, amount}`; forms are
`cash`, `card`, `transfer`, `mobile` and `credit`. Decimals are unsigned plain
strings such as `12.50`, with an optional `.` fraction; no exponent or digit grouping.
Use a matching backend; older protocol shapes are unsupported.

Printer command order: `trinit`, optional `trnipset`, `trline` per line,
`tradvance` per deduction (name `Zaliczka`, VAT slot, amount, gross sales at that
slot), `trpayment` per nonzero payment, then `trend`. Payment types: cash 0, card 2,
transfer 8, mobile 6 (name `Płatność mobilna`), credit 5. `trend.to` is gross total
minus advances; `trend.fp` sums payments. Fully prepaid sales close at zero without
payment commands. Before dispatch, slot/rate pairs must match fresh telemetry,
deductions cannot exceed slot sales, and payments must exactly cover fiscal value.
Amounts must be nonnegative whole grosze within the printer's range.

## Development and releases

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh # if rustup is missing
rustup toolchain install nightly-2026-10-06 --profile minimal --component rustfmt
cargo run -p holuto-fiscal
cargo nextest run --workspace --all-features # or: cargo test --workspace --all-features
cargo +nightly-2026-10-06 fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny --locked check && cargo shear --deny-warnings
```

`rust-toolchain.toml` pins Rust. Formatting uses the dated nightly because
`.rustfmt.toml` enables unstable options. Linux needs `pkg-config` and fontconfig
headers (`libfontconfig1-dev` / `fontconfig-devel`). Lint policy is in `Cargo.toml`
(pedantic, nursery and restriction lints) and `clippy.toml`; dependency and license
policy is in `deny.toml`.

Exercise printer behavior through the socket simulator and tests, **never real
fiscal commands**. Run locally with `cargo run -p holuto-fiscal --features simulator`
to replace serial I/O with an in-process Temo peer. Its state lives in
`holuto-fiscal-simulator` beside the real state directory. Release builds reject this feature.

`bash packaging/build.sh` writes the host OS/CPU package to `release/`. CI builds
Windows x64, macOS ARM64/x64 and Linux ARM64/x64. A push to `master` publishes only
when the root `Cargo.toml` `[workspace.package] version` increases above the previous
version and all published releases. Manual **Release** runs create Actions artifacts
without publishing. Retry a failed release by rerunning its original workflow run.

Buyer NIP and payment forms follow
[Posnet POT-I-DEV-37 v5978, pp. 295, 297 and 314](https://raw.githubusercontent.com/kashiash/PosnetLibrary/master/DOCS/pot-i-dev-37-5978-specyfikacja-protokolu-posnet-online-mr2021-2%20(1).pdf)
(Online MR2021; Temo Online). No vendor SDK or specification is redistributed.
