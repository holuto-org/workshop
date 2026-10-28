# Holuto Workshop — fiscal printers

Tray app for the workshop PC connected to a supported Posnet, Novitus or Elzab
online fiscal printer. Prints **only Holuto sales and advances** queued in the
Holuto web app, while processing is running. Receipts are queued and browsed
only in the web app; the desktop has no receipt list, manual queuing or receipt
editor, and needs no separate agent. Closing the window does not stop processing.

Contributors start with [AGENTS.md](AGENTS.md).

## Install

Use an official [release](https://github.com/holuto-org/workshop/releases)
whose assets include the matching `install-with-ai.md`, `install.sh`,
`install.ps1` and `SHA256SUMS.txt`. Older releases may predate this packaging;
if the matching guide or required assets are missing, stop rather than use
instructions from `master` or substitute a legacy binary.

The current release workflow builds these packages:

| Platform | File |
| --- | --- |
| Windows x64 | `workshop-win-x64.exe` |
| macOS Apple Silicon | `workshop-mac-arm64.dmg` |
| macOS Intel | `workshop-mac-x64.dmg` |
| Ubuntu 24.04+ / Debian 13+ x64 | `workshop-linux-x64.deb` |
| Ubuntu 24.04+ / Debian 13+ ARM64 | `workshop-linux-arm64.deb` |

Windows builds are unsigned; macOS bundles are ad-hoc signed, not Developer-ID
signed or notarized. Software rendering needs no GPU or OpenGL driver, but a
graphical desktop is required.

The [installation guide](docs/install-with-ai.md) explains requirements and the
release-matched download, checksum/attestation, installation, update and
uninstall procedure. Never pipe a download to a shell.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

[Slint](https://slint.dev) is used under the
[Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0](https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md);
this badge provides its required attribution.

## Sign in and connect

Pair in **Settings** with a short-lived, one-time code generated for the
printer in Holuto; see the
[guided configuration sequence](docs/install-with-ai.md#guided-configuration-sequence).
Pairing obtains a device token internally and never opens a printer.
Settings save when committed; **Start** starts processing after they persist.
Pairing and installation are not permission to print. Every launch needs an
explicit start unless [autostart](#autostart) is on.

Release packages carry the production Holuto API URL. Changing it through the
CLI forgets the saved pairing. A 401 stops processing: generate a fresh pairing
code in Holuto and pair again. There is no token refresh. Other configuration
and HTTPS requirements live in the installation guide.

### UI language

In **Settings → Language**, choose **System**, **English**, **Polski**,
**Español** or **Русский**. System is the default: the first supported OS
language preference is used, with English when none match. Regional variants
such as `es-MX` use the corresponding supported language.

The choice saves immediately and updates the window, tray, confirmations and
operator messages without restarting processing. It does not change receipts,
fiscal commands or CLI output. CLI diagnostics remain Polish; original external
device/server diagnostics are shown verbatim inside translated explanations.
UI names in this guide use English and follow the selected language in the app.

### State directory

| OS      | Directory                                                      |
| ------- | -------------------------------------------------------------- |
| Linux   | `$XDG_STATE_HOME/workshop` (default `~/.local/state/workshop`) |
| macOS   | `~/Library/Application Support/workshop`                       |
| Windows | `%LOCALAPPDATA%\workshop` (Local AppData Known Folder)         |

This directory contains private settings (including the internal token),
display-only status, process locks, the `operation.json` fiscal journal and
possibly sensitive native crash dumps. Configure through the GUI or CLI, not
by editing files; do not include the directory in diagnostic reports.

Only one app or configuration command can own a state directory. Starting
waits for settings to persist; quitting waits for pending writes and the worker.
Uninstalling keeps fiscal state. Never delete the journal to unblock printing;
unresolved evidence needs human reconciliation.

## CLI

One binary: `workshop` opens the GUI; a subcommand runs in the terminal without
opening a printer port. Every command accepts `--json`. `config show` reports
pairing as `paired`, never a token. Configuration, pairing and autostart
changes refuse while the app or another command holds the instance lock.
`ports` prints names only (its `--json` adds a USB `hint`); `check` and
`status` never prove printer readiness. Commands, JSON fields, exit codes and
examples live in the
[installation guide](docs/install-with-ai.md#4-configure-through-the-cli-not-files).

Never put pairing codes or credentials in reports or logs.

## Autostart

Opt-in: **Start at login** registers a per-user login item and starts
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
evidence per model/firmware before qualification; physical checks are human-only.
Typical ports: Linux `/dev/ttyACM0` (prefer `/dev/serial/by-id/...`), Windows
`COM3`, macOS `/dev/cu.usbmodem…`. The user needs read/write access
(`dialout` group on Linux); never run with sudo.

The GUI and `workshop ports` list candidates without opening a port; USB hints
are not verified printer identities. An unlisted port can be entered manually.

If the saved port fails, discovery identifies other serial candidates read-only
and verifies the first supported device before using and saving its port/vendor.
It never scans with an unresolved operation: the device must not change.

## Desktop app

- **Status** — read-only processing, Holuto, printer identity and readiness
  status. This view does not replace fiscal memory.
- **Reports** — daily, monthly and periodic fiscal reports, confirmed before
  printing. These are real fiscal operations, not test pages. Periodic dates
  use zero-padded `dd.mm.yyyy` (for example `01.01.2025`); report availability
  also depends on the
  [printer's date and brand-specific guards](docs/fiscal-contract.md#report-dates-and-printer-clock).
- **Settings** — pairing, printer port/speed, language, theme, autostart,
  [diagnostics](#diagnostics) and processing controls.

Reports and receipts share one worker and cannot interleave. A confirmed
report stays disabled until its command finishes.
Closing the window hides it in the tray without stopping processing; reopen
from the tray. Without a working tray icon, closing keeps the window visible.
**Stop** stops processing; **Quit**, the tray's quit, the macOS Dock/⌘Q
quit, macOS logout, restart or shutdown (macOS waits instead of being cancelled)
and, on Linux and macOS, SIGTERM, SIGHUP or SIGINT (Linux logout or shutdown,
Ctrl+C) exit. All of them finish the current operation first. Windows logout
or shutdown ends the app without that wait; an operation it interrupts may be
left uncertain, which blocks printing until a person resolves it.

Prices are gross and the printer calculates VAT. Text must print in full,
without transliteration or truncation. Unsupported jobs fail before fiscal
commands; capability, VAT and amount details live in the
[fiscal contract](docs/fiscal-contract.md).

## Fiscal safety

- Before each operation, verify identity, fiscal mode, header, mechanism,
  observable busy/open-document/read-only state, VAT and clock in the session
  that prints. Posnet also checks queue emptiness and other-interface resource
  ownership. Novitus/Elzab expose no equivalent ownership readout: dedication
  to Workshop is mandatory; no open document is adopted or cancelled.
- Journal claimed jobs in `operation.json` before their preflight; journal every
  fiscal command **before sending it**. Startup/discovery creates no fiscal
  intent and sends only read-only commands, apart from the Novitus session's
  `3#e` response-mode selection. Never retry a device command or reprint a
  receipt.
- Paper or operator waits never cause a resend. If the printer stops answering
  and completion cannot be established, the outcome is **unknown**. See
  [per-brand wait limits](docs/fiscal-contract.md#silence-and-operator-waits).
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
- Only refusals classified as certain by the
  [documented host policy][certain-refusals] can be reported failed after
  dispatch. Other failures remain **unknown**; no refusal permits resending.
  A Posnet footer failure after a completed receipt does not undo
  the print; the printer stays not ready until its open footer is cleared.
  An accepted Elzab daily report needs human resolution even if counters advance.
- Interrupted reports also block. After inspecting the printout, use **Resolve
  report** with the same printer connected and ready; an overdue daily report
  or read-only mode does not prevent resolution. Resolution prints nothing.
- Persist the job, attempt and execution-start timestamp before dispatch;
  persist the result before sending it. A `done` result uses the printer clock,
  not the PC clock. Unreadable receipt clocks fail before fiscal commands;
  [Warsaw clock rules](docs/fiscal-contract.md#report-dates-and-printer-clock)
  differ by brand and operation. Resending a saved API result preserves its
  attempt and issue time; it is not reprinting. Recovery reconciles durable
  evidence with server state without replaying commands.
- Terminal server resolutions are authoritative, except against completed-print
  evidence: if the receipt printed but Holuto already marked the job `failed` or
  `expired`, the app keeps the journal, blocks and names the job and receipt
  number. Record the receipt in Holuto, then confirm with **Resolve receipt**
  (same printer, connected and ready as for reports); it prints and reports
  nothing. Corrupt or unreadable journals fail closed; deleting the journal is
  not recovery.

[certain-refusals]: docs/testing/fiscal-safety.md#recovery-classification

## Fiscal API contract

The `/agent` API, device capability profiles, discovery and printer command
sequences are specified in [docs/fiscal-contract.md](docs/fiscal-contract.md).

## Diagnostics

**Udostępniaj diagnostykę** defaults on in configured release builds. Live
opt-out gates new structured telemetry; restart to disable native crash
reporting. Simulator builds do not report.

Structured diagnostics include reviewed messages, sanitized errors,
timing/status and printer identifiers. **Native crash dumps contain process
memory and may expose credentials or fiscal/customer data** despite structured
scrubbing. Read [Diagnostics and privacy](docs/telemetry.md) for the exact data
policy and opt-out behavior.

## Development and releases

[AGENTS.md](AGENTS.md) holds the crate layout, conventions and
[build/test and simulator commands](AGENTS.md#verify). The debug-only desktop
simulator uses a separate `workshop-simulator` state directory, all three
offline printer peers and a loopback Holuto with sample queued jobs; it never
contacts production Holuto or a real printer. It starts processing automatically
on launch, printing those sample jobs in the selected simulator.

The [testing guide](docs/testing/README.md), [workflows](docs/workflows.md) and
[CI](docs/ci.md) cover verification and releases. Automated printer verification
uses only simulators; physical qualification follows the separate
[human-supervised acceptance procedure](docs/testing/hardware-acceptance.md).
