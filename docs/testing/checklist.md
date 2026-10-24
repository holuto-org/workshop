# Feature coverage checklist

These are requirements for changes, not a claim that every cell is already
covered. Read the matching [concern guide](README.md); every fixed bug gets a
regression that fails without it.

## Posnet driver

- **Windows-1250 text and CRC frames**
  - Required guarantee: Published independent byte/checksum vectors; exact
    Polish bytes; no transliteration/truncation; reject unassigned bytes,
    controls, duplicate keys, corrupt/oversized frames
  - Test shape: Unit boundary table + round-trip properties + driver scenario
- **Decimal, quantity and grosze**
  - Required guarantee: Plain unsigned decimals only; range/overflow boundaries;
    half-up line rounding; exact sums
  - Test shape: Unit table + properties against independent integer arithmetic
- **Receipt construction**
  - Required guarantee: Line/name/count limits; per-slot VAT; NIP unchanged;
    full company footer only when needed; backend zero-price omission and agent rejection
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
  - Required guarantee: First write failure sends nothing; later failure is
    uncertain; journal stage precedes each actual command
  - Test shape: Held-command scenario + journal failure injection
- **Operator waits**
  - Required guarantee: Each paper/cover/lever/temperature/power/key state;
    asynchronous status during hold; continuation once; wait expiry/no
    reason/corrupt answer fail safely
  - Test shape: Fast simulator timing + controlled hold/reached, never a real
    ten-minute wait
- **Receipt faults**
  - Required guarantee: Execute then lose/corrupt reply or disconnect at
    first/middle/final/footer stage leaves uncertainty and no retry
  - Test shape: Fault matrix; assert effects and exact command sequence
- **Daily/monthly/periodic reports**
  - Required guarantee: Exact dates/summary parameters; readiness and same
    serial; report `ERR` frame rejection naming the command is certain;
    execution-error/silent/wrong-command failures uncertain
  - Test shape: Fixed-clock snapshot + scenario per classification

## Agent and persistence

- **`/agent` production client**
  - Required guarantee: Correct route/prefix, method,
    bearer/version/language/serial headers and body; nullable fields; wrong
    IDs/attempt/echo rejected
  - Test shape: Loopback Holuto fake request log + exact result/error
- **Session and heartbeat**
  - Required guarantee: Physical serial binding, current VAT/limits, pairing
    conflict; heartbeat when unready; 401 stops without refresh
  - Test shape: Stateful HTTP fake + simulator + worker events
- **Documents and queue**
  - Required guarantee: Limit 100, opaque cursor encoded once,
    next/back/current-page refresh, stable selected identity; failures preserve
    page; valid NIP/document only
  - Test shape: HTTP scenario + worker/UI boundary
- **Claim and execution**
  - Required guarantee: Claim persisted before preflight; no effects for
    invalid/unowned/stale work; unchanged line/amount/payment semantics
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
    evidence; blocked notice identifies job/order and receipt or `nieczytelny`;
    explicit local acknowledgement requires same ready serial, appends history,
    settles; no print or server result
  - Test shape: Fake terminal state + restart + acknowledgement scenario
- **Staff resolution**
  - Required guarantee: Unknown receipt reconciles with Holuto without reissue;
    unreadable number can use staff-entered number; no local acknowledgement
    bypass for ordinary unknown
  - Test shape: Fake staff helper + real persisted state
- **Reports and resolution**
  - Required guarantee: Interrupt blocks across restart; same ready serial
    required to resolve; resolution sends no fiscal command
  - Test shape: Simulator + journal scenario
- **Worker start/retry**
  - Required guarantee: Start network/5xx remains active with notice and 15 s
    retry policy; every 401 stops; no busy-loop retry or device resend
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
- **History**
  - Required guarantee: One entry per job; torn line does not swallow next;
    correct order and display bound; cache error cannot prevent server
    settlement
  - Test shape: File fixture + agent scenario
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
  - Required guarantee: Polish/JSON output and stable exit codes; redacted
    config; settings mutations refuse a running app; metadata/check/status
    never open serial
  - Test shape: Child process commands with isolated state + exact
    JSON/output/exit assertions
- **Autostart**
  - Required guarantee: Opt-in only; exact per-user registration/removal;
    preference starts worker only after full preflight; unresolved journal
    still blocks
  - Test shape: OS-gated temp HOME/registry artefact + worker scenario
- **Document selection and sorting**
  - Required guarantee: Raw-value numeric/date/Polish sorting, ties,
    ascending/descending; refresh retains identity, not row index
  - Test shape: Unit table + real UI selection/refresh
- **Queue confirmation**
  - Required guarantee: Exact document/kind/amount/NIP preview; cancel sends
    nothing; double confirmation consumes action once; readiness/block rechecked
  - Test shape: Real application + fake request log + simulator
- **Report confirmation**
  - Required guarantee: Daily/monthly/periodic wording and parameters; valid
    leap dates; from ≤ to ≤ today; periodic lower bound 2000-01-01
  - Test shape: Injected-today table + accessible dialog scenario
- **Worker event bridge**
  - Required guarantee: `Connected`, `Device`, notices, `HistoryChanged` and
    `Stopped` reach Slint; blocked/stopping disables fiscal actions
  - Test shape: Real worker + Slint event-loop backend
- **GUI responsiveness**
  - Required guarantee: UI can process a marker/action while command held; no
    API/printer work on UI; no join before final event
  - Test shape: Controlled hold + event acknowledgement, not elapsed-time
    assertion
- **Close/reopen/tray**
  - Required guarantee: Close hides only with tray; without tray remains
    visible; recreate preserves selection/sort; worker remains active
  - Test shape: Testing backend application scenario + Linux `session_bus`
- **Stop/quit**
  - Required guarantee: Current print finishes; no second submission; quit
    after `Stopped`/join, not abandoned worker
  - Test shape: End-to-end held-print shutdown scenario
- **Local orphan acknowledgement**
  - Required guarantee: Button only for completed-but-failed/expired conflict;
    exact evidence in confirmation; cancel no effect; confirm no print/server
    write
  - Test shape: Accessible-label scenario + persisted journal/history
- **Sentry**
  - Required guarantee: No DSN/opt-out means no transmission; allowlisted
    errors/logs/breadcrumbs/traces; tags and propagation; no
    secret/PII/host/URL leak
  - Test shape: Capturing transport + malicious synthetic canaries +
    child-process panic/crash test where supported
- **Production features**
  - Required guarantee: Default serial build compiled; simulator/testing
    rejected in release; separate simulator state
  - Test shape: Feature-matrix build check + isolated process fixture
- **Packages**
  - Required guarantee: Workshop
    executable/version/resources/dependencies/replacement metadata;
    mount/extract/install; installed CLI help/check and main window without
    configured printer; debug files match release
  - Test shape: Package check on Windows x64, Linux x64/ARM64, macOS x64/ARM64
