# Holuto Workshop — fiscal printers

Tray app for the workshop PC connected to a supported Posnet, Novitus or Elzab
online fiscal printer. Prints **only Holuto sales and advances** queued in the
web app or selected in Holuto Workshop. No manual receipt editor or separate
agent; processing continues with the window closed.

Contributors start with [AGENTS.md](AGENTS.md).

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

The [installation guide](docs/install-with-ai.md) covers requirements, pinned
downloads, checksums/attestations, OS security prompts, install paths, updates
and uninstall; each release ships it with `install.sh`, `install.ps1` and
`SHA256SUMS.txt`. Never pipe a download to a shell.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

[Slint](https://slint.dev) uses the Slint Royalty-free Desktop, Mobile, and Web
Applications License 2.0; the badge is required attribution.

## Sign in and connect

Pair in **Ustawienia** with a one-time code generated for the printer in Holuto
(valid 15 minutes; see the
[guided configuration sequence](docs/install-with-ai.md#guided-configuration-sequence)).
Pairing replaces the previous device token, which stays internal. **Zapisz**
saves; **Uruchom** starts processing. Every launch needs an explicit start
unless [autostart](#autostart) is on.

Use the final API URL, including any path prefix. HTTPS is required except on
loopback (`localhost`, `127.0.0.0/8`, `::1`); redirects are rejected. TLS uses
the OS certificate store. Changing the API URL forgets the saved pairing. A 401
stops processing; generate a fresh pairing code in Holuto and pair again. There
is no token refresh.

State directory:

| OS      | Directory                                                      |
| ------- | -------------------------------------------------------------- |
| Linux   | `$XDG_STATE_HOME/workshop` (default `~/.local/state/workshop`) |
| macOS   | `~/Library/Application Support/workshop`                       |
| Windows | `%LOCALAPPDATA%\workshop` (Local AppData Known Folder)         |

- `settings.json`: API URL, internal token, port, speed, `vendor` (`"posnet"`,
  `"novitus"`, `"elzab"` or `null` to detect), `telemetryEnabled`,
  start-on-launch. Keep private and out of logs.
- `operation.json`: fiscal journal; each receipt/report records its vendor and
  fiscal memory unique number, both required for resolution. Malformed files
  block.
- `history.jsonl`: local receipt history, newest 500 receipts.
- `status.json`: processing/connection and fiscal-block flags, last device
  vendor/unique number, update time. Display state; never credentials or
  customer data.
- `instance.lock`: one instance per state directory.

On Unix, opening tightens the directory to **0700** and the lock to **0600**;
every write replaces a data file with a **0600** copy, and history loading
tightens its file. Settings, status and journal reads do not repair modes.
On Windows, profile ACLs protect the directory.

One background FIFO writer persists settings and status. Starting waits for
settings to persist; quitting waits for pending writes and the worker.

Uninstalling keeps fiscal state. Never delete the journal to unblock printing;
unresolved evidence needs human reconciliation.

## CLI

One binary: `workshop` opens the GUI; a subcommand runs in the terminal without
opening a printer port. Every command accepts `--json`. `config show` reports
pairing as `paired`, never a token. Configuration, pairing and autostart
changes refuse while the app holds the instance lock. `ports` lists names only;
`check` and `status` never prove printer readiness. Commands, JSON fields, exit
codes and examples live in the
[installation guide](docs/install-with-ai.md#4-configure-through-the-cli-not-files).

Never put pairing codes or credentials in reports or logs.

## Autostart

Opt-in: **Uruchamiaj przy logowaniu** registers a per-user login item and starts
processing at launch, so login can print queued fiscal jobs without a click;
approve that separately. Preflight and unresolved-operation blocks still apply.
See the [autostart command](docs/install-with-ai.md#4-configure-through-the-cli-not-files)
and [OS artefacts](docs/testing/platforms.md#autostart-artefacts).

## Printer

Dedicate the printer to Workshop; do not share it with other software or
interfaces. Only USB virtual serial and RS232 connections are supported, not
LAN/RNDIS, Novitus XML or NoviAPI. The brand is detected, not selected by the
operator. Configure the printer manually; discovery never changes its settings.

### Posnet

Temo Online: **PC services → USB → POSNET → Windows 1250**, 9600 baud.
Workshop stays not ready while any PC interface set to POSNET uses another
code page. Advances, zero-total receipts and company footers are supported.

### Novitus

Candidate profiles cover HD II Online, Deon Online, Bono Online and Point,
with exact model/firmware allowlists. Select native **NOVITUS** (dialect
option 5 = `1`), Windows-1250 for **both input and output** (options 91/92 =
`1`) and 8N1 at the configured speed (Workshop defaults to 9600).
Use the fiscal **PC1** endpoint: read-only `50$i` must identify PC1.
USB A/B names do not prove the endpoint; PC2 can answer queries but cannot
fiscalize receipts. Full CP1250 text requires the device's `52$i` capability
bit; otherwise only ASCII is advertised. Advance deductions, zero-total
receipts and company footers remain disabled pending hardware qualification.
Program the complete company name in the header for production use.

### Elzab

Candidate profiles cover Zeta Online, Mera Online and Cube Online, not legacy
Zeta/D10 or network-only T10. USB uses native **ElzabSTX** regardless of the
menu protocol; **RS232 must explicitly select STX**, 8N1 and the configured
speed (9600 by default). Select **CP1250** manually. Per-VAT advance
settlement is supported; zero-total receipts and company footers remain
disabled. Program the complete company name in the header.
Daily reports are accepted asynchronously and always require human
reconciliation; acceptance is not proof of completion.

Profiles are not hardware certificates. The
[supervised acceptance checklist](docs/testing/hardware-acceptance.md) records
evidence per model/firmware before qualification; agents never run it.
Typical ports: Linux `/dev/ttyACM0` (prefer `/dev/serial/by-id/...`), Windows
`COM3`, macOS `/dev/cu.usbmodem…`. The user needs read/write access
(`dialout` group on Linux); never run with sudo.

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
and verifies the first supported device before using and saving its port/vendor.
It never scans with an unresolved operation: the device must not change.

## Desktop app

- **Do wydruku** — eligible advances and sales for the printer's branch, showing
  kind, amount and legal deadline; overdue documents are marked. Queue a
  document with an optional buyer NIP, which clears after each queued receipt;
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
tray icon, closing keeps the window visible; on Linux this includes an icon that
could not register because the desktop's tray started after the app.
**Zatrzymaj** stops processing; **Zakończ**, the tray's quit, the macOS Dock/⌘Q
quit and, on Linux and macOS, SIGTERM, SIGHUP or SIGINT (Linux logout or
shutdown, Ctrl+C) exit. All of them finish the current operation first.

Prices are gross; API quantities and amounts are decimal strings. The schema
allows unit prices with up to eight decimal places, but this driver requires
whole grosze (at most two fractional digits in the string). It does not round
higher-precision prices; inconsistent line gross totals also fail before printing.
The printer calculates VAT. Each line uses its assigned slot for `23`, `8`, `5`,
`0` or `zw`; mixed rates are supported, with no highest-rate or whole-receipt
fallback. Holuto omits zero-price lines; the agent rejects any it receives.
Missing VAT-letter mappings reject queueing, never substitute a rate.
Buyer NIP, advance deductions and payments reach the device unchanged.
Line and company names must print in full as
received; unsupported text fails before fiscal commands, never transliterated or
truncated.

## Fiscal safety

- Before each operation, verify identity, fiscal mode, header, mechanism,
  observable busy/open-document/read-only state, VAT and clock in the session
  that prints. Posnet also checks queue emptiness and other-interface resource
  ownership. Novitus/Elzab expose no equivalent ownership readout: dedication
  to Workshop is mandatory; no open document is adopted or cancelled.
- Journal claimed jobs in `operation.json` before their preflight; journal every
  fiscal command **before sending it**. Startup/discovery uses read-only commands
  without creating fiscal intent. Never retry a device command or reprint a receipt.
- **Posnet:** after a silent timeout, asynchronous `!sprn`/`!sdev` queries
  observe operator waits (paper, cover, lever, temperature, power or a key).
  Wait at most ten minutes from the first wait, without resending.
  **Novitus:** real-time DLE/ENQ/DLE2 observe waits without resending;
  undocumented DLE2 byte order is never guessed. **Elzab:** no concurrent
  status polling is assumed, so silence is not an operator wait: after 10 s on
  an item or payment, 30 s at close or on a daily report, or ten minutes on a
  periodic report, the outcome is **unknown**. Paper-out auto-cancellation also
  leaves an uncertain outcome. Unexplained silence or unreadable status is
  **unknown**.
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
- Certain post-send refusals are brand-specific: Posnet initial `trinit`
  errors and report `ERR` frame rejections; Novitus initial `$h` refusal and
  daily-report keyboard cancellation (37); Elzab transport rejection of the
  initial `LOR` only. Later receipt errors remain **unknown**, including Elzab
  zero-close error 1008. No refusal permits resending. An accepted Elzab
  `RRP` daily report is always **unknown**, even if counters advance.
- Interrupted reports also block. After inspecting the printout, use **Wyjaśnij
  raport** with the same printer connected and ready; an overdue daily report
  or read-only mode does not prevent resolution. Resolution prints nothing.
- Persist the job, attempt and execution-start timestamp before dispatch;
  persist the result before sending it. A `done` result uses the printer clock
  (`rtcget` on Posnet, `#c` on Novitus, `XCR` on Elzab), interpreted in Polish
  time, not the PC clock. Unreadable clocks fail before fiscal commands.
  Novitus and Elzab also reject ambiguous or nonexistent Warsaw local times.
  Posnet prefers offset-bearing `tm`; legacy offset-free `da` selects the
  earlier instant in Warsaw's repeated autumn hour. Resending a saved API
  result preserves its attempt and issue time; it is not reprinting. Recovery
  reconciles durable evidence with server state without replaying commands.
- Terminal server resolutions are authoritative, except against completed-print
  evidence: if the receipt printed but Holuto already marked the job `failed` or
  `expired`, the app keeps the journal, blocks and names the job and receipt
  number. Record the receipt in Holuto, then confirm with **Wyjaśnij paragon**
  (same printer, connected and ready as for reports); it prints and reports
  nothing. Corrupt or unreadable journals fail closed; deleting the journal is
  not recovery.
- History is a display cache that the worker publishes to the window; the window
  never reads it while processing. Report its read/write errors separately; they
  must not prevent settling a server-accepted result.

## Fiscal API contract

The `/agent` API, device capability profiles, discovery and printer command
sequences are specified in [docs/fiscal-contract.md](docs/fiscal-contract.md).

## Diagnostics

Release-workflow packages require a build-time `SENTRY_DSN`; absent DSN and simulator
builds never report. **Udostępniaj diagnostykę** defaults on. Live opt-out gates
new structured telemetry; restart to disable native crash reporting.

Structured diagnostics retain reviewed messages, sanitized errors/frames,
timing/status, release-health sessions and printer ID/device unique number; payloads,
arbitrary fields, hostnames and local paths are scrubbed. Native dumps contain
process memory and may expose sensitive data despite those hooks. See
[Diagnostics and privacy](docs/telemetry.md) for the exact allowlist, lifecycle,
500 ms flush bound and native-dump warning, and
[CI configuration](docs/ci.md#release-telemetry-artifacts).

## Development and releases

[AGENTS.md](AGENTS.md) holds the crate layout, conventions and commands; the
[testing guide](docs/testing/README.md), [workflows](docs/workflows.md) and
[CI](docs/ci.md) cover verification and releases. Printer behavior is exercised
only through offline simulators, **never real fiscal commands**.

`bash packaging/build.sh` writes the host OS/CPU package to `release/`. A push to
`master` publishes only when the root `Cargo.toml` `[workspace.package] version`
increases above the previous version and all published releases. Manual
**Release** runs create Actions artifacts without publishing.
