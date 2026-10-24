# Native NOVITUS ESC-P driver specification and parity audit

## Sources

Citation line numbers refer to `pdftotext -layout` output, not PDF pages.
Download each URL to the named PDF, then run
`pdftotext -layout <pdf> <txt>` with the exact filenames below.
No archive extraction is needed for these PDFs.

- **N**: [official PDF](https://novitus.pl/file/get/695),
  *Opis protokołu komunikacyjnego drukarek fiskalnych oraz drukarek
  Aptecznych*, version 1.15. PDF `novitus-protocol-online-1.15.pdf`;
  text `novitus-protocol-online-1.15.txt`.
- **XE**: [official PDF](https://novitus.pl/file/get/701),
  *Description of XML communication protocol*, version 1.0.2 ENG.
  PDF `novitus-xml-online-en.pdf`; text `novitus-xml-online-en.txt`.
- **XP**: [official PDF](https://novitus.pl/file/get/700),
  *Opis protokołu komunikacyjnego XML*, version 1.08 PL.
  PDF `novitus-xml-online-pl.pdf`; text `novitus-xml-online-pl.txt`.
- **XD** (supplementary XML description):
  [official PDF](https://novitus.pl/file/get/1441),
  *Description of XML communication protocol*, version 1.09.ENG (2023).
  PDF `novitus-xml-desc.pdf`; text `novitus-xml-desc.txt`.
- **NoviAPI** (supplementary SDK distribution manifest):
  [official JSON](https://noviapi.novitus.pl/en/dist.json), saved as
  `noviapi.json`; direct UTF-8 JSON, no PDF or archive extraction.

## Notation and scope

Primary source **N** is version 1.15. All `N:Lx-Ly` references below are
line numbers in the N extract, not PDF page numbers. Secondary cross-checks
**XE** and **XP** are the English and Polish online XML manuals. XML is
comparison evidence only, NOT the proposed transport or implementation.

Workshop's used Posnet behaviors are in `posnet-coupling.md:57-76`, and
directly `crates/posnet/src/receipt.rs:346-410`,
`crates/posnet/src/printer.rs:217-259,262-317`: verify
status/VAT/header/identity, obtain printer time, begin, optional NIP, items,
per-VAT advance deductions, five payment forms, close, optional company footer,
receipt number, daily/monthly/periodic reports. The app requires names printed
unchanged, no truncation/transliteration, and advances/payments unchanged
(`README.md:153-164,280-301`).

**This document does not establish full parity as implementable yet.** Confirmed
gaps are the contradictory exempt/inactive VAT sentinel definitions, advance
settlement scope, changeable company footer semantics, complete printable
repertoire, USB details, and transaction ownership across ports. Each is
described below; do not implement guessed behavior as if specified.

Byte notation (all concatenation is literal, with no decorative spaces):

- `S = 1B 50` = `<ESC>P`; `E = 1B 5C` = `<ESC>\`; `CR = 0D`; `; = 3B`; `/ = 2F`.
- `F(b) = S || b || CC(b) || E`; `Q(b) = S || b || E` for commands explicitly
  without checksum. `CC` is TWO ASCII hex digits, not a binary byte. All decimal
  numbers below are ASCII, never binary integers. Text is encoded using the
  configured input/output page. Basis: N:L538-L562,L583-L604.
- `ACK(cmd) = Q(decimal_error + "#Z" + cmd)`; zero error means OK; the command
  identifier has 1–5 characters. No checksum is specified in this envelope.
  Basis: N:L660-L668.
- Recommendations labeled **[DESIGN]** are conservative Workshop integration
  choices, not vendor promises. **[UNVERIFIED]** means the primary documentation
  does not settle the fact; it must not be silently assumed.

## 1. Transport, framing, acknowledgments, and status

### 1.1 Frame parser and checksum

A general frame is `S P1;...;Pn command text CC E`; parameter presence and exact
separators are command-specific. Command identifiers are described as 2–4
characters (later commands include longer identifiers). Text strings end in
`CR`; numeric amounts usually end in `/`, occasionally `CR`. Payload strings
exclude control bytes. Amounts have at most eight integral digits plus two
fractional digits separated by a PERIOD; insignificant zeros may be omitted
(`13`, `13.00`, `13.` all accepted). N:L538-L562.

Checksum: initialize `c=0xFF`, XOR every byte after `S` up to but excluding
checksum and `E`, and encode the result in two uppercase ASCII hex digits. The
checksum includes parameter separators, command bytes, `CR`, text in its wire
encoding, and numeric separators. N:L556-L562. Vendor worked example: cash
deposit body `0#i100/`, bytes `30 23 69 31 30 30 2F`;
`FF xor 30 xor 23 xor 69 xor 31 xor 30 xor 30 xor 2F = 9B`; complete frame is
`1B 50 30 23 69 31 30 30 2F 39 42 1B 5C`. This is illustrative only, NEVER an
identification/probe command: it changes cash and prints. N:L565-L576.

Execution starts only after `E`. Syntax errors cause the remaining sequence to
be ignored until its terminator; a new `S` restarts analysis, and CAN aborts
parsing. Receiving `S` suspends the previous command if not yet executed.
Therefore a status-query frame is NOT a harmless substitute for a raw
asynchronous status byte during a pending command. Raw ENQ/DLE/BEL
interpretation resumes after the frame terminator. N:L577-L598.

Checksum exceptions: display/drawer control, cash-information `#s`, clock `#c`,
last error `#n`, model/version `#v` do not require outgoing checksum; some
specialized `#s` variants explicitly DO show one, e.g. `100;kind;#s`. Follow the
individual schema, not a blanket rule. N:L583-L587,L1043-L1059. Replies also
differ: `#v`, `#c`, `#n` have no documented checksum, while `$I`, `2#X`, `?l`,
and DLE2 do.
N:L699-L705,L800-L842,L1236-L1248,L1326-L1356,L2082-L2093,L12639-L12725.

**[DESIGN]** One incremental byte-stream parser must demultiplex raw status
bytes, framed data, and command ACK frames, preserving any outstanding command
and accepting fragmented/concatenated input. No ACK may be associated merely by
arrival order when its identifier names another command. This follows the
separate raw status alphabets and identifier-bearing ACK format
(N:L418-L429,L448-L466,L660-L668); USB packet/chunk boundaries are
**[UNVERIFIED]**, not framing.

### 1.2 Serial and USB

RS-232 supports 1200/2400/4800/9600/19200/38400/57600/115200 baud, **8N1**.
Receiver flow control is RTS/CTS, XON/XOFF, or none; transmitter flow control is
none, so the host cannot pause printer output. Intro also mentions hardware
RTS/CTS and DTR/DSR. N:L20325-L20354. Administrative section instead lists
XONXOFF+RTSCTS or RTSCTS; this is a documented configuration discrepancy, so use
the printer's actual configuration, not an inferred universal default.
N:L20020-L20027. Factory baud/default flow mode per current model
**[UNVERIFIED]**.

N:L17047-L17075 documents USB configuration readout `Q("?usb")`, reply
`F("?usb" + modem_pid CR + external_pid CR + modem_vid CR + external_vid CR +
modem_type/ + external_type/ + modem_interface CR + external_interface CR +
modem_protocol CR + external_protocol CR)`,
hex fields four characters and type 0=`Acmx`, 1=`USBx`. These are configuration
fields; they are NOT a documented USB CDC host connection contract. USB CDC ACM
enumeration, interface-to-PC1/PC2/PC3 mapping, whether baud/DTR/RTS affect USB,
port exclusivity, reconnect/reset semantics, VID/PIDs and per-OS device names
are **[UNVERIFIED]** in this protocol manual. Do not infer them from the
unrelated USB modem configuration. Read current logical port with `Q("50$i")` →
`F("50$I" + port CR)`, port `PC1|PC2|PC3`; N:L1969-L1993.

### 1.3 Error mode and acknowledgment contract

Set `F("3#e")` for noninteractive error handling with automatic completion/error
replies. Modes: 0=display + stop until OK (power-on default), 1=no display/no
stop + explicitly query error, 2=display/stop plus automatic code, 3=no
display/no stop + automatic code after command execution, 4=code only for
packets without their own response. Mode command errors 2=checksum,4=parameter.
N:L631-L685. Whether changing mode itself immediately acknowledges under the NEW
or OLD mode is **[UNVERIFIED]**; don't deadlock initialization by assuming it
does.

Completion reply: `ACK(cmd)`, e.g. notation `Q("0#Z$h")` for successful begin.
Error codes must accept more than the ACK table's claimed 1–2 digits: the same
manual lists 1002,1021,1037,1504,1530 etc.
N:L660-L668,L20480-L20520,L20597-L20599. A nonzero ACK says that command failed;
it is NOT automatically proof that no fiscal effect occurred. Syntax errors
explicitly ignore the malformed command; execution errors include hardware
failure/overflow and have no rollback guarantee. N:L610-L621.

Last-error read: `Q("#n")` (optional ignored prefix 0..255), reply
`Q("1#E" + decimal_error)`, no checksum. It does not itself produce automatic
mode-3 ACK and has no documented execution errors. Error is cleared by each
command; the manual recommends checking ENQ's CMD bit after each command as
well. N:L690-L719. **[DESIGN]** Prefer matching automatic ACK; do not send
another framed query just to discover a still-running fiscal command's state,
because it can suspend parsing or overwrite last-error evidence
(N:L591-L598,L715-L717).

No separate universal ACK/NAK single byte is specified for native ESC-P.
Automatic `#Z` frames are the completion/error mechanism, with exceptions listed
on individual commands. N:L660-L668,L714-L719. ENQ, DLE, DLE2, BEL, CAN do not
generate mode-3 last-error ACK (N:L428-L442,L463-L466,L535-L536). `#v`, `#c`,
`#s` modes0..21 and23 also suppress it
(N:L792-L793,L849-L850,L1289-L1290,L2111-L2112). Other data commands may have
both data and completion frames; **[UNVERIFIED]** exhaustive mode3/mode4
behavior on every firmware.

### 1.4 Raw real-time bytes and bit fields

- **Request — ENQ `05`**: **Response / meaning**: ONE byte `0x60 | FSK<<3 |
  CMD<<2 | PAR<<1 | TRF`; FSK0=training,1=fiscal (also read-only!); CMD=last
  command completed correctly; PAR=in transaction; TRF=last transaction
  finalized. CMD is cleared on new `S` except cash-info request, then set on
  successful completion. N:L383-L408,L425-L427.
- **Request — DLE `10`**: **Response / meaning**: ONE byte `0x70 | ONL<<2 |
  PE<<1 | ERR`; ONL1=can process; PE1=no paper OR low/discharged 24V battery;
  ERR1=mechanism/controller error. Asynchronous, usable while paper/error blocks
  synchronous processing. N:L445-L466.
- **Request — BEL `07`**: **Response / meaning**: Beep only; not a state query
  or acknowledgment. N:L433-L436.
- **Request — CAN `18`**: **Response / meaning**: Abort command interpretation;
  NOT documented transaction cancellation, NOT reconciliation, and not harmless
  status. N:L439-L442,L577-L581.
- **Request — DLE2 `1A`**: **Response / meaning**: Asynchronous extended status,
  framed `F("@s" + last_error; + last_print_no; + last_receipt_no; +
  flags_hex8;)`. It has no mode3 ACK. N:L470-L536.

ENQ is synchronous and can be delayed indefinitely or never arrive during
printing/paper-out/error; use DLE/DLE2 for that situation. ENQ TRF is
nonvolatile; the manual guarantees receipt printing, totalizer update and TRF
setting inseparable even across power loss. A cancelled transaction leaves PAR=0
and TRF=0. TRF does NOT identify YOUR transaction; it is cleared on successful
next begin. N:L400-L427.

DLE2's table numbers bits **1..8**, not 0..7. Preserve this convention,
translate bit k→mask `1<<(k-1)`:

- Listed **byte4**: b8 cannot process; b7 daily report running; b6 synchronous
  e-copy verification; b5 waiting for user; b4 menu; b3 paper-out; b2
  print-mechanism error; b1 cover open. N:L484-L506.
- Listed **byte3**: b8 last command ended in error. N:L507-L508.
- Listed **byte2**: b8 transaction; b7 additional-receipt-lines mode; b5
  nonfiscal print open; b4 e-document sending; b3 printer lock mode; b2
  periodic-report generation. N:L509-L525.
- Listed **byte1**: b8 printing; b7 paper low. N:L526-L531.
- **[UNVERIFIED]** the table displays four labeled bytes, eight hex characters,
  without a worked wire sample proving whether byte4 is first or last in the
  textual field. Do NOT silently choose machine endianness. Unlisted bits are
  unspecified, not proof of an error. The source is N:L475-L533.

### 1.5 Timeouts and operator intervention

No normative host command-response timeout, frame interbyte timeout, DLE2 poll
interval, or maximum report duration was found: **[UNVERIFIED]**. Documented
time limits are receipt-specific: begin with no subsequent command exits
transaction after 30 minutes without printing; a receipt not completed within 30
minutes is cancelled with `PARAGON ANULOWANY`. N:L6062-L6064,L6316-L6320.
E-receipt network timeouts elsewhere do not establish serial deadlines.

**[DESIGN]** After a silent fiscal command, retain its parser/ACK state; send
ONLY raw DLE2 (and optionally DLE) without resending fiscal command. Surface
paper/cover/mechanism/user-wait/menu states from DLE2; DLE alone cannot
distinguish paper from low battery, or cover from generic mechanism error
(N:L454-L456,L484-L531). Continue receiving original `#Z` acknowledgment; report
execution success only when the proper completion arrives. A cleared wait flag
alone is not completion. Workshop can retain its 10-minute first-wait wall-clock
cap, but that is APP policy (`README.md:174-179`), **not a Novitus-specified
timeout**. Keep cap below the documented receipt cancellation deadline. No
positive wait reason, lost link, malformed status, unexplained silence, or cap
expiry after dispatch → uncertain/unknown; never CAN, cancel, restart, or
reprint as timeout recovery. Basis: `.omp/RULES.md:6-12`; parser hazards
N:L577-L598.

Some operations require a physical YES even with error display disabled,
especially durable fiscal-memory writes/daily reports (N:L682-L685). Distinguish
this from an error acknowledgment. Daily variant with matching supplied date
avoids keyboard confirmation for nonzero totals (N:L9150-L9156).

## 2. Identity and mode

Read-only brand/model discovery probe: `Q("30#v")` →
`Q("1#R" + original_model + "/" + firmware)`. Plain `Q("#v")` returns the
CONFIGURED/reported model, potentially spoofed for compatibility; prefix30
requests ORIGINAL model/version. Example model `NOVITUS HD ONLINE`, firmware
`1.60`; error1=uninitialized RTC; no automatic mode3 ACK.
N:L1230-L1290,L20094-L20108. **[DESIGN]** Accept a validated known
original-model/firmware profile, not a generic configured `NOVITUS ZGODNY`
identity. Exact returned names for every current HD II/Deon/Bono/Point firmware
**[UNVERIFIED]**.

Extended identity `Q("0$i")` →
`F("0$I" + name CR + program_version CR + printing_module_version CR + OS_name
CR + OS_version CR + display_count CR + print_buffer_width_chars CR +
electronic_copy CR + fiscal_memory_bytes CR)`,
electronic_copy0 for Online. N:L1296-L1356. This has error1=RTC (N:L1965-L1967).

Use **NUMER UNIKATOWY** for backend device identity, not factory serial, JPKID,
NIP, or mutable reported model. `Q("51$i")` →
`F("51$I" + unique_no CR + factory_no CR + manufacturer_options/ + MAC CR +
CPD_server CR)`,
unique_no13 characters, factory_no16; options decimal bitset. N:L1849-L1875.
`Q("1$i")` also returns unique number and NIP after fiscal-memory sizes/mode;
full sequence in N:L1360-L1421. Cash-info `23#s` returns unique number at the
end (below), described `ABCNNNNNNNNNN` N:L1215-L1216, but legacy-format
compatibility can change returned length N:L20102-L20108. Whether `51$i`
bypasses that compatibility setting is **[UNVERIFIED]**.

Read mode using `Q("11$i")` →
`F("11$I" + status CR + fiscalization_datetime CR + NIP CR +
registration_no CR + protected_memory_identifier CR + device_type CR +
receipt_numbering CR + ownership_type CR)`.
status0=nonfiscal,1=fiscal,2=read-only; datetime `YYYY-MM-DD HH:MM:SS`;
type0=general (others taxi/pharmacy/ticket/gastronomy/automats/dutyfree);
numbering0=global,1=daily. N:L1739-L1785. Require status1 for receipts, do not
rely on ENQ FSK because it remains1 in read-only (N:L425-L427). Closed
fiscal-mode date is also exposed by `1$i`, N:L1407-L1421. Error1021 explicitly
means read-only,1006 not fiscal,1007 no programmed VAT,1008 full fiscal memory;
N:L20480-L20499.

## 3. Preflight state, VAT, header, clock, and ownership

**[DESIGN]** In the same exclusive connection before each fiscal operation:
identity/unique number, status11$i fiscal mode, raw DLE/DLE2 readiness and no
open document, header read, VAT read and exact job mapping, and readable clock.
No query repairs settings or cancels another document. Safety requirement:
`README.md:166-173`; relevant Novitus fields below.

### 3.1 Open transaction and printer state

DLE2 exposes transaction, additional lines, nonfiscal print, print activity,
report activity, lock/menu/operator/paper/cover/errors; refuse on those
pending/busy states (N:L470-L533). ENQ PAR is also available (N:L396-L399). For
a synchronous detailed state, `F("100;0;#s")` requests gross totals; reply
`F("100;" + kind + ";" + state + "#X" + total/ + A/ + B/ + C/ + D/ + E/ + F/ +
G/ + 0/ + 0/)`,
kind0=gross,1=net,2=VAT, state0=no transaction,1=receipt,17=block/offline
receipt,19=VAT invoice. N:L1043-L1099. This variant explicitly requires checksum
and the semicolon after kind in the REQUEST; reply's state is adjacent to `#X`.
Do not confuse header `100;0;0#X` with plain `2#X`.

Mechanism/config detail: `Q("3$i")` →
`F("3$I" + print_module_version CR + width_chars CR + paper_width CR + font CR +
cutter CR + paper_level CR)`.
paper_width0=57mm,1=80mm; font0=normal,1=enlarged; cutter0=off,1=on,2=absent;
paper_level0=below critical,1=above,2=unsupported. N:L1451-L1478. This paper
level is not a substitute for the asynchronous paper-out/cover bits.

### 3.2 VAT table: seven slots and a fatal documentation contradiction

Use `Q("23#s")` for fixed seven-slot reply:
`F("2#X" + last_error; + fiscalized; + transaction; + transaction_ok; + 1; +
reset_count; + YY; + MM; + DD/ + ptuA/ + ptuB/ + ptuC/ + ptuD/ + ptuE/ + ptuF/ +
ptuG/ + receipt_count/ + totalizerA/ + totalizerB/ + totalizerC/ + totalizerD/ +
totalizerE/ + totalizerF/ + totalizerG/ + returnable_packages/ + unique_no)`.
The final unique number has NO terminator before checksum, normally13
characters; this is not a slash-terminated numeric field. N:L795-L850. Mode22
reports current receipt totals,23 totals since daily report; mode23 suppresses
mode3 ACK. Do not use old mode0..21 variable-length reply, which assumes exempt
G and may omit rates. N:L742-L747,L844-L850. State field definitions and
zero-error claim: N:L1182-L1227.

Slots A..G map to backend0..6; the table is explicit in the per-PTU packet
N:L7200-L7222, and each letter can individually be exempt N:L6248-L6252.
Ordinary active rates are exact decimals; store e.g.23.00→2300 basis
points,0.00→0. Never infer exemption from letter G.

**STOP: contradictory sentinels.**

- Cash-info field description says exempt=`98.99`, inactive=`99.99`:
  N:L1203-L1205.
- VAT-programming chapter agrees exempt98.99/inactive99.99 and excludes both
  from numeric active range0..99.99: N:L4769-L4773.
- Native NOVITUS administrative description REVERSES them,
  exempt99.99/inactive98.99: N:L20034-L20037.
- NOVITUS ZGODNY administrative description gives exempt101/inactive100:
  N:L20028-L20032.

**[UNVERIFIED]** which mapping current firmware actually RETURNS, whether
programming vs readout differs, and how configured native/compatible mode is
read without changing it. Fail closed on unresolved sentinel interpretation;
never publish guessed `zw` or let an inactive rate pass. Configuring VAT as an
experiment is forbidden (durable fiscal-memory operation N:L4789-L4802). This
contradiction prevents claiming a ready production driver from documentation
alone.

### 3.3 Header programmed and company lookup

Simplest read-only text query: `F("^u")` (checksum optional per table), reply
`F("1#U" + header_text)`, text0..450 characters, spaces included, no
formatting-control characters; format depends on device. N:L16080-L16101. This
is the closest match to Posnet `hdrget` for searching companyName. Require a
valid nonempty header; whether empty response is the ONLY unprogrammed-header
signal is **[UNVERIFIED]**. Begin error40 explicitly means missing RAM header in
fiscal mode, N:L6042-L6050; general error40=header not programmed,1011=header
checksum wrong N:L20457,L20490.

Structured alternative `F("^h")` →
`F(line_count; + field_count_per_line;... + "#a" + line_blocks)`, count1..8,
each line `alignment/ + font_attributes/ + (field_index/ + text CR)*`.
Alignment0=left,1=center,2=right; field indexes0=absent,1=taxpayer name,2=postal
code,3=city,4=building,5=unit,6=post office,7=street,8=user-print-only.
N:L5003-L5079. The table is internally loose about field-block ordering (`n+1`)
and index/attribute ranges; use `^u` for plain-text comparison unless structured
parsing is confirmed. Newer ten-line programming exists as `!a` (N:L5084-L5101);
ten-line readback via `^h` is **[UNVERIFIED]**.

### 3.4 Clock

`Q("#c")` → `Q("1#C" + YY; + MM; + DD; + hh; + mm; + ss)`, no checksum; each
component1–2 ASCII digits, year
table0..49,month1..12,day1..31,hour0..23,minute/second0..59.
Errors1=uninitialized RTC,5=read error; no mode3 ACK. N:L2073-L2112. Year epoch
for this specific response, behavior after2049, timezone/UTC offset and DST-fold
signaling are **[UNVERIFIED]**. Other dated commands explicitly use two last
digits of2000..2099 (daily report N:L9122-L9127); it is reasonable to
propose2000+YY but it is not a clock-response guarantee. **[DESIGN]** use
verified Polish local time/Europe-Warsaw only after vendor/profile confirmation,
reject invalid/ambiguous impossible dates rather than use PC time. Current app
requires printer time before begin (`README.md:199-204`).

Configuration option219 controls summer time0=automatic,1=summer,2=winter;
N:L14599-L14604. This does not put an offset into `#c`; timezone interpretation
remains **[UNVERIFIED]**.

### 3.5 Queue, another interface, and resources

No readout equivalent to all Posnet `scomm.qe`, `strns.td/tl` ownership/resource
fields was located: **[UNVERIFIED]** global queue emptiness and which interface
owns an open fiscal transaction. DLE2 shows global busy/open modes but no owner
(N:L470-L533). Error1033=fiscal memory busy,1034=printer
busy,1026=transmit-buffer overflow are failures, NOT positive readiness evidence
(N:L20504-L20518).

`Q("50$i")` identifies YOUR logical port only (N:L1969-L1993). `5$i` gives
print-buffer sizes, critical threshold and occupied bytes/lines (N:L1494-L1534),
not explicitly a pending-fiscal-command queue. AUX tunnel `^x` queue queries
refer to AUX RX/TX, not fiscal transaction ownership (N:L12420-L12458). Do not
mislabel either as Posnet queue parity.

PC2 notifications `^e` can hold PC port for5 seconds after receipt/other print;
events include transaction/item/payment/end and port queue activity; defaults
off, persistent/temporary setup available. Active PC2 polling is needed for
those holds. N:L15800-L15874. `9^e` event reply includes special states
bits0/1=PC held after receipt/print, N:L15877-L15926. This is a limited
cross-port coordination feature, NOT documented exclusion/ownership for an
entire transaction. Enabling it during discovery would change settings and is
not an acceptable read-only identification probe.

## 4. Receipt command sequence

All receipt commands below use `F(...)`, and mode3 completes with
`ACK(exact command identifier)`, not a special per-receipt return object
(N:L631-L668). Dispatch journal intent BEFORE each send; after sending any
fiscal stage do not retry it (`.omp/RULES.md:6-12`). Product names, quantities,
VAT slots, and payment sums are validated before dispatch; verify unchanged
fresh VAT/identity in the printing session (`README.md:153-173`).

### 4.1 Begin and maximum lines

Minimal begin `F(count + "$h")`, count0..255; count0 selects ON-LINE, lines
print as received, nonzero selects buffered/offline and declares exact item
count. N:L5976-L6001,L6032-L6034,L6304-L6307. Begin **does not print header**,
only sets PAR/CMD on success; N:L6055-L6059. For the used unbuffered behavior
choose `F("0$h")`.

Declared item count and line index are each limited0..255; sale indices1..255
are continuous,0=STORNO; discounts count as receipt lines.
N:L5980,L6061,L6072,L6180-L6181,L6304-L6307. **[DESIGN]** advertise at most255
sale lines absent a confirmed extended numbering rule, and subtract any
line-consuming adjustments if necessary. A Posnet-style500-line receipt or index
wrapping for online mode is **[UNVERIFIED]**, not supported by this table. No
per-model higher line cap is specified.

Begin with footer definitions:
`F(count + ";" + n + "$h" + line1 CR + ... + linen CR)`, n0..3; this is
additional footer text after fiscal logo, subject to administrative
source/constant-text configuration. N:L5991-L6001,L6035-L6039,L6065-L6066. See
§4.6; it is not unrestricted dynamic text.

Begin errors1 RTC,2 checksum,4 bad count,5 RTC read,6 fiscal memory,40 missing
header,94 service jumper sales block. N:L6042-L6050. A matched refusal of this
first begin cannot itself have printed a receipt: the command explicitly prints
no header, and finalization comes later (N:L6057-L6059,L400-L408). **[DESIGN]**
certain-failed only for matched acknowledged begin refusal, not for a
timeout/disconnect which may have opened a transaction. Next operation must
still reject open transaction; never auto-cancel it.

### 4.2 Buyer NIP

Preferred separate command after begin and before end:
`F("100;1$z" + buyer_nip CR)`, NIP0..16 characters, may contain letters/digits.
Only error2 checksum is enumerated. N:L7785-L7813. This is not the numbered
footer154 `NIP Nabywcy`, which would merely print a footer label
(N:L7606-L7608). Use fiscal buyer-ID command, not a generic text replacement.

Older extended-begin form embeds buyer ID (0..13) and ID-type/PL flags
(N:L6006-L6028), but its type range0..2 contradicts listed PESEL3 and its
missing separator between flags is ambiguous. **[DESIGN]** do not use that form;
separate100;1$z is clearer and has larger documented limit.

### 4.3 Item line

`F(index + "$l" + name CR + quantity CR + VAT_letter + "/" + unit_price + "/" +
gross + "/")`.
index1..255, name1..60 except older model limits, quantity text1..16, rate ONE
letter, price/gross1..11 ASCII characters; N:L6068-L6080. Gross must be
positive, price whole grosze, maximum ten digits including decimals; general
amount limit eight integral+two fractional digits. Quantity parser extracts up
to ten-digit number with decimal point from quantity text (units possible);
**[DESIGN]** send pure decimal, not unit decorations. N:L551-L555,L6244-L6259.

Use explicit `A`..`G` corresponding to backend slot; do not use alias`Z`/space,
which is ambiguous if multiple exempt slots. N:L6248-L6252. Printer checks
price×quantity, rounds to two decimals and compares supplied gross
(N:L6287-L6294,L19820-L19827). Rounding is documented:
fractional0.005..0.009→0.01,0.001..0.004→0.00 (positive half-up),
N:L19987-L19989. Implement positive sale arithmetic in exact decimal/integer
operations, matching the app's half-up gross calculation; negative/STORNO
rounding beyond these stated examples is **[UNVERIFIED]** and not needed for
this app's sales.

Name limits by documented firmware: Bono1.10=40,210/300=60;
Deon1.40=40,220/310=60; HD1.60/210/320=60; HD II1.40/2.20/3.50=60; Point1.0=60.
N:L6210-L6231. Firmware strings as printed in the doc are not normalized (210
may mean2.10); exact version parsing/profile lookup is **[UNVERIFIED]**. Names
print original case; goods-database matching uppercases and strips many symbols
for tax-step protection, so different displayed names can share fiscal goods
identity. N:L6210-L6214,L6323-L6346. Never normalize/transliterate the printed
name to evade that protection.

Line errors1 RTC,2 checksum,4 index,16 name,17 quantity,18 VAT
inactive/invalid/ambiguous exempt/tax-step protection,19 price,20
total/discount/arithmetic,21 no transaction,22 STORNO/discount problem,90
deposit-only transaction,91 payment already sent. N:L6273-L6300. Any line
failure after begin is conservative **unknown**, not proof that earlier
lines/header did not print.

### 4.4 Advance / prepayment mechanisms: full audit

#### A. Fiscal advance and settlement `@z`

`F(action + ";@z" + item_name CR + VAT_letter CR + advance_amount + "/" +
surcharge + "/")`.
Action0=advance,1=advance settlement,2=advance STORNO,3=settlement STORNO.
item_name0..60, VAT letterONE byte, monetary fields as generic amounts;
surcharge/Do dopłaty is currently IGNORED, printer computes from the referenced
item's value. Only error2 listed (not an exhaustive proof all other errors
impossible). N:L8617-L8649.

Must be inside open receipt. Settlement attaches to the **first found item** on
the receipt. Item cannot be STORNO'd until its advance settlements have first
been removed. N:L8647-L8651. No explicit algorithm states whether match is
name+VAT, case-sensitive/canonicalized, whether empty name selects first item,
what happens for duplicate names/rates, or whether amount can exceed the
referenced item's value: all **[UNVERIFIED]**. It is NOT documented as a
free-standing per-VAT-total deduction.

XML cross-check only: XP:L4144-L4155 shows one `Telewizor` itemA500, settlement
same name/rate100 with doplata400, close400. XE:L3791-L3799 agrees, but has
trailing whitespace in item name vs settlement name. This supports
single-item500−100=400, not an arbitrary slot aggregate or exact matching rule.
XE:L648-L670 describes all four XML advance actions; XP:L4119-L4141 shows
standalone advance100 and reversal. None establishes safe native multi-item
allocation.

**[DESIGN candidate, not deployable guarantee]** To emulate per-slot deduction D
over item gross totals Gi without changing sale lines/order: deterministically
allocate `di=min(remainingD, Gi)` in original order among matching VAT-slot
lines; after that item's `$l`, issue `1;@z` with THAT exact original name, slot
letter, di, and suggested remainder `Gi-di` (ignored). Reduce remainingD until0;
do this before any `$b`. But this algorithm is valid ONLY if hardware/vendor
proves: (i) first-found matching selects intended item, (ii) repeated equal
names can be allocated without repeatedly modifying the first occurrence, (iii)
cumulative settlements don't exceed per-item value, (iv) zero remainder is
supported, (v) several settlements/rates preserve exact gross and VAT, (vi)
printed legal advance descriptions are appropriate. Without those proofs there
is NO exact general algorithm specified by the manual; reject unsupported jobs
before begin rather than split/reorder/rename products. Source constraints:
N:L8626-L8651; inability to reorder follows app unchanged-data contract
`README.md:161-164`.

Exact literal printed label/template for native advance/settlement (`ZALICZKA`,
`ROZLICZENIE ZALICZKI`, item-name placement etc.) is **[UNVERIFIED]**; the
chapter names the operation but provides no rendered legal receipt. Do not
invent the print text or force constant `Zaliczka` as lookup name: unlike Posnet
this field identifies the relevant product (N:L8626-L8628,L8649).

#### B. Per-PTU amount discount `$L` (not a proven legal replacement)

`F(slot + ";" + kind + ";" + description_id + "$L" + sales_before_discount +
"/" + discount + "/" + custom_description CR)`.
slot0..6=A..G, kind1=percent discount,2=percent markup,3=amount
discount,4=amount markup; optional description_id0..16,16=user-defined; custom
description0..20 optional. Sales is gross at selected PTU before adjustment.
N:L7198-L7251. This DOES target one PTU total independent of product
names/order, and kind3 deducts an exact amount. It prints information about a
**discount/markup to subtotal** and changes the selected PTU totalizer. However
discount **cannot zero totalizers**, and error82 says an adjustment was already
sent. N:L7257-L7269. Whether the zero prohibition means selected totalizer vs
all totalizers, and whether multiple different PTUs can receive separate `$L`
packets, are **[UNVERIFIED]**. It therefore cannot promise fully prepaid
zero-total receipts or several-slot parity.

Online adjustment arithmetic is specified further: amount discount converts to a
percentage, computes per-item discounts using direct method, then corrects the
difference by “dystrybucja groszy” so the applied sum equals requested amount.
The distribution cycles over positions in grosz quanta, skipping a position if
it would become negative; chapter marks this for Online from HDO2.10.
N:L19864-L19936. This supports exact requested aggregate adjustment, but
INTERNAL allocation depends on item order; firmware coverage for older Bono/Deon
and repeated per-rate adjustments remains **[UNVERIFIED]**. It still does not
turn a discount into a documented advance settlement.

Custom text `Rozliczenie zaliczki` (19 letters/spaces) fits20 bytes in a Polish
single-byte codepage, but a custom label does NOT establish fiscal/legal
equivalence to an advance settlement. **[UNVERIFIED]** legality and exact
receipt text. Do not silently use discount as payment settlement. Error set1
RTC,4 parameter,21 no receipt/deposit-only,26 discount,82 prior adjustment
(N:L7257-L7269).

#### C. Other discount forms and nonfiscal prepayment

- `$n`: `F(kind + "$n" + name CR + amount_or_percent + "/")`, kind0 resets prior
  adjustment AND ALL payments;1/2 percentage discount/markup,3/4 amount
  discount/markup. Must precede `$b` and `$y`. It has NO VAT selector.
  Errors1,4,21,26,82. N:L7161-L7194. Not a per-rate settlement replacement.
- `$Y`:
  `F(kind + ";" + description_id + "$Y" + subtotal + "/" + discount + "/" +
  description CR)`,
  optional description id0..16,custom text0..20; changes TOTALIZERS for receipt
  subtotal and cannot zero them; errors1,4,21,26,82. N:L7274-L7335. Not a
  selected-rate advance replacement.
- Per-item `$l` discount variants (N:L6084-L6152) offer amount/percentage
  discount but are discounts attached to items, not documented settlement; same
  no-substitution principle.
- Additional footer id10 `Zaliczka` is merely numeric text max25, not a fiscal
  deduction (N:L7498-L7502).
- Nonfiscal document8 `PRZEDPŁATA` has order/payment lines (N:L10014-L10028); it
  is in the nonfiscal-print chapter, not a fiscal receipt settlement command,
  and cannot satisfy app fiscal advance jobs.

Conclusion: `@z` is the only explicitly named FISCAL advance-settlement
mechanism in the primary manual; `$L` is the only clearly selected-PTU-total
amount subtraction candidate, but the documented restrictions and different
legal operation prevent claiming it as unrestricted replacement. Full per-VAT
Posnet `tradvance` parity remains **[UNVERIFIED]**.

### 4.5 Payments and close

Record each payment: `F("1;" + type + "$b" + amount + "/" + name CR)`,
name0..24, amount1..11. Action2 cancels instead and MUST NOT be used as
recovery. Native types:
cash0,card1,cheque2,voucher/bond3,other4,credit5,customer-account6,foreign-currency7,transfer8,**mobile9**,voucher10.
These differ from Posnet: card is1 not2, mobile9 not6. Transfer8 only in native
NOVITUS; compatible mode8 means currency. N:L6837-L6872. At most16 payment
forms; errors1,2,4,5,6,19 amount,22 cancellation exceeds accumulated,26 too many
forms. N:L6874-L6884. Send no zero-value payments; do not remap mobile to
other/credit.

Full `$y` syntax (all fields are significant, no omitted placeholders):
`F(nlines; + ending; + summary; + dsp_sign; + discount_kind; + ntaken; +
nreturned; + sysflag; + npay; + changeflag; + paidflag; + (paytype;)*npay +
discount_name_flag + "$y" + checkout CR + cashier CR + system_no CR + (line
CR)*nlines + (payment_name CR)*npay + (taken_id CR + taken_qty CR)*ntaken +
(returned_id CR + returned_qty CR)*nreturned + [discount_name CR] + total/ +
DSP/ + discount/ + paid/ + (payment_amount/)*npay + change/ +
(taken_amount/)*ntaken + (returned_amount/)*nreturned)`.
N:L6888-L6992. `discount_name` present if flag set; lines/deposit/payment blocks
absent when their corresponding count0, BUT system_no,discount,paid,change
fields MUST remain even when flags0 (N:L7131-L7157).
Checkout0..8,cashier0..32,system_no nominally1..60 digits (still required when
sysflag0),line0..40,payment name0..24. The same chapter contradicts longer QR
limits120vs512, so don't use QR for transaction ID (N:L6924-L6944,L7072-L7076).

Native ending values other than3 ignored; ending0 is standard,3 opens
additional-line sequence. When ending3 follows `$b`, set paid to sum of ALL sent
payment forms, no bundled payment entries/lines in `$y`; reconcile discount if
used, and deposit aggregate blocks if used. N:L7001-L7020.
No-deposit/no-discount candidate with previously recorded `$b` forms:
`F("0;3;0;0;0;0;0;0;0;0;1;0$y" + CR + CR + CR + fiscal_total + "/0/0/" +
payments_total + "/0/")`,
followed by `F("12$z")` (print payment breakdown), company/information footer if
supported, and `F("28;0$z")` to finish. Prefix comes directly from fields
N:L6896-L6923; numeric tail N:L6965-L6975; ending3 requirements N:L7001-L7015;
payment/footer-end commands N:L7444-L7470,L7894-L7924. **[UNVERIFIED]** this
exact composed candidate on every firmware, especially zero-total full
settlement; not a tested packet trace.

`12$z` prints payment forms gathered by `$b`, must follow `$y`, and `$y` must
omit forms; errors1,2,4,5,82. N:L7444-L7470. `28;0$z` or28;1 ends print and
transaction;28;2 prints a NEW receipt header and MUST NOT be used. `28;0$z` also
prints missing payment/deposit sections automatically. Errors1,2,4,5.
N:L7894-L7924.

If NO additional nonfiscal lines planned, manufacturer recommends standard
ending0 rather than3 (N:L7017-L7020). A standard `$y` can bundle payment
declarations; they override previously sent payment/deposit values
(N:L7131-L7138). Its descriptive type list only enumerates1..8 while `$b`
includes mobile9/voucher10 (N:L7057-L7065 vs L6856-L6868); mobile in bundled
close is **[UNVERIFIED]**. For full five-form support retain proven `$b` +
ending3 path or obtain vendor clarification instead of assuming unsupported
bundled codes.

`total/Razem` must equal actual receipt gross after adjustments; printer
controls it. `DSP` ignored native. Standard paid field documented as cash, but
ending3 specifically requires total of all earlier `$b` forms.
N:L7081-L7102,L7009-L7010. Advance `@z` example closes at reduced total
(XP:L4144-L4155; comparison only). Native fully prepaid zero receipt, no
payments, zero fiscal_total close are **[UNVERIFIED]**.

`$y` errors1 RTC,2 checksum,4 parameter,5 RTC I/O,6 fiscal memory,23 line
count,24 buffer overflow,25 checkout/cashier/extra text,26 paid/change
amounts,27 total/discount mismatch,28 totalizer overflow,29 transaction not
open. N:L7110-L7127. These are not guaranteed rollback/no-print refusals; treat
post-begin failures and uncertain close as unknown.

### 4.6 CompanyName / footer mechanisms and limits

**No arbitrary changeable text footer equivalent to Posnet id25 is
established.** A company already present in the programmed header needs no extra
line (app behavior `crates/posnet/src/printer.rs:244-246`); otherwise choose
only a verified faithful mechanism, never omit/truncate/transliterate it
silently (`README.md:161-164`).

1. **Begin additional lines:** `F(count; + n + "$h" + lines CR...)`, n0..3;
   maximum text length is NOT specified in the begin schema,
   administrative-source selection applies
   (N:L5991-L6001,L6035-L6039,L6065-L6066). Exact per-model max for THIS form is
   **[UNVERIFIED]**, not automatically the item-name limit.
2. **Close additional lines:** `$e` standard close has0..3 lines but no explicit
   max length in its schema; depends on configuration
   (N:L6610-L6652,L6667-L6669,L6715-L6716). `$x` has FIVE fixed line fields,
   each0..40 characters, cashier0..31, payment names0..16; it has no line-count
   selector (N:L6721-L6753). Its full alternative frame is
   `F(ignored1; + ignored2; + ignored3; + discount_kind; + cashflag; +
   cardflag; + chequeflag; + bondflag; + takenflag; + returnedflag; +
   changeflag + "$x" + cashier CR + line1 CR + line2 CR + line3 CR + line4 CR +
   line5 CR + card_name CR + cheque_name CR + bond_name CR + total/ +
   discount/ + cash/ + card/ +
   cheque/ + bond/ + deposits_returned/ + deposits_issued/ + change/)`
   (N:L6721-L6759). No transfer/mobile/credit fields are provided, so it is not
   the primary five-form close route. `$y` has0..3 lines0..40 each
   (N:L6896-L6897,L6935-L6939), not sent with ending3 (N:L7013-L7015). `$y` says
   one text set between daily reports; other sets ignored (N:L6996-L7000).

   Standard `$e` cash-only alternative, with a concrete no-discount header:
   `F("1;0;" + n + ";0$e" + cashier CR + (line CR)*n + paid + "/" + total +
   "/")`,
   n0..3. The optional percent field is explicitly supplied0, native ending
   field ignored, action1 confirms (0 cancels and MUST NOT be used for unknown
   recovery). Cashier0..31; line lengths unspecified. N:L6612-L6627,L6659-L6686.
   Extended `$e` can add discount-kind/flag and discount value (N:L6632-L6654),
   but it is not needed for the app and cannot replace five-form `$y` close.
   Both legacy `$e/$x` footer forms remain subject to daily constant-text
   restrictions in item3, not arbitrary-changing companyName routes.

3. **Administrative behavior applies to incoming extra lines:** setting `Brak`
   ignores; `Stałe` remembers FIRST receipt's text as declared daily text,
   prints subsequent identical text, ignores different text, and lists declared
   text in daily report. Source can be begin/stopka or close/nagłówek. A
   designated system-number line can vary and prints `Nr Sys.:`, but only when
   extra lines enabled. N:L20039-L20092. Thus changing companyName per job is
   not safe; even constant companyName requires verified configuration and daily
   declared-text state.
4. **Explicit informational definition after ending3:**
   `F("24;" + n + "$z" + lines CR...)`, n0..3, prints at final28$z, only ONE
   definition between daily reports. Max64 HDO/Point wide-normal;44
   wide-enlarged or narrow;42 Bono/Deon. HD II missing from THIS table, so its
   max here is **[UNVERIFIED]** (do not borrow another model's rule).
   Errors1,2,4,5. N:L7682-L7729.
5. **Predefined keyword footer after ending3:**
   `F("20;" + kind + "$z" + text CR)`. Many kinds; id25 is **numeric**, max25,
   no keyword; numeric arguments allow at most3 letters, extra letters REPLACED
   BY SPACES; literary arguments max3 digits, extra digits spaces. Therefore
   id25 cannot print an arbitrary companyName. Id32=`Nazwa` and id33=`Opis` are
   alphanumeric25 but ADD A LABEL; no demonstrated label-free unrestricted
   alphanumeric alternative. Enlarged-font footer on HDO/HDIIO/Point limits22
   characters. Errors1,2,4,5,84 (too many differing predefined strings).
   N:L7473-L7484,L7524-L7535,L7665-L7679. A labeled `Nazwa` line might preserve
   company bytes if <=25, but is NOT unlimited40-character parity and requires
   confirmation of limits/state; do not choose silently.
6. **Stored post-receipt print, PC2-only `@ws`:** start `F("1@ws")`, each line
   `F("2@ws" + attributes CR + text CR)`, finish `F("3@ws")`;4 cancels. Default
   attributes empty=normal single width/height; supported
   Wn,Hn,N(headline),I(inverse),U(masked-copy),E(barcode),Q(QR),G(graphic). Text
   max64 HD/HDII/Point wide-normal;44 wide-enlarged/narrow;42 Bono/Deon.
   N:L16134-L16189. **[UNVERIFIED]** staging order relative to receipt,
   triggering/attachment to WHICH receipt, max line count, whether text is
   separate nonfiscal document vs receipt footer, across-port
   association/exclusion, error codes/queue persistence and completion after
   printing. A PC2-only mechanism is not automatically usable from the app's ONE
   chosen PC/USB port. This is a promising investigation target, NOT production
   parity.
7. A separately opened `$w` nonfiscal document is not an in-receipt footer; do
   not create a second document merely to hide missing capability. Nonfiscal
   begin/line/end live at N:L9618-L9850. Likewise header programming `#a`/`!a`
   changes printer configuration (N:L4925-L4998,L5084-L5101), not a per-job
   footer workaround.

### 4.7 Completion and receipt number

After successful close/final28$z, read `?l` summary (below), confirm state
CLOSED and obtain printer receipt number/date/total/system number; distinguish
number of print from receipt number. Alternatively DLE2 includes last receipt
number + print number, and `51#s` counters include last receipt number.
N:L481-L483,L926-L1000,L12639-L12725. Do NOT use mode23's daily receipt count as
globally unique number. Numbering can be global/daily (`11$i`, N:L1776-L1778)
and counters reset on daily report if daily numbering chosen (N:L1000).

**[DESIGN]** Record preflight last document/receipt evidence and post-close
summary, ensure identity unchanged and no interleaving; same serial plus raw
counter increment alone does not uniquely correlate a job. No readable number
after success → completed-but-unknown according to app policy
(`README.md:180-185`); no reprint or command retry.

### 4.8 Certain refusal versus uncertainty

- Before ANY fiscal bytes: failed/preflight, no print from this operation
  (`.omp/RULES.md:6-12`; `README.md:186-190`).
- First `$h` matching refusal: certain no receipt issued; begin explicitly does
  not print (N:L6057-L6059). It can still be prudent to block further printing
  if another transaction is open, without cancelling it.
- Syntax/checksum failure of the FIRST fiscal command is explicitly ignored
  (N:L614-L615) and can prove no command execution only if reliably associated
  to that frame. Generic `#Z` execution errors are NOT a separate Posnet `ERR`
  pre-execution envelope (N:L660-L668).
- Daily error37 explicitly means keyboard interruption **before printing
  started** (N:L9146); after matching this single report command it is certain
  no report printed. Other errors need exact no-side-effect proof; generic6
  fatal fiscal-memory I/O is NOT such proof (N:L9141,L9238-L9243).
- Any fiscal write timeout, partial write, disconnect, malformed/unmatched
  acknowledgment, cancellation observed after any line, nonzero later
  `$l/@z/$b/$y/$z`, process death during dispatch → unknown unless durable
  independent evidence settles it. This is conservative Workshop policy, not a
  vendor rollback promise (`README.md:171-198`). Never turn TRF0/open-state into
  permission to reprint.

## 5. Reports

All use `F(body)`, matching mode3 `ACK("#r")` or `ACK("#o")`; no
command-specific result payload is documented in these print-command sections
(N:L9080-L9243 with generic ACK N:L660-L668). Complete
preflight/identity/journal, and never retry after silence. Use DLE2
user/report/printing flags while waiting (N:L484-L531).

### Daily fiscal report

Minimal `F("#r")` optionally followed by checkout CR,cashier CR before checksum.
This variant requires physical confirmation. Other variants
`F("0#r" + [checkout CR + cashier CR])` and date-guarded
`F("1;" + YY; + MM; + DD + "#r" + [checkout CR + cashier CR])`, YY last2
digits2000..2099. Optional checkout0..8,cashier0..32. N:L9080-L9135.

Prefer date-guarded form using printer's date just read: if same printer date
and nonzero totals, writes without confirmation; permits multiple daily reports
with that date; if zero totals and already reported today error36.
N:L9150-L9156. Errors1 RTC,2 checksum,6 fatal fiscal memory,7 date earlier than
last fiscal record or supplied date mismatch,35 zero totalizers,36 existing
same-date report while zero,37 keyboard cancellation BEFORE printing.
N:L9137-L9146. Only37 has explicit before-print guarantee; do not assume every
report error is certain no-write/no-print.

### Periodic by date and monthly

`F(YY1; + MM1; + DD1; + YY2; + MM2; + DD2; + kind + "#o" + [cashier CR +
checkout CR])`;
note optional field order is CASHIER THEN CHECKOUT (opposite daily). Kind0=full
fiscal periodic,1=nonfiscal summary,6=full fiscal monthly,7=monthly nonfiscal
summary,96/97=fiscal-mode closure/settlement reports and NOT ordinary periodic
options. N:L9158-L9199.

For requested monthly full report use same command with kind6 and a
calendar-month date range. Exact month selector convention (whether all supplied
days must equal1,last day, matching month/year rules, future/current-month
refusal) is **[UNVERIFIED]** in this section; do not fabricate a special
`monthly` command. **[DESIGN]** form first+last calendar day and enforce past
completed month only, pending firmware/vendor confirmation. For periodic summary
map app summary=true→kind1, false→kind0. Basis N:L9181-L9191; date validation
policy is app-level `README.md:131-135`.

Report errors1 RTC,2 checksum,3 number of parameters,4 dates invalid/reversed OR
no records in date interval,6 fatal fiscal-memory I/O. N:L9236-L9243. There is
no distinct rejection envelope guaranteeing nothing printed; classify uncertain
after dispatch unless specific proof exists. Date-by-report-number alternative
types16/17 exists (N:L9202-L9233) but not needed for app date-range capability.

## 6. Character encoding, printable set, and capability profiles

Input/output pages are separately configurable per logical PC port: options91/92
for PC input/output,93/94 for PC2,
values0=Mazovia,1=Windows1250,2=ISO8859-2,3=CP852. Tables say0..2 despite
listing3, so CP852 must be gated by capability bit. N:L13994-L14033. `52$i`
capability attr1 bit0x0200 means print ALL Windows1250 characters;0x8000 means
CP852 support. Reply `F("52$I" + attr1 CR + ... + attr8 CR)`, each attr eight
hex digits; N:L1879-L1961.

Read config `F("^c")` on PC1 returns
`F(option_count + "^c" + (number CR + value CR)*count)`; narrow range read
`F("0;1^c" + first_option + "/" + last_option + "/")`; single-option program
`F("0^c" + option_no CR + value CR)`, response same numbered-option pattern.
N:L13323-L13349,L15761-L15797. Set91 and92 to1 for CP1250 on PC,93/94 for PC2
only as explicit configuration step AFTER identity, never while a fiscal command
is pending. **[UNVERIFIED]** changing these persists/restarts, permitted source
ports beyond PC1, and settings for a PC3 USB channel. Prefer instructing manual
configuration and reading it before operations; no configuration writes during
discovery.

Printable ASCII excluding control bytes is the baseline claim,
N:L538-L546,L599-L602. Reject ESC1B,CR0D and all raw control bytes inside
arbitrary names/text; they are protocol structure, not printable data. Exact
allowed byte range0x80..0xFF on firmware WITHOUT all-CP1250 capability and
behavior of DEL/unassigned CP1250 bytes/NBSP/soft hyphen/table glyphs are
**[UNVERIFIED]**. The five unassigned CP1250 bytes are not thereby printable
merely because attr0x0200 says all characters. This manual has no complete
Unicode→printer glyph table. Do NOT copy Posnet's allowedCharacters list as
Novitus truth. Build exact telemetry allowlist only from a confirmed
codepage+firmware repertoire; no fallback transliteration/truncation
(`README.md:161-164`).

- **Profile — HD II Online1.40/2.20/3.50**: **Item name**: 60; **Numbered sale
  lines**: 1..255 documented; **`$y` extra line**: 40; **informational24$z**:
  [UNVERIFIED] HD II not named; **PC2 stored2@ws**: 64 wide-normal,44
  enlarged/narrow
- **Profile — Deon Online1.40**: **Item name**: 40; **Numbered sale lines**:
  same; **`$y` extra line**: 40; **informational24$z**: 42; **PC2 stored2@ws**:
  42
- **Profile — Deon Online220/310**: **Item name**: 60; **Numbered sale lines**:
  same; **`$y` extra line**: 40; **informational24$z**: 42; **PC2 stored2@ws**:
  42
- **Profile — Bono Online1.10**: **Item name**: 40; **Numbered sale lines**:
  same; **`$y` extra line**: 40; **informational24$z**: 42; **PC2 stored2@ws**:
  42
- **Profile — Bono Online210/300**: **Item name**: 60; **Numbered sale lines**:
  same; **`$y` extra line**: 40; **informational24$z**: 42; **PC2 stored2@ws**:
  42
- **Profile — Point1.0**: **Item name**: 60; **Numbered sale lines**: same;
  **`$y` extra line**: 40; **informational24$z**: 64 wide-normal,44
  enlarged/narrow; **PC2 stored2@ws**: same64/44

Sources: item model/firmware N:L6216-L6231; line indices N:L6072,L6180-L6181;
close lines N:L6935-L6939; informational N:L7682-L7699; stored lines
N:L16161-L16167. Count/size in bytes vs Unicode scalar values is not explicitly
distinguished, but these single-byte pages make encodable character count equal
byte count; combining marks/NFC handling **[UNVERIFIED]**. CompanyName maximum
is NOT generally a single model constant: it depends on header already
containing it, chosen footer mechanism, settings and daily-declaration state.
For arbitrary-changing names the supported limit is **not established**, not “40
everywhere.”

## 7. Read-only reconciliation evidence: last document and IDs

### 7.1 Last receipt summary `?l`

Request `F(kind + ";" + index + "?l")`, kind0=summary,index0..255; PCs PC/PC2.
Each item must be requested separately; out-of-range returns empty
`F(kind+"?l")`, not a fatal error. N:L12608-L12635. Use `F("0;0?l")` after
completion and optionally before first fiscal command.

Reply kind0 exact field order:
`F("0?l" + state/ + receipt_type/ + receipt_no/ + datetime CR + print_no/ +
system_no CR + checkout CR + cashier CR + item_count/ + deposit_count/ +
payment_count/ + fiscal_gross/ + discount_type/ + discount_value/ +
discount_name CR + after_discount/ + discount_method/ + deposits_taken/ +
deposits_returned/ + to_pay/ + cash/ + card/ + cheque/ + bond/ + other/ +
credit/ + account/ + currency/ + transfer/ + cash_euro/ + card_name CR +
cheque_name CR + bond_name CR + other_name CR + credit_name CR +
account_name CR + transfer_name CR + euro_rate/ + change_pln/ + change_euro/)`.
state0=empty,1=open,2=closed,3=cancelled; receipt_type0=normal,1=deposit
settlement,2=pharmacy,3=invoice,4=ticket,5=pharmacy invoice; datetime
`YYYY-MM-DD HH:MM:SS`. N:L12639-L12725. This reply includes receipt number,
print number, system number and amounts, so it is stronger evidence than raw
TRF.

Kind4 adds mobile/voucher sums and names (N:L12832-L12935); kind6 adds buyer NIP
to extended summary (N:L12980-L13070). Kind1 items include
names/code/quantity/VAT/price/gross/discount; name table says2..40 despite
current60-character sale commands: round-trip long-name fidelity
**[UNVERIFIED]**. N:L12730-L12782. Last-receipt storage duration across
reboot/new begin, transactional snapshot consistency during reads, and retrieval
by a host id rather than ONLY last receipt are **[UNVERIFIED]**. Never use a
matching amount alone to resolve an unknown job.

Do not reuse payment IDs returned by kind3 as input `$b` codes: kind3 readout
lists7=transfer,8=euro,9=mobile (N:L12820-L12843), whereas native `$b` input
lists7=currency,8=transfer (N:L6856-L6868). Decode each packet's enumeration
independently.

### 7.2 Counters, TRF, and system number

DLE2 includes last print and receipt numbers (N:L481-L483); `Q("51#s")` returns
`F("51#X" + zero_totalizers/ + document_counter/ + documents_since_daily/ +
receipt_counter/ + cancelled_receipt_counter/ + receipts_since_daily/ +
cancelled_receipts_since_daily/ + invoice_counter/ +
cancelled_invoice_counter/ + invoices_since_daily/ +
cancelled_invoices_since_daily/ +
nonfiscal_since_daily/ + valuation_count/ + cancelled_valuation_count/ +
valuations_since_daily/ + cancelled_valuations_since_daily/ + receipt_jpk/ +
cancelled_receipt_jpk/ + invoice_jpk/ + cancelled_invoice_jpk/ +
nonfiscal_jpk/)`;
counters vary with numbering configuration and daily resets. N:L926-L1000. These
are useful evidence, not replay permission.

`$y` system_no permits a numeric per-transaction identifier, returned by `?l`;
set sysflag1, include only digits within60 characters.
N:L6924-L6934,L7040-L7042,L7072-L7076,L12669-L12671. **[DESIGN]** a durable
numeric encoding of an attempt could aid correlation, but no
idempotency/deduplication or query-by-system_no guarantee is documented. Never
interpret this as safe device retry.

### 7.3 E-receipt transaction ID is not a generic idempotency token

`F(type + ";" + [protocol + ";"] + "@e" + id CR)`, ID0..100 letters/digits,
protocol0=NOVITUS,1=MF; type2 clears. Basic setter type other than2 is NOT
specified by this table, hence **[UNVERIFIED]**. Errors4 bad parameter,21 no
transaction,1504 e-receipt inactive. Must be inside receipt. N:L7816-L7841.

Extended
`F("3;" + protocol_flag; + mode_flag; + "@e" + id CR + [protocol/] + [mode/])`,
mode0=print on no server confirmation/no retry,1=retry e-receipt until
success,2=print plus retry,3=always print plus retry. N:L7843-L7891. This
changes e-receipt delivery/paper behavior and may introduce device-managed
resend semantics; it is NOT necessary to emulate paper receipt identity. `?e`:
`F("?e")` → `F("?e" + status/ + print_no/)`, status bit1 sent,2 printed,3
queued,4 new MF KID required, N:L4259-L4295. Do not require/enable e-receipts
merely to get a correlation ID.

### 7.4 Protected-memory readback (never reprint)

Every protected-memory document has JPKID independent of receipt/report/print
numbers, starting1 for each protected-memory medium, with
JSON/print-data/metadata sections. Medium can change during printer life, so key
by physical printer+medium ID+JPKID, not JPKID alone. N:L17798-L17820. Vendor
example read status `F("210#s")` shown literally as `S210#s9CE`, response
`S210#X0000;ZBN1901007722.0000 CR 3;0;2954;1024;FE E`; detailed field semantics
beyond the example remain **[UNVERIFIED]** here. N:L17821-L17824.

Record query
`F(state + "@PMI" + jpkid + "/" + include_idz + "/" + content + "/" +
document_type_mask + "/")`;
state0=find,1=next; content0=JPK,1=print-copy,2=metadata (table range
erroneously0..1 while listing2); types1 fiscal report,2 receipt,4 cancelled
receipt,8 invoice,16 cancelled invoice,32 daily,64 nonfiscal,128 event. Reply
`F("@PMD" + status/ + type/ + size/ + offset/ + jpkid CR + buyer_code CR +
encoded_data CR)`,
status0=data,1=end,2=no copy/metadata,9=error; data DEFLATE+Base64URL.
N:L17385-L17451.

Query by daily-report number+receipt:
`F(state + "@PMJ" + daily_report_no/ + receipt_no/ + content/)`, same `@PMD`
reply. N:L17456-L17506. This READS a copy; never use protected-memory PRINT
commands as recovery. Native error lists/firmware support/permission grants and
external JSON schema/IDZ meanings **[UNVERIFIED]** in these packet sections.
Such evidence can support HUMAN reconciliation; without unambiguous durable
attempt correlation it must not automatically unblock unknown.

## 8. Hardware/vendor confirmation gates (no real-printer exercise in this task)

1. **VAT sentinels** returned in native vs compatible mode, and read-only way to
   identify active protocol dialect: primary contradiction
   N:L1203-L1205,L4769-L4773 versus L20028-L20037.
2. **DLE2 hex byte order**, actual complete samples and flags on
   paper-out/cover/user/menu/fiscal reports;1-based bits are documented, textual
   byte ordering is not N:L475-L533.
3. **USB CDC/PC port mapping**, baud and modem-line effects, serial-default flow
   control and per-OS connection lifecycle; N:L17047-L17075,L20325-L20354 do not
   define CDC.
4. **Mode3 ACK ordering**, initialization mode-switch ACK, data+ACK exceptions,
   errors beyond two digits and how error evidence survives asynchronous status
   reads; N:L631-L719.
5. **Preflight ownership/queue**: no documented all-interface exclusion owner
   equivalent; validate exclusive native session and how other interfaces
   contend without modifying transactions, N:L470-L533,L15848-L15874.
6. **Advance settlement** exact matching/canonicalization, duplicate product
   names, several lines/rates, partial and full per-item/whole-receipt
   deduction, item-based allocation, zero totals, exact fiscal printed labels;
   N:L8617-L8651. XML one-item example is not proof of aggregate behavior
   XP:L4144-L4155.
7. **Per-PTU `$L`** whether multiple rates can be adjusted, which zero totals
   are forbidden, and whether it can legally represent advances; do not
   substitute discount without explicit fiscal approval N:L7198-L7269.
8. **CompanyName** daily text declaration/config readback; all permitted
   changing alphanumeric label-free footer routes, HD II informational width,
   @ws placement/attachment/queue/ACK/port2 semantics
   N:L7473-L7729,L16134-L16189,L20039-L20092.
9. **All current model/firmware profiles**, original returned identity strings,
   long-name support and round-trip?l40-vs60 discrepancy, online line
   counts>255, adjustment line-count consumption N:L6216-L6231,L6072,L12739.
10. **Printable repertoire** per firmware and codepage, CP1250
    unassigned/control-like bytes, CP852 option-range contradiction,
    font-dependent glyph replacement N:L13994-L14033,L1916-L1935.
11. **Clock epoch**, year range2049/2099, local zone/DST fold/offset behavior
    and actual issued timestamp precision; N:L2085-L2100,L9122-L9127.
12. **Close sequence** native `$b` + `$y` ending3 + payment/footer/end
    acknowledgment timing, zero-payment fully prepaid close, five payment types
    including mobile9, standard bundled close compatibility
    N:L6837-L6884,L6888-L7157,L7894-L7924.
13. **Report monthly date conventions**, current-month/future rules and exact
    before-write/before-print refusals; report completion and user-confirmation
    timing N:L9080-L9243.
14. **Reconciliation durability** last-receipt snapshot/reboot retention,
    numeric system ID correlation, protected-medium/JPK retrieval and schema,
    safe identity-consistent reads after uncertainty
    N:L12608-L12725,L17385-L17506,L17798-L17834.

No repo files were changed and no printer was contacted. This is a protocol
research deliverable, not exercised driver verification.
