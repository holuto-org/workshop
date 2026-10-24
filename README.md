# Holuto Workshop

Tray app for the workshop PC connected to a Posnet Temo Online fiscal printer.
Prints **only Holuto sales and advances** queued in the web app or selected in
Holuto Workshop. No manual receipt editor or separate agent; processing
continues with the window closed.

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
version. Builds are unsigned. Windows SmartScreen may prompt on first launch. On
macOS 15 and newer, Gatekeeper blocks the first launch: open **System Settings →
Privacy & Security** and click **Open Anyway** (right-click → Open no longer
bypasses it). The Windows `.exe` links the C runtime statically and needs no
redistributable. The `.deb` installs `/usr/bin/workshop` and a launcher; it
requires the glibc of the Ubuntu 24.04 build host (2.39) or newer. Software
rendering needs no GPU or OpenGL driver, including on remote desktops and VMs.

For verified per-user installation, follow the
[AI/manual installation guide](docs/install-with-ai.md). Each release includes
[install.sh](https://github.com/holuto-org/workshop/releases/latest/download/install.sh),
[install.ps1](https://github.com/holuto-org/workshop/releases/latest/download/install.ps1)
and
[SHA256SUMS.txt](https://github.com/holuto-org/workshop/releases/latest/download/SHA256SUMS.txt).
Pin one release and verify its guide, script and package before running them;
never pipe a download to a shell. The scripts refuse running apps and do not
launch, pair, enable autostart or access fiscal state. Windows installs
`%LOCALAPPDATA%\Programs\Holuto Workshop\workshop.exe` plus a Start Menu
shortcut and user PATH entry. macOS installs
`~/Applications/Holuto Workshop.app` and `~/.local/bin/workshop`; add that
directory to PATH yourself if needed, or use
`~/Applications/Holuto Workshop.app/Contents/MacOS/workshop` directly.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

[Slint](https://slint.dev) uses the Slint Royalty-free Desktop, Mobile, and Web
Applications License 2.0; the badge is required attribution.

## Sign in and connect

1. In Holuto, add/select a printer under **Ustawienia → Drukarki fiskalne** and
   generate a **one-time pairing code**. It expires after 15 minutes.
2. In Holuto Workshop's **Ustawienia**, enter the final **Adres API**, select
   the printer port/speed, then enter **Kod parowania** and choose **Sparuj**.
   Pairing stores credentials internally; no device token is displayed or
   copied. Codes are single-use; successful pairing revokes the previous token.
3. Click **Uruchom** when ready to process the branch's queued receipts.
   **Zapisz** saves without starting. Processing requires an explicit start on
   each launch unless you separately opt into [autostart](#autostart).

Use the final API URL, including any path prefix. HTTPS is required except on
loopback (`localhost`, `127.0.0.0/8`, `::1`); redirects are rejected. TLS uses
the OS certificate store. A 401 stops processing; generate a fresh pairing code
in Holuto and pair again. There is no token refresh.

State directory (Unix: **0700**, data files **0600**):

| OS      | Directory                                                      |
| ------- | -------------------------------------------------------------- |
| Linux   | `$XDG_STATE_HOME/workshop` (default `~/.local/state/workshop`) |
| macOS   | `~/Library/Application Support/workshop`                       |
| Windows | `%LOCALAPPDATA%\workshop` (Local AppData Known Folder)         |

Uninstalling the application retains fiscal state. Never delete the journal to
unblock printing; unresolved evidence requires human reconciliation.

`settings.json` stores the API URL, internal token, port, speed, diagnostics
(`telemetryEnabled`) and start-processing-on-launch preferences. Keep it
private, out of logs, and protected by profile ACLs on Windows. `operation.json`
is the fiscal journal; `history.jsonl` is local receipt history, compacted to
the newest 500 receipts. `instance.lock` prevents a second instance from using
the same state directory. On Unix every one of these files is **0600**, and an
existing state directory is tightened to **0700**.

Settings and `status.json` are persisted by one background FIFO writer. Starting
waits for successful settings persistence; quitting waits cooperatively for
pending writes and the worker. `status.json` contains only processing/connection
flags, device identity, fiscal-block flags and its update timestamp, never
credentials or customer data. It is display state, not fiscal evidence.

## CLI

One binary serves both modes: `workshop` opens the GUI; a subcommand runs in the
terminal without opening a printer port. Use `workshop --help` and the
[CLI contract and stable exit codes](docs/install-with-ai.md#4-configure-through-the-cli-not-files).
`--json` is available on every command. `config show` reports pairing as
`paired`, never a token. Configuration changes, pairing and autostart changes
refuse while the app holds the instance lock: choose **Zakończ**, let pending
work finish, then configure. `ports` lists names only; `check` and `status`
never prove printer readiness.

`status` reports the running app's published `state`: `processing`, `connected`,
`serial`, `printerId`, `blocked`, `reportUnresolved`, `orphanedReceipt` and
`updatedAt`. With no instance lock it reports `appRunning:false` and omits state;
a snapshot left after a crash is ignored. While a running app has not published
state yet, the command reports `status_unavailable` (exit 1), not a guessed value.

```sh
workshop config show --json
workshop ports --json
workshop config set --api-url https://holuto.example --port auto --speed 9600 \
  --diagnostics off
workshop pair XXXX-XXXX
workshop check --json
workshop status --json
```

Replace the example URL and pairing code with the intended Holuto instance and
fresh code; never include codes or credentials in reports/logs.

## Autostart

Opt-in only: use **Ustawienia** or `workshop autostart on` /
`workshop autostart off` while the app is stopped (**Uruchamiaj przy
logowaniu**). Enabling registers a per-user login item and saves a preference to
start processing at launch. Login can then print queued fiscal jobs without
another click; approve it separately. Full printer preflight and unresolved
operation blocks remain mandatory. Disable autostart before removing the app.

- **Linux**: `$XDG_CONFIG_HOME/autostart/workshop.desktop` (default
  `~/.config/autostart/workshop.desktop`).
- **macOS**: `~/Library/LaunchAgents/com.holuto.workshop.plist`
  (`com.holuto.workshop`).
- **Windows**: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value
  `Holuto Workshop`.

## Printer

Printer settings: **PC services → USB → POSNET → Windows 1250**, 9600 baud.
Typical ports: Linux `/dev/ttyACM0` (prefer `/dev/serial/by-id/...`), Windows
`COM3`, macOS `/dev/cu.usbmodem…`. Only serial connections are supported. The
user needs read/write access (`dialout` group on Linux); never run with sudo.

If the saved port does not answer, the app sends one read-only identification
command per other serial port, then uses and saves the first Temo Online found.
It never scans with an unresolved operation: the device must not change.

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

Prices are gross; API quantities and amounts are decimal strings. Holuto
supplies unit prices with at most two decimal places and line gross totals.
Quantities and prices print unchanged. Higher-precision prices or totals
differing from quantity × price rounded half up to grosze fail before printing.
The printer calculates VAT. Each line uses its assigned slot for `23`, `8`, `5`,
`0` or `zw`; mixed rates are supported, with no highest-rate or whole-receipt
fallback. Zero-price lines are omitted. Missing VAT-slot mappings reject
queueing, never substitute a rate. Buyer NIP, advance deductions and payments
reach the device unchanged. Line and company names must print in full as
received; unsupported text fails before fiscal commands, never transliterated or
truncated.

## Fiscal safety

- Before each operation, verify printer identity, fiscal mode, header,
  mechanism, command queue, transaction state, read-only mode, transaction
  resources held by another interface and VAT table in the session that prints.
- Journal intent **before the first command reaches the printer**: claim taken
  jobs in `operation.json` before preflight; journal every command before
  sending. Never retry a device command or reprint a receipt.
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
types from it at build time and the test fake validates every request and
response against it. After a backend `/agent` change, run
`scripts/openapi-sync.sh` (`BACKEND_DIR` defaults to `../backend`) and commit
the `openapi/` diff. Nothing checks the live API against this copy: the backend
deploys first.

Requests use `/agent`, `Authorization: Bearer <device token>`,
`App-Version: agent/<version>` and `Accept-Language: pl` for Polish error
details, plus the `sentry-trace` header the Sentry SDK provides, so a fiscal
operation's trace continues in the backend. `POST /agent/session` and
`POST /agent/heartbeat` send fresh telemetry:

```json
{
  "serial": "device-serial",
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
  "allowedCharacters": " !\"#$%&'()*+,-./0123…"
}
```

`vatSlots` is the actual device table (Temo Online has slots 0–6, A–G; the
contract allows 0–15), rates `"23"`, `"8"`, `"5"`, `"0"` or `"zw"`. Each
session/heartbeat replaces the backend mapping. `maxItemNameLength`,
`maxLinesPerReceipt`, `maxCompanyNameLength` and `allowedCharacters` (all
printable characters, NFC) are required before queueing; the backend rejects
line or company names outside these limits. The protocol cannot report them, so
`posnet::text` defines the documented Temo Online limits: 80-character item
names (`trline`), 500 lines (`trinit`), 40-character company footer lines
(`trftrln`) and printable CP1250 from 0x20–0xFF, excluding DEL, no-break space,
soft hyphen and the bytes the protocol prints as table-drawing glyphs
(`¤ ¨ ˛ ´ ¸`, POT-I-DEV-37 p. 14).

Session/heartbeat bind and verify the physical serial; pairing with another live
printer fails with 409. Other requests carry the same `X-Device-Serial` header;
device swaps are rejected before taking or printing jobs.

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

Release builds use Sentry in the EU. Build-time `SENTRY_DSN` and
`SENTRY_ENVIRONMENT` select ingestion/environment; absent DSN and simulator
builds never report. **Ustawienia → Udostępniaj diagnostykę** defaults on; turn
it off to opt out. Disabling native crash reporting requires restarting.

Structured diagnostics send actionable errors/panics, safe logs/breadcrumbs,
operation/HTTP timings, release-health sessions and printer ID/device serial.
They never send pairing codes/tokens, customer/company names, NIP, receipt
lines, amounts, settings/journals/history, API URL path/query or hostname.
Structured diagnostics remove local paths while retaining file basenames and
debug identifiers. Native crash dumps contain process memory and may contain
sensitive data. Sentry processes and discards them by default unless attachment
storage is enabled. See the
[privacy policy and native-dump warning](docs/telemetry.md) and
[required GitHub configuration](docs/ci.md#release-telemetry-artifacts).

## Development and releases

```bash
# If rustup is missing
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install nightly-2026-10-06 --profile minimal --component rustfmt
cargo run -p workshop
cargo nextest run --workspace --all-features
cargo +nightly-2026-10-06 fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny --locked check && cargo shear --deny-warnings
```

`rust-toolchain.toml` pins Rust. Formatting uses the dated nightly because
`.rustfmt.toml` enables unstable options. Linux needs `pkg-config` and
fontconfig headers (`libfontconfig1-dev` / `fontconfig-devel`). Lint policy is
in `Cargo.toml` (pedantic, nursery and restriction lints) and `clippy.toml`;
dependency and license policy is in `deny.toml`.

Exercise printer behavior through the socket simulator and tests, **never real
fiscal commands**. Run locally with `cargo run -p workshop --features simulator`
to replace serial I/O with an in-process Temo peer. Its state lives in
`workshop-simulator` beside the real state directory. Release builds reject this
feature.

Linux's complete suite runs under `dbus-run-session`; nextest's default profile
excludes Windows COM-pair tests. The `ci` profile includes them and requires
`HOLUTO_TEST_COM_PAIR` pointing only at a signed virtual com0com pair on
Windows. Review fiscal snapshots with
`cargo insta test --test-runner nextest --review -p posnet --all-features`, then
rerun ordinary nextest. After backend contract changes regenerate its OpenAPI
and run `bash scripts/openapi-sync.sh` here. See
[testing](docs/testing/README.md), [finishing workflows](docs/workflows.md) and
[CI](docs/ci.md).

`bash packaging/build.sh` writes the host OS/CPU package to `release/`. CI
builds Windows x64, macOS ARM64/x64 and Linux ARM64/x64. A push to `master`
publishes only when the root `Cargo.toml` `[workspace.package] version`
increases above the previous version and all published releases. Manual
**Release** runs create Actions artifacts without publishing. Retry a failed
release by rerunning its original workflow run.

Buyer NIP and payment forms follow
[Posnet POT-I-DEV-37 v5978, pp. 295, 297 and 314](<https://raw.githubusercontent.com/kashiash/PosnetLibrary/master/DOCS/pot-i-dev-37-5978-specyfikacja-protokolu-posnet-online-mr2021-2%20(1).pdf>)
(Online MR2021; Temo Online). No vendor SDK or specification is redistributed.
