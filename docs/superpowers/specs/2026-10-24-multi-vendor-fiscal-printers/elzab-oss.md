# ElzabSTX supplementary SDK / integration research

Read-only research, 2026-10-24. No printer contacted, no application or vendor
executable run against a printer, no repository files modified. Offline CRC
calculations only. **The fully prepaid zero-net receipt blocker is NOT
settled.** The most useful new evidence is: independently confirmed CRC
arithmetic, current ONLINE manuals' 64-character names and 9600-baud default,
actual integrator firmware identifiers, SDK completion-wait APIs, and a warning
not to treat SDK-buffered footer calls as native LAL ordering evidence.

## Sources

- **P**, **B**, **E**, **O** below are members of the
  [official programmer archive](https://www.elzab.com.pl/ftp/kody/om_iprg.zip).
  They are respectively
  `POLISH/ElzabSTX/Protokół ElzabSTX dla drukarek fiskalnych.pdf` (edition 0.36,
  2025-12-19), `Podstawy protokołu ElzabSTX.pdf` (0.17),
  `Lista błędów protokołu  ElzabSTX - DRUKARKI.pdf`, and
  `Drukarki online - obsługa protokołu.pdf`. References are lines in the
  corresponding `elzab-*.txt` text aliases listed below,
  exactly as used by the original vendor note.
- **H**, **C**, **CH**:
  [official ElzabDR ZIP](https://www.elzab.com.pl/ftp/kody/elzabdr.zip):
  `elzabdr.h` v6.107.4, `Examples/Receipt/source/main.cpp`, and `CHANGELOG.md`;
  extracted with `unzip elzabdr.zip -d elzabdr`. H/C/CH line numbers address
  these three members directly; no binary execution or decompilation.
- **J**:
  [official Java/Kotlin SDK
  ZIP](https://www.elzab.com.pl/ftp/pub/kody/elzabdrJAR.zip),
  download as `elzabdrJAR.zip`; run `unzip elzabdrJAR.zip -d elzab-extra`.
  **J/** names archive root directory `elzabdrJAR/`;
  `version.txt:1` says **2.00.20**. Public API HTML under
  `Documentation/HTML/elzabstx/pl.com.elzab/-elzab-dr/`; the substantive method
  descriptions are generally on HTML line 59.
- **Z**:
  [Zeta ONLINE expanded
  manual](https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_zeta_online.pdf),
  v2.1, text alias `elzab-zeta.txt` (PDF `elzab-zeta.pdf`).
- **M**:
  [Mera ONLINE
  manual](https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_mera_online.pdf),
  v2.9, text alias `elzab-mera.txt` (PDF `elzab-mera.pdf`).
- **Q**:
  [Cube ONLINE expanded
  manual](https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_cube_online.pdf),
  v2.4, text alias `elzab-cube.txt` (PDF `elzab-cube.pdf`).

Download the programmer archive as `om_iprg.zip`; extract Polish filenames
with `unzip -O CP852 om_iprg.zip -d om_iprg`. All P/B/E/O PDF members are
under `POLISH/ElzabSTX/`. Run `pdftotext -layout <pdf> <txt>` for each
member, quoting shell paths containing spaces; `<pdf>` is `om_iprg/` plus
its member path. Preserve the two spaces before ElzabSTX in E's PDF name.
Text aliases are:

- **P**: `elzab-Protokół_ElzabSTX_dla_drukarek_fiskalnych.txt`.
- **B**: `elzab-Podstawy_protokołu_ElzabSTX.txt`.
- **E**: `elzab-Lista_błędów_protokołu__ElzabSTX_-_DRUKARKI.txt`.
- **O**: `elzab-Drukarki_online_-_obsługa_protokołu.txt`.

For Z/M/Q, download the linked manuals as the named PDFs and run
`pdftotext -layout <pdf> <txt>` with their paired text alias names above.
J's cited HTML files are read directly, without text conversion. License
references name the PDF members `elzabdr/ElzabDR licence.pdf` and
`elzab-extra/elzabdrJAR/ELZABDR.JAR licence.pdf` directly. No executables
need to run to reproduce these extracts.

- **USB/elzab/**: [official CDC package][elzab-cdc],
  Communication Device Class Virtual Serial Port (HCC Embedded),
  DriverVer 08/04/2010,2.0.0.0. Save as `elzab-usb.zip`; run
  `unzip elzab-usb.zip -d elzab-usb`; this alias names that directory.
  Read member `usb_cdc_ser.inf` directly; no text conversion.
- **EXTRA/**: supplementary [linuxip tools][linuxip] and
  [ecrinst programmer instructions][ecrinst], with the latter manual dated
  2026-06-25. Save as `linuxip.tar.gz` and `ecrinst.zip`; create directory
  `elzab-extra`, then run `tar -xzf linuxip.tar.gz -C elzab-extra` and
  `unzip ecrinst.zip -d elzab-extra`. EXTRA/ names that destination;
  `ecrinst/ecrinst.pdf` is an archive member read directly. The tar members
  are the tools named below; none were executed or disassembled.

[elzab-cdc]: https://www.elzab.com.pl/download/program/Usb_cdc_ser.zip
[linuxip]: https://www.elzab.com.pl/ftp/pub/kody/linuxip.tar.gz
[ecrinst]: https://www.elzab.com.pl/ftp/pub/kody/ecrinst.zip

### Licensing and what was not done

ElzabDR is **proprietary freeware, not open source**. Its supplied
`ElzabDR licence.pdf:5-8` allows commercial/noncommercial use/copy/distribution
but expressly prohibits modification, decompilation and disassembly. The Java
ZIP's `ELZABDR.JAR licence.pdf:5-8` has the same prohibition. Thus only supplied
headers, examples, changelogs and published API documentation were read; neither
binary was decompiled/disassembled. C's example carries an additional
proprietary/all-rights-reserved notice at `C:1-9`. Do not copy it into the Rust
implementation. These restrictions also mean a missing transport implementation
was not 'recovered' from binaries.

## 1. Fully prepaid final receipts / LDS / zero LCL total

**STILL OPEN: no source located establishes a successful native ONLINE exchange
with LDS consuming the entire taxable sale and LCL closing a zero-net receipt.**

What is actually established:

1. P:4131-4166 specifies ONLINE advance settlement as the LDS extension with
   subfield D present, an amount discount, apply-not-cancel, and an explicit
   tax-rate symbol. It does not state an exception allowing a zero LCL total.
   H:1391-1418 exposes
   `discountOrSurchargeToSubtotalSTX(... advancePayment=1 ...)` for ONLINE and
   newer, but gives neither full-prepayment semantics nor a worked wire
   sequence.
2. E:229-238 says error 1000 is settlement larger than sales in the tax rate,
   1001 is a nonpositive settlement, and **1008 is zero/negative sum SENT IN THE
   RECEIPT-END SEQUENCE**. This wording alone does not resolve whether LCL's
   asserted total after LDS is the original sale sum or the remaining sum.
   P:4903-4918 calls LCL A the receipt/invoice total; it does not define the LDS
   interaction precisely enough to select the former interpretation safely.
3. The SDK has a _second_, item-oriented function
   `SettleAdvance(productName, value, taxSymbol)` at H:2076-2088. The name must
   match a current receipt sale item and its tax rate. This is **not evidence
   that the LDS rate-level extension can close a fully prepaid receipt**, nor
   proof that SettleAdvance has a zero-total exception. No such example is
   supplied. The English header calls its `value` a base-currency-unit amount,
   while the Polish text just calls it the advance amount; do not infer its
   scaling from other API methods without further evidence.
4. The manufacturer itself forbids implementing advance settlement through
   arbitrary negative/storno sale items on ONLINE: O:15-21 says only whole
   existing positions may be voided and advance settlement has its own STX
   command. This rules out substituting an old negative-line workaround.
5. **Real integration support exists, but not proof of this edge case.**
   [Novitus SOHO
   changelog](https://oprogramowanie.novitus.pl/?inc=aktualizacje&news=SOHO&opc=update),
   2026-01-22 v4.04b, says “poprawiono rozliczanie zaliczek na drukarkach ELZAB”
   and “poprawiono błędne wysyłanie pozycji rozliczenia zaliczki do drukarki
   fiskalnej.” The
   [SOSPA
   changelog](https://oprogramowanie.novitus.pl/?inc=aktualizacje&news=SOSPA&opc=update)
   has the same entries. Neither gives a trace, total calculation, firmware
   prerequisite, nor fully prepaid example. Treating these entries as zero-net
   confirmation would be unsupported.
6. [Mico's integrator
   article](https://mico.pl/baza-wiedzy/rejestrowanie-zaliczki-na-kasie-fiskalnej-jak-zrobic-to-poprawnie/),
   section “Paragon zaliczkowy i końcowy,” explains dedicated Elzab/Posnet
   settlement functions and reduction by prior payments, but does not address a
   zero remaining sum or native printer commands.

**Do not settle this from search summaries.** A search summary for
[Salesystems FAQ](https://kasy-fiskalne.elblag.pl/FAQ_Najczestsze_Pytania)
claimed an Elzab zero-total example; reading the page showed the relevant FAQ
context is Novitus. It is not Elzab proof. No zero-total exception was found in
the actual sources inspected. No tax/legal conclusion that a final fiscal
receipt should be skipped is established here either; that would change
application behavior, not resolve the protocol gap.

Required missing evidence is specific: an Elzab vendor statement or successful
ONLINE-model/version trace specifying sale sum, each LDS settlement, actual LCL
A value, response, and resulting receipt. Until then, keep fully prepaid final
receipts blocked before fiscal dispatch rather than changing totals, adding a
dummy grosz, replaying, or silently omitting settlement.

## 2. LAL footer/company text and LNP timing

### Native LAL ordering: STILL OPEN

P:4828-4841 establishes that LAL prints additional lines below the fiscal logo,
one line per command, up to ten (`P:418-419`), with the same line grammar as
RNP. It does **not** say whether these commands must be sent before or after
native LCL, when the footer physically prints/cuts, or how an interrupted footer
affects completion.

The SDK examples are easily misread:

- C:603-605 calls `pFillLines` while reading receipt input, before `ReceiptEnd`
  at C:641-656. C#: `Examples/Test printout - example C#/Program.cs:115-128`
  likewise fills lines before ending the receipt.
- But H:2136-2143 describes `xFillLines` as **storing** additional text to print
  at the end; `EraseLines` clears stored lines (H:2043-2048). H:1776-1905
  describes their printed location below the fiscal logo. J's
  `add-nonfiscal-line-under-fiscal-printout.html:59` only says “additional line
  after fiscal logo”; it supplies no native call sequence.

Therefore **a pre-ReceiptEnd call to a buffered SDK helper does not prove
pre-LCL native LAL is legal**. The library could emit it during its own
receipt-end processing; the implementation is unavailable under the license
above. Do not manufacture an additional footer-end command or reuse ESC
finalization ordering.

### NIP: SETTLED at SDK/API workflow level; strict native timing STILL OPEN

P:4578-4589 defines `LNP GS OA<nip> [RS B<additional-line>]`. SDK C:630-632
invokes `pReceiptPurchaserNIP` during the input-processing loop, before
`ReceiptEnd` at C:641-656. This is genuine manufacturer example evidence for
**setting purchaser NIP during receipt construction before API-level close**,
rather than printing it as a later nonfiscal footer.

J documents `addNip(NIP)` and `addPurchaserInformation(NIP,address)`
(`add-nip.html:59`, `add-purchaser-information.html:59`); old
`receiptPurchaserNIP` is deprecated in favor of those methods
(`receipt-purchaser-n-i-p.html:59`). None specifies “must precede first LIT” or
gives a direct LNP-after-LOR trace. Exact allowed native state transitions and
latest safe insertion point remain open. Do not equate an optional LNP address
line with arbitrary company branding.

## 3. Daily RRP versus synchronous/asynchronous reports

**SETTLED: a positive ONLINE daily-report RRP response is acceptance, NOT
completion.** This is explicit in P:10229-10236, with P:10265 again saying it
reports initiation or absence of printable data. Daily report kind 1 and
protected-memory-only daily kind 12 are distinct (`P:9546-9577`), not alternate
spellings of completion.

**SETTLED: RPP, not RRP, has documented sync/async selection.** P:9493-9505 says
RPP flag 0 waits until printing ends; flag 1 replies immediately. P:10278-10288
defines optional S and defaults omitted S to synchronous 0. RRP's listed fields
are only I/J/O (`P:10224-10227`): do not attach RPP's S flag to a daily RRP
command or assume omitted S makes RRP synchronous.

New corroboration from SDKs:

- H:2940-2948 `PrintControl(BeforePrinting=0)` explicitly waits until printing
  ends; `=1` returns immediately. CH:82-84 (v6.103.1, 2024-03-06) specifically
  records faster **STX** print control. This is evidence that the supported
  integration workflow distinguishes issuing a command from waiting for physical
  print availability, even though the header links an older ESC chapter for
  detail.
- J `is-receipt-end.html:59`: “check if receipt is saved. Using after
  ReceiptClose. Only online devices.” `wait-until-receipt-en.html:59`: wait
  until receipt end after ReceiptClose. These are **receipt**, not daily-report,
  APIs. They prove no daily-report completion condition.
- H:2187-2233 offers separate daily APIs using library print settings, device
  print settings, paper, or internal memory. They return API success/error, but
  the public docs do not reveal their internal RRP/status sequence or guarantee
  that a return of 0 means paper complete.

Available native evidence remains useful but incomplete: ySR package 2 bit 81 =
occupied print buffer; package 3 bit 0 = printing, bit 3 = a document
interrupted for no paper, bits 1/2/4 = no paper/fault/head raised
(`P:9959-9963,10095-10105`). RNR/RDR can provide fiscal-memory report evidence.
**No obtained source defines a safe daily-report completion algorithm combining
those readings, mandates a busy edge, or proves paper completion/attribution
after restart.** An idle snapshot could occur before the report starts; a report
record need not prove all paper output completed. Daily completion remains STILL
OPEN, and an accepted report must not be marked completed merely on RRP success.

## 4. Transport / CRC / retransmission / pending-command status

### Framing and command IDs: SETTLED by the primary transport specification

B:128-139 and B:148-169 define `STX header FS data CRC16-as-four-ASCII-hex ETX`,
with
`header = command_id GS receiver_status GS frame_no GS max_frame_no GS
data_transformation`
plus optional direction/sender where applicable. Header/data cannot contain
STX/ETX; FS cannot be in the header. B:300-332 allows omitted/default fields and
removing trailing GS, so compact `STX "1" FS "LOR" CRC ETX` is legal. P:281-287
restricts printer header applicability; do not import an unrelated cash-register
directional header.

B:158-169: command ID is decimal 0..999999999, at most nine characters; use a
different ID for each next command (increment is recommended). B:296-298: reply
has the request's ID. **It is response correlation, not an execution idempotency
token.** The receiver/status header field is an addressing filter in a request
and a transmission status in a reply (`B:181-196`), not a fiscal-success code.
Generic ARC framing is not ESC's single-byte ACK/NAK.

### CRC arithmetic: independently SETTLED; observed native vendor frame STILL OPEN

B:342-362 explicitly specifies CRC-16/ARC, initial 0, polynomial x16+x15+x2+1;
CRC covers first header byte through last data byte (including FS) and is
rendered high nibble first as four hex ASCII digits.

The
[RevEng catalogue's CRC-16/ARC
entry](https://reveng.sourceforge.io/crc-catalogue/16.htm#crc.cat.crc-16-arc)
independently gives poly=0x8005, refin/refout=true, init/xorout=0, check=0xBB3D.
I downloaded **crccheck 1.3.1** from
[PyPI](https://pypi.org/project/crccheck/1.3.1/) and evaluated its
`crccheck.crc.Crc16Arc.calc` offline, directly from the wheel. Package metadata
says MIT; source is
[MartinScharrer/crccheck](https://github.com/MartinScharrer/crccheck),
`crccheck/crc.py`, class `Crc16Arc` (source HEAD inspected:
`94f1b0967fdb59428e2702391fe3390a41c6ead1`). This independently reproduces the
original note's bitwise calculations:

| Input bytes                      | Result |
| -------------------------------- | ------ |
| ASCII `123456789`                | `BB3D` |
| `31 1C 4C 4F 52` = ID 1, FS, LOR | `4E0E` |
| `31 1C 58 54 52` = ID 1, FS, XTR | `BA44` |

Thus full calculated LOR frame is `02 31 1C 4C 4F 52 34 45 30 45 03`; XTR is
`02 31 1C 58 54 52 42 41 34 34 03`. **These are independently computed frames,
not captured printer exchanges or vendor-published numerical worked examples.**
The original note's request for an independent arithmetic cross-check is
satisfied; hardware acceptance/vendor trace is not.

### Retransmission and ySR during execution: STILL OPEN

B:281-294 establishes replacement of matching frames **while assembling a
message** and discarding an incomplete message on a new command ID. It does not
establish suppression of a completed one-frame fiscal command, cached-result
lifetime, persistence, or safe replay after disconnect. B:119 says
retransmission is possible; B:359-362 says a wrong CRC value gets no reply.
Silence therefore cannot distinguish corrupt request, lost answer, executing
command, or completed operation. No observed trace or implementation settled
completed-command deduplication.

Likewise no inspected SDK documentation/source establishes that a new-ID ySR can
be serviced while a synchronous fiscal command is pending, or what that does to
its late response. General transport correlation does not prove concurrent
command execution is supported. **Do not send a new frame into an incomplete
multi-frame command**: that case explicitly discards its assembly. Keep
Workshop's no-fiscal-retry policy; never infer replay safety from use of the
same command ID.

## 5. XTR/FNR identity / ONLINE model matrix

**Partially SETTLED:** P:6683-6692 gives XTR fields B actual model name, C model
number, D/E firmware name/number, F controllers, G/H group/subgroup, I
additional identifiers, J/K controller firmware, L/M modifiable names.
P:6363-6377 gives FNR → empty-error-field + `GS B<unique_no>`; P:3255-3257 and
P:477 define unique number as a printable string of 1..14 characters. **FNR is
an instance identifier, not a proven model/generation selector.** Do not use
modifiable XTR names as the only identity evidence.

The P constants do explicitly give **CUBE = 172**, name `ELZAB CUBE online`,
subgroup `CUBE` (`P:323-324,344,357`). That is primary protocol evidence for
Cube. P also gives legacy ZETA=150 / `ELZAB ZETA`, MERA+=160 / `ELZAB MERA+`
(`P:339-356`), but **does not map these constants to all currently sold Zeta
ONLINE and Mera ONLINE generations**. Do not reuse 150/160 solely from
marketing-name similarity.

New real integrator evidence:
[Paragony.pl e-receipt compatibility list](https://paragony.pl/e-paragony),
section “Kompatybilność z najpopularniejszymi drukarkami na rynku,” explicitly
supports STX with:

- **Zeta ONLINE MD09.02 or MD19.01**;
- **Mera ONLINE MD07.02 or MD17.01**;
- **Cube ONLINE MD06.02**.

This is valuable firmware-family evidence, including more than one Zeta/Mera
generation, but it is **not a captured XTR response and does not establish their
decimal XTR C values**. A vendor SDK recognition bug history reinforces caution:
CH:14-24,38-40,54-56,86-89 documents added missing devices/identifiers/names and
detection fixes. No trustworthy complete XTR/FNR transcript for the three
current models was located. Exact native identity matrix beyond documented Cube
constant remains STILL OPEN.

## 6. Encoding, printable characters and dimensional limits

### Code pages: SETTLED (supported choices and real integration convention)

All three current manuals list **CP852 (Latin2), CP1250 (Windows), MAZOVIA
(CP790)**: Z:1528-1544, M:1425-1435, Q:1418-1428. P:3229-3236 gives matching
wire names `CP852`, `CP1250`, `MAZOVIA`. J `read-language-settings.html:59`
confirms those strings; H:2695-2701 confirms SDK numeric results 0/1/2 for the
same pages. This does not prove a factory default; read actual configured page
instead of assuming CP1250.

[GoPOS's current Elzab setup
guide](https://help.gopos.pl/pl/articles/5089613-drukarki-fiskalne-elzab-konfiguracja)
explicitly covers Mera/Zeta/Cube, selects **Elzab STX**, and requires **CP1250**
for Polish characters. It says default sales-interface license count is one;
additional concurrent POS interfaces need authorized-service licensing.
[elzAPP POS setup by
kasy.pl](https://kasy.pl/blog-wspolpraca-z-drukarkami-fiskalnymi-elzab-elzapp-pos)
lists all three ONLINE models for STX and says keep “Rozłączać po każdym
rozkazie” off. Neither is a wire-level concurrency/retry guarantee.

### Item names and lines: partly SETTLED, with a real documentation conflict

Current model manuals explicitly say **1..64 characters in item names**,
descriptions **3×35 characters**, and distinguish name-comparison characters
from spaces/case: Z:1971-1989, M:1867-1875, Q:1833-1858. M's ordinary print
mechanism is 42/21 columns (`M:1849`); Cube offers 64/32 or 48/24 columns
(`Q:1811-1814`) and configurable 3-inch 48/64 or 2-inch width (`Q:1570-1576`).
**A 64-character item-name capacity does not imply every free-text line is 64
characters or that the name stays on one physical line.**

However, P:765-775 still ties art_name to `<line_max_len>` and documents
normalization (uppercasing; nonletters/non-digits except comma/period/slashes
converted to spaces; repeated spaces collapsed). The current product manuals
count percent among significant name characters, unlike that generic P
normalization text. So the old blanket 42-character/old normalization rule is
**CONTRADICTED as a universal description of current ONLINE capability**, but
the exact native LIT length/normalization on every generation is not fully
reconciled. Prefer actual model/firmware capability evidence; do not silently
truncate or assume printed glyphs equal the name-comparison key.

P:569-573 calls printable characters codes ≥0x20 and uses configured encoding
for Polish diacritics. It does not supply a complete byte-to-glyph repertoire
for every chosen page. Z:816-819 / M:783-786 / Q:826-829 show a local-menu input
alphabet with ASCII and Polish letters; this is **not proof that all defined
CP1250 symbols, controls such as DEL, or arbitrary UTF-8 are printable**. Full
glyph whitelist remains open.

### Quantities and receipt capacity

**SETTLED at protocol level:** maximum quantity fractional precision is **5
digits**, P:420-421,781-785; decimal separator is comma. Amount precision is 2,
P:460-461,564-568. The Java SDK explicitly accepts an integer quantity plus MP
(digits after decimal), `add-receipt-item.html:59`; do not inherit another
vendor's 3-decimal cap. This is documented protocol support, not a test of every
firmware.

**STILL OPEN:** maximum count of receipt items, numeric magnitude ceilings
beyond the stated type precision, exact multiplication/rounding, and per-model
buffer capacity. The manuals' **500,000 PLU** is the database size (Z:1973,
M:1869, Q:1835), **not** items per receipt. The status “open receipt buffer
nearly full” (`P:9981`) is not a numerical limit. No maximum count found in SDK
public docs/examples or real-world source.

## 7. USB descriptors on Linux

**STILL OPEN for Zeta/Mera/Cube ONLINE exact normal-mode VID:PID, iManufacturer
and iProduct.** Peer research (`UsbIdsOSS`) separately searched Linux
observations and supplied driver files: no model-attributed ONLINE lsusb/dmesg
evidence was found. The manufacturer ONLINE driver
[programs download
page](https://www.elzab.com.pl/pl/strefa-dla-klienta/do-pobrania/programy)
links CDC support; `USB/elzab/usb_cdc_ser.inf:22-32`
matches class/subclass/protocol, not a model VID/PID.

The old official Android Zeta/D10 sample uses STM **0483:5740**, but plain
Zeta/D10 are separately listed legacy devices, not proof of an ONLINE mapping
(see programmer download-page sections; existing `usb-backend.md` citations). Do
not promote that generic STM virtual-COM pair or bootloader IDs into the
supported-online-device allowlist. Manuals prove the serial
facility—Z:1958-1960, M:1852-1855, Q:1817-1820—but not USB descriptor strings.
Exact strings need a real observation tied to each model/version; absence here
is not evidence that those fields are empty.

## 8. RS232 baud/parity defaults

**SETTLED: 9600 baud default for the three ONLINE models.** Current manuals
explicitly say “Domyślna prędkość: 9600 bitów/s”: Z:1367-1377, M:1263-1273,
Q:1263-1273. These user manuals list 9600,19200,28800,38400,57600,115200. They
expose configurable parity (Cube also data/stop bits). These are stronger
ONLINE-model evidence than importing the 115200 fixed configuration of a K10
cash register.

**STILL OPEN: factory parity/data/stop setting specifically for STX on ONLINE.**
P:3243-3247 fixes legacy `ELZABSTX` to **8,N,1**, while P:9007-9037 exposes
independent ONLINE serial speed/parity/data-length/stop configuration; parity
values 0 NONE, 1 EVEN, 2 ODD. The current ONLINE user manuals say parity is
configurable but do not name its default. Thus 8N1 is the documented legacy STX
convention and a configuration target, not proven automatic ONLINE default. Do
not select EVEN just because older ElzabESC examples do so; verify/configure
actual printer settings with the user, not by issuing fiscal probes.

The downloaded
[linuxip.tar.gz](https://www.elzab.com.pl/ftp/pub/kody/linuxip.tar.gz) and
[ecrinst.zip](https://www.elzab.com.pl/ftp/pub/kody/ecrinst.zip), extracted
under **EXTRA/**, do **not** settle this: ecrinst's
2026-06-25 manual is for ECR cash registers, price checkers and ETA label
printers (`ecrinst/ecrinst.pdf:289-303`). Its 115200 STX example at :369-407 is
explicitly K10, not a printer default. linuxip contains corresponding executable
tools, not a native online-printer source implementation. No vendor binary
executed or disassembled.

## 9. Open-source / integration search audit and remaining original gaps

- GitHub repository API query `ElzabSTX` returned **0**; `elzab` returned mostly
  unrelated projects, a legacy PHP ESC example, K10 connector and a Java-SDK
  documentation mirror. Broad fiscal-printer searches found
  [bartprokop/fiscal-printers](https://github.com/bartprokop/fiscal-printers),
  Apache-2.0; inspected commit `cdf34cdc826b4859da001859b987b03ebb3683eb`,
  `src/main/java/name/prokop/bart/fps/drivers/ElzabMera.java` and
  `ElzabOmega2.java`. They send ESC byte sequences, configure 9600/8E1/RTSCTS,
  and wait for single-byte ACK. They **do not implement ElzabSTX or ONLINE
  LDS**. Their footer calls before final ESC close are not native LAL/LCL
  evidence.
- [UNKpl/Elzab](https://github.com/UNKpl/Elzab), commit
  `1c4035a8f915cc3d0b9325e263f662c54ea89216`, has no declared license.
  `example.php` compares daily report number before/after `drukujdobowy` and
  later sleeps; this is old code, not a safe ONLINE RRP physical-completion
  proof. Do not copy its logic or sleeps.
- [Stack Overflow D10/STX SDK
  question](https://stackoverflow.com/questions/35507976/can-i-find-sdk-to-communicate-with-receipt-printer-by-my-own-app-on-android-usin)
  has **zero answers**, not a hidden implementation.
- npm search `elzab` and NuGet search `elzab` returned zero packages. grep.app
  API returned HTTP 429. Context7 resolver returned no ELZABDR library. These
  limit what can be concluded about public implementations; they are not proof
  no implementation exists anywhere.
- [Systim support list](https://www.systim.pl/drukarka_fiskalna) returned by
  search uses THERMAL for Zeta ONLINE; this is not native-STX evidence, and no
  settlement/completion trace was obtained from it.
- Original preflight gap remains: LPC explicitly checks whether a
  receipt/invoice can open (P:3509-3555); SDK ReceiptBegin calls
  ReceiptConditions internally (H:1072-1080). That is useful but not the missing
  read-only fiscalized/header/queues truth table or a report-specific safe
  status mask.
- Original error-persistence gap is partly clarified: H:1043-1052 explicitly
  warns `LastSTXError` may be incorrect unless called **immediately** after the
  error command. Do not poll other commands and later rely on it as durable
  causal evidence. No general certain-refusal allowlist, all-or-nothing
  side-effect guarantee or persistence across restart is established.
- Original reconciliation gap remains: no source obtained proves counter
  atomicity/attribution, protected-memory search compatibility across the new MD
  families, or a JWS schema linking an uncertain job to physical completion. SDK
  `isReceiptEnd` is not permission to auto-clear an unknown outcome or reprint.

## Final gap/status table

“SETTLED” below is scoped to the proposition in that row, not a claim of
hardware certification.

- **Gap / proposition — Fully prepaid zero-net sale can safely finish with LDS +
  a known LCL total**: **Status**: **STILL OPEN**; **Best evidence / remaining
  boundary**: LDS extension and error 1008 remain unreconciled; SOHO/SOSPA
  confirm settlement support, not this edge case
- **Gap / proposition — Substitute negative/storno item for advance
  settlement**: **Status**: **CONTRADICTED**; **Best evidence / remaining
  boundary**: O:15-21 expressly says use dedicated STX settlement
- **Gap / proposition — Native LAL order relative to LCL, physical footer/cut
  completion**: **Status**: **STILL OPEN**; **Best evidence / remaining
  boundary**: SDK stores footer text; API call order cannot establish wire order
- **Gap / proposition — Purchaser NIP supplied during construction before API
  close**: **Status**: **SETTLED**; **Best evidence / remaining boundary**:
  Manufacturer C:630-656; exact LNP-before-first-LIT/latest-native-state timing
  still open
- **Gap / proposition — Positive ONLINE daily RRP reply means finished
  printing**: **Status**: **CONTRADICTED**; **Best evidence / remaining
  boundary**: P:10229-10236 explicitly acceptance only
- **Gap / proposition — RPP supports sync/async and defaults to sync**:
  **Status**: **SETTLED**; **Best evidence / remaining boundary**:
  P:9493-9505,10278-10288; not a daily RRP switch
- **Gap / proposition — Safe native daily-report completion /
  paper-versus-memory proof**: **Status**: **STILL OPEN**; **Best evidence /
  remaining boundary**: SDK PrintControl wait exists, but algorithm and report
  attribution not published
- **Gap / proposition — Transport header grammar / per-command ID correlation**:
  **Status**: **SETTLED**; **Best evidence / remaining boundary**: B §§1.3–1.4;
  not execution deduplication
- **Gap / proposition — CRC-16/ARC algorithm and independent 4E0E / BA44
  arithmetic**: **Status**: **SETTLED**; **Best evidence / remaining boundary**:
  Vendor B + RevEng + crccheck 1.3.1 offline results
- **Gap / proposition — Vendor-published/observed complete native CRC frame**:
  **Status**: **STILL OPEN**; **Best evidence / remaining boundary**: Calculated
  vectors are not device traces
- **Gap / proposition — Completed-command deduplication / safe retransmission**:
  **Status**: **STILL OPEN**; **Best evidence / remaining boundary**: Only
  incomplete-assembly replacement documented; never fiscal-retry
- **Gap / proposition — ySR accepted during pending synchronous fiscal
  command**: **Status**: **STILL OPEN**; **Best evidence / remaining boundary**:
  No implementation/trace establishes it or late-response behavior
- **Gap / proposition — Cube native model constant 172 / ELZAB CUBE online**:
  **Status**: **SETTLED**; **Best evidence / remaining boundary**:
  P:323-324,344,357; not an observed firmware response
- **Gap / proposition — Complete Zeta/Mera/Cube ONLINE XTR/FNR identity
  matrix**: **Status**: **STILL OPEN**; **Best evidence / remaining boundary**:
  Real MD09/19, MD07/17, MD06 firmware names found; decimal IDs for current
  families unproven
- **Gap / proposition — CP852 / CP1250 / MAZOVIA support and CP1250 integrator
  convention**: **Status**: **SETTLED**; **Best evidence / remaining boundary**:
  Current three manuals + GoPOS; no proven factory default
- **Gap / proposition — Universal old 42-char item-name / old normalization
  profile**: **Status**: **CONTRADICTED**; **Best evidence / remaining
  boundary**: Current three manuals explicitly 64-char names; percent-sign
  significance conflicts with generic P
- **Gap / proposition — Current manual item names 1..64, description 3×35**:
  **Status**: **SETTLED**; **Best evidence / remaining boundary**: Z:1971-1989,
  M:1867-1875, Q:1833-1858; exact native per-firmware normalization remains open
- **Gap / proposition — Full printable-glyph repertoire and exact dynamic line
  behavior**: **Status**: **STILL OPEN**; **Best evidence / remaining
  boundary**: Code-page options/menu alphabet insufficient to prove complete
  glyph whitelist
- **Gap / proposition — Quantity precision maximum five fractional digits**:
  **Status**: **SETTLED**; **Best evidence / remaining boundary**:
  P:420-421,781-785; no per-firmware device test
- **Gap / proposition — Maximum items/receipt, magnitude ceilings,
  multiplication/rounding**: **Status**: **STILL OPEN**; **Best evidence /
  remaining boundary**: 500,000 PLU is not receipt capacity
- **Gap / proposition — ONLINE Linux VID:PID / iManufacturer / iProduct per
  model**: **Status**: **STILL OPEN**; **Best evidence / remaining boundary**:
  CDC class matching and legacy 0483:5740 are not model-attributed observations
- **Gap / proposition — ONLINE RS232 factory baud 9600**: **Status**:
  **SETTLED**; **Best evidence / remaining boundary**: Explicit in all three
  current user manuals
- **Gap / proposition — ONLINE STX factory parity/data/stop**: **Status**:
  **STILL OPEN**; **Best evidence / remaining boundary**: 8N1 is legacy STX
  convention; ONLINE settings independently configurable
- **Gap / proposition — Full preflight truth table / report-specific status
  mask**: **Status**: **STILL OPEN**; **Best evidence / remaining boundary**:
  Receipt LPC / SDK auto-check documented, report equivalence absent
- **Gap / proposition — LastSTXError may be used long after intervening calls**:
  **Status**: **CONTRADICTED**; **Best evidence / remaining boundary**:
  H:1043-1052 requires immediate retrieval
- **Gap / proposition — Certain-refusal allowlist / counter atomicity / JWS
  attribution**: **Status**: **STILL OPEN**; **Best evidence / remaining
  boundary**: No new source establishes these stronger safety properties
