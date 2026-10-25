# Holuto Workshop

Tray app for the workshop PC connected to a Posnet Temo Online fiscal printer.
Prints **only Holuto sales and advances** queued in the web app or selected in
Holuto Workshop. No manual receipt editor or separate agent; processing
continues with the window closed.

- **`crates/fiscal`**: Shared fiscal values, receipt validation against driver
  capabilities, the `Driver` trait and `Device` model, report/journal contracts,
  Windows-1250 codec and serial links.
- **`crates/posnet`**: Serial driver: framing, CRC, CP1250, printer state,
  receipts, reports and socket simulator.
- **`crates/agent`**: Holuto `/agent` API, durable journal, receipt execution
  and recovery, single printer worker.
- **`crates/desktop`**: Polish Slint app and `workshop` CLI with tray, pairing,
  reports and history; `.exe`, `.dmg` and `.deb` packages.

## Install

Download from the
[latest release](https://github.com/holuto-org/workshop/releases/latest):

| Platform | File |
| --- | --- |
| Windows x64 | [`workshop-win-x64.exe`](https://github.com/holuto-org/workshop/releases/latest/download/workshop-win-x64.exe) |
| macOS Apple Silicon | [`workshop-mac-arm64.dmg`](https://github.com/holuto-org/workshop/releases/latest/download/workshop-mac-arm64.dmg) |
| macOS Intel | [`workshop-mac-x64.dmg`](https://github.com/holuto-org/workshop/releases/latest/download/workshop-mac-x64.dmg) |
| Ubuntu 24.04+ / Debian 13+ x64 | [`workshop-linux-x64.deb`](https://github.com/holuto-org/workshop/releases/latest/download/workshop-linux-x64.deb) |
| Ubuntu 24.04+ / Debian 13+ ARM64 | [`workshop-linux-arm64.deb`](https://github.com/holuto-org/workshop/releases/latest/download/workshop-linux-arm64.deb) |

Filenames are stable; the release tag, `SHA256SUMS.txt` and app identify the
version. Windows builds are unsigned; macOS bundles are ad-hoc signed, not
Developer-ID signed or notarized. The Windows `.exe` links the C runtime
statically. The `.deb` installs `/usr/bin/workshop` and a launcher; its `libc6`
dependency records the build host's glibc version. Software rendering needs no
GPU or OpenGL driver, including on remote desktops and VMs.

Follow the [AI/manual installation guide](docs/install-with-ai.md) for platform
requirements, pinned downloads, checksums/attestations, OS security prompts,
per-user installation paths, updates and uninstall. Each release includes that
guide, `install.sh`, `install.ps1` and `SHA256SUMS.txt`. Never pipe a download to
a shell.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

[Slint](https://slint.dev) uses the Slint Royalty-free Desktop, Mobile, and Web
Applications License 2.0; the badge is required attribution.

## Sign in and connect

Pair in **Ustawienia** using a one-time code generated for the printer in Holuto.
Codes expire after 15 minutes; successful redemption replaces the previous
device token, which stays internal. See the
[guided configuration sequence](docs/install-with-ai.md#guided-configuration-sequence)
for setup. **Zapisz** saves without starting; **Uruchom** starts processing.
Each launch requires explicit start unless the user opts into
[autostart](#autostart).

Use the final API URL, including any path prefix. HTTPS is required except on
loopback (`localhost`, `127.0.0.0/8`, `::1`); redirects are rejected. TLS uses
the OS certificate store. A 401 stops processing; generate a fresh pairing code
in Holuto and pair again. There is no token refresh.

State directory (Unix: **0700**; created/replaced data files: **0600**):

| OS      | Directory                                                      |
| ------- | -------------------------------------------------------------- |
| Linux   | `$XDG_STATE_HOME/workshop` (default `~/.local/state/workshop`) |
| macOS   | `~/Library/Application Support/workshop`                       |
| Windows | `%LOCALAPPDATA%\workshop` (Local AppData Known Folder)         |

Uninstalling the application retains fiscal state. Never delete the journal to
unblock printing; unresolved evidence requires human reconciliation.

`settings.json` stores the API URL, internal token, port, speed, vendor preference
(`vendor`: `"posnet"`, `"novitus"`, `"elzab"` or `null` for detection), diagnostics
(`telemetryEnabled`) and start-processing-on-launch preferences. Pre-vendor settings
load as detection without rewriting the file. Keep it private, out of logs, and
protected by profile ACLs on Windows. `operation.json` is the fiscal journal:
every receipt/report records its vendor and serial, both required for resolution.
Under the instance lock, pre-vendor journals are atomically rewritten once as
`"posnet"` without clearing unresolved evidence; malformed files still block.
`history.jsonl` is local receipt history, compacted to
the newest 500 receipts. `instance.lock` prevents a second instance from using
the same state directory. Unix opens tighten the state directory to **0700**
and lock to **0600**; writes replace data files with **0600** copies. History
loading also tightens its file. Settings/status reads and current-format journal
reads do not repair modes.

Settings and `status.json` are persisted by one background FIFO writer. Starting
waits for successful settings persistence; quitting waits cooperatively for
pending writes and the worker. `status.json` contains only processing/connection
flags, last known device vendor/serial, fiscal-block flags and its update timestamp, never
credentials or customer data. It is display state, not fiscal evidence.

## CLI

One binary serves both modes: `workshop` opens the GUI; a subcommand runs in the
terminal without opening a printer port. Use `workshop --help` and the
[CLI contract and stable exit codes](docs/install-with-ai.md#4-configure-through-the-cli-not-files).
`--json` is available on every CLI command. `config show` reports pairing as
`paired`, never a token. Configuration, pairing and autostart mutations refuse
while the app holds the instance lock. `ports` lists names only; `check` and
`status` never prove printer readiness. The
[installation guide](docs/install-with-ai.md#4-configure-through-the-cli-not-files)
owns commands, JSON fields, exit codes and configuration examples.

Never include pairing codes or credentials in reports/logs.

## Autostart

Opt-in only: **Uruchamiaj przy logowaniu** registers a per-user login item and
saves the preference to start processing at launch. Login can then print queued
fiscal jobs without another click; approve it separately. Printer preflight and
unresolved-operation blocks still apply. See the
[autostart command](docs/install-with-ai.md#4-configure-through-the-cli-not-files)
and [OS artefacts](docs/testing/platforms.md#autostart-artefacts).

## Printer

Printer settings: **PC services → USB → POSNET → Windows 1250**, 9600 baud.
Typical ports: Linux `/dev/ttyACM0` (prefer `/dev/serial/by-id/...`), Windows
`COM3`, macOS `/dev/cu.usbmodem…`. Only serial connections are supported. The
user needs read/write access (`dialout` group on Linux); never run with sudo.

The GUI, `workshop ports` and discovery share a passive candidate list: hinted
USB devices (`1424:*` Posnet, `1A28:2512` Novitus, `0483:5740` Elzab), then
FTDI/Prolific/WCH/Silicon Labs RS232 bridges, then native ports, sorted by name
within each group. USB hints are not verified printer identities. Other USB
devices and Bluetooth are excluded; macOS uses `/dev/cu.*` rather than duplicate
`/dev/tty.*` nodes. Linux drops native UARTs whose sysfs `type` is zero, retaining
ports with unreadable metadata. Windows retains non-USB COM ports, including
virtual/Bluetooth ports reported as `Unknown`. An unlisted port can still be
entered manually. Listing never opens a port.

If the saved port fails, discovery identifies other serial candidates read-only
and verifies the first Temo Online found before using and saving it. It never
scans with an unresolved operation: the device must not change.

## Desktop app

- **Do wydruku** — eligible advances and sales for the printer's branch, showing
  kind, amount and event date. Queue a document with an optional buyer NIP;
  Holuto derives payments and advance deductions. Unpaid final-sale balances
  print as credit. Receipt lines and totals are not locally editable. Documents
  above the printer's amount limit remain listed but cannot execute. Each poll
  refreshes the current page of up to 100 documents, oldest first; **Następna
  strona** and **Poprzednia strona** navigate. Column headers sort the page
  without changing the selected document.
- **Raporty** — daily, monthly and periodic fiscal reports, confirmed before
  printing. Periodic dates require zero-padded `dd.mm.rrrr` (for example
  `01.01.2025`), must be on or after `01.01.2000`, cannot be in the future, and
  require start ≤ end. Reports and receipts share one worker thread; commands
  cannot interleave.
- **Historia** — receipts printed on this computer, with printer-assigned
  numbers or numbers entered in Holuto when unreadable. Sort by column header.
  This local log does not replace the printer's fiscal memory.
- **Ustawienia** — API URL, pairing code, printer port/speed, autostart,
  **Udostępniaj diagnostykę** (see [Diagnostics](#diagnostics)) and processing
  controls.

Polling runs every **3 seconds**, or **15 seconds** after errors or while
blocked. If Holuto is unreachable or answers 5xx when processing starts, the app
retries every 15 seconds and shows a notice; a 401 stops processing. A confirmed
receipt or report stays disabled until the worker finishes that command, so a
second click cannot queue it twice. Closing the window hides it in the tray and
releases its memory without stopping processing; reopen from the tray. Without a
tray, closing keeps the window visible. **Zatrzymaj** stops processing;
**Zakończ**, the tray's quit and the macOS Dock/⌘Q quit exit. All of them finish
the current operation first.

Prices are gross; API quantities and amounts are decimal strings. The schema
allows unit prices with up to eight decimal places, but this driver requires
whole grosze (at most two fractional digits in the string). It does not round
higher-precision prices; inconsistent line gross totals also fail before printing.
The printer calculates VAT. Each line uses its assigned slot for `23`, `8`, `5`,
`0` or `zw`; mixed rates are supported, with no highest-rate or whole-receipt
fallback. Holuto omits zero-price lines; the agent rejects any it receives.
Missing VAT-slot mappings reject queueing, never substitute a rate.
Buyer NIP, advance deductions and payments reach the device unchanged.
Line and company names must print in full as
received; unsupported text fails before fiscal commands, never transliterated or
truncated.

## Fiscal safety

- Before each operation, verify printer identity, fiscal mode, header,
  mechanism, command queue, transaction state, read-only mode, transaction
  resources held by another interface and VAT table in the session that prints.
- Journal claimed jobs in `operation.json` before their preflight; journal every
  fiscal command **before sending it**. Startup/discovery uses read-only commands
  without creating fiscal intent. Never retry a device command or reprint a receipt.
- Printer replies follow execution. After a silent command timeout, asynchronous
  `!sprn`/`!sdev` queries check for operator intervention (paper, cover, lever,
  temperature, power or a key). Wait up to 10 minutes from the first wait,
  however often the printer flaps between waiting and ready, and show the
  required action; resend nothing. No reported wait reason or no response leaves
  the outcome **unknown**.
- Uncertain outcomes, including process death during dispatch, are **unknown**
  and block receipts and reports across restarts. Inspect the printout and
  printer, then resolve the job in the **web app**. Resolution unblocks, never
  reissues. Completed prints with unreadable receipt numbers also remain
  **unknown** until staff enter the number in Holuto; retain completed journal
  evidence.
- A claimed job whose journal proves no fiscal command was sent (the printer
  check failed after the claim, or the app died before the first command)
  reports `failed` without blocking. If Holuto already marked that job `unknown`
  (it expired while the app was down), the app keeps blocking and tells staff
  that nothing was printed, so they can resolve it as failed in the web app.
- A `trinit` error (new certificates, read-only mode, full memory) cannot have
  issued a receipt, which needs `trend`; fail the job with the device's reason,
  without blocking. A transaction it might leave open fails the next preflight.
  A report command the printer rejects as a frame (`ERR` reply naming that
  command) printed nothing and fails without blocking. Any other refusal of a
  later receipt command or of a report remains **unknown**.
- Interrupted reports also block. After inspecting the printout, use **Wyjaśnij
  raport** with the same printer connected and ready. Resolution prints nothing.
- Persist the job, attempt and execution-start timestamp before dispatch;
  persist the result before sending it. A `done` result uses the printer clock
  (`rtcget`, Polish time, minute precision) read just before `trinit`, not the
  PC clock, which Holuto rejects if ahead. An unreadable clock fails before
  fiscal commands. Resending a saved API result preserves its attempt and issue
  time; it is not reprinting. Recovery reconciles durable evidence with server
  state without replaying printer commands.
- Terminal server resolutions are authoritative, except against completed-print
  evidence: if the receipt printed but Holuto already marked the job `failed` or
  `expired`, the app keeps the journal, blocks and names the job and receipt
  number. Record the receipt in Holuto, then confirm with **Wyjaśnij paragon**
  (same printer, connected); it prints and reports nothing. Corrupt or
  unreadable journals fail closed; deleting the journal is not recovery.
- History is a display cache that the worker publishes to the window; the window
  never reads it while processing. Report its read/write errors separately; they
  must not prevent settling a server-accepted result.

## Fiscal API contract

The contract is the backend's generated OpenAPI document, vendored as
[`openapi/agent.json`](openapi/agent.json); `crates/agent` generates its wire
types from it at build time. The test fake validates normal requests/responses
against it; scripted malformed-response faults deliberately bypass validation.
See [contract synchronization](docs/testing/compatibility.md#agent-and-the-backend)
for the update procedure. Nothing checks the live API against this copy: deploy
backend changes first.

Authenticated requests use `/agent`, `Authorization: Bearer <device token>`,
`App-Version: agent/<version>` and `Accept-Language: pl`, plus SDK-provided
`sentry-trace` propagation. `POST /agent/pair` exchanges `{code}` for
`{token, printerId}` without bearer or device-serial headers.
`POST /agent/session` and `POST /agent/heartbeat` send fresh telemetry:

```json
{
  "serial": "device-serial",
  "vendor": "posnet",
  "model": "POSNET TEMO ONLINE",
  "firmware": "32.01",
  "appVersion": "0.2.0",
  "vatSlots": [
    { "slot": 0, "vatRate": "23" },
    { "slot": 1, "vatRate": "8" }
  ],
  "maxItemNameLength": 80,
  "maxLinesPerReceipt": 500,
  "maxCompanyNameLength": 40,
  "maxQuantityDecimals": 8,
  "capabilities": {
    "advanceDeductions": true,
    "zeroTotalReceipts": true
  },
  "allowedCharacters": " !\"#$%&'()*+,-./0123…"
}
```

`vendor` is `"posnet"`, `"novitus"` or `"elzab"` and binds alongside the physical
serial. `vatSlots` is the actual device table (Temo Online has slots 0–6, A–G;
the contract allows 0–15), rates `"23"`, `"8"`, `"5"`, `"0"` or `"zw"`. Each
session/heartbeat replaces the backend mapping. `maxItemNameLength`,
`maxLinesPerReceipt`, `maxCompanyNameLength`, `allowedCharacters` (all printable
characters, NFC), `maxQuantityDecimals` (0–8) and `capabilities` are required
before queueing. The backend rejects names outside the limits, excessive
quantity precision (ignoring trailing zeros), advance deductions without
`advanceDeductions`, and fully prepaid sales without `zeroTotalReceipts`
(409 `fiscal_printer_quantity_precision`, `fiscal_printer_advances_unsupported`,
`fiscal_printer_zero_total_unsupported`). The agent independently checks the
fresh device profile before any fiscal command and reports unprintable jobs as
failed. These limits come from each driver's `fiscal::device::Capabilities`;
the wire capabilities contain only the two receipt flags, not `company_footer`.
`posnet::capabilities` defines the documented Temo Online limits: eight quantity
decimals, 80-character item names (`trline`), 500 lines (`trinit`),
40-character company footer lines (`trftrln`) and printable CP1250 from
0x20–0xFF, excluding DEL, no-break space, soft hyphen and table-drawing glyphs
(`¤ ¨ ˛ ´ ¸`, POT-I-DEV-37 p. 14).

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
simulator tests are not hardware certification. See the
[protocol audit](docs/superpowers/specs/2026-10-24-multi-vendor-fiscal-printers/novitus-protocol.md)
and [implementation evidence](docs/superpowers/specs/2026-10-24-multi-vendor-fiscal-printers/novitus-oss.md).

Session/heartbeat bind and verify the physical serial and vendor; a mismatch
fails with 403 `fiscal_serial_mismatch` or `fiscal_vendor_mismatch`.
Other requests carry the same `X-Device-Serial` header; device swaps are rejected
before taking or printing jobs.

- **`GET /agent/documents?limit=100[&cursor=…]`**:
  `{items:[{kind, orderId, orderNumber, allocationId, amount, customerName,`
  `occurredAt}], nextCursor}`. Return `nextCursor` unchanged and URL-encoded;
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

Job lines: `name`, `quantity`, `unitPrice`, `total`, `vatRate`, `vatSlot`.
Metadata: `orderId`, integer `orderNumber`, `printerId`, `kind`, nullable
`allocationId`, `etag`, `buyerNip`, `companyName`, `total`, `advances`,
`payments`, immutable attempt and lifecycle timestamps. Advances identify their
allocation and have one line per VAT rate, quantity 1.

`total` sums gross line totals. `advances` contains `{vatSlot, vatRate, amount}`
deductions, at most one per slot, each positive; duplicates and zero deductions
fail before fiscal commands. Advance jobs have no deductions. `payments`
contains `{form, amount}`; forms are `cash`, `card`, `transfer`, `mobile` and
`credit`. Decimals are unsigned plain strings such as `12.50`, with an optional
`.` fraction; no exponent or digit grouping. Use a matching backend; older
protocol shapes are unsupported.

Printer command order: `trinit`, optional `trnipset`, `trline` per line,
`tradvance` per deduction (name `Zaliczka`, VAT slot, amount, gross sales at
that slot), `trpayment` per nonzero payment, then `trend`. Payment types: cash
0, card 2, transfer 8, mobile 6 (name `Płatność mobilna`), credit 5. `trend.to`
is gross total minus advances; `trend.fp` sums payments. Fully prepaid sales
close at zero without payment commands. Before dispatch, slot/rate pairs must
match fresh telemetry, deductions cannot exceed slot sales, and payments must
exactly cover fiscal value. Amounts must be nonnegative whole grosze within the
printer's range.

## Diagnostics

Release-workflow packages require a build-time `SENTRY_DSN`; absent DSN and simulator
builds never report. **Udostępniaj diagnostykę** defaults on. Live opt-out gates
new structured telemetry; restart to disable native crash reporting.

Structured diagnostics retain reviewed messages, sanitized errors/frames,
timing/status, release-health sessions and printer ID/device serial; payloads,
arbitrary fields, hostnames and local paths are scrubbed. Native dumps contain
process memory and may expose sensitive data despite those hooks. See
[Diagnostics and privacy](docs/telemetry.md) for the exact allowlist, lifecycle,
500 ms flush bound and native-dump warning, and
[CI configuration](docs/ci.md#release-telemetry-artifacts).

## Development and releases

Use [contributor commands](AGENTS.md#verify),
[testing](docs/testing/README.md) and [finishing workflows](docs/workflows.md)
for checks, snapshots, mutation/stress runs and contract synchronization.

`rust-toolchain.toml` pins Rust. Formatting uses the dated nightly because
`.rustfmt.toml` enables unstable options. Linux needs `pkg-config` and
fontconfig headers (`libfontconfig1-dev` / `fontconfig-devel`). Lint policy is
in `Cargo.toml` (pedantic, nursery and restriction lints) and `clippy.toml`;
dependency and license policy is in `deny.toml`.

Exercise printer behavior through offline simulators and tests, **never real
fiscal commands**. `cargo run -p workshop --features simulator` starts Posnet,
Novitus and Elzab peers and selects Posnet by default. Set
`WORKSHOP_SIMULATOR=novitus` or `WORKSHOP_SIMULATOR=elzab` to select another
brand; invalid values are refused. State lives in `workshop-simulator` beside
the real state directory. Release builds reject this feature.

Linux watcher tests require `dbus-run-session` and spawn their own private bus;
Windows COM tests need a signed virtual pair. See [platform procedures](docs/testing/platforms.md).

`bash packaging/build.sh` writes the host OS/CPU package to `release/`. CI
builds Windows x64, macOS ARM64/x64 and Linux ARM64/x64. A push to `master`
publishes only when the root `Cargo.toml` `[workspace.package] version`
increases above the previous version and all published releases. Manual
**Release** runs create Actions artifacts without publishing. Retry a failed
release by rerunning its original workflow run.

Buyer NIP and payment forms follow
[Posnet POT-I-DEV-37 v5978, pp. 295, 297 and 314](<https://raw.githubusercontent.com/kashiash/PosnetLibrary/master/DOCS/pot-i-dev-37-5978-specyfikacja-protokolu-posnet-online-mr2021-2%20(1).pdf>)
(Online MR2021; Temo Online). No vendor SDK or specification is redistributed.
