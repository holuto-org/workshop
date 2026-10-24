# Multi-vendor fiscal printers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Print Holuto receipts and reports on Novitus and Elzab online
printers, detected automatically, with Posnet's fiscal safety guarantees.

**Architecture:** A new vendor-neutral `fiscal` crate owns the receipt domain,
transport, port enumeration and a `Driver` trait. `posnet`, `novitus` and
`elzab` implement it with their own wire layers and simulators. The agent
detects the brand, reports vendor and capabilities to Holuto, and runs any
driver through the same journal/worker invariants. The backend learns
`vendor`, capabilities and `maxQuantityDecimals`, and refuses unprintable jobs.

**Tech Stack:** Rust 2024 (workspace lints in root `Cargo.toml`), `serialport`,
`serde`, `thiserror`, `jiff`, `tracing`, `insta`, `proptest`, nextest; backend
Rust + Postgres + sqlx + utoipa.

**Spec:**
`docs/superpowers/specs/2026-10-24-multi-vendor-fiscal-printers-design.md`
and its research folder
`docs/superpowers/specs/2026-10-24-multi-vendor-fiscal-printers/`. Protocol
facts come from `novitus-protocol.md`, `novitus-oss.md`, `elzab-protocol.md`,
`elzab-oss.md`, `usb-oss.md`, with their line citations. Vendor PDFs are
reproducible per each note's Sources section (local extracts in
`/tmp/fiscal-docs/txt/`).

## Global Constraints

- Never talk to a real printer or serial device; printer behavior is exercised
  only through simulators and tests (`.omp/RULES.md`).
- Fiscal invariants (README "Fiscal safety"): verify state before every
  operation; journal intent before every fiscal command; never retry a command
  or reprint; anything after the first fiscal send is `Uncertain` unless the
  vendor documents that nothing was issued; unknown outcomes block across
  restarts; resolution reissues nothing.
- Never use cancel/resume/reprint commands in recovery: Novitus `$e` action 0,
  `28;2$z`; Elzab `LCL RB`, `LCP`, `OPC`, `RRP` kind 14, `dPC` without `d`.
- Never change printer configuration (code page, protocol, header, VAT) in
  detection or preflight.
- No substitution: no discount for advances, no 0.01 for zero totals, no
  transliteration or truncation of names.
- Workspace conventions (AGENTS.md): `foo.rs` + `foo/tests.rs`, no `mod.rs`, no
  `pub use`, `#[expect(lint, reason = "…")]` never `allow`, `thiserror` enums
  with Polish `Display`, versions only in root `Cargo.toml`.
- Persisted shapes that must not change: `Report` serde
  (`{"kind":"daily"|"monthly"|"periodic",…}`), `PaymentForm` snake_case,
  `Grosze` as u64, history line shape.
- Vendors serialize as `"posnet"`, `"novitus"`, `"elzab"` everywhere.
- Initial qualified capabilities: Posnet all true; Novitus
  `advance_deductions=false`, `zero_total_receipts=false`,
  `company_footer=false`; Elzab `advance_deductions=true`,
  `zero_total_receipts=false`, `company_footer=false`.
- Supported identities: Posnet `POSNET TEMO ONLINE`; Novitus HD II Online, Deon
  Online, Bono Online, Point; Elzab Zeta Online, Mera Online, Cube Online. No
  T10, D10, legacy Zeta, Ergo.
- USB hints: `1424:*` Posnet; `1A28:2512` Novitus; `0483:5740` Elzab. Bridges
  kept unhinted: `0403`, `067B`, `1A86`, `10C4`.
- Verify per AGENTS.md: `cargo +nightly-2026-10-06 fmt`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo nextest run --workspace --all-features`. Backend per its AGENTS.md.

## Review Focus

- A Novitus `PC2` endpoint answers every query: it must never be adopted or
  printed on (Task 8 test
  `pc2_endpoint_is_not_ready_and_receives_no_fiscal_command`).
- A pre-change `operation.json` with an unresolved Posnet receipt must still
  block, now tagged `posnet` (Task 5 test
  `legacy_unresolved_journal_is_migrated_to_posnet_and_still_blocks`).
- An Elzab daily report acknowledged by `RRP` must end `unknown`, never `done`
  (Task 9 test `daily_report_acceptance_is_unknown_and_blocks`).
- A job needing an unqualified capability that is already claimed must fail
  before any fiscal command (Task 4 test
  `claimed_job_needing_unqualified_capability_fails_without_fiscal_commands`).
- Foreign-protocol probes on an unhinted port must not execute anything on the
  other brands' simulators (Task 10 test
  `unhinted_probe_order_is_inert_on_every_other_brand`).

---

### Task 1: Backend telemetry vendor, capabilities and quantity limit

Repository: `../backend` (read its `AGENTS.md` and every rule it names for the
files you touch; it has unrelated uncommitted work in `crates/legacy-import`
files `lib.rs`, `money.rs`, `reports.rs`, `walk_in*`: never stage those).

**Files (from `usb-backend.md` §B3; extend if the code says otherwise):**

- Create: `migrations/0012_fiscal_printer_vendor.sql` (next free number)
- Modify: `crates/db/src/enums.rs`, `crates/db/src/enums/tests.rs`,
  `sqlx.types.toml`, `sqlx.toml` (regenerated), `.sqlx/`
- Modify: `crates/fiscal/src/receipts/model.rs` (Telemetry, Printer),
  `crates/fiscal/src/receipts/repo/printers.rs`,
  `crates/fiscal/queries/receipts/printers.sql`,
  `crates/fiscal/src/receipts/printers.rs` (pinning),
  `crates/fiscal/src/receipts/queueing.rs` (refusals)
- Modify: `locales/{en,pl,es,ru}/fiscal.ftl` (new problem codes)
- Modify: `crates/legacy-import/src/archive.rs` (`printer_values`),
  `crates/legacy-import/src/project_archive.rs` (drop `_legacy_vendor`),
  `crates/legacy-import/src/archive/tests.rs`
- Tests: `crates/fiscal/src/receipts/printers/tests.rs`,
  `crates/fiscal/src/receipts/tests/{access,pairing,queue,storage}.rs`,
  fixture `tests::telemetry` in `crates/fiscal/src/receipts/tests.rs`
- Regenerate: `openapi/` via `cargo run -p holuto -- openapi`

**Interfaces:**

- Produces (wire, consumed by Task 4 through `openapi/agent.json`):
  - `Telemetry` gains required `vendor: FiscalVendor` (`"posnet" | "novitus" |
    "elzab"`), `maxQuantityDecimals: integer 0..=8`,
    `capabilities: { advanceDeductions: bool, zeroTotalReceipts: bool }`
    (both required, `additionalProperties: false`).
  - `Printer` gains `vendor` (nullable), `maxQuantityDecimals` (nullable),
    `capabilities` (nullable object, same shape).
  - `POST /agent/jobs` problem codes `fiscal_printer_advances_unsupported`
    (409) and `fiscal_printer_zero_total_unsupported` (409), and
    `fiscal_printer_quantity_precision` (409) for a line quantity with more
    fractional digits than `maxQuantityDecimals`.
  - A session/heartbeat whose `vendor` differs from the stored non-null
    vendor answers the same 409 as a serial mismatch.

- [ ] **Step 1: Failing tests.** In `printers/tests.rs` replace
  `session_and_heartbeat_replace_typed_slots_without_legacy_fields` with
  `session_requires_vendor_capabilities_and_quantity_precision` (missing or
  unknown `vendor` → 400 `validation_failed` pointer `/vendor`; missing
  `capabilities` → pointer `/capabilities`; `maxQuantityDecimals` 9 → 400) and
  `vendor_is_pinned_like_serial` (posnet session then novitus heartbeat → 409,
  stored vendor unchanged). In `tests/queue.rs` add
  `queue_refuses_advances_when_printer_cannot_deduct`,
  `queue_refuses_fully_prepaid_sale_when_printer_cannot_close_at_zero`,
  `queue_refuses_quantity_beyond_printer_precision`. In `tests/storage.rs` add
  `vendor_migration_keeps_existing_printers_null`. In `tests/access.rs` and
  `tests/pairing.rs` update exact response shapes (`vendor`, `capabilities`,
  `maxQuantityDecimals`, null before the first session).
- [ ] **Step 2: Run them and see them fail** (`docker compose up -d --wait
  postgres-test s3`; `cargo nextest run -p fiscal --all-features`).
- [ ] **Step 3: Implement.** Migration: `CREATE TYPE fiscal_vendor AS ENUM
  ('posnet','novitus','elzab')`; nullable `vendor`, `max_quantity_decimals`
  (`smallint` 0..8 check), `advance_deductions`, `zero_total_receipts`
  (nullable booleans) on `fiscal_printers`; existing rows stay null; follow
  the audit-trigger exclusions pattern from the latest fiscal migration.
  Update typed enum, sqlx mapping, repo writes, DTOs, pinning in
  `printers.rs`, queue checks next to the existing limit checks (a printer
  whose telemetry is unknown keeps today's `fiscal_printer_limits_unknown`),
  importer `printer_values` writes the validated legacy vendor (null stays
  null), and remove `_legacy_vendor`. Add Fluent messages in all four locales.
- [ ] **Step 4: Regenerate and verify.** `cargo run -p sqlx-config`,
  refresh `.sqlx/` with `COMPILE_DATABASE_URL` per `docs/workflows.md`,
  `cargo run -p holuto -- openapi`, then the backend Verify block
  (fmt, clippy excluding legacy-import, nextest
  `-E 'rdeps(fiscal) | rdeps(db)'`, plus `cargo nextest run -p legacy-import`).
  Whole-repo search for `_legacy_vendor` returns nothing.
- [ ] **Step 5: Commit** in `../backend` only the files of this task:
  `feat(fiscal): printer vendor, capabilities and quantity precision`.

### Task 2: Extract the vendor-neutral `fiscal` crate (pure move)

**Files:**

- Create: `crates/fiscal/Cargo.toml`, `crates/fiscal/src/lib.rs`,
  `crates/fiscal/src/{money,decimal,receipt,report,journal,link,error,vat}.rs`
  with their `tests.rs` moved from posnet
- Modify: root `Cargo.toml` (`fiscal = { path = "crates/fiscal" }`),
  `crates/posnet/src/{lib,printer,receipt,session,simulator,text}.rs`,
  every `posnet::` import in `crates/agent` and `crates/desktop`,
  `crates/agent/build.rs:72` (`FiscalPaymentForm` → `fiscal::receipt::PaymentForm`)

**Interfaces (Produces):**

- `fiscal::money::{Grosze, zloty}`, `fiscal::decimal::{parse, format}` (moved
  unchanged).
- `fiscal::receipt::{VatSlot, VAT_SLOTS, PaymentForm, Payment, Advance,
  Quantity, Line, Receipt}`: same types and validation as today. Posnet-only
  `Step`, `Receipt::steps`, `PaymentForm::wire_type`, `ADVANCE_NAME`,
  `MOBILE_NAME` move to `posnet::receipt` as free functions over
  `&fiscal::receipt::Receipt` (`pub fn steps(receipt: &Receipt, footer:
  Option<&str>) -> Vec<Step>`). Expose read accessors the drivers need:
  `Receipt::{lines, advances, payments, buyer_nip, brand, sales, fiscal_total}`
  and `Line::{name, quantity, vat, price, total}`; `Quantity::{units, times}`.
- `fiscal::report::Report` (serde shape unchanged) with `label()`.
- `fiscal::journal::Journal` trait (unchanged signature) and
  `fiscal::journal::Printed`.
- `fiscal::vat::VatRate { slot: u8, basis_points: u32 }`. Keep `active`,
  `exempt` and `label`; exempt/inactive classification becomes
  `VatRate { slot, kind: VatKind }` in Task 3. In this task keep the Posnet
  semantics verbatim.
- `fiscal::link::{Port, Link, Connection, SerialLink, serial_ports,
  DEFAULT_TIMEOUT}` moved from `posnet::session` (frame-level `Session` stays
  in posnet).
- `fiscal::error::Error`: today's `posnet::Error` moved, except the
  `Device { code, command, field, rejected }` variant becomes
  `Device { code: u32, command: String, detail: String, rejected: bool }`,
  where `detail` is the driver-made Polish explanation (posnet fills it from
  its `explanation`/`parameter`). The `Display` text of every variant stays
  byte-identical for Posnet errors.
- The Posnet simulator keeps its public API; its import paths change only.

- [ ] **Step 1:** Move code with `git mv` where whole files move; split
  `receipt.rs`. Keep tests with the code they test.
- [ ] **Step 2:** `cargo nextest run --workspace --all-features`; expected:
  303 passed, identical names except module paths. Insta snapshots unchanged.
- [ ] **Step 3:** clippy `-D warnings` and fmt clean; `cargo shear
  --deny-warnings` clean.
- [ ] **Step 4: Commit** `refactor: extract vendor-neutral fiscal crate`.

### Task 3: `Driver` trait, `Device`, `Capabilities`; Posnet implements it

**Files:**

- Create: `crates/fiscal/src/device.rs` (+ `device/tests.rs`),
  `crates/fiscal/src/driver.rs`
- Modify: `crates/fiscal/src/{vat,receipt}.rs`, `crates/posnet/src/printer.rs`,
  `crates/agent/src/{agent,worker,api/model}.rs`, `crates/desktop/src/model.rs`

**Interfaces (Produces):**

```rust
// fiscal::device
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vendor { Posnet, Novitus, Elzab }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VatKind { Rate { basis_points: u32 }, Exempt, Inactive }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VatRate { pub slot: u8, pub kind: VatKind } // replaces fiscal::vat

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub max_item_name: usize,
    pub max_lines: usize,
    pub max_company_name: usize,
    pub max_buyer_nip: usize,
    pub max_quantity_decimals: u8,
    pub allowed_characters: String, // NFC, printable, for telemetry
    pub advance_deductions: bool,
    pub zero_total_receipts: bool,
    pub company_footer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub vendor: Vendor,
    pub model: String,
    pub firmware: String,
    pub serial: String,
    pub problems: Vec<String>, // empty when ready
    pub vat: Vec<VatRate>,
    pub capabilities: Capabilities,
}
// Device::{ready, description} keep Status semantics and texts.

// fiscal::driver
pub trait Driver: Send + fmt::Debug {
    fn vendor(&self) -> Vendor;
    fn connection(&self) -> &Connection;
    fn probe(&self) -> Result<Device, Error>;
    fn verify(&self, serial: &str) -> Result<Device, Error>;
    fn print_receipt(&self, receipt: &Receipt, expected: &Device,
                     journal: &mut dyn Journal) -> Result<Printed, Error>;
    fn print_report(&self, report: Report, serial: &str,
                    journal: &mut dyn Journal) -> Result<(), Error>;
}
```

- `Line::new(name, quantity, unit_price, vat, &Capabilities)` and
  `Receipt::new(lines, advances, payments, buyer_nip, brand, &Capabilities)`
  validate names, NIP, line count, quantity decimals against the capabilities
  and a `printable: &dyn Fn(&str) -> bool` supplied by the driver through
  `Capabilities::allowed_characters` (a character set membership check, NFC).
  Polish messages keep today's text with the numbers coming from capabilities
  (e.g. "Nazwa usługi może mieć najwyżej {max_item_name} znaków.").
- `posnet::printer::Printer` implements `Driver` (`vendor() == Posnet`) and
  keeps `detect`. Its `Device.capabilities` uses the existing Posnet
  constants (80, 500, 40, 30, 8 decimals, `text::printable_characters()`, all
  three booleans true). `posnet::printer::Status` is deleted.
- `Agent` owns `Box<dyn Driver>`; `Event::Device { status: Option<Device> }`
  in `worker.rs`; desktop `model.rs` reads `Device`.
- `printer_slots(&[VatRate])` maps `VatKind`; `Telemetry` limits come from
  `Device.capabilities` in `describe` (not `Telemetry::new`).

- [ ] **Step 1: Failing tests.** `fiscal/src/receipt/tests.rs`:
  `line_name_limit_follows_capabilities` (a 64-char limit rejects 65 chars,
  accepts 64), `quantity_decimals_follow_capabilities` (5 decimals accepted,
  6 rejected with limit 5). `fiscal/src/device/tests.rs`:
  `vendor_serializes_snake_case` (`"posnet"`, `"novitus"`, `"elzab"`).
- [ ] **Step 2:** run, see failures.
- [ ] **Step 3:** implement as above; migrate all agent/desktop/test call
  sites (tests may build `Device` through a `posnet` helper or the simulator,
  never by hand-copying constants).
- [ ] **Step 4:** full workspace nextest passes (Posnet behavior unchanged);
  clippy and fmt clean.
- [ ] **Step 5: Commit** `feat(fiscal): driver trait with device
  capabilities; posnet implements it`.

### Task 4: Contract sync: vendor, capabilities, quantity precision

**Files:**

- Modify: `openapi/agent.json` (via `BACKEND_DIR=../backend bash
  scripts/openapi-sync.sh`), `crates/agent/build.rs`,
  `crates/agent/src/api/model.rs`, `crates/agent/src/agent.rs`,
  `crates/agent/src/testing.rs` (fake printer JSON, queue refusals),
  `crates/agent/src/api/contract.rs` if schema helpers need it, README "Fiscal
  API contract"
- Tests: `crates/agent/src/agent/tests.rs`, `crates/agent/src/api/tests.rs`,
  `crates/agent/src/testing/tests.rs`

**Interfaces:**

- Consumes: Task 1 wire shapes; Task 3 `Device`, `Capabilities`, `Vendor`.
- Produces: `Agent::describe(&mut self, device: Device)` fills
  `telemetry.{vendor, model, firmware, vat_slots, max_item_name_length,
  max_lines_per_receipt, max_company_name_length, allowed_characters,
  max_quantity_decimals, capabilities}` from the device. Build-time mapping
  `FiscalVendor` → `fiscal::device::Vendor`.
- `Agent::receipt()` refuses before any fiscal command, as `failed`:
  advances when `!advance_deductions` ("Drukarka nie obsługuje rozliczenia
  zaliczek."); fiscal total zero when `!zero_total_receipts` ("Drukarka nie
  obsługuje paragonu opłaconego w całości zaliczkami."); quantity precision
  via `Line::new`.

- [ ] **Step 1: Failing tests.**
  `claimed_job_needing_unqualified_capability_fails_without_fiscal_commands`
  (simulator device with `advance_deductions=false` via a simulator setter;
  job with an advance → result `failed` with the exact message,
  `sim.frames()` contains no fiscal command, journal settled failed);
  `telemetry_reports_device_vendor_capabilities_and_precision` (the fake
  receives `vendor:"posnet"`, `capabilities`, `maxQuantityDecimals:8`). The
  fake validates requests against the synced schema.
- [ ] **Step 2:** run, see failures.
- [ ] **Step 3:** sync openapi, update build mapping, model, agent, fake. Add a
  Posnet simulator setter `set_capabilities(..)` only if the test needs the
  device to lack a capability (simulators may override the qualified profile
  for tests; production cannot).
- [ ] **Step 4:** `cargo nextest run --workspace --all-features
  -E 'rdeps(agent)'`, clippy, fmt.
- [ ] **Step 5: Commit** `feat(agent): report vendor and capabilities; refuse
  unprintable jobs`.

### Task 5: Persisted formats carry the vendor

**Files:**

- Modify: `crates/agent/src/journal.rs` (+ fixtures under
  `journal/fixtures/`), `crates/agent/src/settings.rs` (+ fixture),
  `crates/agent/src/status.rs`, `crates/agent/src/agent.rs`,
  `crates/desktop/src/cli.rs` (`config show`, `status` JSON),
  `docs/install-with-ai.md` (CLI JSON fields), README state-dir section
- Tests: `journal/tests.rs`, `settings/tests.rs`, `status/tests.rs`,
  `cli/tests.rs`, `agent/tests.rs`

**Interfaces (Produces):**

- `ReceiptOperation.vendor: Vendor`, `ReportOperation.vendor: Vendor`
  (serde field `vendor`).
- `Journal::load()` reads a pre-change file (no `vendor`), rewrites it once
  atomically with `vendor: "posnet"` and returns the migrated operation. Any
  other shape stays a corrupt-journal block.
- Resolution (`settle`, `resolve_report`, `acknowledge_orphaned_receipt`)
  requires `operation.vendor == driver.vendor()` and the same serial;
  otherwise the existing "podłącz drukarkę…" blocked messages name both.
- `Settings.vendor: Option<Vendor>` (`"vendor": null` = detect), validated;
  `Settings::default()` has `None`. A pre-change settings file (no `vendor`
  key) must still load, as "detect" (`#[serde(default)]` on this one field):
  missing really means unknown, and settings are read without the instance
  lock (telemetry preference, crash reporter), so a write-on-read migration
  is not allowed there. `deny_unknown_fields` stays.
- `status::Snapshot.vendor: Option<Vendor>`; `workshop status --json` and
  `config show --json` include `vendor`.

- [ ] **Step 1: Failing tests.**
  `legacy_unresolved_journal_is_migrated_to_posnet_and_still_blocks` (literal
  pre-change fixture `receipt-dispatching.json` → load returns vendor posnet,
  file on disk now contains `"vendor":"posnet"`, `blocks()` true);
  `resolution_requires_the_same_vendor_and_serial` (Novitus-tagged journal
  with a Posnet device of the same serial → blocked, journal unchanged);
  `legacy_settings_load_with_detection_vendor`;
  `config_show_and_status_report_vendor`.
- [ ] **Step 2:** run, see failures.
- [ ] **Step 3:** implement; update current-format fixtures and keep the
  pre-change fixtures as literal old-format files.
- [ ] **Step 4:** `-E 'rdeps(agent)'` and `package(workshop)` nextest,
  clippy, fmt.
- [ ] **Step 5: Commit** `feat(agent): persist printer vendor in journal,
  settings and status`.

### Task 6: Port enumeration keeps only plausible printer ports

**Files:**

- Modify: `crates/fiscal/src/link.rs` (+ `link/tests.rs`), root `Cargo.toml`
  (`serialport` feature `usbportinfo-interface` if needed),
  `crates/desktop/src/{cli,application}.rs`, README "Printer"
- Linux sysfs check: `crates/fiscal/src/link/linux.rs` (`#[cfg(target_os =
  "linux")]`)

**Interfaces (Produces):**

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate { pub port: String, pub hint: Option<Vendor> }
pub fn serial_ports() -> Vec<Candidate>;              // filtered, ordered
fn filter(ports: Vec<SerialPortInfo>, os: Os,
          uart_type: &dyn Fn(&str) -> Option<u32>) -> Vec<Candidate>;
```

- `Link::candidates(&self) -> Vec<Candidate>` (was `Vec<String>`).
- Order: hinted USB ports (Posnet, Novitus, Elzab), then bridges, then native
  ports; stable by name within a group.
- Rules exactly as the design's port table; Linux native UART kept only if
  `/sys/class/tty/<name>/type` parses to a value `> 0`; unreadable → kept
  (manual candidate), parsed `0` → dropped.

- [ ] **Step 1: Failing tests** in `link/tests.rs` using
  `SerialPortInfo` fixtures per OS:
  `keeps_printer_usb_ids_with_brand_hints`,
  `keeps_rs232_bridges_and_native_uarts_without_hint`,
  `drops_bluetooth_other_usb_and_phantom_linux_uarts`,
  `macos_keeps_callout_drops_dialin_duplicates`,
  `orders_hinted_then_bridges_then_native`.
- [ ] **Step 2:** run, see failures.
- [ ] **Step 3:** implement; CLI `ports` prints names (JSON adds `hint`),
  GUI lists names.
- [ ] **Step 4:** nextest `-E 'rdeps(fiscal)'`, clippy, fmt.
- [ ] **Step 5: Commit** `feat(fiscal): list only plausible printer ports
  with brand hints`.

### Task 7: Novitus wire layer, session and simulator transport

**Files:**

- Create: `crates/novitus/Cargo.toml` (feature `simulator`),
  `crates/novitus/src/{lib,frame,text,session,simulator}.rs` and their
  `tests.rs`; root `Cargo.toml` workspace dependency;
  `.omp/rules/novitus-wire-encoding.md` (same shape as the Posnet rule, scope
  `crates/novitus/src/**`)

**Interfaces (Produces):**

- `frame::encode(body: &[u8]) -> Vec<u8>`: `ESC P` + body + two uppercase hex
  digits of the XOR control byte + `ESC \`; `frame::decode` returns the body
  or `Error::Protocol`. Control byte algorithm and the worked example are in
  `novitus-protocol.md` §1.1 (`9B` example; `210#s` → `9C` per
  `novitus-oss.md` §5).
- `text::{encode, decode, printable_characters}` for Windows-1250, rejecting
  bytes the protocol forbids in fields (CR inside text, control bytes).
- `session::Session::{open(link, connection), command(&mut self, body: &str,
  timeout) -> Result<Reply, Error>, realtime(&mut self, byte: u8) ->
  Result<u8, Error>}`. Error mode 3 (`3#e`) set once per session; never
  retries; DLE (0x10), ENQ (0x05) and DLE2 parsing per §1.4.
- Simulator: `novitus::simulator::Simulator::{start, address, link,
  link_with(candidates)}` over the shared `fiscal` link conventions, answering
  `30#v` (identity), `50$i` (endpoint name, default `PC1`, setter
  `set_endpoint`), `^c` option 5 (dialect, default `1`), `#c` clock,
  `23#s`, `^u` header, DLE/ENQ/DLE2, and recording frames (`frames()`).

- [ ] **Step 1: Failing tests:** `checksum_matches_vendor_example`,
  `frame_round_trip_rejects_bad_checksum`,
  `encodes_polish_text_as_windows_1250`,
  `session_never_retries_a_silent_command`,
  `realtime_status_bits_decode_paper_and_cover`.
- [ ] **Step 2–4:** implement until green; insta snapshot of the identity
  query frame bytes.
- [ ] **Step 5: Commit** `feat(novitus): native protocol frames, session and
  simulator transport`.

### Task 8: Novitus driver: identity, preflight, receipts, reports

**Files:**

- Create: `crates/novitus/src/{printer,receipt,profile}.rs` and tests;
  extend `simulator.rs` with fiscal commands and faults.

**Interfaces:**

- Consumes: Tasks 3 and 7.
- Produces: `novitus::printer::Printer::new(link, connection)` implementing
  `fiscal::driver::Driver` (`vendor() == Novitus`) and
  `novitus::printer::identify(link, connection) -> Result<Option<Device>,
  Error>` (read-only: `30#v`, then `50$i`, then `^c` option 5; returns
  `None` for other brands or non-PC1 endpoints).
- `profile::lookup(model: &str, firmware: &str) -> Option<Profile>`; profiles
  for HD II Online, Deon Online, Bono Online, Point with limits from
  `novitus-protocol.md` §6 and §4.3 (item name ≤ 60, info line widths per
  model) and qualified flags from Global Constraints.
- Preflight in the printing session: identity, `PC1`, dialect, fiscal mode,
  DLE2 idle/no open document, header (`^u`), VAT (`23#s`; `98.99`/`100`
  exempt, `99.99`/`101` inactive), clock `#c` (Europe/Warsaw), code page
  CP1250 for PC input/output (options 91/92); anything else → not ready.
- Receipt: `0$h`, `100;1$z` NIP, `$l` per line, `@z` allocation (only when
  `advance_deductions` is enabled), `$b` per nonzero payment (cash 0, card 1,
  transfer 8, mobile 9, credit 5), `$y` ending 3, `12$z`, footer
  `200;0$w` lines when needed and enabled, `28;0$z`; receipt number via
  `0;0?l` after success, best-effort. Certain refusal only for `$h`.
- Reports: daily `1;YY;MM;DD#r` with printer date; monthly `#o` kind 6 over
  the calendar month; periodic kind 0/1. Certain refusal only daily error 37.
- Operator waits via DLE2 during execution, capped at 10 minutes, through
  `Journal::waiting`, never resending.

- [ ] **Step 1: Failing tests** (`printer/tests.rs`, simulator-based):
  `identify_accepts_supported_models_only`,
  `pc2_endpoint_is_not_ready_and_receives_no_fiscal_command`,
  `non_native_dialect_is_not_ready`,
  `vat_sentinels_map_exempt_and_inactive`,
  `receipt_frames_match_snapshot` (insta, mixed VAT, NIP, five payment
  forms), `lost_reply_after_begin_is_uncertain`,
  `begin_refusal_is_certain_failure`,
  `unqualified_advance_is_refused_before_begin`,
  `daily_report_keyboard_cancel_is_certain_others_uncertain`,
  `operator_wait_resumes_without_resending`.
- [ ] **Step 2–4:** implement until green; review every snapshot against the
  spec citations.
- [ ] **Step 5: Commit** `feat(novitus): fiscal driver with preflight,
  receipts and reports`.

### Task 9: Elzab ElzabSTX wire layer, driver and simulator

**Files:**

- Create: `crates/elzab/Cargo.toml` (feature `simulator`),
  `crates/elzab/src/{lib,frame,text,session,printer,receipt,profile,simulator}.rs`
  and tests; root `Cargo.toml`; `.omp/rules/elzab-wire-encoding.md`

**Interfaces:**

- Consumes: Tasks 3 and 6.
- Produces: `elzab::printer::{Printer, identify}` mirroring Task 8's shapes
  (`vendor() == Elzab`).
- Frame: `STX header FS data CRC ETX`, header
  `command_id GS receiver_status GS frame_no GS max_frame_no GS transform`
  (trailing empty fields omitted), CRC-16/ARC over header..data as four
  uppercase hex digits (`elzab-protocol.md` §1.1–1.2; vector `LOR` → `4E0E`).
  Fresh command ID per request; replies matched by ID; transport statuses
  `SA`…`SN` decoded; never retransmit.
- Identity `XTR` (+ `FNR` serial); profiles for Zeta Online, Mera Online,
  Cube Online (64-char names, 5 quantity decimals); code page `XHR GS 'w'`
  must be `CP1250`.
- Preflight: `ySR` packages, header `YYR`/`yHR`, VAT `FTR` (`=` exempt, `?`
  inactive), clock `XCR`.
- Receipt: `LOR` (online variant per profile), `LNP` after `LOR`, `LIT` per
  line, `LDS` per advance slot, `LBP` with `X` kinds cash 1, card 2,
  transfer 7, mobile 9, credit 6, `LCL GS 'RA'<total>`; `LAL` only when
  `company_footer` is enabled; receipt number `SER` best-effort. Certain
  refusal only for a transport-status rejection of the first `LOR` frame.
- Reports: daily `RRP GS 'I1' GS 'J1'` → always `Uncertain` after
  acknowledgement (design); monthly `RPP GS 'E1' GS 'FB'<YYMM> GS 'S0'`;
  periodic `E1`/`E3` with `FE…RS F…`, `S0`.

- [ ] **Step 1: Failing tests:** `crc_matches_reference_vectors`,
  `frame_header_and_separators_round_trip`,
  `transport_status_rejection_of_first_frame_is_certain`,
  `reply_with_foreign_command_id_is_ignored`,
  `identify_accepts_online_models_only`,
  `non_cp1250_code_page_is_not_ready`,
  `receipt_frames_match_snapshot` (insta),
  `paper_out_mid_receipt_is_uncertain`,
  `daily_report_acceptance_is_unknown_and_blocks`,
  `fully_prepaid_receipt_is_refused_before_open`.
- [ ] **Step 2–4:** implement until green.
- [ ] **Step 5: Commit** `feat(elzab): ElzabSTX driver and simulator`.

### Task 10: Multi-vendor detection and worker wiring

**Files:**

- Create: `crates/agent/src/device.rs` (+ `device/tests.rs`)
- Modify: `crates/agent/{Cargo.toml,src/worker.rs}` (features: `testing`
  enables all three simulators), `crates/desktop/{Cargo.toml,src/main.rs}`
  (`simulator` feature starts all three simulators;
  `WORKSHOP_SIMULATOR=posnet|novitus|elzab` picks which one sits on the
  configured port, default posnet), `crates/desktop/src/application/preferences.rs`
  (persist detected vendor with the port)

**Interfaces:**

- Consumes: Tasks 5, 6, 8, 9.
- Produces: `agent::device::detect(link: Arc<dyn Link>, connection:
  Connection, remembered: Option<Vendor>, may_switch: bool) ->
  Result<(Box<dyn Driver>, Device), fiscal::error::Error>` implementing the
  design's order (remembered vendor; hint; Posnet → Elzab → Novitus).
  `Event::PortDetected { port: String, vendor: Vendor }`.

- [ ] **Step 1: Failing tests:** `remembered_vendor_probes_only_that_brand`,
  `hinted_port_probes_only_the_hinted_brand`,
  `unhinted_probe_order_is_inert_on_every_other_brand` (each simulator's
  `frames()` shows only rejected/ignored foreign probes and no state change),
  `no_scan_while_an_operation_blocks`,
  and the worker scenarios
  `each_vendor_prints_a_receipt_end_to_end` and
  `each_vendor_lost_reply_blocks_across_restart_without_replay`
  parameterized over the three simulators.
- [ ] **Step 2–4:** implement until green; full workspace nextest.
- [ ] **Step 5: Commit** `feat(agent): detect printer brand and drive any
  supported printer`.

### Task 11: Acceptance build, hardware checklist and documentation

**Files:**

- Modify: `crates/{novitus,elzab,posnet,desktop}/Cargo.toml` (`acceptance`
  feature), drivers' profile qualification lookup, `crates/desktop/src/main.rs`
  (`compile_error!` when `acceptance` without debug assertions, matching
  `simulator`)
- Create: `docs/testing/hardware-acceptance.md`
- Modify: README (title line, Printer, Fiscal safety per brand, contract),
  AGENTS.md (crates list, nextest filters `rdeps(fiscal)`), `llms.txt`,
  `docs/install-with-ai.md`, `docs/testing/{checklist,compatibility,platforms}.md`,
  `docs/workflows.md`, `.cargo/mutants.toml` (exclude all simulators),
  `crates/desktop/build.rs` FileDescription `Holuto Workshop · drukarki
  fiskalne`, `packaging/verify-windows.ps1`, `packaging/linux/control.in`

**Interfaces:** `acceptance` enables unqualified capabilities and relaxes only
the fiscal-mode readiness check; it never compiles into release builds.

- [ ] **Step 1:** a release build with acceptance mode cannot be unit
  tested; add a `compile_error!` for `all(workshop_acceptance,
  not(debug_assertions))` and verify that
  `RUSTFLAGS='--cfg workshop_acceptance' cargo build --release` fails with
  that error, while the same flag on a debug build succeeds.
- [ ] **Step 1b: keep `--all-features` from hiding production gates.**
  `acceptance` must not be reachable from `--all-features`: name it so the
  standard suite never enables it (gate it behind a dedicated
  `cfg(workshop_acceptance)` set by `RUSTFLAGS` in the checklist's build
  command, not a Cargo feature), so `cargo nextest run --workspace
  --all-features` keeps proving production behavior. Production-gate tests
  (run in that standard suite):
  `non_fiscal_device_is_not_ready_without_acceptance`,
  `unqualified_advance_zero_total_and_footer_are_refused_before_any_fiscal_command`
  (one case per brand, asserting simulator `frames()` has no fiscal command).
  Acceptance-path tests live in a separate module compiled only under
  `cfg(workshop_acceptance)`, run by a separate documented command in
  `docs/testing/hardware-acceptance.md` and in CI as its own job.
- [ ] **Step 2:** write the checklist: read-only phase (USB descriptors via
  OS tools, identity, `50$i`, dialect, VAT sentinels, code page, clock,
  status bits, Elzab CRC), destructive phase on demo units, final pass on
  one fiscalized unit per brand; each step names the expected value and the
  profile field it fills.
- [ ] **Step 3:** docs updates; markdownlint clean
  (`bunx --bun markdownlint-cli2 "**/*.md"`).
- [ ] **Step 4:** full Verify block, `cargo deny --locked check`,
  `cargo shear --deny-warnings`.
- [ ] **Step 5: Commit** `docs: multi-vendor printers, acceptance checklist`.
