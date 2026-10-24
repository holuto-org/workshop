# Posnet coupling map (read-only investigation)

This is a snapshot of the code as of 2026-10-24 (commit `673122d`).

Workspace: `Cargo.toml` members `crates/*`; `posnet = { path = "crates/posnet"
}` (workspace dep). Crates: `posnet` (driver + simulator), `agent` (lib;
journal, worker, API), `workshop` = `crates/desktop` (Slint UI + CLI, binary).
Line numbers below are from grep/read output; "~" = approximate.

---

## 1. Public API surface of `posnet` used outside it

Module list: `crates/posnet/src/lib.rs:8-18` — `decimal, frame, money, printer,
receipt, session, [simulator cfg(test|feature="simulator")], text`. `frame` is
`pub mod` but NOT imported by agent/desktop. `Error` is defined in lib.rs:20-62.

### 1a. Items used (by importer)

- **posnet item — `posnet::Error` (variants: `Invalid, NoPort,
  Open{port,source}, Io, Timeout(&'static str),
  Device{code,command,field,rejected}, Protocol, NotReady, Journal,
  Uncertain(Box<Self>)`; `is_denied()`)**: **Defined**: lib.rs:20-62; **agent
  (non-test)**: `lib.rs:33` (`Error::Printer(#[from] posnet::Error)`),
  `lib.rs:54` Debug redaction; `agent.rs:208` `posnet::Error::Device{..}`;
  `agent.rs:428` `posnet::Error::Uncertain(_)`; `worker.rs:302,309,465,469`;
  **desktop (non-test)**: `cli.rs:90` `Error::Printer(_) => (1,"local")`
  (agent::Error, not posnet::Error directly); **tests only**:
  `agent/tests.rs:581` `Device{code:2000,..}`; `worker/tests.rs:208` (PanicLink
  `open` returns `posnet::Error`); `settings/tests.rs:299`
- **posnet item — `printer::Printer` (`new(link, Connection)`, `connection()`,
  `probe()`, `detect(may_switch)`, `verify(serial)`,
  `print_receipt(&Receipt,&Status,&mut dyn Journal)->Printed`,
  `print_report(Report,&str serial,&mut dyn Journal)`)**: **Defined**:
  printer.rs:~124-330; **agent (non-test)**: `agent.rs:15,40` (field `printer`),
  calls: `agent.rs:~144 probe`, `:207 print_report`, `:238 verify`, `:~388
  probe`, `:413 print_receipt`, `:596 verify`; `worker.rs:21,349 Printer::new`,
  `:356 printer.detect(may_switch)`, `:357 printer.connection()`; **desktop
  (non-test)**: —; **tests only**: `agent/tests.rs:4`
- **posnet item — `printer::Status {name, version, serial, problems:
  Vec<String>, vat: Vec<VatRate>}` + `ready()`, `description()`**: **Defined**:
  printer.rs:75-110; **agent (non-test)**: `agent.rs:15,65,97,106` (`describe`),
  `agent.rs:107-119` reads `.serial/.vat/.name/.version`; `agent.rs:163-166`
  `.ready()/.description()`; `worker.rs:21,103,156`
  (`Event::Device{status:Option<Status>}`); **desktop (non-test)**:
  `model.rs:12,75` (`device: Option<Status>`), `model.rs:286,334,344-345`
  (`ready()`, `description()`); `application.rs:558-564` (assigns); **tests
  only**: `application/tests.rs:21,99-103` builds `Status{name:"TEMO",...}`;
  `model/tests.rs:22`; `tests/end_to_end.rs:330` `status.ready()`
- **posnet item — `printer::VatRate {slot, basis_points}` + `active()`,
  `exempt()`, `label()`**: **Defined**: printer.rs:43-72; **agent (non-test)**:
  `api/model.rs:7,53 (VatCode::of)`, `:194-205 printer_slots(&[VatRate])`;
  **desktop (non-test)**: —; **tests only**: —
- **posnet item — `printer::Report {Daily, Monthly{year:i16,month:i8},
  Periodic{from:Date,to:Date,summary:bool}}` (serde `tag="kind"`,
  `rename_all=snake_case`) + `label()`**: **Defined**: printer.rs:112-135;
  **agent (non-test)**: `agent.rs:15,~189 run_report(report)`,
  `worker.rs:21,638-640` (maps `Command::*Report` → `Report::*`), `:642,646
  report.label()`; `journal.rs:12` (`ReportOperation.report: Report` —
  PERSISTED); **desktop (non-test)**: —; **tests only**: `agent/tests.rs:5,580`,
  `journal/tests.rs:5`
- **posnet item — `printer::Journal` trait (`dispatching(&mut self,
  command:&'static str)->io::Result<()>`, `waiting(&mut self, Option<&str>)`)**:
  **Defined**: printer.rs:31-37; **agent (non-test)**: `agent.rs:15 (as
  DeviceJournal)`, impls `agent.rs:622-641 ReceiptDispatch`, `:645-660
  ReportDispatch`; **desktop (non-test)**: —; **tests only**: —
- **posnet item — `printer::Printed {receipt_number: Option<String>, issued_at:
  Timestamp}` (returned, not named)**: **Defined**: printer.rs:~139; **agent
  (non-test)**: `agent.rs:~415-421` fields used; **desktop (non-test)**: —;
  **tests only**: —
- **posnet item — `receipt::{Receipt, Line, Quantity, Advance, Payment,
  PaymentForm, VatSlot, VAT_SLOTS}`**: **Defined**: receipt.rs; **agent
  (non-test)**: `agent.rs:16` `Advance, Line, Payment, Quantity, Receipt,
  VatSlot`; `agent.rs:465 Line::new`, `:503 Receipt::new`, `:670
  Quantity::parse`, `:678 VatSlot::new`,
  `Receipt::{lines(),payments(),fiscal_total()}` in `journal.rs:~95-105
  Summary::of`; `journal.rs:13` `Payment, Receipt`; `history.rs:16`
  `PaymentForm`; `api/model.rs:8,199 VAT_SLOTS`; `agent/build.rs:72` maps schema
  `FiscalPaymentForm` → `posnet::receipt::PaymentForm`; **desktop (non-test)**:
  `model/table.rs:161,195` `form.label()` (via `HistoryEntry.payments`); **tests
  only**: `journal/tests.rs:6`, `history/tests.rs:3`
- **posnet item — `money::Grosze` (`parse`, `get`, `Sum`, `Display`, serde
  try_from/into u64) and `money::zloty(u64)`**: **Defined**: money.rs; **agent
  (non-test)**: `agent.rs:14,674 Grosze::parse`, `:~495 .sum::<Grosze>()`;
  `journal.rs:11 Summary.total`; `history.rs:16,35-36`
  (`HistoryEntry.total/payments` — PERSISTED); **desktop (non-test)**:
  `model.rs:12,452 zloty()`; `model/table.rs:7,128,191-195` `Grosze`, `zloty`;
  **tests only**: many tests
- **posnet item — `decimal::parse(text, scale)`**: **Defined**: decimal.rs;
  **agent (non-test)**: `documents.rs:2,83` (parses złoty strings from Holuto
  docs); **desktop (non-test)**: —; **tests only**: —
- **posnet item — `text::{MAX_NAME_LENGTH(80), MAX_LINES(500),
  MAX_COMPANY_NAME_LENGTH(40), printable_characters()}`**: **Defined**:
  text.rs:6-11,183; **agent (non-test)**: `api/model.rs:9,80-85` (Telemetry::new
  limits), `:146` (`MAX_LINES` in `Job::check`); **desktop (non-test)**: —;
  **tests only**: —
- **posnet item — `session::{Link, Port, Connection, SerialLink,
  serial_ports()}`**: **Defined**: session.rs:19-82; **agent (non-test)**:
  `worker.rs:22,208,327,351` (`Arc<dyn Link>`, `Connection{port,baud_rate}`;
  `Worker::spawn` public signature takes `Arc<dyn Link>`); **desktop
  (non-test)**: `application.rs:17,51,67,116,234` (`Link`, `serial_ports()`
  populates UI port list); `cli.rs:21,283` (`session::serial_ports()` for
  `ports` cmd); `main.rs:36,38,82` (`Link`, `SerialLink`); **tests only**:
  `agent/tests.rs:6 Connection`; `worker/tests.rs:15 Link, Port`
- **posnet item — `simulator::{Simulator, Fault, SERIAL, SimReport, Hold}`**:
  **Defined**: simulator.rs; **agent (non-test)**: — (only under `testing`
  feature / tests); **desktop (non-test)**: `main.rs:40,72-79` (`--features
  simulator`: starts a simulator, sets `Settings.port = simulator.address()`,
  `link = simulator.link()`); **tests only**: see §5

Not used by agent/desktop: `posnet::frame::*`, `session::Session`,
`Printer::connection` in desktop, `receipt::Step`, `Error::is_denied`,
`text::check_printable`.

### 1b. Cross-crate type embedding

(posnet types inside agent's public/persisted types)

- `agent::Error::Printer(posnet::Error)` (lib.rs:33) — public enum.
- `agent::worker::Event::Device{status: Option<posnet::printer::Status>}`
  (worker.rs:103) — consumed by desktop.
- `agent::worker::Worker::spawn(dir, Settings, Arc<dyn posnet::session::Link>,
  sink)` (worker.rs:~205-215).
- `agent::history::HistoryEntry.{total: Grosze, payments:
  Vec<(PaymentForm,Grosze)>}` (history.rs:35-36).
- `agent::journal::{ReportOperation.report: posnet::printer::Report,
  Summary.{total: Grosze, payments: Vec<Payment>}}`
  (journal.rs:~66-95,~140-150).
- Build-time: `crates/agent/build.rs:72`
  `("FiscalPaymentForm","posnet::receipt::PaymentForm")` – generated wire type
  `JobPayment.form` is posnet's enum.

---

## 2. Posnet-specific semantics leaking into `agent`

### 2a. Journal stage strings = Posnet command names

`ReceiptDispatch::dispatching(command)` / `ReportDispatch::dispatching(command)`
(agent.rs:622-660) store `Device::Dispatching{stage: command.to_owned()}`
verbatim. The strings come from `posnet::receipt::Step.command`
(receipt.rs:346-415: `trinit, trnipset, trline, tradvance, trpayment, trend,
trftrln, trftrend`) and `printer.rs` report steps (`dailyrep, monthlyrep,
periodicrepbydates`). The agent never interprets `stage`; it is opaque in logic
(`Device::Dispatching{..}` only matched structurally;
`Operation::blocks`/`ReportOperation::unresolved` use only the variant), but it
IS persisted and shown in tests (`stage:"trend"`, `"dailyrep"`, `"trline"` in
agent/tests.rs:323,489; tests/reconciliation.rs:141; journal/tests.rs:66,188;
fixtures `receipt-dispatching.json:9`, `receipt-result-unknown.json:9`,
`report-*.json` — `report-dispatching.json` even uses `"stage":"periodic"`,
proving it's free-form).

### 2b. Error classification

(the "certain vs uncertain" rule lives in posnet, consumed in agent via 2 match
arms)

- Driver side — `printer.rs` `dispatch()` / `refusal_is_certain()` / `refuses()`
  / `uncertain()` (printer.rs ~340-375):
  - `journal.dispatching(step.command)` before every step; error on index 0 →
    `Error::Journal`, later → `Uncertain(Journal)`.
  - `refusal_is_certain(error, sent)`: `refuses(error,sent)` (i.e.
    `Error::Device{command == sent}`) AND (`sent == "trinit"` OR (`sent ∈
    {dailyrep, monthlyrep, periodicrepbydates}` AND
    `Error::Device{rejected:true}` i.e. `ERR` envelope)). Only valid on `index
    == 0`. Everything else after dispatch → `Error::Uncertain(Box<..>)`.
  - `Error::Device.rejected = (envelope command == "ERR")` set in `frame.rs
    Reply::into_result` (frame.rs ~38-48); `decode_response` rejects `ERR` w/o
    code as Protocol.
  - `!sprn`/`!sdev` operator-wait logic: `execute()` (printer.rs ~385-445),
    `watch()` (~455-515), `query()`, `operator_problem()` (`pr ∈ {1,3,5,6}` or
    `ds ∈ {2,3}`), `mechanism_problem()`; `OPERATOR_WAIT = 600s`,
    `WATCH_INTERVAL = 2s`; reaches agent only through
    `Journal::waiting(Option<&str>)`.
- Agent side (only two places):
  - `agent.rs:207-217` (`run_report`): `if matches!(error,
    posnet::Error::Device{..}) && matches!(operation.device,
    Device::Dispatching{..})` → reset to `Device::NotStarted` and save (comment:
    "driver returns a bare Device error only for a refusal naming this report").
    **Agent relies on the driver contract that a bare (un-wrapped-in-Uncertain)
    `Device` error from `print_report` == certain refusal.**
  - `agent.rs:413-440` (`execute`): `Ok(printed)` → `Completed{receipt_number}`;
    `Err(posnet::Error::Uncertain(_))` → `JobResult::unknown(..)` (blocks);
    **any other `Err`** → `JobResult::failed` + `finish(.., JobState::Failed)`
    (so trinit refusal, Journal-on-first-command, preflight
    `NotReady/Invalid/Protocol/Timeout`, `Open`, etc. all become FAILED,
    non-blocking).
  - `worker.rs:465,469` `trace_failure`:
    `Error::Printer(posnet::Error::Uncertain(_))` → error-level log; other
    `Printer` → warn.
  - Preflight failure at claim time: `agent.rs:~386-398` — `printer.probe()`
    error or `describe()` (serial mismatch) → `JobResult::failed` before any
    fiscal command (journal already saved as `NotStarted` at `agent.rs:~384`).

### 2c. Preflight checks (driver, run per operation in a fresh `Session`)

- `Printer::print_receipt` (printer.rs ~230-290): `read_status` →
  `require_ready` → `require_device(expected.serial)` → `status.vat !=
  expected.vat` ⇒ `Invalid("Stawka VAT zmieniła się…")` → every receipt slot
  `status.vat.get(slot).is_none_or(!active)` ⇒ `Invalid` → `hdrget` (header text
  lowercased; company `brand` appended as footer via `trftrln id=25` if brand
  absent from header) → `clock()` (`rtcget`) → `dispatch(steps)` → `scnt` (`bt`
  = receipt number).
- `Printer::print_report` (printer.rs ~290-345): period check, `read_status`,
  `require_ready`, `require_device`, report step; Daily uses `rtcget` field
  `da`.
- `read_status` (printer.rs ~540-560) = 6 commands `getrealid, sdev, sprn,
  scomm, strns, vatget`; `status()` (~560-625) builds `problems`: model must
  equal `"POSNET TEMO ONLINE"` (const `TEMO`, printer.rs:14), fiscal mode
  (`fs`), header (`hr`), `ds==0`, queue empty (`qe`), mechanism `pr`, read-only
  `td`, `tl`, open transaction (`to/fe/ts`), ≥1 active VAT slot.
- `Printer::verify(serial)` (printer.rs ~215): probe+ready+same serial; used by
  agent `resolve_report` (agent.rs:238) and `acknowledge_orphaned_receipt`
  (agent.rs:596).
- `Printer::probe()` used in `Agent::step` (agent.rs:~144) every poll (heartbeat
  even if probe fails).

### 2d. Receipt number reading / clock

- Receipt number: `scnt` field `bt` read AFTER `trend`, best-effort (`.ok()`,
  never retried) in printer.rs print_receipt → `Printed.receipt_number:
  Option<String>`; agent persists `Device::Completed{receipt_number: Option}`
  (agent.rs:416-420), `None` ⇒ `done()` returns `JobResult::unknown(UNNUMBERED)`
  (agent.rs `done` ~664-668, const `UNNUMBERED` agent.rs:~38) and the job stays
  blocked until staff supply number in Holuto (`finish(.., staff_number)`
  agent.rs:~552).
- Clock: `rtcget` `tm` (ISO8601, minute floor) else `da` civil + `PRINTER_ZONE =
  "Europe/Warsaw"` (printer.rs:19, `clock()` ~515-540); `Printed.issued_at`
  replaces `operation.issued_at` (agent.rs:~419) and is what Holuto receives in
  `ResultForm.issued_at` (journal.rs `result_form`). Simulator imports
  `printer::PRINTER_ZONE` (simulator.rs:24).

### 2e. VAT slot table

- Device table: `vatget` keys `va..vg`, `VAT_SLOTS = 7` (receipt.rs:14),
  `basis_points` from decimal string; `10000` = exempt, `>9999` otherwise
  inactive (printer.rs consts `EXEMPT_BASIS_POINTS`, `MAX_ACTIVE_BASIS_POINTS`).
- Agent mapping: `api/model.rs:53-66 VatCode::of(VatRate)` (only 23/8/5/0/zw;
  others skipped with notice "Pomijam nieobsługiwaną stawkę VAT…"),
  `api/model.rs:194-205 printer_slots` (filters `slot < VAT_SLOTS`),
  `agent.rs:115-118 describe` sets `telemetry.vat_slots`.
- Job→printer match: `agent.rs:~445-462 receipt()` requires each
  `(line.vat_slot, line.vat_rate)` and advance pair to be in
  `telemetry.vat_slots` before building the `Receipt`; `slot(u8)` →
  `VatSlot::new` (agent.rs:677).
- Backend schema allows 0–15 (`api/model.rs:48-49 MAX_BACKEND_SLOTS=16`; comment
  "the printer has seven").

### 2f. Text limits / allowed characters

`posnet::text` consts (text.rs:6-11): `MAX_NAME_LENGTH=80` (trline `na`),
`MAX_LINES=500` (trinit), `MAX_BUYER_NIP_LENGTH=30`,
`MAX_COMPANY_NAME_LENGTH=40` (trftrln `na`); `printable_characters()`
(text.rs:183) = CP1250 0x20–0xFF minus DEL/table-drawing glyphs. Agent uses
them: `Telemetry::new` (api/model.rs:79-90) hard-codes them as
`maxItemNameLength/maxLinesPerReceipt/maxCompanyNameLength/allowedCharacters`
(Telemetry is constructed ONCE with `Telemetry::new(api.serial())`,
agent.rs:~79, limits are static, never derived from `Status`); `Job::check`
(api/model.rs:146) uses `MAX_LINES`. Enforcement in `Line::new` (name ≤80
printable) and `Receipt::new` (brand ≤40, lines 1..=500, NIP ≤30 printable) —
driver returns `Error::Invalid`, agent turns it into failed job (`receipt()`
returns `Err(String)` → `JobResult::failed`, agent.rs:400-410).
Quantity: ≤8 decimals, ≤9 999 999 999 (receipt.rs:15-17). Money: `Grosze::MAX =
9_999_999_999` (money.rs:15). Payment wire types (cash 0, card 2, transfer 8,
mobile 6 + name `Płatność mobilna`, credit 5): receipt.rs `wire_type`. Advance
name `Zaliczka`.

### 2g. Serial-port discovery / Temo identification

- `Printer::detect(may_switch)` (printer.rs ~170-205): probes configured port
  (`probe`); if it fails and `may_switch`, iterates `link.candidates()` (=
  `serial_ports()` for `SerialLink`: USB first, macOS `/dev/cu.*` only —
  session.rs:67-80) calling `identify()` (printer.rs ~517): opens, sends only
  `getrealid` with `SCAN_TIMEOUT=500ms`, accepts only `nm == "POSNET TEMO
  ONLINE"`, then `read_status`. Permission-denied →
  `Error::Invalid("…dialout…")`.
- Agent gating: `worker.rs:345-356` — `may_switch =
  journal.load()?.is_none_or(|op| !op.blocks())`; `Event::PortDetected(port)`
  (worker.rs:358-359) makes desktop persist new port (`application.rs:537-540`,
  `preferences.rs:78-82 Request::Port`).
- Serial binding: `worker.rs:361` `Api::new(.., &status.serial)`;
  `api.rs:205-215` requires serial 1–32 printable ASCII; `X-Device-Serial`
  header (api.rs:~319). `agent.rs:107` rejects a status with a different serial
  than the paired one; journal `device_serial` is compared in `settle()`
  (agent.rs:~262) and reports/verify.

### 2h. Telemetry fields (what the agent fills from `Status`)

`agent.rs:115-118 describe()`: `telemetry.model = status.name` (the `getrealid`
`nm`, must be `"POSNET TEMO ONLINE"` to be "ready"), `telemetry.firmware =
status.version` (`vr`), `telemetry.serial = api.serial()` (from `nu`),
`vat_slots` from `vatget`. Limits + `allowed_characters` are `Telemetry::new`
constants from `posnet::text` (see 2f). `app_version = APP_VERSION` (api.rs:33).
`Agent::session()` / `Agent::step()` send this via
`Api::session`/`Api::heartbeat` (api.rs `telemetry()` ~293-304;
`PrinterInfo::check(serial)` in api/model.rs:~96-128 validates the response:
device_serial match, zero limits, empty chars, vat slots ≤16, slot < 16).

### 2i. Other Posnet-language / semantic leakage

- Report variants are Polish fiscal reports specific to Posnet's command set
  (`Report::{Daily,Monthly,Periodic{summary}}`; `Daily` closes day in fiscal
  memory using printer date).
- `agent::worker::Command::{DailyReport,MonthlyReport,PeriodicReport,...}`
  (worker.rs:~38-55) map 1:1 to `posnet::Report`.
- `agent/build.rs:72` payment forms are posnet's enum (`cash, card, transfer,
  mobile, credit`; matches openapi `FiscalPaymentForm`).
- `Quantity`/`Line`/`Receipt` are posnet types constructed in
  `agent.rs::receipt()` (agent.rs:~438-510) — vendor-neutral-looking but
  implement Posnet-specific limits/rounding (`Line::new`: qty×price half-up to
  grosze, 80-char name).
- Agent's own `agent::Error` Debug/redaction treats `Printer(_)` opaquely
  (lib.rs:54).

---

## 3. Persisted formats

State dir = `StateDir` (store.rs): files `settings.json`, `operation.json`,
`history.jsonl`, `status.json`, `instance.lock`. Simulator builds use
`workshop-simulator` dir (`store.rs:31-37 default_path_for(simulator)`;
`cli.rs:206`; `main.rs:69`).

### 3a. `operation.json`

— `crates/agent/src/journal.rs` (all `deny_unknown_fields`, snake_case)

```rust
// journal.rs:~23-30
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Operation { Receipt(ReceiptOperation), Report(ReportOperation) }

pub(crate) enum Device {            // same serde attrs
    NotStarted,
    Dispatching { stage: String },
    // stage == driver command name (opaque string)
    Completed { receipt_number: Option<String> },  // counter may be unreadable
}

#[serde(deny_unknown_fields)]
pub(crate) struct ReceiptOperation {
    pub(crate) job_id: Uuid,
    pub(crate) attempt: Uuid,
    pub(crate) issued_at: Timestamp,
    // claim time → printer time (rtcget) after completion
    pub(crate) device_serial: String,  // physical serial (Posnet `nu`)
    pub(crate) device: Device,
    pub(crate) summary: Option<Summary>,
    // { total: Grosze(u64), payments: Vec<Payment{form: PaymentForm, amount:
    // Grosze}>, items: Vec<String> }
    pub(crate) result: Option<JobResult>,
    // snake_case enum: Done{receipt_number}|Failed{reason}|Unknown{reason}
    pub(crate) settled: Option<Settlement>,
    // { at: Timestamp, state: JobState }
}

#[serde(deny_unknown_fields)]
pub(crate) struct ReportOperation {
    pub(crate) report: Report,
    // posnet::printer::Report, serde tag="kind", snake_case:
    // daily|monthly{year,month}|periodic{from,to,summary}
    pub(crate) device_serial: String,
    pub(crate) started_at: Timestamp,
    pub(crate) device: Device,
    pub(crate) acknowledged_at: Option<Timestamp>,
}
```

Fixtures (literal, current format): `crates/agent/src/journal/fixtures/*.json`,
e.g.
`{"receipt":{"job_id":..,"attempt":..,"issued_at":..,"device_serial":"POSNET-123456","device":{"dispatching":{"stage":"trline"}},"summary":{"total":100,"payments":[{"form":"card","amount":100}],"items":[..]},"result":{"unknown":{"reason":..}},"settled":null}}`
and
`{"report":{"report":{"kind":"periodic","from":"2026-09-01","to":"2026-09-30","summary":false},"device_serial":..,"started_at":..,"device":{"dispatching":{"stage":"periodic"}},"acknowledged_at":null}}`.
Vendor-specific persisted parts: `Report` enum shape (shared from posnet),
`PaymentForm` snake_case names, `Grosze` as u64, `device.dispatching.stage`
strings, `device_serial` (has "serial" semantics of any vendor; no model
stored).
Journal blocks: `Operation::blocks()` (journal.rs ~190): receipt unsettled;
report `Dispatching` and not acknowledged. Unreadable/corrupt journal ⇒
`Error::Blocked` (journal.rs `load`).

### 3b. `settings.json` — `crates/agent/src/settings.rs:~55-85`

```rust
pub const BAUD_RATES: [u32; 5] = [9600, 19200, 38400, 57600, 115_200];
pub const DEFAULT_BAUD_RATE: u32 = 9600;   // "printer's factory baud rate"
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub api_url: String,
    pub token: Token,            // transparent String, redacted Debug
    /// Empty until a printer port is chosen or detected.
    pub port: String,
    pub baud_rate: u32,
    pub telemetry_enabled: bool,
    pub start_on_launch: bool,
}
```

Fixture: `settings/fixtures/settings.json`:
`{"apiUrl","token","port","baudRate","telemetryEnabled","startOnLaunch"}`.
Validation: `validate_configuration` rejects control chars in `port`, `baud_rate
∉ BAUD_RATES` (settings.rs ~165-175). No vendor/model field exists.
`deny_unknown_fields` ⇒ adding a field (e.g. vendor) breaks older binaries
reading a newer file and requires explicit default handling for older files (no
`#[serde(default)]` currently).

### 3c. `status.json` — `crates/agent/src/status.rs:22-37`

```rust
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub processing: bool, pub connected: bool,
    pub serial: Option<String>,      // last known physical device serial
    pub printer_id: Option<String>,  // Holuto printer id
    pub blocked: bool, pub report_unresolved: bool, pub orphaned_receipt: bool,
    pub updated_at: Timestamp,
}
```

No vendor-specific fields; written by desktop (`model.rs:170-190 status()`),
read by CLI `status` (`cli.rs ~320-345`; its JSON shape is test-pinned at
`cli/tests.rs:196-200`).

### 3d. `history.jsonl`

— `crates/agent/src/history.rs:~26-38` (one JSON/line, ≤500, camelCase; no
deny_unknown_fields)

```rust
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub job_id: Uuid, pub printed_at: Timestamp,
    pub receipt_number: Option<String>,
    pub total: Grosze,
    pub payments: Vec<(PaymentForm, Grosze)>,
    // serialized as [["card",40],["credit",60]]
    pub items: Vec<String>,
}
```

Fixture `history/fixtures/history-torn.jsonl`. Display-only cache.

### 3e. CLI JSON outputs touching printer config

`config show` (cli.rs:~243-247):
`{"ok":true,"config":{"apiUrl","paired","port":"auto"|port,"baudRate","telemetryEnabled","startOnLaunch"}}`;
test-pinned at `cli/tests.rs:88-91`.

---

## 4. Desktop / CLI places mentioning Posnet/Temo or port/speed

### Rust source

- `crates/desktop/Cargo.toml:11,16` `simulator = ["posnet/simulator"]`,
  `posnet.workspace = true`; dev-dep `agent` with `testing` (line ~45).
- `crates/desktop/build.rs:14` `.set("FileDescription", "Holuto Workshop ·
  Posnet Temo Online")` (Windows resource); asserted by
  `packaging/verify-windows.ps1:45`.
- `crates/desktop/src/main.rs:3-4` (`compile_error!` if simulator &&
  !debug_assertions), `:36-40` imports, `:68-89` `start()`:
  `StateDir::open_default(cfg!(feature="simulator"))`; with `simulator`:
  `Simulator::start()`, `Settings{port: simulator.address(), ..settings}`, `link
  = simulator.link()`; else `Arc::new(SerialLink)`; then `application::run(dir,
  settings, link)`.
- `crates/desktop/src/application.rs:17` imports `posnet::session::{Link,
  serial_ports}`; `:51,67,116,445` carry `Arc<dyn Link>` into `Worker::spawn`;
  `:234` `window.set_ports(strings(serial_ports()))`; `:537-540`
  `Event::PortDetected(port)` → `settings.port` + `Request::Port(port)`;
  `:462-466,546` `telemetry::printer(printer_id, serial)`; `:541-549`
  `Event::Connected{printer_id,serial,port}`; `:558-567`
  `Event::Device{status,..}`.
- `crates/desktop/src/model.rs:12` `posnet::{money::zloty, printer::Status}`;
  `:75` `device: Option<Status>`; `:228` `window.set_baud_rates(BAUD_RATES…)`;
  `:247-252` captures `window.get_port()` (trimmed) and `baud_index` →
  `settings.port / baud_rate`; `:296-303` sets port + baud_index;
  `:286,334,344-345` readiness/description; `:347` `"Połączono · {serial}"`.
- `crates/desktop/src/model/table.rs:7,128,161,191-195`
  `posnet::money::{Grosze,zloty}`, `PaymentForm::label()` (Polish labels).
- `crates/desktop/src/cli.rs:21` `use posnet::session;`; help text `:28-45`
  incl. line 31 `config set [--api-url URL] [--port auto|PORT] [--speed LICZBA]
  [--diagnostics on|off]` and line 32 `ports   Wypisz nazwy portów (bez
  otwierania)`; flag parsing `:156-166` (`--port auto` → empty string; `--speed`
  parsed as u32, validated only by `Settings::save`→`validate_configuration`);
  `config show` `:243-247`; `Command::Ports` `:282-288`
  `session::serial_ports()`; `config_set` `:353-370` (`settings.port`,
  `settings.baud_rate`); error mapping `:84-91` (`Error::Printer(_)` → exit 1
  "local"); pair via `Worker::pair` `:~373-420`.
- `crates/desktop/src/application/preferences.rs:18,29,78-83`
  `Request::Port(String)` persists detected port (only if settings.json exists).
- `crates/desktop/src/telemetry.rs:109,140,150-157,209` tags `printer_id`,
  `device_serial` (allow-listed for Sentry; no model/vendor tag).
- Strings: `application.rs:32`, `:453` "Łączenie z drukarką i Holuto…", `:532`,
  `model.rs:119,345,398` generic "drukarka" (no Posnet brand).

### Slint

— `crates/desktop/ui/app.slint` (no "Posnet"/"Temo" text anywhere in it)

- `:3` `export enum Action {save, pair, start, stop, quit, queue-receipt,
  previous-page, next-page, daily-report, monthly-report, periodic-report,
  resolve-report, acknowledge-orphaned-receipt}`.
- `:~115-150` `PortInput` component: `accessible-label: "Port drukarki"`
  (`:124`), editable combobox over `model` (ports list), hint `:146` `"Nie
  znaleziono portów. Wpisz adres drukarki."`.
- `:199-203` `in-out property <string> port; in property <[string]> ports; in
  property <[string]> baud-rates; in-out property <int> baud-index;`.
- `:310` `Text "Port drukarki"` + `PortInput { text <=> root.port; model:
  root.ports; enabled: root.settings-editable; }`; `:311` `ComboBox {
  accessible-label: "Prędkość transmisji"; model: root.baud-rates; current-index
  <=> root.baud-index; }`; `:305` hint "…wybierz drukarkę…".
- `:273-281` "Raporty fiskalne" card (daily/monthly/periodic report buttons
  `Drukuj raport dobowy|miesięczny|okresowy`, date range hint "Daty od
  01.01.2000…"), `:330-331` "Wyjaśnij raport" / "Wyjaśnij paragon" buttons;
  `:174-175 connection-text/-tone`.

### Non-Rust docs/packaging mentioning Posnet/Temo

(would need edits for multi-vendor)

`README.md:3,8,112-119,235,249-258,331-334,347-348`;
`AGENTS.md:4,38-39,50-52,62,112`; `llms.txt:3`;
`docs/install-with-ai.md:4,62,362-366`; `docs/windows-code-signing.md:33,37-38`;
`packaging/linux/control.in:9-10`; `packaging/verify-windows.ps1:45`;
`.omp/rules/posnet-wire-encoding.md` (scope `crates/posnet/src/**`);
`.cargo/mutants.toml:5` excludes `crates/posnet/src/simulator*`;
`docs/testing/*` (checklist.md "## Posnet driver", compatibility.md:72-73,
platforms.md:61 `com_pair`), `docs/workflows.md:18,37`, `AGENTS.md:112,126`
nextest filters `rdeps(posnet)`, `-p posnet`.

---

## 5. Simulator architecture and test harnesses

### `crates/posnet/src/simulator.rs` (781 lines)

- Module doc: "Offline TCP Temo peer using the serial driver's `Session`, with
  one session at a time." Gated `#[cfg(any(test, feature="simulator"))]` in
  lib.rs:13-14; feature `simulator = []` in `crates/posnet/Cargo.toml`
  ("excluded from release builds" by convention; posnet itself has NO
  `compile_error!` guard — AGENTS.md:61-63; agent `lib.rs:11-12` guards
  `testing`; desktop `main.rs:3-4` guards `simulator`).
- Public API: `SERIAL = "DEMO0000001"` (l.30), `Params =
  BTreeMap<String,String>`, `Fault {Error, Reply(Vec<(String,String)>),
  LoseReply, CorruptReply, Disconnect, ExecutionError,
  Device{code,command,field}}` (l.40-60),
  `SimReceipt{total,lines,payments,buyer_nip}`, `SimReport{command,params}`,
  `Hold{reached(timeout),release()}`, `Simulator::{start(), address(), link(),
  link_with(candidates, denied), receipts(), reports(), footers(), frames(),
  transaction_open(), set_transaction_open(), set_header(), set_vat([&str;7]),
  set_clock(Timestamp), set_status(field,value), fault(cmd,Fault),
  clear_fault(cmd), set_paper(bool), run_out_of_paper_at(cmd),
  hold(cmd)->Hold}`.
- Transport: binds `127.0.0.1:0` TCP listener on a thread (`serve` l.402,
  `serve_session` l.414: one connection at a time, `POLL=1ms` read timeout);
  `SimLink: Link` (l.299-343) — `open(port,_baud)` treats `port` as the
  `host:port` TCP address (`port != address` ⇒ `Open{NotFound}`, `denied` ports
  ⇒ `PermissionDenied`, second concurrent open ⇒ `PermissionDenied` via
  `connected` flag); `candidates()` configurable; `time_scale() = 1000`
  (accelerates driver timeouts — `Session::timeout()` divides, min 100ms;
  production `SerialLink` has scale 1). `SimPort` implements `Port` incl.
  `operator_wait_limit` hook for holds. So **the simulator exercises the real
  `Session`/`frame`/`Printer` code path** over TCP; baud ignored.
- Wire emulation hard-codes Posnet protocol: `answer()` (l.507) builds
  `ERR\t?2000\t…`/`<cmd>\t…` replies with CRC via `frame::wrap`+`text::encode`;
  query replies `State::query` (l.~590): `getrealid{nm:"POSNET TEMO
  ONLINE",vr:"32.01",nu:SERIAL}`, `sdev/!sdev`, `sprn/!sprn`, `scomm`, `strns`,
  `hdrget`, `scnt{bt: receipts.len()}`, `vatget va..vg` (default
  `["23,00","8,00","0,00","101,00","101,00","101,00","100,00"]`),
  `rtcget{da,tm}` in `PRINTER_ZONE`; `State::handle` (l.~660-750) implements
  `trinit, trnipset, trline, tradvance, trpayment, trend, trftrln, trftrend,
  dailyrep|monthlyrep|periodicrepbydates` and validates amounts/slots/payment
  types (`["0","2","5","6","8"]`, mobile name). Status overrides via
  `set_status(field,value)` on Posnet field keys (`nm, ds, pr, fs, hr, qe, td,
  tl, ts, to, fe…`). Holds/paper-out queue "printing" commands but let
  `!`-prefixed queries bypass (l.480-490).
- It imports driver internals: `crate::{decimal,
  frame::{self,ETX,MAX_FRAME,Reply}, printer::PRINTER_ZONE, receipt::Quantity?,
  session::{Link,Port,…}, text}` (l.21-28).
- Tests of simulator: `crates/posnet/src/simulator/tests.rs`.

### Feature flags / harness wiring

- `agent`: `testing = ["posnet/simulator","dep:tiny_http","dep:jsonschema"]`
  (agent/Cargo.toml:11); `[dev-dependencies] posnet = {features=["simulator"]}`
  so `cargo test -p agent` works without the feature; `agent::testing`
  (`#[cfg(any(test, feature="testing"))] pub mod testing`, lib.rs:20-21) is the
  loopback Holuto HTTP fake (`Holuto`, `job_json()`, `state_dir()`, `with()`,
  `Fault`, consts `JOB_ID/ATTEMPT/PRINTER_ID/ORDER_ID/ALLOCATION_ID`) validated
  by `api/contract.rs` against `openapi/agent.json`; compile_error in release
  (lib.rs:11-12).
- `desktop`: `[features] simulator = ["posnet/simulator"]`; dev-dep `agent =
  {features=["testing"]}`; tests use `posnet::simulator::{Simulator, Fault,
  SERIAL, Hold}` + `agent::testing::Holuto`.
- Test boundary rules: `.omp/rules/writing-tests.md` (printer scenarios MUST use
  `Simulator`; cross-crate reuse `posnet::simulator` + `agent::testing`).

### How agent tests use it

- `agent/src/agent/tests.rs:1-30`: helper `agent(sim,&dir,&server)` →
  `Api::new(server.url(), server.token(), SERIAL)`, `Printer::new(sim.link(),
  Connection{port: sim.address(), baud_rate: 9600})`, `agent.printer.probe()`,
  `describe`, `session()`. Uses `sim.set_clock`, `sim.fault("dailyrep"|"scnt",
  DeviceFault::…)`, `sim.set_paper(false)` (:608), `sim.frames()` filtered by
  Posnet command names (:663-676).
- `agent/src/agent/tests/reconciliation.rs` (proptest-state-machine) —
  `Simulator::start()` at :135; journals with `stage:"trend"` (:141).
- `agent/src/worker/tests.rs`: `Worker::spawn(dir, Settings{port: sim.address(),
  ..}, sim.link(), sink)` pattern; faults `sim.fault("getrealid"|command,
  PrinterFault::{Error,LoseReply})`, `sim.hold("trend"|"dailyrep")`,
  `sim.set_header(..)`, `sim.reports()` → `SimReport{command:"dailyrep",
  params:{da:"2026-10-16"}}` (:767,1076), `sim.receipts()`, `FISCAL_COMMANDS`
  list of 8 Posnet commands (:1316-1325), `fiscal_frames(&sim)` (:1345) decoding
  frames; `PanicLink` custom `Link` (:~200-215).
- `agent/src/testing.rs:791-795` fake backend printer JSON hard-codes
  `"model":"POSNET TEMO
  ONLINE","firmware":"32.01","deviceSerial":"DEMO0000001"`, limits 80/500/40,
  allowed chars; `testing/tests.rs:29-31`, `api/tests.rs:20-23`,
  `api/contract/tests.rs:19-23` set `telemetry.model = "POSNET TEMO ONLINE"`.

### How desktop tests use it

- `application/tests.rs:21,45-49,99-103,526-531,580-584`,
  `application/tests/{end_to_end.rs,pairing.rs,start.rs}`: `make_app(dir,
  Settings{port: simulator.address(), token: holuto.token()},
  simulator.link())`; `simulator.hold("trend")`, `.fault("dailyrep",
  Fault::CorruptReply)`, `.set_clock(..)`, `.receipts()`, `.frames().is_empty()`
  ("pairing never opens a printer", pairing.rs:187-190);
  `application/macos/tests.rs` + `bin_tests/macos_quit.rs`.
- Slint testing backend `i_slint_backend_testing` (exact version pin `=1.18.1`).
- Posnet's own tests: `crates/posnet/src/tests.rs` (+ `tests/output.rs` insta
  snapshots `tests/snapshots/posnet__tests__output__*.snap`: 15 binding frame
  snapshots; `tests/telemetry.rs`), `printer/tests.rs`, `session/tests.rs`,
  `session/tests/serial_zero.rs`, `frame/tests.rs`, `receipt/tests.rs`,
  `text/tests.rs` + `text/CP1250.TXT`, `money/tests.rs`, `decimal/tests.rs`,
  `simulator/tests.rs`. Windows-only `com_pair` module
  (docs/testing/platforms.md:60-64).

---

## 6. `openapi/agent.json`

(OpenAPI 3.1.0; vendored backend contract; `crates/agent/build.rs` generates
`OUT_DIR/agent-wire.rs` with typify)

Session/heartbeat: `/agent/session` (`operationId session`, l.518) and
`/agent/heartbeat` (l.95) both `POST` body `#/components/schemas/Telemetry`, 200
→ `PrinterStatus`, bearer auth; 400/401/403/404/408/409/413/426/429.

**Telemetry** (l.1638-1688):

```json
"Telemetry": { "type":"object",
  "required":["maxItemNameLength","maxCompanyNameLength","maxLinesPerReceipt",
    "allowedCharacters","serial","model","firmware","appVersion","vatSlots"],
  "properties":{
    "allowedCharacters":{"$ref":"#/components/schemas/AllowedCharacters"},
    "appVersion":{"$ref":".../Code"}, "firmware":{"$ref":".../Code"},
    "maxCompanyNameLength":{"$ref":".../PrinterLimit"},
    "maxItemNameLength":{"$ref":".../PrinterLimit"},
    "maxLinesPerReceipt":{"$ref":".../PrinterLimit"},
    "model":{"$ref":".../Code"}, "serial":{"$ref":".../Code"},
    "vatSlots":{"type":"array",
    "items":{"$ref":"#/components/schemas/VatSlot"}} },
  "additionalProperties": false }
```

Constraint schemas:

- `Code` (l.874):
  `{"type":"string","minLength":1,"maxLength":64,"pattern":"^[^\\u0000-\\u001f\\u007f-\\u009f]+$"}`
  — `model`, `firmware`, `serial`, `appVersion` are free strings (1–64 chars, no
  control chars). **No enum / no vendor pattern for `model`.**
- `PrinterLimit` (l.1467): integer, min 1, max 2147483647.
- `AllowedCharacters` (l.757): string, minLength 1, pattern
  `^[^\u0000-\u001f\u007f-\u009f]+$`, "Every character the printer can print,
  including spaces, in Unicode NFC."
- `VatSlot` (l.1710-1730): `{slot: VatSlotNumber, vatRate: VatRate}`, both
  required, `additionalProperties:false`.
- `VatSlotNumber` (l.1731): integer `minimum 0, maximum 15`.
- `VatRate` (l.1704): string enum `["23","8","5","0","zw"]`, description "Polish
  VAT rates as `KSeF` codes them; `zw` marks an exempt supply." (this is a
  Polish-fiscal, not Posnet, constraint; applies also to Novitus/Elzab).
- No uniqueness constraint on vatSlots; no `maxItems` (agent additionally
  enforces ≤16 on the *response*: `api/model.rs:118-124`).
- `Printer` (response, l.1347-1465): same fields nullable (`oneOf [...,
  {"type":"null"}]`) plus `id, branchId, name, deviceSerial, model, firmware,
  appVersion, vatSlots`; `additionalProperties:false`; `PrinterStatus` wraps
  `printer`.
- Headers on every non-pair `/agent/*` call: `X-Device-Serial` — "Serial number
  pinned by the printer's first session." (string; l.24-31 etc.).
  Session/heartbeat "bind and verify the physical serial; pairing with another
  live printer fails with 409" (README:~263-265; fake: testing.rs:~630
  `fiscal_serial_mismatch` 403).
- `/agent/jobs` POST summary: "Missing telemetry answers 409
  `fiscal_printer_limits_unknown`. Names are never truncated." (l.158) → fake at
  testing.rs:681.
- Job-line fields: `ReceiptLine.vatSlot: VatSlotNumber`, `vatRate: VatRate`;
  `AdvanceDeduction.{vatSlot,vatRate,amount}` (l.672-684, 1546-1567);
  `ReceiptPayment.form: FiscalPaymentForm` enum
  `["cash","card","transfer","mobile","credit"]` (l.1054-1062; mapped to
  `posnet::receipt::PaymentForm`, build.rs:72); `Amount`
  `^-?[0-9]{1,16}(\.[0-9]{1,2})?$`; quantity ≤6 decimals pattern
  `PositiveQuantity_Quantity`; unit price ≤8 decimals.
- Build-time transforms touching printer fields
  (`agent/build.rs:29-31,69-71,85-112`): `Printer→PrinterInfo`,
  `VatSlot→VatSlotRate`, `VatRate→VatCode`, `PrinterLimit→u32` (and for
  Telemetry limits `AgentTelemetryLimit→usize`), `VatSlotNumber→u8`;
  `Code/Label/AllowedCharacters` → `String`; `additionalProperties:false` is
  stripped from generated types (fake enforces the full schema in tests).
- README "Fiscal API contract" (README.md:216-303) documents the telemetry JSON
  example with `"model":"POSNET TEMO ONLINE","firmware":"32.01"`; `vatSlots`
  "Temo Online has slots 0–6, A–G; the contract allows 0–15".

**Conclusion §6**: the schema is vendor-neutral for `model/firmware/serial`
(arbitrary 1–64 char strings), `vatSlots` (0–15, rates 23/8/5/0/zw), limits (≥1)
and `allowedCharacters`. Nothing vendor-specific is constrained; Posnet
specifics are only in what the Rust agent fills in (§2h).

---

## 7. `//!` module-doc invariants and key doc-comment contracts to preserve

### posnet

- `lib.rs:1-5`: Each operation opens an **exclusive session** and verifies
  identity, fiscal mode, header, mechanism, command queue, transaction state and
  VAT table before fiscal commands. **Commands are never retried.** A lost reply
  leaves an **uncertain outcome** for the caller to resolve. The caller's
  `printer::Journal` persists intent **before each command**, surviving
  restarts. `#![forbid(unsafe_code)]`.
- `session.rs:1`: One exclusive connection to the printer. No command is ever
  retried automatically.
- `frame.rs:1-4`: STX command TAB (key value TAB)* # CRC16 ETX, payload
  Windows-1250; CRC-16/XMODEM (spec p.13).
- `text.rs:1`: Required encoding Windows-1250; tabs separate protocol fields.
  (`.omp/rules/posnet-wire-encoding.md`: encode/decode ONLY via `posnet::text`;
  five unassigned CP1250 bytes must remain a decode error.)
- `money.rs:1` amounts in grosze; `decimal.rs:1` exact decimals; `receipt.rs:1`
  "Fiscal receipts validated before printer I/O"; `simulator.rs:1-2` one session
  at a time.
- Item-level contracts: `printer::Journal::dispatching` ("Persists intent before
  each fiscal command; an error prevents that command's send",
  printer.rs:31-34); `Printer::detect` ("Adopts the first Temo Online;
  unidentified devices receive only `getrealid`. Switching must be forbidden
  while an operation is unresolved", printer.rs ~162-165); `Printer::verify`
  ("Requires the paired device to be ready before resolving an interrupted
  operation"); `Printer::print_receipt` ("Rechecks identity, readiness and
  expected VAT table in the printing session"); `dispatch` ("After dispatch,
  failures are uncertain unless no fiscal record can have been issued");
  `refusal_is_certain` comment (trinit error cannot have issued a receipt,
  reports are the record so only `ERR` frame rejection is certain; never
  reissue); `execute` ("Sends once… on timeout, query operator-wait status… wait
  up to OPERATOR_WAIT…; silence or no operator-wait state remains a timeout");
  `Printed.receipt_number` ("Post-print counter; None if unreadable" — "an
  unreadable counter must not trigger a retry"); `Printed.issued_at` ("Printer
  time just before opening the receipt, to the minute; never after printing");
  `Error::Uncertain` text ("Nie wysyłaj jej ponownie przed sprawdzeniem
  drukarki"); `Error::Device.rejected` ("`ERR` rejects a frame before execution;
  command-named errors may follow effects").

### agent

- `lib.rs:1-6`: Fiscal operations run on **one worker thread**; journal intent
  before dispatch; never retry device commands or reprint; **uncertain outcomes
  block receipts and reports across restarts** until staff resolve receipts in
  Holuto or an operator confirms reports on the same device; **resolution never
  reissues commands**. `#![forbid(unsafe_code)]`; `compile_error` for `testing`
  in release.
- `agent.rs:1-8`: Recovery reconciles the journal with Holuto without replaying
  printer commands; saved results resent unchanged; terminal server states
  authoritative; a journal proving no fiscal command was sent reports FAILED
  while Holuto still says TAKEN; if attempt expired to UNKNOWN staff must
  resolve as failed (backend refuses automatic UNKNOWN→FAILED); completed
  evidence conflicting with FAILED/EXPIRED stays blocked until operator
  acknowledges locally on the same ready printer.
- `journal.rs:1-2`: Persisted before each printer command and retained until
  settlement; an unreadable journal blocks printing.
- `worker.rs:1-5`: Sole thread for printer I/O and Holuto requests; commands
  yield to due polls between operations; receipts and reports never interleave;
  cooperative shutdown finishes the current operation before stopping; Event
  diagnostics expose metadata, never operator text or receipt contents.
- `status.rs:1-8`: read-only display status, never fiscal authority; readers
  check instance lock; contains no credentials/API URLs/customer data.
- `history.rs:1-5`: display cache, not the fiscal record; errors never block
  settlement; ≤500 receipts; only a missing number may be upgraded.
- `settings.rs:1`: contains device token; stored privately (0600).
- `api.rs:1-3`: pairing + device API; rejects malformed responses; refuses
  redirects; retry decisions belong to the agent.
- `api/model.rs:1-4`: decimal strings let malformed amounts fail individual
  jobs, not whole responses.
- `store.rs:15`: exclusive state-dir lock prevents concurrent printer or journal
  access (`instance.lock`).
- `testing.rs:1-8`, `api/contract.rs:1`: loopback Holuto fake; offline contract
  checks only.
- Worker-level: `Worker::pair` doc "Pairs without constructing or opening a
  printer" (worker.rs:~196); `Worker::stop`/`join` ("Call only after
  `Event::Stopped` to avoid blocking the GUI").

### desktop

- `cli.rs:1`: terminal-only configuration; **never opens a printer or starts
  fiscal processing**; `console.rs:1-2`; `preferences.rs:1` "credentials remain
  worker-owned"; `telemetry.rs:1` scrubbing; `application/macos.rs:1-6`
  cooperative stop on Cmd+Q.

### Repo-level

(README "Fiscal safety" README.md:166-214, `.omp/RULES.md`,
`.omp/rules/worker-thread.md`)

Verify printer state before every operation; journal fiscal intent before first
fiscal command; never retry on device / never reprint; uncertain outcome blocks
across restarts until a person resolves it; resolving never reissues; printer
I/O only on `Worker` thread; GUI never blocks on it; pair without opening a
printer; never talk to a real printer (use Simulator).

---

## Appendix: quick coupling hot-spot list (by file)

1. `crates/agent/src/agent.rs` — Posnet types in struct fields, `describe()`
   telemetry fill (model/firmware/vat), `run_report` bare-`Device` rule,
   `execute` `Uncertain` rule, `receipt()` builds posnet `Receipt`,
   `DeviceJournal` impls.
2. `crates/agent/src/worker.rs` — `Printer::new/detect` + `Connection{port,
   baud_rate}` from `Settings`, `Link` injection, `Event::{PortDetected,
   Device{Status}}`, `trace_failure` `Uncertain` arm.
3. `crates/agent/src/api/model.rs` — `Telemetry::new` hard-coded Posnet limits,
   `VatCode::of`, `printer_slots`, `VAT_SLOTS`.
4. `crates/agent/src/journal.rs`, `history.rs` — persisted posnet types
   (`Report`, `PaymentForm`, `Payment`, `Grosze`), `stage` strings.
5. `crates/agent/src/settings.rs` — `port`, `baud_rate`, `BAUD_RATES`,
   `DEFAULT_BAUD_RATE` (serial-specific), `deny_unknown_fields`.
6. `crates/agent/build.rs:72` — payment-form type mapped to posnet.
7. `crates/agent/src/lib.rs:33` — `Error::Printer(posnet::Error)`.
8. `crates/desktop/src/{main.rs, application.rs, cli.rs, model.rs,
   model/table.rs}` + `ui/app.slint:124,146,199-203,310-311` —
   `Link`/`SerialLink`/`serial_ports`/`Status`/`zloty`/baud/port UI + CLI flags.
9. `crates/posnet/src/printer.rs` — everything vendor protocol (TEMO const l.14,
   `PRINTER_ZONE` l.19), simulator imports its internals.
