# Supervised printer hardware acceptance

**A person executes this procedure, never an AI agent or CI.** Receipts and
reports on fiscalized printers are irreversible legal records. Installation,
pairing and running simulator tests are not authorization to print. Obtain the
owner's approval for each exact unit, scenario, amount and report date range.
Disconnect production printers during automated verification.

## Scope and evidence record

Create a separate signed record per physical unit, model, firmware, transport
and OS tested. Do not turn an untested entry into a pass. Record `PASS`, `FAIL`
or `UNPROVEN`, the expected and observed values, timestamp, operator/reviewer,
and evidence attachment IDs for **every numbered step below**. An absent
measurement remains `UNPROVEN`; never substitute a simulator trace.

Record the Workshop commit/build hash, Rust toolchain, acceptance cfg present
or absent, OS/version/architecture, driver version, printer label/model,
firmware, masked serial, demo/fiscal status, paper width/font, physical socket,
port path, USB descriptors and menu configuration. Keep full identity and raw
wire captures in an access-controlled evidence store, not source control,
Sentry or chat. Use approved synthetic company/NIP data on demo units.
Record fiscalized test documents in Holuto and retain physical printouts.

Supported candidates: Posnet Temo Online; Novitus HD II Online, Deon Online,
Bono Online and Point; Elzab Zeta Online, Mera Online and Cube Online.
Exclude legacy Zeta/D10, T10 (network only), other Posnet models, NoviAPI/XML
and RNDIS/LAN. A USB hint or marketing name is not qualification.

The evidence register must link each open item to the module docs and
capability gates it qualifies (`crates/novitus/src/profile.rs`,
`crates/elzab/src/profile.rs` and USB hints in `crates/fiscal/src/link.rs`).

## Build and automated checks (no hardware)

Ordinary `--all-features` must continue to prove production behavior. Acceptance
is **not a Cargo feature**. Its only switch is `cfg(workshop_acceptance)` via
`RUSTFLAGS`; the workspace declares this cfg for `unexpected_cfgs` checking.

```sh
cargo nextest run --workspace --all-features
RUSTFLAGS='--cfg workshop_acceptance' cargo build -p workshop
RUSTFLAGS='--cfg workshop_acceptance' cargo clippy \
  --workspace --all-targets --all-features -- -D warnings
RUSTFLAGS='--cfg workshop_acceptance' cargo nextest run --workspace --all-features
# REQUIRED negative check: must fail with the explicit compile_error below.
RUSTFLAGS='--cfg workshop_acceptance' cargo build --release
```

Expected release diagnostic:
`workshop_acceptance is restricted to local debug builds.` The guard in
`fiscal` applies even to standalone drivers; the desktop also declares it.
A dependency/toolchain error is not evidence that the release guard worked.
`checks.yml` runs the whole workspace acceptance suite, Clippy and release guard
in a separate job, never against hardware. Only exact production-only cases
relaxed by the cfg are excluded; the ordinary workspace run still exercises them.

Production tests named `non_fiscal_device_is_not_ready_without_acceptance`
run for all three brands. Novitus and Elzab also run
`unqualified_advance_zero_total_and_footer_are_refused_before_any_fiscal_command`;
Elzab advances are already qualified, so its refusals cover zero total/footer.
Simulator `frames()` must contain no fiscal command after each refusal.
Separate cfg-gated `acceptance` modules exercise the actual driver, not only
command builders. Elzab's simulator still rejects zero close with 1008:
acceptance permits an attempt, not an invented successful outcome.

Acceptance enables the known profiles' unqualified flags: all three for
Novitus; zero-total/footer for Elzab. Posnet has no unqualified receipt flags.
It relaxes **only** non-fiscal/training readiness: Posnet `fs=0`, Novitus
`11$i=0`, `23#s` fiscalized flag and ENQ fiscal bit, Elzab ySR package 2
training bit 3. Read-only,
identity, PC1, dialect, code page, header, VAT, clock, open document, busy,
mechanism and power checks remain enforced. Journaling, no retry/reprint,
unknown blocking, worker ownership and human resolution remain unchanged.
It never treats Elzab daily-report acceptance as completion.

Build in a dedicated technician OS account and isolated Holuto test branch,
with autostart off and no queued production jobs. Acceptance without the
`simulator` feature uses that account's normal state directory; it does not
create a new acceptance state path. Never delete or edit fiscal evidence to
isolate a run. Do not ship this executable or set these flags in packaging.
Only the supervising person launches it with the approved device connected.

## Phase A: read-only qualification

Do not start processing or submit a receipt/report in this phase. A technician
uses passive OS tools and audited read-only vendor queries/captures, not an
unreviewed diagnostic tool's test-print/configuration functions. Do not change
protocol, VAT, encoding, clock or header through discovery/preflight. If setup
is wrong, stop; an authorized person configures it separately and restarts the
read-only phase. Workshop's normal Novitus session selects response mode 3
(`3#e`); that is not permission to change dialect or code pages.

1. **USB inventory on every tested OS.** Capture normal-mode descriptors:
   Linux `lsusb -v` plus `udevadm info`, Windows Device Manager hardware IDs
   and parent/interface/driver properties, macOS System Information USB and
   IORegistry serial ancestry. Record VID:PID, manufacturer, product, USB
   serial, class/subclass/protocol, interface numbers and resulting port names.
   Expected hints are Posnet `1424:*`, Novitus `1A28:2512` (A `MI_00`, B
   `MI_02`), and candidate Elzab `0483:5740`; none proves identity. Capture
   actual Temo and each ONLINE Elzab model, not legacy STM/FTDI examples.
   **Fills:** evidence for `fiscal::link` USB-hint/filter fixtures, not a
   capability flag. Any new ID needs reviewed model-attributed evidence.
2. **Transport lifecycle.** Record actual baud, 8N1, flow control, DTR/RTS,
   OS driver, close/reopen and unplug/replug port stability on USB and RS232.
   Expected: exclusive serial open, no unsolicited fiscal action or reset,
   stable by-id path where available. Elzab RS232 must be STX (documented
   factory baud 9600); USB baud/parity behavior must be measured, not assumed.
   **Fills:** `Connection.baud_rate`, transport/OS qualification evidence;
   there is no profile field for modem-line behavior.
3. **Identity.** Save literal original replies: Posnet `getrealid`; Novitus
   `30#v` and unique number from `23#s`; Elzab `XTR` fields B/D and `FNR` B.
   Expected: exact brand/model/firmware and a stable matching unit serial.
   Elzab numeric XTR IDs alone cannot distinguish legacy from ONLINE.
   **Fills:** `Device.{vendor,model,firmware,serial}`, Novitus
   `profile::lookup(model, firmware)` allowlist; Elzab `lookup(model)` exact
   names. Elzab currently has no firmware gate: record actual D versions and
   require a reviewed profile change before asserting firmware qualification.
4. **Novitus PC1 and native dialect.** On each physical USB A/B/RS232 socket,
   save `50$i` and option 5 readback (`0;1^c5/5/`). Expected selected socket:
   `50$IPC1` followed by CR, dialect `1` (NOVITUS). Map A/B to USB1/USB2 and
   logical PC ports separately; OS enumeration order is not a mapping. A PC2
   socket must remain unready and receive no fiscal commands. Record observed
   refusals without deliberately fiscalizing PC2. **Fills:** PC1/dialect
   preflight evidence; no capability flag may bypass these gates.
5. **Fiscal/read-only truth table.** Save Posnet `scomm`/`sdev`/`strns` and
   mechanism state; Novitus `11$i`, raw DLE/ENQ/DLE2; Elzab all three lowercase
   `ySR` packages and read-only `LPC` receipt permission (inner B zero/empty).
   Expected production: fiscal, writable, header initialized, no open document,
   no menu/buffer/printing/error and healthy mechanism. Novitus mode 0 is demo,
   1 fiscal, 2 read-only; Elzab training is package 2 bit 3 and writable bit 2
   must remain set even in acceptance. Record all required/forbidden bits,
   missing-memory/display/service/power states and report-specific masks.
   Do not confuse uppercase `YSR` with `ySR`. **Fills:** driver's readiness
   truth table, not new profile booleans. Record absent all-interface queue or
   ownership proof explicitly; dedicate the unit, never cancel/adopt a document.
6. **Header and VAT.** Capture complete decoded header, taxpayer identity and
   A–G VAT readbacks: Posnet `vatget`, Novitus `23#s`, Elzab `FTR`.
   Expected Novitus exempt `98.99`/`100`, inactive `99.99`/`101`; Elzab exempt
   `=`, inactive `?`. Compare to the unit's programmed table/vendor evidence;
   contradictory administrative prose is not a mapping. Unknown sentinels
   block; never map them to a rate. Confirm company-name containment matches
   full decoded text. **Fills:** `Device.vat`, header evidence and local
   company-footer prerequisite. Do not program VAT to manufacture a test.
7. **Code page and repertoire.** Read Novitus options 91/92: both `1`;
   `52$i` attr1 bit `0x0200` enables full CP1250, otherwise ASCII only.
   Elzab `XHR` w must be `ACP1250`; Posnet uses Windows-1250. Record any
   input/output asymmetry, unsupported glyphs, undefined bytes and font effects.
   Expected: no translation, substitution or truncation. **Fills:**
   `Capabilities.allowed_characters` and Novitus `enable_full_cp1250` evidence;
   selected CP1250 alone does not establish every printable glyph.
8. **Printer clock.** Capture `rtcget` / `#c` / `XCR` and compare with the
   operator-observed local date/time in Europe/Warsaw. Record precision, YY
   epoch, supported years and DST ambiguity behavior/vendor statement. Expected:
   no PC-clock substitution. Novitus/Elzab reject repeated or skipped Warsaw
   local times; Posnet prefers offset-bearing `tm`, but legacy offset-free
   `da` chooses the earlier instant in the repeated autumn hour. Novitus/Elzab
   currently accept 2000–2049; do not widen from a guess. **Fills:**
   `Printed.issued_at` and clock-parser qualification, no profile clock field.
9. **Frames and response ordering.** Capture Novitus mode-3 switch ACK,
   data+ACK exceptions, checksum, multi-digit error and raw DLE2 byte order
   under known states; zero flags alone cannot establish byte order. For Elzab,
   capture a complete observed `XTR` request/reply with fresh matching command
   IDs. Offline references: CRC-16/ARC input `31 1C 58 54 52` gives `BA44`;
   `31 1C 4C 4F 52` gives `4E0E`. The latter is a fiscal `LOR` vector:
   **calculate offline, never send it in this phase**. Expected full-frame CRC
   and ID correlation agree; calculated vectors are not hardware traces.
   Record response timing/chunking, silence/SL behavior and whether concurrent
   status is vendor-proven safe; never test retransmission of fiscal commands.
   **Fills:** frame/session evidence and timing policy, no capability field.

## Phase B: destructive scenarios on non-fiscal demo units only

Obtain explicit approval for this phase. Verify demo/training status from Phase
A; no fiscalized unit may receive fault injection, power loss or cable pulls.
Use the debug acceptance build without `simulator`. Arrange required header,
VAT and identity through authorized setup beforehand; do not weaken another
readiness gate to make a demo unit usable. If it cannot meet those gates, mark
it blocked and obtain vendor evidence instead. Start from an empty approved
Holuto test queue. Confirm each scenario's full input and expected output.

1. **Baseline receipts.** One receipt per payment form (cash/card/transfer/
   mobile/credit), mixed VAT including exempt, buyer NIP before first item,
   Polish names and exact rounded line totals. Expected payment codes:
   Posnet `0/2/8/6/5`, Novitus `0/1/8/9/5`, Elzab X `1/2/7/9/6`.
   Capture full wire sequence, ACK/completion time, printed amounts/VAT/NIP,
   receipt number and cut. **Fills:** receipt/payment/NIP command evidence,
   not a new capability. Specifically prove Elzab `LNP` before first `LIT`.
2. **Limits and text fidelity.** Print names at the candidate max and glyph
   repertoire, distinguish case/spacing/percent/Polish characters, quantities
   at max precision, boundary multiplication/rounding and approved magnitude
   limits. Expected no normalization on paper, exact gross/VAT/payment sums;
   over-limit jobs are refused before the first fiscal command. Record line
   counts and adjustment-line consumption; do not infer receipt capacity from
   PLU capacity. **Fills:** `Capabilities.{max_item_name,max_lines,
   max_company_name,max_buyer_nip,max_quantity_decimals,allowed_characters}`.
   Candidate Novitus max lines 255, names 40/60; Elzab 255 (host ceiling),
   names 64 and quantities 5 decimals. No evidence means no increased limit.
3. **Partial advances.** Single item, multiple same-VAT items, mixed VAT,
   duplicate/canonicalized names and more than one deduction. Expected exact
   per-slot settlement, remaining payment and taxable basis, no discount,
   storno or negative item. Novitus uses first-match item-bound `@z`; prove
   each allocated deduction attaches to the intended line, including repeated
   names. Elzab uses per-slot `LDS` with D settlement marker. **Fills:**
   `Capabilities.advance_deductions` for the tested model/firmware. A single
   Novitus 1000−100=900 example is insufficient for general qualification.
4. **Fully prepaid close.** Deduct every slot completely and send zero total,
   with no nominal 0.01 sale/payment. Expected fiscal zero total and correct
   settlements if supported. Capture actual close bytes and any refusal;
   Elzab error 1008 remains a failing capability, not proof of no effects.
   **Fills:** `Capabilities.zero_total_receipts`; it stays false without a
   successful legal print and vendor reconciliation of zero-close semantics.
5. **Company text and completion.** Compare company present in header (no
   footer) with absent (exact full company footer). Novitus expected order:
   `$b` payments, `$y` ending 3, `12$z`, `200;0$w` lines, `28;0$z` (capture
   actual sequence); Elzab implemented candidate is `LCL` then `LAL`.
   Expected attached label-free text, correct cut and final completion, not
   a detached non-fiscal page. Test different daily company names and widths.
   **Fills:** `Capabilities.company_footer`, Novitus `Profile.info_line_width`
   (HD II 40, Bono/Deon 42, Point 44), Elzab `max_company_name` 42 and
   `Profile.open` (`LOR GS C0` ONLINE variant). No timing/placement proof means
   footer remains false; do not invent an end-footer command.
6. **Reports.** Daily, completed calendar month, periodic detail and summary;
   record date boundaries, future/current-month refusal and user confirmation.
   Expected Novitus date-guarded `#r`, monthly `#o` kind 6, periodic 0/1;
   Elzab monthly/periodic `RPP` with `S0`. Observe Elzab daily `RRP` ACK,
   idle state, `RNR` increment and `RDR` record, then compare fiscal memory
   with complete paper. ACK proves only acceptance; Workshop must remain
   unknown/blocking even if all readbacks look successful. **Fills:** report
   date/classification/completion evidence; no existing profile field enables
   automatic daily completion. Such a policy change needs separate review.
7. **Fault matrix.** At begin, item, advance, payment, close, footer and each
   report: paper-out, cover/head lift, operator/menu wait, reply loss/corruption,
   cable pull and power loss at approved points. Capture bytes already sent,
   physical effects, persisted intent/result, operator notice and reconnect
   state. Expected: each fiscal command sent once; no hidden transport retry,
   cancel, resume or reprint. Novitus waits observe DLE/ENQ/DLE2; record byte
   order by known physical state. Elzab paper-out may auto-cancel, but still
   leaves uncertainty. **Fills:** fault/wait/readiness masks and narrow certain
   refusal evidence; never a retry permission or new capability.
8. **Restart and resolution.** For each uncertain scenario restart with the
   same journal and show all receipts/reports blocked, with no scan or device
   switch. Inspect read-only last-document/counter evidence before and after
   reboot/day change; record number scope, persistence and attribution limits.
   Resolve through the documented human path on the same vendor and serial;
   expected zero fiscal commands and no reprint. **Fills:** journal/recovery
   and number-readback evidence, not automatic resolution. Novitus system IDs,
   protected-memory/JPK metadata and Elzab JWS/`SER`/`RNR` counters are not
   idempotency tokens. Any vendor evidence for protected-memory search must
   distinguish read-only Elzab `dPC` with `d` from printing without `d`.

Do not exercise alternate `$L`/`$Y` discounts as advance substitutes, Novitus
cancel/resume, Elzab `LCL RB`/`LCP`/`OPC`, `RRP` kind 14 or protected-memory
reprints. Missing evidence for deduplication, error persistence (Elzab last
error must be read immediately), concurrent `ySR` or counter atomicity stays
explicitly open and does not relax Workshop's stronger safety policy.

## Qualification decision and production cutover

A reviewer compares paper, exact bytes and persisted outcomes for **every**
applicable row, and signs the tested model/firmware/OS/transport scope. Attach
vendor confirmations for unresolved semantics; list failed/unproven rows and
capabilities that must stay false. No automatic promotion from an evidence
file or one happy-path receipt is supported.

Change the owning `profile.rs` lookup only for the proven scope; add literal
identity/firmware entries, limits and per-profile qualified booleans with an
evidence reference. Novitus currently has an exact firmware allowlist; Elzab
has exact candidate names and reports actual firmware without a firmware gate.
Do not enable an entire family from one firmware sample; refine the profile
selection before doing so. USB evidence belongs in `fiscal::link` fixtures and
filter rules. Parser/status/report-policy evidence belongs in that driver's
module and regression tests, not invented profile fields. Preserve deliberate
unknown classifications until separately justified and reviewed.

## Phase C: one fiscalized unit per brand, unmodified release only

After approved profile changes and simulator suites, build/package **without**
`RUSTFLAGS`, acceptance or simulator/testing features. Record binary hash and
version. The owner and fiscal operator approve exact real Holuto records and
amounts first. No destructive scenarios in this phase.

1. Re-run Phase A on the fiscalized unit. Expected: all production readiness
   checks pass and session/heartbeat vendor, serial, VAT, limits and qualified
   flags match the profile. **Fills:** production profile/telemetry sign-off.
2. Print one authorized receipt per payment form, mixed VAT and buyer NIP;
   include partial advance only if enabled, and zero-total/footer only if
   specifically qualified and approved. Expected exact full text, amounts,
   VAT, NIP, number and issue time; record every document in Holuto.
   **Fills:** release receipt/capability sign-off for that brand.
3. Print one authorized daily report. Expected complete physical report and
   brand-specific outcome; Elzab still blocks as unknown until inspected and
   explicitly resolved by a person. Confirm resolution emits nothing.
   **Fills:** release report/reconciliation sign-off, not a stronger completion
   guarantee. Retain evidence and unresolved state; never erase the journal.

A release qualification record lists the exact pass scope, remaining gates,
operator/reviewer signatures and evidence locations. Untested models and
firmware remain candidates; document them as such, not as certified support.
