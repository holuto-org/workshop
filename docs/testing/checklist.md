# Feature coverage checklist

These are requirements by changed area, not a claim of complete existing
coverage. Select the guarantees a change can affect, retain their existing
evidence and add or strengthen tests where needed. Every fixed bug gets a
regression that fails without it. An unrelated edit does not require the whole
matrix.

The **Test shape** lines are evidence examples, not a requirement to add every
listed test kind. Use the cheapest consumer boundary that proves the guarantee;
see the [testing guide](README.md) and its concern guides. Exact protocol fields,
limits and invariants belong in the implementing modules and
[fiscal contract](../fiscal-contract.md), not a second specification here.

## Posnet driver

- **Windows-1250 text and CRC frames**
  - Required guarantee: Independently checked byte/checksum vectors; exact
    Polish bytes; no transliteration/truncation; reject unassigned bytes,
    controls, duplicate keys, corrupt/oversized frames
  - Test shape: Unit boundary table + round-trip properties + driver scenario
- **Decimal, quantity and grosze**
  - Required guarantee: Plain unsigned decimals only; range/overflow boundaries;
    half-up line rounding; exact sums
  - Test shape: Unit table + properties against independent integer arithmetic
- **Receipt construction**
  - Required guarantee: Line/name/count limits; per-slot VAT; NIP unchanged;
    full company footer only when needed; incoming zero-price lines rejected
    before fiscal dispatch
  - Test shape: Boundary table + fixed-output snapshot + scenario
- **Advances and payments**
  - Required guarantee: Unique deduction slot; deduction ≤ slot sales; correct
    gross-at-slot field; all payment codes/names; exact coverage; prepaid zero
    close
  - Test shape: Properties + semantic command assertions + reviewed snapshot
- **Serial sessions**
  - Required guarantee: Split/coalesced/noisy frames; token replies;
    wrong-command replies; partial timeout/EOF; no resend after lost reply
  - Test shape: Simulator scenario + Unix pty / Windows `com_pair`
- **Preflight and discovery**
  - Required guarantee: Correct model/serial,
    fiscal/header/mechanism/queue/transaction state and fresh VAT; only
    read-only discovery; no scan while blocked
  - Test shape: Table of simulator status overrides; zero fiscal commands after
    every refusal
- **Clock and receipt number**
  - Required guarantee: Printer issue time, Warsaw DST boundaries, unreadable
    clock before dispatch; unreadable number after print is not permission to
    print again
  - Test shape: Fixed simulator clock + scenario
- **Journal-before-command**
  - Required guarantee: First intent-write failure sends no fiscal command;
    later fiscal-stage failure is uncertain; intent precedes each fiscal send
  - Test shape: Held-command scenario + journal failure injection
- **Operator waits**
  - Required guarantee: Each paper/cover/lever/temperature/power/key state;
    asynchronous status during hold; continuation once; wait expiry/no
    reason/corrupt answer fail safely
  - Test shape: Fast simulator timing + controlled hold/reached synchronization,
    never a real ten-minute wait
- **Receipt faults**
  - Required guarantee: Execute then lose/corrupt reply or disconnect at
    first/middle/final fiscal stage leaves uncertainty and no retry; a footer
    failure after `trend` preserves the printed receipt and stops remaining
    footer commands; a still-open footer blocks the next operation
  - Test shape: Fault matrix; assert effects and exact command sequence
- **Daily/monthly/periodic reports**
  - Required guarantee: Exact dates/summary parameters and brand-specific
    [report date rules](../fiscal-contract.md#report-dates-and-printer-clock);
    readiness (read-only still allows monthly/periodic) and same serial;
    [refusal classification](fiscal-safety.md#recovery-classification);
    silent/wrong-command failures uncertain
  - Test shape: Fixed-clock snapshot + scenario per classification

## Shared fiscal values and native drivers

Apply arithmetic, exact text/VAT/payment, journal-before-send, no retry,
restart-blocking and resolution-without-replay guarantees to **all three**
drivers, using that brand's simulator through the real driver. Exercise supported
receipt paths and prove unsupported ones refuse before dispatch; do not copy
Posnet's capabilities, waits, report dates or completion rules to other brands.

- **Shared `fiscal` domain**
  - Required guarantee: Capability-driven name/line/NIP/quantity limits;
    stable vendor and persisted values; shared serial filtering never equates
    a USB hint with identity
  - Test shape: Boundary/property tables plus OS port-metadata fixtures
- **Novitus**
  - Required guarantee: Exact model/firmware allowlist; PC1, native dialect and
    input/output CP1250; ASCII-only repertoire unless full CP1250 is verified;
    VAT/readiness status decoded without guessing unqualified byte order
  - Test shape: Simulator readback tables; no fiscal frames after refusal
  - Required guarantee: Exact NIP/payment/close and date-guarded report frames;
    advances/zero/footer production gates; operator waits never retransmit;
    late fiscal-stage failures remain uncertain
  - Test shape: Snapshot + command/effect/journal and fault matrices
- **Elzab**
  - Required guarantee: Exact ONLINE identity, CP1250 and complete readiness,
    receipt permission, header/VAT/clock checks; no legacy fallback
  - Test shape: Simulator status/identity tables; no fiscal frames after refusal
  - Required guarantee: Independent CRC vector, fresh matching reply IDs,
    partial/late replies; exact advance/NIP/payment frames; zero/footer production
    gates; paper-out auto-cancel remains uncertain; daily ACK is not completion
  - Test shape: Frame/session unit vectors + driver snapshots/fault matrices
- **Detection and cross-brand recovery**
  - Required guarantee: Hints/remembered vendor obey the
    [discovery policy](../fiscal-contract.md#discovery-and-readiness); PC2
    refusal; no scan while blocked; journal identity binds vendor + serial;
    unknown capabilities never form invalid telemetry or allow a claim
  - Test shape: Mixed simulator candidates + real worker/HTTP/restart matrix
- **Acceptance isolation**
  - Required guarantee: Normal all-feature builds retain production fiscal-mode
    and capability gates; the dedicated debug-only cfg relaxes only the
    [documented acceptance gates](hardware-acceptance.md#build-and-automated-checks-no-hardware),
    not identity, remaining readiness checks or fiscal safety; release rejects it
  - Test shape: Ordinary production-gate tests, separate cfg-gated simulator
    module and negative release build in CI. Physical qualification is
    [human-only evidence](hardware-acceptance.md), never automated tests.

## Agent and persistence

- **`/agent` production client**
  - Required guarantee: Correct route/prefix, method,
    bearer/version/language/serial headers and body; nullable fields; wrong
    IDs/attempt/echo rejected
  - Test shape: Loopback Holuto fake request log + exact result/error
- **Session and heartbeat**
  - Required guarantee: Session vendor + serial binding and current per-profile
    VAT/limits, quantity decimals and receipt capability flags; pairing conflict;
    identity-bound heartbeat when unready but describable; 401 stops without refresh
  - Test shape: Stateful HTTP fake + simulator + worker events
- **Claim and execution**
  - Required guarantee: Received claim persisted before preflight; no fiscal
    effects for invalid/unowned/stale work; unchanged line/amount/payment semantics
  - Test shape: Real journal + fake + simulator
- **Durable result delivery**
  - Required guarantee: Persist before HTTP; identical attempt, receipt number
    and printer issue time on replay; commit-then-drop reconciles without print
  - Test shape: Fake drop-after-commit + restart scenario
- **Recovery lifecycle**
  - Required guarantee: `NotStarted` + taken → accepted failed without block;
    `NotStarted` + server unknown retains evidence for staff resolution;
    `Dispatching` → unknown/block; completed evidence preserved; attempt/serial
    mismatch fails closed
  - Test shape: State/command table + generated model + real restarts
- **Orphaned completed receipt**
  - Required guarantee: Server failed/expired never discards completed
    evidence; blocked notice identifies job/order and receipt or unreadable number;
    explicit local acknowledgement requires the same vendor/serial under
    [resolution readiness](fiscal-safety.md#recovery-classification) and settles;
    no print or server result
  - Test shape: Fake terminal state + restart + acknowledgement scenario
- **Staff resolution**
  - Required guarantee: Unknown receipt reconciles with Holuto without reissue;
    unreadable number can use staff-entered number; no local acknowledgement
    bypass for ordinary unknown
  - Test shape: Fake staff helper + real persisted state
- **Reports and resolution**
  - Required guarantee: Interrupted dispatch blocks across restart; resolution
    requires the same vendor/serial under
    [resolution readiness](fiscal-safety.md#recovery-classification), not full sales
    readiness; resolution sends no fiscal command
  - Test shape: Simulator + journal scenario
- **Worker start/retry**
  - Required guarantee: Start printer detection/network/403/408/429/5xx
    failures remain active with notice and 15 s retry policy; every 401 stops;
    no busy-loop retry or device resend
  - Test shape: Production worker + scripted fake; scheduling policy tested
    without sleeps
- **Worker serialization/stop**
  - Required guarantee: All printer/Holuto work on one worker; queued
    receipts/reports never interleave; stop finishes current work then emits
    final `Stopped`
  - Test shape: Held command + channels + real `Worker::spawn`
- **Settings**
  - Required guarantee: Exact current save/load and validation; diagnostics on
    and autostart off for fresh settings; private token/debug/errors
  - Test shape: Current-format literal fixture + table + filesystem/security
    scenario
- **Operation journal**
  - Required guarantee: Current receipt/report variants retain meaning;
    corrupt/unreadable files block; atomic durable replacement and retained
    evidence on failure
  - Test shape: Current-format golden fixtures + filesystem failure/restart
    tests on every OS
- **State directory/instance lock**
  - Required guarantee: OS `workshop` paths, Unix 0700/0600, safe replacement;
    one process per directory
  - Test shape: Platform filesystem tests + child-process lock contenders
- **HTTP security/limits**
  - Required guarantee: HTTPS except genuine loopback; OS trust roots; no
    redirect or credential escape; proxy behavior explicit; bounded
    malformed/error bodies
  - Test shape: URL table + local TLS/redirect/proxy fake + child process

## Desktop and release boundary

- **Save/start/settings**
  - Required guarantee: Real callbacks validate/save; explicit start unless
    autostart opted in; running settings cannot silently change worker;
    telemetry opt-out persists
  - Test shape: Slint accessible-label scenario + real files
- **Pairing**
  - Required guarantee: One-time code exchanged through production HTTP;
    invalid/expired/used/throttled errors; credentials never displayed; failed
    exchange does not replace settings
  - Test shape: Loopback fake + CLI/UI + stored-state assertions
- **CLI**
  - Required guarantee: Documented human/JSON output and stable exit codes;
    redacted config; settings mutations refuse a running app; CLI commands
    never open serial, including pairing
  - Test shape: Child process commands with isolated state + exact
    JSON/output/exit assertions
- **Autostart**
  - Required guarantee: Opt-in only; exact per-user registration/removal;
    preference may start processing, but never bypasses preflight or an
    unresolved journal
  - Test shape: OS-gated temp HOME/registry artefact + worker scenario
- **Status view**
  - Required guarantee: Read-only rows show processing, Holuto connection,
    printer vendor/unique number/port and ready/not-ready/blocked state from
    worker events; no control queues or prints a receipt
  - Test shape: Real application with accessible labels + real worker
- **Report confirmation**
  - Required guarantee: Daily/monthly/periodic wording and parameters; valid
    leap dates; ordered periodic dates and lower bound 2000-01-01; UI date
    validation is not the driver's
    [brand-specific date guard](../fiscal-contract.md#report-dates-and-printer-clock)
  - Test shape: Injected-today table + accessible dialog scenario
- **Worker event bridge**
  - Required guarantee: Worker connection, device, problem, completion and stop
    events reach Slint; blocked/stopping disables fiscal actions
  - Test shape: Real worker + Slint event-loop backend
- **GUI responsiveness**
  - Required guarantee: UI can process a marker/action while command held; no
    API/printer work on UI; no join before final event
  - Test shape: Controlled Posnet hold + event acknowledgement, not elapsed-time
    assertion
- **Close/reopen/tray**
  - Required guarantee: Close hides only while a host shows this app's icon;
    otherwise remains visible; recreate restores displayed state; worker remains
    active
  - Test shape: Testing backend application scenario + Linux `session_bus`
- **Stop/quit**
  - Required guarantee: Current print finishes; no second submission; quit
    after `Stopped`/join, not abandoned worker
  - Test shape: End-to-end held-print shutdown scenario
- **Local orphan acknowledgement**
  - Required guarantee: Button only for completed-but-failed/expired conflict;
    exact evidence in confirmation; cancel no effect; confirm no print/server
    write
  - Test shape: Accessible-label scenario + persisted journal
- **Sentry**
  - Required guarantee: No DSN/opt-out means no transmission; structured
    errors/logs/breadcrumbs/traces follow the allowlist, tags and propagation
    policy without secret/PII/host/URL leaks.
    [Native minidumps have a separate privacy boundary](../telemetry.md#native-minidumps-important-exception),
    not a guarantee of scrubbed process memory.
  - Test shape: Capturing transport + malicious synthetic canaries +
    child-process panic/crash test where supported
- **Production features**
  - Required guarantee: Default serial build compiled; desktop `simulator` and
    agent `testing` rejected without debug assertions; separate simulator state
  - Test shape: Feature-matrix build check + isolated process fixture
- **Packages**
  - Required guarantee: Workshop
    executable/version/resources/dependencies;
    mount/extract/install; installed CLI help/check and main window with isolated,
    unconfigured state and no connected printer; debug files match release
  - Test shape: Package check on Windows x64, Linux x64/ARM64, macOS x64/ARM64
