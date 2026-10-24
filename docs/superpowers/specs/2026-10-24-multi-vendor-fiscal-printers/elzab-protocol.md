# ELZAB native ElzabSTX driver specification for Workshop

## Sources

Aliases cite extracted-text line numbers, not PDF pages. Download the
[official programmer archive](https://www.elzab.com.pl/ftp/kody/om_iprg.zip)
as `om_iprg.zip`; extract with `unzip -O CP852 om_iprg.zip -d om_iprg`.
For each PDF member below, run `pdftotext -layout <pdf> <txt>`; `<pdf>` is
`om_iprg/` plus the exact member path, and `<txt>` is the listed text file.
All B/P/BE/PE/E/O/A URLs are the programmer archive linked above.

- **B**: *Podstawy protokołu ElzabSTX*, edition 0.17, 12.01.2024 (B:6).
  Member: `POLISH/ElzabSTX/Podstawy protokołu ElzabSTX.pdf`.
  Text: `elzab-Podstawy_protokołu_ElzabSTX.txt`.
- **P**: *Protokół ElzabSTX dla drukarek fiskalnych*, edition 0.36, 19.12.2025
  (P:6-16); primary command specification.
  Member: `POLISH/ElzabSTX/Protokół ElzabSTX dla drukarek fiskalnych.pdf`.
  Text: `elzab-Protokół_ElzabSTX_dla_drukarek_fiskalnych.txt`.
- **BE**: *Basics of ElzabSTX communications protocol*, release 0.16,
  29.11.2021.
  Member: `ENGLISH/ElzabSTX/Basics of ElzabSTX communications protocol.pdf`.
  Text: `elzab-Basics_of_ElzabSTX_communications_protocol.txt`.
- **PE**: *ElzabSTX Communications Protocol for ELZAB D10 and ELZAB ZETA
  Printers*, release 0.13, 14.12.2018; older D10/Zeta English cross-check, not
  an override of online extensions.
  Member: `ENGLISH/ElzabSTX/ElzabSTX Communications Protocol for ELZAB D10 and
  ELZAB ZETA Printers.pdf`.
  Text:
  `elzab-ElzabSTX_Communications_Protocol_for_ELZAB_D10_and_ELZAB_ZETA_Printers.txt`.
- **E**: *Lista błędów protokołu ElzabSTX - DRUKARKI*, no edition stated in the
  extracted text.
  Member: `POLISH/ElzabSTX/Lista błędów protokołu  ElzabSTX - DRUKARKI.pdf`.
  Text: `elzab-Lista_błędów_protokołu__ElzabSTX_-_DRUKARKI.txt`.
- **O**: *Obsługa protokołu transmisji w drukarkach online*, no edition stated
  in the extracted text.
  Member: `POLISH/ElzabSTX/Drukarki online - obsługa protokołu.pdf`.
  Text: `elzab-Drukarki_online_-_obsługa_protokołu.txt`.
- **A**: *Zalety protokołów komunikacyjnych typu ElzabSTX*, no edition stated in
  the extracted text.
  Member: `POLISH/ElzabSTX/Zalety protokołów komunikacyjnych typu ElzabSTX.pdf`.
  Text: `elzab-Zalety_protokołów_komunikacyjnych_typu_ElzabSTX.txt`.
- **H**: [official DLL/SDK
  archive](https://www.elzab.com.pl/ftp/kody/elzabdr.zip),
  *ElzabDR* C header version 6.107.4 (H:1-4).
  Extract with `unzip elzabdr.zip -d elzabdr`; member `elzabdr.h`, cited
  directly without text conversion. Supplementary evidence only, not a
  dependency proposal or an exact native-wire specification.

## Scope, evidence, and notation

This is a read-only protocol investigation, not a claim that a driver or
hardware compatibility matrix has been implemented. No real printer was
contacted. Target: the receipt/report/status capabilities currently used by
Workshop, including buyer NIP, per-VAT advance settlement, the five payment
forms, company text when absent from the header, receipt-number evidence, and
non-replaying recovery. Current equivalents:
`crates/posnet/src/printer.rs:217-259,262-319` and
`crates/posnet/src/receipt.rs:344-411`; integration requirements are mapped in
`posnet-coupling.md:57-85`. Worker-thread confinement, preflight,
durable intent, no retries/reprints, and human resolution remain requirements,
not vendor-dependent options (`README.md:166-211`; `.omp/RULES.md:3-22`).

`||` means byte concatenation, never literal pipe bytes; quoted strings are
literal ASCII bytes, placeholders are encoded field values; `ε` means zero
bytes. `STX=02`, `ETX=03`, `FS=1C`, `GS=1D`, `RS=1E`, `US=1F` hexadecimal.
Decimal point is **comma**, byte `2C`, in native monetary/quantity values
(B:69-80; P:289-298). Command names are case sensitive (B:118). No TAB, line
terminator, or NUL is appended. The framing grammar has no such suffix (B:128);
commands are data blocks, not escape sequences (P:277-280).

Labels: **[DERIVED]** is an explicit engineering conclusion from cited
semantics; **[UNVERIFIED]** means the supplied sources do not establish the
behavior, value, or guarantee. All proposed driver policies are distinguished
from printer guarantees.

## 1. Transport and correlation

### 1.1 Exact frame format

```text
STX || header || FS || data || crc_ascii4 || ETX
header = command_id || GS || receiver_status || GS || frame_no || GS ||
max_frame_no || GS || transformation
```

Display line breaks above are formatting only; concatenate the continued
syntax with a single space at each line break; no wire newline is implied.

Optional `GS || direction || GS || sender` exists in the transport grammar, but
B says to omit those fields (and their separators) when the device manual does
not define them. Workshop should omit them: **[DERIVED]** from B:256-274 and the
printer header rules P:281-287. Fields may be empty to select defaults; trailing
GS separators may be removed (B:300-332).

- **Header field — `command_id`**: **Request value / validation**: ASCII decimal
  0..999999999, at most 9 characters; default `1`; **Reply**: Same ID as
  request; this, not a command-name echo, correlates the answer.
  B:156-165,296-298.
- **Header field — `receiver_status`**: **Request value / validation**: Default
  `ALL`; printer accepts own model number, subgroup, group, `ALL`, or empty.
  B:181-191; P:281-287.; **Reply**: Transmission status, default `SA`
  (B:193-196).
- **Header field — `frame_no`**: **Request value / validation**: ASCII decimal
  1..999999999; default 1.; **Reply**: Same grammar; multipart order established
  by number. B:198-205,276-286.
- **Header field — `max_frame_no`**: **Request value / validation**: ASCII
  decimal 0..999999999; default 1; printer-specific maximum also applies.;
  **Reply**: When initially unknown, 0 until final frame, then final frame
  number. B:207-229.
- **Header field — transformation**: **Request value / validation**: `NDT`,
  default, supported by every device; **Reply**: No transform; leave raw encoded
  data. B:234-254.

A compact single-frame request is therefore
`02 || ascii(id) || 1C || data || ascii4(crc) || 03`. B:165-169 recommends
changing ID for every command, practically by incrementing it; use that within a
connection and exactly one fiscal command outstanding (**driver policy**, not a
documented global/persistent ID requirement). ID wrap/restart has no documented
replay safety. Max frame length is **1024 bytes including framing**; STX/ETX
cannot occur in header/data, FS cannot occur in header. Bad shape → `SE`,
overlength → `SB` (B:128-139). Use bounded buffers; calculate byte length after
character encoding. A data-less frame `STX header FS CRC ETX` receives no answer
(B:141-145); it is also the natural shape of a successful no-data reply. Never
acknowledge such replies with another frame (**[DERIVED]**).

### 1.2 CRC, including a reproducible vector

CRC covers **header, FS, data**, excluding STX, the four CRC characters and ETX.
Initialize 0; polynomial `x^16+x^15+x^2+1`, CRC-16/ARC; output four hexadecimal
ASCII digits, most-significant nibble first (B:342-362; BE:345-357). Bitwise
implementation: for each input byte `crc ^= byte`, eight times
`crc = (crc >> 1) ^ (0xA001 if old_lsb else 0)`, no final XOR (**[DERIVED]**
from named CRC-16/ARC). Accept either hex case; hexadecimal numbers are
case-insensitive (B:97-99).

The supplied documents contain **no worked full-frame CRC vector found**; a
vendor-published numerical example is **[UNVERIFIED]**. Here is a calculated
worked example using the manual's exact `LOR` example payload (P:3626-3639), not
a falsely attributed vendor vector:

```text
compact header "1", FS, payload "LOR"
CRC input bytes: 31 1C 4C 4F 52
running CRC:     D4C1 5914 FA58 0EBA 4E0E
frame bytes:    02 31 1C 4C 4F 52 34 45 30 45 03
```

These numbers were computed offline using the bitwise algorithm above. They need
an independent implementation/vendor-vector cross-check before production.
Likewise a compact `XTR` identity request with ID 1 calculates CRC `BA44` and
bytes `02 31 1C 58 54 52 42 41 34 34 03` (**calculated**, not an observed device
exchange).

### 1.3 ACK/NAK, errors and retransmission: no exactly-once guarantee

**Do not import ElzabESC's single-byte ACK/NAK protocol.** ElzabSTX returns
framed transmission statuses and precise command errors; the vendor contrasts
this with ESC's ACK/NAK behavior (A:34-40,148-164). Status values: `SA` good
transmission; `SB` length; `SC` malformed CRC field; `SE` malformed frame; `SF`
ID; `SG` frame number; `SH` maximum frame number; `SI` frame > max; `SJ`
transform; `SK` too many frames; `SL` temporarily cannot accept command; `SM`
direction; `SN` sender. A wrong CRC **value produces no answer**, not a NAK
(B:357-390).

B says retransmission is possible (B:119), but does **not** prescribe an
automatic retry schedule, ACK timer, retry count, cached-result lifetime, or
post-execution replay suppression. Documented deduplication is only assembly:
frames sharing ID belong to one message; matching
`(command_id, frame_no, max_frame_no)` replaces the previously saved frame;
execute only after all frames are received; a different ID discards an
**incomplete** prior assembly (B:276-298; BE:224-227,276-288). This does **not**
establish that retransmitting a completed one-frame command cannot execute it
twice. Persistent deduplication across disconnect/power loss is
**[UNVERIFIED]**.

**Required Workshop policy:** no retransmission of any fiscal command, even with
the same ID. Do not use an SDK transport whose hidden retries are unknown. On
timeout, malformed/corrupt/missing answer or connection loss after dispatch,
retain unknown outcome and block. A fresh status query is a different read-only
operation, not a replay. No new ID rescues an uncertain fiscal command. Do not
send a status query until the original request frame has been fully written: a
new ID can discard incomplete assembly (B:288-291). Ability to process a
read-only query while an earlier command is executing is **[UNVERIFIED]**; see
§5.

### 1.4 Serial/USB configuration and timeouts

- Legacy protocol selector `ELZABSTX` uses **8 data bits, NONE parity, 1 stop
  bit**; `ELZAB` (ESC) uses **8,E,1**. The English source agrees (P:3237-3248;
  PE:1976-1987). Never choose EVEN merely because the brand is ELZAB.
- Online selector spelling is `STX`, not `ELZABSTX` (P:3249-3254). Modern serial
  interface settings are configurable: speed 9600/19200/38400/57600/115200,
  parity 0 NONE/1 EVEN/2 ODD, data bits 7..8, stop bits 1..2 (P:9007-9040).
  Legacy list additionally includes 28800 (P:3221-3228). Thus “all online
  printers must be 8N1 regardless of configuration” is **[UNVERIFIED]**; the
  modern explicit settings are authoritative. Start with an operator-configured
  STX/8N1 profile, use read-only identity, and never rewrite printer
  communication settings during discovery (**driver policy**).
- Online serial parameters can be read through
  `xHR GS 'z' 'A'<interface_no> RS 'F'`; response is
  `ε GS 'z' 'A'<interface_no> RS 'F' 'B'<speed> US 'C'<parity> US 'D'<data_bits>
  US 'E'<stop_bits>`
  (P:9007-9040,10448-10456,10466-10474). This query requires already working
  communication; it cannot bootstrap an unknown parity/baud.
- USB presence/settings exist (`XHR GS 's'` / `XHR GS 't'`,
  P:3258-3270,6790-6806,7150-7158). Online interfaces can carry a protocol
  service independent of socket settings (P:9630-9637). These sources do **not**
  establish CDC ACM vs vendor serial driver, VID/PID, Linux/Windows/macOS
  enumeration, DTR/RTS requirements, whether USB ignores baud/parity, or a
  shared factory speed: **[UNVERIFIED]**. Do not assume USB is raw libusb or
  that RNDIS Ethernet-over-USB is a serial channel; RNDIS is separately
  described at P:9068-9074.
- Configurable transmission timeout has active bit (`XHR GS 'x'`, subfield byte
  `20h` + bit value) and seconds (`XHR GS 'y'`, subfield `'A'` + integer 1..10),
  P:3170-3180,7162-7166. It is not a documented execution deadline. Vendor
  explanation says STX has no command-frame timeout and ETX completes the frame
  (A:186-188), so actual interaction between this configurable setting and
  firmware behavior is **[UNVERIFIED]**.
- No command-specific response deadlines, interbyte timeout, fiscal-completion
  deadline or allowed polling cadence found. H exposes connection timeout
  default 10 and caller-selected timeouts, not printer completion guarantees
  (H:124,900-914,951-968). Use explicit host deadlines and fail unknown after
  fiscal dispatch; a proposed 10-minute operator wait is inherited app policy,
  not an ELZAB guarantee (`README.md:174-179`).

## 2. Common command/reply decoder and identity

Command data is `command_desc { GS descriptor+content }`;
fields/subfields/elements are normally mandatory unless marked optional,
ordering does not matter and duplicates are forbidden except where a command
explicitly permits them (P:3367-3393). Successful answer data is
`ε { GS field }`, or just `ε`; error answer is `error_desc [ GS error_info ]`,
usually an ASCII decimal error code (P:3395-3425). `'0'` and `ε` both mean no
error; exception: YER's T field requires literal `'0'`, not empty, for no-error
text lookup (P:1373-1381). Validate transport status and CRC before interpreting
command success; `SA` means transport success, **not fiscal success**. Replies
normally do not echo the command name: retain ID→command state (B:296-298;
P:3397-3410).

### Identity queries, all read-only

- **Request data — `XTR`**: **Successful reply data**: `ε GS 'B'<model_name> GS
  'C'<model_no> GS 'D'<firmware_name> GS 'E'<firmware_no> GS 'F'<controllers> GS
  'G'<group> GS 'H'<subgroup> GS 'I'<additional_ids> GS 'J'<FMCU_info> GS
  'K'<IMCU_info> GS 'L'<modifiable_model_name> GS
  'M'<modifiable_firmware_name>`; **Use**: Detect native ELZAB and publish
  actual B/D fields. Do not use writable L/M identity aliases. P:6675-6692.
- **Request data — `FNR`**: **Successful reply data**: `ε GS 'B'<unique_no>`;
  **Use**: Fiscal unique number to pin sessions/journal/backend identity; 1..14
  printable bytes (P:477,3255-3257,6363-6377).
- **Request data — `YNR`**: **Successful reply data**: Readable device number,
  but it has a matching `YNW` setter; **Use**: **Not** a substitute for unique
  fiscal identity; P:7349-7354,7401-7406.
- **Request data — `ySR GS 'R2'`**: **Successful reply data**: `ε GS 'S' 'A2' RS
  'B'<hex_status>`; **Use**: Online training / read-write mode and transaction
  flags; §3. P:9737-9742,10056-10059,10583-10610.

Model constants in the primary manual: D10 149/`ELZAB D10`, Zeta
150/`ELZAB ZETA`, Mera+ 160/`ELZAB MERA+`, Cube 172/`ELZAB CUBE online`, DF6
174/`ELZAB DF6 online`, K2 Pay online 176; Sigma 159 or 261; Infis 279; Omega
282 (P:323-363). **These are not a complete, proven “all current online models”
compatibility matrix.** The manual explicitly mentions Mera Online v2 and Cube
Online v2 (P:9140,9445) and H mentions Mera Online v1/v2 (H:587-596), but exact
XTR IDs/names for Mera Online and T10 Online, and whether D10/Zeta marketing
names denote online variants, are **[UNVERIFIED]**. Never identify “online”
solely from a reused model number or substring; require a verified
model/firmware profile plus working online status/capabilities.

Online status package 2: bit 3 means training, bit 2 means write/read
(P:10056-10059). **[DERIVED]** fiscal-ready policy requires training=0 and
write/read=1, plus unique number and all other preflight checks. Bit2=0 is the
candidate read-only indicator; explicit inverse truth table and distinction
between all non-fiscal states need hardware/vendor confirmation. E:64 defines
error 228 “fiscal memory in read-only mode”; do not discover that by attempting
a sale. No separate universally documented fiscalized boolean was found. Do not
auto-fiscalize, leave training mode, or change read-only mode.

## 3. Preflight state in the actual printing session

Read fresh XTR, FNR, ySR, header, VAT and clock; compare unique identity and the
expected complete VAT table to the values used to validate the job. Run before
each receipt/report, not only at discovery. Block if another
transaction/document/interface resource is open; never cancel/adopt an orphaned
receipt automatically (`README.md:168-211`). ELZAB additionally offers **`LPC`**
(no fields, default receipt) or `LPC GS 'A1'`; successful envelope is
`ε GS 'B'<error_desc>`, where inner `B=0`/empty means allowed. Inspect the
nested error even when the outer reply is successful (P:3509-3524; receipt=1 at
P:2488-2491). Use this read-only check before receipt LOR, not as a report
availability check.

### 3.1 Online ySR layout and important bits

Request `ySR` for all, or `ySR GS 'R1'`, `'R2'`, `'R3'` for one. Answer is
`ε { GS 'S' 'A'<package_no> RS 'B'<hex_integer> }`; package numbers 1..3
(P:8807,9737-9745,10583-10610). Packages 1/2 describe bits 127..0, package 3
bits31..0 (P:9746-9900,9901-10063). Parse as hexadecimal integers with LSB
numbered 0; do not truncate packages 1/2 into the SDK's legacy 32-bit
`ReadStatusBytesSTX` type (H:414-423). Exact fixed-width/leading-zero
requirements for online package 1/2 are **[UNVERIFIED]**; manual says hex
integer, not exact length (P:9743-9745). A pair of u64s or 16-byte fixed buffer
suffices, without requiring heap big integers (**driver design**).

- **Package — 1**: **Bits**: 123,122; **Documented meaning and suggested
  readiness handling**: RAM erased / forced RAM erase → block/service.
  P:9753-9754.
- **Package — 1**: **Bits**: 115; **Documented meaning and suggested readiness
  handling**: Shutdown requested → block. P:9761.
- **Package — 1**: **Bits**: 103,102; **Documented meaning and suggested
  readiness handling**: External power disconnected / battery low → display,
  conservative operation policy; not intrinsically equivalent to a
  completed/failed receipt. P:9781-9782.
- **Package — 1**: **Bits**: 98; **Documented meaning and suggested readiness
  handling**: Internet available; do not equate offline with unfiscalized.
  P:9786.
- **Package — 2**: **Bits**: 122,118,115; **Documented meaning and suggested
  readiness handling**: Main accumulator low; fewer than 30 fiscal daily reports
  free; overdue daily report. P:9909-9924. Warn/block per operation, do not
  block the daily report merely because it is overdue (**policy**).
- **Package — 2**: **Bits**: 102,101,100,99,98; **Documented meaning and
  suggested readiness handling**: Battery voltage good; customer display absent;
  fiscal memory absent; service jumper; correctly initialized with manufacturer
  server/final certificates allocated. P:9937-9942. Require the necessary
  positive readiness facts; reject absent memory/display, service state
  (**policy**).
- **Package — 2**: **Bits**: 90,87; **Documented meaning and suggested readiness
  handling**: Restricted operation/no selling; sales blocked, e.g. missing
  display. P:9950-9954.
- **Package — 2**: **Bits**: 82,81; **Documented meaning and suggested readiness
  handling**: User-interrupted print; print buffer occupied. P:9959-9960. Block
  new op; useful operator-wait clues.
- **Package — 2**: **Bits**: 80,79,78,77; **Documented meaning and suggested
  readiness handling**: Monthly reminder; daily reminder; currency change
  pending; shutting down. P:9961-9964. Do not treat reminder-only flags as proof
  a fiscal command failed.
- **Package — 2**: **Bits**: 74,73,72,71; **Documented meaning and suggested
  readiness handling**: Last transaction ended correctly; goods-control database
  full; goods blocked in receipt; receipt canceled. P:9976-9979. Useful
  evidence, **not** a job ID.
- **Package — 2**: **Bits**: 70,69,68; **Documented meaning and suggested
  readiness handling**: Invoice open; receipt buffer nearly full; receipt open.
  P:9980-9982.
- **Package — 2**: **Bits**: 67,66,65,64; **Documented meaning and suggested
  readiness handling**: Menu running; control print open; super-template open;
  long test active. P:9983-9986. Block new operation.
- **Package — 2**: **Bits**: 8,3,2,0; **Documented meaning and suggested
  readiness handling**: Repository transmission deadline exceeded; training;
  write/read; manufacturer not entered. P:10050-10059.
- **Package — 3**: **Bits**: 6,5,4,3,2,1,0; **Documented meaning and suggested
  readiness handling**: Near paper end (also set when empty); mechanism healthy;
  print head raised; controller holds a document interrupted by paper-out;
  mechanism fault; paper absent; printing. P:10097-10105. Require healthy=1 and
  no raised-head/fault/no-paper/printing/interrupted-document state before
  starting.

No separate online cover-open bit is documented here: raised head (3:4) is the
observable mechanism condition, not proof every cover switch is exposed. No
Posnet-equivalent explicit queue length or other-interface ownership field was
found. Package2:81 and open-document bits plus LPC are the available checks;
error483 is “transaction opened on another interface”, 484 is “not permitted to
open receipt from this interface” (E:155-157). Do not claim a full
queue/resource-ownership equivalence without vendor confirmation.

Uppercase **YSR is not lowercase ySR**: legacy YSR returns six 32-bit packages,
online ySR returns different package definitions
(P:472-473,2775-2783,8257-8274,9737-9745). Legacy mechanism is YSR package2
bits0..4/8/9; legacy transaction is package4 bits8/10/11/14/15, training is
package6 bit3 (P:2849-2859,2928-2936,2987-3025). Never reuse legacy bit indices
for ySR. Some protected-memory search instructions explicitly still reference
YSR package1 bit20; preserve that documented distinction (§8).

### 3.2 Header (including company-name suppression)

Read `YYR GS 'J'` →
`ε GS 'J' 'A'<max_name_line_no> RS 'B'<max_total_name_length> RS
'C'<street_limit> ... RS 'I'<cpu_id_limit>`
(P:10150-10163,10545-10581). Then for i=1..A request `yHR GS 'v' 'a'<i>` →
`ε GS 'v' 'a'<i> RS 'b'<name_text> [RS 'C'<wide>] [RS 'D'<high>] [RS
'E'<center>]`
(P:9320-9359,10493-10517). Read all configured name lines, not just first
non-empty; Unicode-aware case-insensitive substring comparison for company
suppression is **app policy**, not printer normalization.

`yHR GS 'w'` → `ε GS 'w'<header_addr_def>`; exact address content is
`'A' 'c'<street> RS 'd'<house> RS 'e'<premises> [font fields] GS 'B'
'f'<locality> RS 'g'<postal_code> [font fields] GS 'C' 'h'<post_office> [font
fields]`
(P:9289-9302). This contains embedded GS sections; a decoder cannot blindly
assume every top-level GS has a globally unique descriptor independent of
context. `yHR GS 'x'` →
`ε GS 'x' 'i'<nip_line> [RS 'C'<wide>] [RS 'D'<high>] [RS 'E'<center>]`
(P:9360-9367,10504-10514). Check the actual taxpayer header/NIP is
readable/non-empty and LPC permits sale; explicit online “header programmed”
status flag/validation rule is **[UNVERIFIED]**. Never rewrite the permanent
header to add company text.

### 3.3 VAT read and clock

- Request **`FTR`** →
  `ε GS 'A' 'A'<vatA> RS 'B'<vatB> RS 'C'<vatC> RS 'D'<vatD> RS 'E'<vatE> RS
  'F'<vatF> RS 'G'<vatG>`
  (P:3121-3129,6355-6361). Active percentage has exactly 2 fractional digits and
  comma; `'='` means exempt (`zw`), `'?'` means unused/inactive (P:593-605,475).
  Native tax **symbols A..G**, native numeric slot identifiers elsewhere
  **1..7**. For backend zero-based `vatSlot`, use **policy mapping A→0 … G→6**;
  do not send backend 0 as a native tax letter/number. Preserve the whole fresh
  table and compare before dispatch; no highest-rate fallback.
- Request **`XCR`** → `ε GS 'A'<YYMMDDhhmmss>` (P:6548-6554; exact 12-digit
  format P:517-525). Read immediately before LOR and persist as issue time using
  app's `Europe/Warsaw` policy (current source equivalent
  `posnet-coupling.md:64-66`). Device timezone/DST flag, century pivot
  and validity range are **[UNVERIFIED]**; restrict accepted civil dates to
  supported app range rather than guessing a 19xx/20xx pivot. No PC-clock
  substitution when unreadable.

## 4. Receipt commands, validation and completion

### 4.1 Required transaction flow

Preflight and validate **all** data/encoding/totals before first fiscal byte.
Persist intent before each command. Native core flow is `LOR` → optional `LNP` →
one `LIT` per line → one online `LDS` advance per VAT slot → nonzero `LBP`
payments → `LCL`. Where company footer is required, LAL exists, but its precise
placement/deferred-print behavior relative to LCL must be confirmed (§4.6).
Command definitions cited below establish individual actions, not an atomic
multi-command transaction. Default success for these write commands is empty
data with successful transport status and matching ID (P:3397-3406).

### 4.2 Open and NIP

- Old D10/Zeta: **`LOR`** (no fields; always line-by-line). Mera+/newer online:
  **`LOR GS 'C0'`** explicitly selects line-by-line instead of inheriting global
  block mode (P:3583-3620; mode values P:2506-2510). Online v2 optionally has
  `GS 'Z'<electronic_only>` and `GS 'Y'<QR_graphic>`; omit both for
  paper+electronic and no eReceipt QR (P:3597-3624). Do not send unsupported
  C/Z/Y to legacy D10/Zeta. Missing explicit online firmware→LOR profile mapping
  is **[UNVERIFIED]**.
- Buyer NIP: **`LNP GS 'O' 'A'<buyer_nip>`**, optional `RS 'B'<extra_line>`
  (P:4578-4589). Each is 1..line_max_len printable characters (P:2381-2386).
  Proposed position immediately after LOR matches Workshop's semantic flow, but
  LNP's exact allowed timing relative to first item is **[UNVERIFIED]** in the
  supplied command section; verify before enablement. Do not use permanent
  header NIP setters or invoice buyer-data commands for receipt NIP.

### 4.3 Sale item

```text
LIT || GS || 'G' || 'A' || name
    || RS || 'B' || vat_letter
    || RS || 'C' || quantity
    || RS || 'D' || unit_price
    || RS || 'E' || line_total
```

The first `G` is the item-field descriptor; `A/B/C/D/E` are subfield
descriptors. All five are required. Defaults omit `F` (normal sale, not
correction). Optional unit name G, description H and discount I are not required
by the app; online/newer also has J (exclude from later discounts) and pharmacy
K, neither should be sent for normal Workshop sales (P:3804-3848). Multiple GS G
item fields are permitted, but **one item per frame/command is the proposed
driver policy**, simplifying partial-failure evidence.

Document example payload is
`LIT GS 'GA' "KIWI" RS 'BC' RS 'C' "1,350" RS 'D' "4,00" RS 'E' "5,40"`
(P:3855-3888). VAT letter C means slot C, not “VAT rate C percent”. Unit
price/total have exactly 2 fractional digits (P:460-461,564-568,759-785);
quantity has up to **5** fractional digits (P:420-421,781-785), unlike Posnet's
larger local capability. Do not round unsupported quantities; reject before LOR.
Max magnitude of quantity, price, line total and receipt total is
**[UNVERIFIED]** here; error446/447 report exceeding item/receipt limits
(E:120-121), not numeric limits. Backend limit validation cannot safely inherit
Posnet's maximum.

Names are 1..line_max_len. Manual says the printer transforms them by uppercase,
replacing non-letter/digit/comma/dot/slashes with spaces, collapsing repeated
spaces (P:765-775). Whether this transforms the **printed** name or only
control-database comparison is **[UNVERIFIED]**. This matters to Workshop's
full, unchanged-name contract: do not silently transliterate/rewrite locally;
qualify the firmware's actual print behavior.

If host and device computed line totals differ, response is
**`464 GS <device_computed_total>`**, not success (P:3491-3496,3850-3853;
E:132-133). Device rounding rule/tie behavior not established: maintain
exact-decimal local arithmetic but qualify parity against model-specific
examples before enabling arbitrary fractional quantities.

### 4.4 Advance settlement per VAT rate (not ordinary discount/storno)

Exact online payload for an amount advance on slot letter `S`:

```text
LDS || GS || 'I' || 'A' || 'A1' || US || 'B' || amount
    || US || 'D' || "Zaliczka"
    || RS || 'C' || S
    || RS || 'D'
```

Thus two consecutive A descriptors are intentional (`I` field → `A` discount
subfield → `A1` discount/not-surcharge element). Final bare RS D marks **advance
settlement**, not a description. Amount is two-decimal comma currency. Mandatory
conditions: amount discount A1, description D supplied, slot C supplied, cancel
B absent or B1 (grant); omit B (P:4135-4166; discount grammar
P:1250-1268,1278-1284). Driver validates positive amount and ≤sales in that
slot, one deduction per backend slot, as existing app contract
(`README.md:286-301`). Error1000 means above slot sales,1001 zero deduction
(E:229-231). No original slot-sales amount is sent as in Posnet tradvance; LDS
addresses the accumulated sold positions in that slot (P:4151-4154).

**Important:** online cannot implement advances through negative/storno item
lines; the vendor explicitly forbids that and identifies the separate STX
settlement command (O:15-21,109-112). SDK confirms an online-only advance flag
in subtotal settlement API (H:1391-1418). Item-specific
`SettleAdvance(productName,...)` is a different API and is not a reason to
invent a different native syntax (H:2076-2088).

### 4.5 Payments and close

```text
LBP || GS || 'K' || 'A' || payment_number
    || RS || 'B' || payment_name
    || RS || 'C' || amount
    || RS || 'X' || online_payment_kind
```

`payment_number` is **1..7** (P:303-304,822-826), an ordinal payment entry,
**not** the form kind. Use distinct entries 1..5 for the five app forms,
two-decimal amounts, names 1..(main_line_len−22) (P:819-826). Default `D`
omitted means ordinary payment (not change); default G omitted means set entry
to amount (not add/subtract). Explicit normal-payment `RS 'D1'` / set `RS 'G1'`
may also be sent (P:2024-2034,4287-4302). Repeated payment number overwrites
previous value (H:1472), but that is not permission to replay an uncertain
command.

| App form | X kind | Suggested name (driver policy) |
| -------- | -----: | ------------------------------ |
| cash     |      1 | GOTÓWKA                        |
| card     |      2 | KARTA                          |
| transfer |      7 | PRZELEW                        |
| mobile   |      9 | MOBILNA                        |
| credit   |      6 | KREDYT                         |

Codes come from P:1999-2019, **not Posnet codes**. Kind3/foreign currency is
excluded from LBP; other supported kinds are4 BON,5 CZEK,8 VOUCHER,10 INNA.
Missing X silently defaults to cash; always send X to avoid misclassification
(P:4291-4312). Equal name/type prints only type; different name is appended
parenthetically (same citation; O:25-50). The driver must require payments
exactly cover the post-advance fiscal amount; do not rely on automatic
cash/change balancing noted by the SDK (H:1473-1475).

Close: **`LCL GS 'R' 'A'<receipt_total>`**. Alternative `LCL GS 'RB'` explicitly
cancels and is **not** a timeout/recovery action (P:4903-4918). Proposed total
is gross lines minus advances, as app semantics; the manual only calls it
receipt total, and does not provide a worked advance+close example.
**[UNVERIFIED]** fully prepaid/zero-net close: E:237-238 explicitly lists
error1008 “zero or negative receipt sum” in receipt termination. Do not claim
feature parity or substitute a nominal 0.01 payment/sale. Resolve with ELZAB
whether fully deducted receipts close with original gross sum, net zero, or are
unsupported, before dispatching such jobs. No documented no-payment
zero-net-success path found. Other close errors include465 total mismatch,475
payment imbalance,482 no items (E:134-154).

### 4.6 Company/footer text

**LAL** (“additional lines after fiscal logo”) uses
`GS 'Q'<printout_line_data>`, one line per call, max **10** extra lines
(P:418-419,4828-4841). Exact normal main-generator text line:

```text
LAL || GS || 'Q' || 'A' || 'A' || company_text
```

First A is text subfield; second A means normal-width/height text fragment.
Default generator is main; explicit `RS 'D1'` means main, `RS 'D0'` additional;
optional `RS 'C'<justification>` or `RS 'B'<right_text>` exist but are not
needed (P:2130-2185,5626-5673,1657-1661). Main line limit is runtime main width;
additional-generator width is separately limited (§7). Do not confuse LAL with
persistent footer programming YFW (6 stored lines, P:373,7321-7335) or SDK
helper's 3×40-character wrapper (H:603-616).

**[UNVERIFIED] ordering:** LAL command section says “prints a single line after
fiscal logo”, but gives no explicit legal timing versus LCL, no Posnet-like
deferred-end flag and no footer-end command. It is unsafe to assume an app
receipt flow should send LAL before close or after a fully completed/cut LCL
without confirming firmware behavior. Required implementation gating: prove LAL
positioning and completion using the model/firmware profile, then persist LAL
intent and include it in success determination. A post-close footer failure must
not reprint a fiscal receipt; preserve completed evidence and block if
fulfillment is uncertain. Skip company extra line only when actual decoded
header already contains the full company name
(`crates/posnet/src/printer.rs:244-248`).

### 4.7 Number evidence, refusals and paper-out

After confirmed close/required footer completion, read **`SER`** →
**`ε GS 'J'<last_receipt_no>`**, decimal integer (P:1814-1815,5241-5255).
Missing/corrupt number never triggers close replay; return printed evidence with
absent number and retain unknown until staff enter the number, as current app
(`README.md:180-185`). Before starting, snapshot SER plus current fiscal day
(`RNR`) and useful counters for later inspection (**driver evidence policy**);
SER alone is not a job-specific unique identifier, has no documented
cross-day/restart interpretation and does not prove a missing-reply command was
ours.

Paper-out is materially unlike a resumable Posnet wait: **on online printers an
attempt to issue an item or finish a receipt with no paper automatically cancels
the receipt** (O:5-11). It is not valid to wait for paper and resend that
LIT/LCL, or resume the canceled transaction. Explicit450 canceled (E:125),
ySR2:71 and YLE cancellation reason (§5) can corroborate it; even then
cancellation may have produced an annulment record/partial paper. Never
requeue/reprint automatically.

Certainty classification for Workshop:

1. Validation, identity/readiness/clock/encoding failure **before fiscal
   dispatch**, or journal failure preventing the first send: certain not-started
   → failed (app invariant `README.md:186-190`).
2. Valid matched initial frame rejected at **transport parsing/acceptance**
   (SB/SC/SE/SF/SG/SH/SI/SJ/SK/SL/SM/SN) means it did not become an accepted
   command **[DERIVED]** from B:137-139,162,204,211-219,251,262-269,364-390. For
   a first single-frame fiscal command and no earlier fiscal send, this can
   establish no execution, but host must validate correlation and rule out stale
   bytes. Do not treat silence/wrong CRC as rejection.
3. A syntax error on the first single-command LOR is evidence of rejected syntax
   (P:3427-3480), but a universal guarantee of atomic validation / nothing
   physically printed for every command error is **[UNVERIFIED]**. No vendor
   Posnet-style “ERR vs execution error” envelope exists here. Conservative
   driver may keep all post-dispatch command errors unknown until ELZAB confirms
   a narrow initial-LOR refusal allowlist. A LOR refusal cannot be asserted to
   mean no physical header was printed merely because no LCL occurred.
4. Any later receipt error, lost reply, crash, cancellation,
   fiscal-memory/protected-memory failure, clock failure after dispatch, or
   footer failure is conservatively unknown, not an invitation to cancel/replay.
   Explicit canceled state is not a blanket guarantee that nothing was
   printed/recorded. Generic error list includes conditions both before and
   during execution; it does not specify atomicity (E:118-159,214-238,255-266).
5. Report refusals: a clear initial transport nonacceptance can be failed by the
   same reasoning; generic report command errors must remain unknown unless a
   specific validation-only refusal is proven to precede output. Error503 “no
   daily reports in period” and500/501 invalid ranges describe no-data/invalid
   requests (E:162-177), but documented all-firmware no-output atomicity is
   **[UNVERIFIED]**. Never port Posnet rejected-frame classification without
   evidence.

## 5. Operator intervention without resending

Use **new** read-only ySR queries; retain the original command ID so a late
answer cannot be consumed as a status reply. Show paper, raised head, mechanism
fault, user interruption, buffer occupied and shutdown/battery conditions using
§3.1. Explicit status definitions are P:9959-9986,10097-10105. Do not interpret
near-paper-end alone as “canceled”.

**`YLE`** → `ε GS 'T'<last_error> [ GS 'G'<error_info> ]`; manual explicitly
permits using this to find cancellation reason (P:8286-8304).
**`YER GS 'T'<code>`** → `ε GS 'T'<code> GS 'U'<error_text>` for localized
operator explanation (P:8306-8315). Save the original fiscal failure/code before
these queries; last-error overwrite/persistence semantics are **[UNVERIFIED]**.
Known operator codes:709 printing,710 no paper,711 paused due paper,712
mechanism fault,713 user pause,722 paused (E:212-227);225 battery low,469 menu
(E:61,139).

There is **no documented Posnet-like `!sprn` asynchronous query channel** in the
supplied printer manual; the Polish basics say device never transmits
unsolicited data (B:120-122), whereas older English basics broadly says both
peers should always be ready to receive (BE:103-104). Do not infer asynchronous
status delivery from that. Whether ySR is serviced while a synchronous fiscal
command waits on an operator, whether commands queue behind it, and whether
query IDs alter a pending response must be vendor/hardware-confirmed. Until
established, keep reading for original reply within a bounded app wait, and if
no reliable completion evidence arrives, fail unknown; do not declare success
just because “ready” later appears. A fixed 2-second status poll /10-minute wait
would be **app policy**, not a documented ELZAB timing rule.

**LCP** exists to resume interrupted printing (P:4954-4962), but invoking it is
an additional mutating operation. It is not passive monitoring and is prohibited
as automatic uncertain-outcome recovery under Workshop's no-reprint invariant.
Same for LCL RB cancellation, OPC cancellation, and USB-reset VUO. Operator
resolution never sends them.

## 6. Reports: daily, monthly and date-range periodic

All report dispatches must journal first and use the same verified
printer/session. Dates are YYMMDD; month YYMM; validate app-supported
years/ranges before encoding (P:526-539,559-563; `README.md:131-135`). The
manual's YY year alone does not establish a century range.

### Daily

**`RRP GS 'I1' GS 'J1'`**, omit O entirely. I1 = fiscal daily report; J1 =
device printer; O parameters are only allowed/required for listed kinds and must
be omitted for other kinds (P:9546-9583,9623-9627,10220-10236,10260-10264).
Kind12 is daily to protected memory only, **not** the paper report; do not
substitute (P:9566). `YYR GS 'K'` exposes daily-print configuration (`'!'`/21h
bit, 1 paper+protected,0 protected only; P:9465-9483,10572-10576). Actual
precedence of I1 versus that setting is **[UNVERIFIED]**; ensure paper behavior
in qualified profile rather than rewriting config.

Successful RRP reply is **empty and means only accepted for execution, not
printed/completed** (P:10229-10236). This is a critical incompatibility with a
dispatcher that treats success as completion. Before command, read/persist
**RNR** → `ε GS 'A'<next_daily_no>` (P:5464-5472). Passive completion evidence
candidates: ySR printing/buffer flags clear, RNR advances, and
**`RDR GS 'A'<previous_next_no>`** returns the committed report's
totals/tax/date/time/counters (P:10204-10218). These queries are documented; a
definitive atomic completion recipe covering canceled/interrupted paper vs
committed fiscal memory is **[UNVERIFIED]**. Do not mark paper report done
solely on RRP acknowledgement or a changed counter; require a proven completion
condition or human resolution. Never resend RRP.

### Monthly (detailed)

**`RPP GS 'E1' GS 'FB'<YYMM> GS 'S0'`**. E1 detailed, range F containing
B+month. Omit G (not a settlement report). Sources: P:9506-9536,10274-10288;
settlement auth prohibited on other report kinds P:2739-2741. Current Posnet
monthly uses `su=0` detailed (`crates/posnet/src/printer.rs:297-305`). Error507
means unfinished month (E:176); validate a completed month before dispatch.

### Periodic by date range

Detailed: **`RPP GS 'E1' GS 'FE'<from_YYMMDD> RS 'F'<to_YYMMDD> GS 'S0'`**.
Summary: **`RPP GS 'E3' GS 'FE'<from_YYMMDD> RS 'F'<to_YYMMDD> GS 'S0'`**.

The leading F is the range-field descriptor; following E is the from-date
subfield descriptor; final RS F is end date. Never confuse them with top-level E
report type. Kinds1 detailed,3 summary (P:9506-9515); date syntax P:9533-9536.
`S0` explicitly selects **synchronous** answer after printing finishes; omitted
S also defaults0. S1 answers immediately on receipt and is unsuitable for
current completion semantics (P:9493-9505,10286-10288). Success data defaults to
empty (P:3397-3406). Preserve waiting vs late original reply handling in §5;
failure after dispatch remains uncertain. Legacy non-online RPP explicitly only
acknowledges acceptance (P:5492-5503), so **do not reuse legacy report
completion semantics** for online or vice versa.

## 7. Encoding, character constraints and per-model limits

### Encoding

Read **`XHR GS 'w'`** → `ε GS 'w' 'A'<code_page>`; values CP852/CP1250/MAZOVIA
(P:1804-1805,3229-3236,6790-6806,7158). Choose a supported encoder for the
existing setting, or require operator configuration to CP1250 before processing;
never change printer language in auto-detection. Printable text is described as
bytes ≥20h; national glyphs depend on setting (P:569-573; PE:1970-1975). Control
bytes0..1F are not printable and protocol separators must never be embedded in
text; STX/ETX forbidden in frame content independently (B:135-136). No escaping
mechanism for those fields is specified.

The docs do **not** provide a full model-specific byte→glyph table or disallowed
≥20h holes (DEL/C1, unassigned CP1250 bytes, NBSP/soft hyphen/table-drawing).
Therefore exact backend `allowedCharacters` for each model is **[UNVERIFIED]**;
do not copy Posnet's CP1250 exclusion list. Interpreting the manual's broad
“ASCII >=space” literally as every Unicode or every CP1250 codepoint is unsafe.
Publish only proven Unicode NFC glyphs that round-trip in the actual configured
code page, without replacement/truncation/transliteration; qualification of that
repertoire remains required by the app (`README.md:162-164,251-258`).

### Limits table

- **Device/profile — Legacy ELZAB ZETA**: **Item name / LNP / main LAL width**:
  42; **Additional-generator LAL width**: 24; **Receipt item count**:
  [UNVERIFIED]
- **Device/profile — Legacy ELZAB D10**: **Item name / LNP / main LAL width**:
  42; **Additional-generator LAL width**: 24; **Receipt item count**:
  [UNVERIFIED]
- **Device/profile — Confirmed generic ONLINE profile**: **Item name / LNP /
  main LAL width**: 64; **Additional-generator LAL width**: 36; **Receipt item
  count**: [UNVERIFIED]
- **Device/profile — Mera Online (v1/v2)**: **Item name / LNP / main LAL
  width**: Generic ONLINE says64/36, but exact named-model profile must be
  confirmed; **Additional-generator LAL width**: same qualification; **Receipt
  item count**: [UNVERIFIED]
- **Device/profile — Cube Online (v1/v2)**: **Item name / LNP / main LAL
  width**: Generic ONLINE says64/36, but runtime width/model qualification
  required; **Additional-generator LAL width**: same qualification; **Receipt
  item count**: [UNVERIFIED]
- **Device/profile — T10 Online**: **Item name / LNP / main LAL width**:
  [UNVERIFIED] model not identified in supplied manual/header; do not assume
  D10; **Additional-generator LAL width**: [UNVERIFIED]; **Receipt item count**:
  [UNVERIFIED]
- **Device/profile — “Zeta Online” / “D10 Online”, if marketed separately**:
  **Item name / LNP / main LAL width**: [UNVERIFIED] whether legacy42/24 or
  ONLINE64/36 profile applies; **Additional-generator LAL width**: [UNVERIFIED];
  **Receipt item count**: [UNVERIFIED]

Evidence: constants main42 legacy D10/Zeta/Mera+ and64 ONLINE
(P:378-392,1665-1689); additional24 legacy and36 ONLINE (P:378-385,1639-1656);
item names use line_max_len which equals main width (P:765-767,1824-1825). These
are logical character/encoded single-byte lengths, not UTF-8 byte counts
(**[DERIVED]** from chosen single-byte code pages).

Read runtime widths with **`XHR GS 'n'`** →
`ε GS 'n' 'A'<density> RS 'B'<cut_mode> RS 'C'<main_width> RS
'D'<additional_width> RS 'E'<graphic_factor> RS 'F'<description_width>`
on newer/online profiles (P:2098-2112,6790-6806,7146). Legacy D10/Zeta do not
expose widths and need documented constants (P:1643-1648,1669-1681). Primary
manual's `<printing_settings>` definition is titled Sigma/Mera+ but its XHR
applicability includes online; treat observed missing width fields as a failed
capability check, not a fallback to assumed64 (**[DERIVED]**; exact per-model
support **[UNVERIFIED]**).

Other precise limits: max10 transient LAL lines, six persistent footer lines,
seven payment entries, VAT A..G, unit-name max6, quantity max5 fractional
digits, money/price exactly2 decimals
(P:373,418-421,460-478,303-304,819-826,3121-3129). Receipt item count is
**not**500 merely because `<pricing_buffer_size>=500`; that is pharmacy pricing,
not receipt capacity (P:470,10137-10142). Error452 “no space for further receipt
positions” does not establish a deterministic maximum (E:126). A driver cannot
truthfully advertise maxLinesPerReceipt until vendor/model documentation
supplies a safe value. Numeric amount ceilings and rounding remain unverified as
§4.3.

The SDK `pWriteFooterLines` accepts three40-character lines (H:603-616), but
native YFW has six stored lines and native LAL up to10; **40 is a wrapper
limitation, not proof of native LAL or item-name length**. Current app needs one
full company line, so maxCompanyNameLength should be derived from qualified main
LAL width, not copied from the SDK or Posnet.

## 8. Nonprinting reconciliation evidence / document identifiers

No documented host-supplied idempotency key/job UUID persisted into an ELZAB
fiscal command was found. Transport command_id is **not** such a key (§1.3).
Available read-only evidence:

- **Query — Last receipt**: **Exact syntax / reply**: `SER` → `ε GS
  'J'<last_receipt_no>`; **Limits of evidence**: P:5241-5255; no per-job or
  cross-day uniqueness promise.
- **Query — Fiscal day/report**: **Exact syntax / reply**: `RNR` → `ε GS
  'A'<next_daily_no>`; **Limits of evidence**: P:5464-5472. Pair with receipt
  number in journal for human inspection, not auto-reprint.
- **Query — Online count**: **Exact syntax / reply**: `SQR GS 'A'<kind>` → `ε GS
  'A'<kind> GS 'B'<count>`; **Limits of evidence**: P:10171-10182. Kinds1
  receipts today,2 canceled receipts today,6 current fiscal report/day,10
  documents,20 nonfiscal documents (P:10106-10115,10126-10145). Counter deltas
  do not prove attribution under other interfaces/operator activity.
- **Query — Current last-success/cancel/open**: **Exact syntax / reply**: ySR2
  bits74/71/68; **Limits of evidence**: P:9976-9982. State is not tied to
  request ID; do not infer “our close succeeded” from bit74 alone.
- **Query — Last error**: **Exact syntax / reply**: `YLE` / `YER GS 'T'<code>`;
  **Limits of evidence**: P:8286-8315. Diagnostic only; storage/overwrite
  lifetime unverified.
- **Query — Printed QR**: **Exact syntax / reply**: `LQR` → `ε GS
  'Y'<printed_qr_code>`; **Limits of evidence**: P:10192-10200. QR
  contents/receipt binding not specified here; do not parse as a receipt number
  without format evidence.
- **Query — Manufacturer delivery**: **Exact syntax / reply**: `YJR GS 'A'` → `ε
  GS 'A' 'A'<receipt_no> RS 'B'<fiscal_rep_no>`; **Limits of evidence**:
  P:5438-5460; only online v2+, manufacturer decides which receipts are sent,
  e.g. NIP-only. **Not** reliable universal last receipt/print evidence.
- **Query — JWS fiscal receipt**: **Exact syntax / reply**: `dJR GS
  'A'<receipt_no> GS 'B'<fiscal_rep_no> GS 'C1'`, then C0 until reply
  part_no==max_part_no; reply `ε GS 'A'<part_no> GS 'B'<max_part_no> GS
  'C'<part>`; **Limits of evidence**: P:5409-5436. Online v2+ only; full JWS
  schema and proof linking to job/physical completion **[UNVERIFIED]**. Distinct
  read calls for portions are not fiscal retries.
- **Query — Daily record**: **Exact syntax / reply**: `RDR GS 'A'<daily_no>` →
  fields A..I with totals/rates/date/time/counters; **Limits of evidence**:
  P:10204-10218. Fiscal-memory existence is not by itself paper-completion
  proof.

Protected-memory **read**, not reprint: request **`dPC GS 'YE' GS 'd'`** to
search last receipt (range E = last receipt, P:2337-2343; search d behavior
P:10292-10317). Do **not omit d**: that would initiate device printing. The
manual says search is asynchronous and to check **uppercase YSR package1 bit20**
(P:10309-10317), not ySR package1 bit20 (which is reserved). Then successive
**`dCR`** calls return `ε GS 'I'<data_portion> [GS 'e']`, one printout line per
call; optional e means first line of a document; an empty answer terminates
(P:10329-10349). For selected day/receipt ranges,
`dPC GS 'Y' 'C'<from_day> RS 'D'<to_day> GS 'Z' 'F'<from_receipt> RS
'G'<to_receipt> GS 'd'`
combines documented range/filter grammars (P:2320-2341,10298-10317). It is
nonprinting **only with d present**. Legacy YSR search-bit availability on every
online model, protected-memory read text encoding, canceled-receipt inclusion
for “last”, and forensic attribution rules are **[UNVERIFIED]**; do not let this
read facility silently auto-resolve unknown outcomes.

Explicitly forbidden for reconciliation: `RRP` kind14 original receipt, dPC
without d, old uppercase DPC print-copy, LCP resume, any LOR/LIT/LCL replay.
Vendor offers paper original-print functions
(P:9568-9571,9594-9608,10265-10272), but Workshop's stronger no-reprint
invariant forbids invoking them. Read-only evidence may assist a person;
resolution itself emits no fiscal/printing commands (`README.md:180-211`).

## Gaps requiring vendor/hardware confirmation before claiming complete support

1. **All-current-model matrix:** XTR IDs and online-generation classification
   for Mera/Cube/Zeta/D10/T10, and exact LOR/status/width applicability; T10
   absent from reviewed supplied docs. No unverified marketing-name→legacy
   profile fallback.
2. **Zero-net fully prepaid sale:** reconcile E:1008 with LDS and LCL total
   semantics. This is an explicit app capability blocker, not an optional
   enhancement.
3. **No hidden retransmission / execution deduplication:** obtain a vendor
   statement/test vector establishing completed-frame behavior; until then never
   retransmit, including on SL/silence.
4. **Native CRC reference vector:** confirm calculated LOR `4E0E` vector against
   vendor/reference implementation; supplied docs gave algorithm but no
   numerical worked frame.
5. **USB and serial:** OS drivers/VID-PID/CDC mode/DTR-RTS, factory settings,
   online serial parity defaults and whether USB honors baud; remain separate
   from protocol syntax.
6. **Completion/monitoring:** whether ySR is accepted during a pending
   synchronous fiscal command, late response correlation, wait reasons/timeout
   behavior, daily RRP completion evidence and paper-vs-memory outcome; cannot
   mark an accepted daily report complete.
7. **NIP and LAL timing:** legal insertion order, when company footer physically
   prints/cuts, final completion evidence, behavior when paper ends after fiscal
   close; no invented footer-end command.
8. **Preflight equivalence:** explicit fiscalized/read-only truth table,
   initialized header detection, queues/interface resource ownership, and safe
   required/forbidden status mask by operation (daily report must not be blocked
   solely by a daily-report-due flag).
9. **Data/telemetry limits:** max receipt item count, numeric magnitude
   ceilings, line multiplication/rounding, full printable glyph repertoire and
   name transformation vs actual printout; derive validated dynamic widths, do
   not reuse Posnet constants.
10. **Certain-refusal classification:** vendor-backed initial-LOR/report
    validation-only allowlists, atomicity on errors/auto-cancel, and error
    persistence. Generic command error must not be misclassified as nothing
    printed.
11. **Reconciliation:** receipt-number scope across fiscal days/power loss,
    counter update atomicity, JWS schema and search-bit compatibility. These
    help inspection but never authorize replay or automatic unknown clearing.

These gaps are intentionally visible. The supplied documentation establishes
native syntax for the main capabilities, but it does **not** justify an
unconditional “all models/full Posnet parity/safe exact-once retransmission”
implementation claim.
